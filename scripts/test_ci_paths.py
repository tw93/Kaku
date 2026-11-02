#!/usr/bin/env python3
"""Lock the Checks workflow's path filter against files the checks read.

A push that touches only skipped files starts no Checks run, so a skipped
file that a check reads or the build embeds can break main with nothing red.
This test evaluates the filter the way GitHub does (last matching pattern
wins, `*` stops at `/`, `**` crosses it) over every tracked file.
"""
from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/checks.yml"

# Non-code files a check step reads. Add a file here and to the filter in the
# same change when a check starts reading it.
READS = {
    "docs/incidents.md": "scripts/check_incidents.py",
    ".github/RELEASE_NOTES.md": "scripts/check_release_notes.sh",
}

CODE_SUFFIXES = {
    ".rs", ".toml", ".lock", ".lua", ".sh", ".bash", ".zsh", ".fish", ".py",
    ".js", ".cjs", ".mjs", ".ts", ".json", ".yml", ".yaml", ".c", ".h", ".m",
    ".swift", ".plist", ".txt", ".tsv",
}

# Code-suffixed files skipped on purpose: GitHub reads issue templates when a
# visitor opens an issue, and no check does.
SKIPPED_CODE = (".github/ISSUE_TEMPLATE/",)

INCLUDE = re.compile(r'include_(?:bytes|str)!\(\s*"([^"]+)"\s*\)')


def event_paths(text, event):
    match = re.search(rf"^  {event}:\n(.*?)(?=^  \S|^\S|\Z)", text, re.S | re.M)
    if not match:
        raise AssertionError(f"no {event} trigger in {WORKFLOW.name}")
    block = re.search(r"^    paths:\n((?:      - .*\n)+)", match.group(1), re.M)
    if not block:
        raise AssertionError(f"no paths list under {event}")
    return [re.match(r"      - '(.*)'", line).group(1) for line in block.group(1).splitlines()]


def compile_pattern(pattern):
    out, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**", i):
            out, i = out + ".*", i + 2
        elif pattern[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif pattern[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(pattern[i]), i + 1
    return re.compile(out + r"\Z")


def make_filter(patterns):
    compiled = [(p.startswith("!"), compile_pattern(p.lstrip("!"))) for p in patterns]

    def included(path):
        result = False
        for negated, regex in compiled:
            if regex.match(path):
                result = not negated
        return result

    return included


class CiPathFilterTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        text = WORKFLOW.read_text()
        cls.push = event_paths(text, "push")
        cls.pull_request = event_paths(text, "pull_request")
        cls.included = staticmethod(make_filter(cls.push))
        cls.files = subprocess.check_output(
            ["git", "ls-files"], cwd=ROOT, text=True
        ).split()

    def test_push_and_pull_request_filters_match(self):
        self.assertEqual(self.push, self.pull_request)

    def test_filter_skips_docs(self):
        # Positive control: without it, a filter that skips nothing passes.
        self.assertFalse(self.included("README.md"))
        self.assertFalse(self.included("docs/configuration.md"))
        self.assertTrue(self.included("docs/incidents.md"))

    def test_code_files_are_never_skipped(self):
        skipped = [
            f for f in self.files
            if Path(f).suffix in CODE_SUFFIXES
            and not f.startswith(SKIPPED_CODE)
            and not self.included(f)
        ]
        self.assertEqual(skipped, [])

    def test_files_checks_read_are_never_skipped(self):
        missing = [f for f in READS if f not in self.files]
        self.assertEqual(missing, [], "READS names a file that is no longer tracked")
        skipped = [f"{f} (read by {reader})" for f, reader in READS.items() if not self.included(f)]
        self.assertEqual(skipped, [])

    def test_embedded_files_are_never_skipped(self):
        found, skipped = 0, []
        for source in self.files:
            if not source.endswith(".rs"):
                continue
            for target in INCLUDE.findall((ROOT / source).read_text(errors="ignore")):
                if "{" in target or "concat!" in target:
                    continue
                resolved = (ROOT / source).parent.joinpath(target).resolve()
                try:
                    relative = resolved.relative_to(ROOT).as_posix()
                except ValueError:
                    continue
                if relative not in self.files:
                    continue
                found += 1
                if not self.included(relative):
                    skipped.append(f"{relative} (embedded by {source})")
        self.assertGreater(found, 0, "include_bytes!/include_str! scan matched nothing")
        self.assertEqual(skipped, [])


if __name__ == "__main__":
    unittest.main()
