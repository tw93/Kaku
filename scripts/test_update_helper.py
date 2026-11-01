#!/usr/bin/env python3
"""Run the update helper on fixture bundles; never signal, launch or replace a real app."""
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]

MOCKS = r'''
ps() {
    while read -r pid; do
        [[ -n "$pid" ]] && printf '%s %s/Contents/MacOS/kaku-gui\n' "$pid" "$FIXTURE_TARGET"
    done < "$FIXTURE/running"
}
kill() {
    if [[ "$1" == -0 ]]; then builtin kill "$@"; return; fi
    local staged=no
    compgen -G "$FIXTURE_TARGET.incoming.*" >/dev/null && staged=yes
    printf 'kill %s %s staged=%s\n' "$1" "$2" "$staged" >> "$FIXTURE/calls"
    : > "$FIXTURE/running"
}
sleep() { :; }
'''

FAKE_TOOL = '''#!/bin/bash
printf '%s %s\\n' "$(basename "$0")" "$*" >> "$FIXTURE/calls"
if [[ "$(basename "$0")" == launchctl && "$1" == bootstrap ]]; then
    cp "$3" "$FIXTURE/job.plist"
    exit "${FIXTURE_BOOTSTRAP_RC:-0}"
fi
exit 0
'''


def make_app(path: Path, marker: str, version: str):
    macos = path / "Contents/MacOS"
    macos.mkdir(parents=True)
    (path / "Contents/marker").write_text(marker)
    cli = macos / "kaku"
    cli.write_text("#!/bin/bash\nexit 0\n")
    cli.chmod(0o755)
    with open(path / "Contents/Info.plist", "wb") as f:
        plistlib.dump({"CFBundleShortVersionString": version}, f)


class UpdateHelperTests(unittest.TestCase):
    def run_helper(self, *, inline=True, new_app_exists=True, interrupt_mid_swap=False,
                   lose_staged_copy=False, stale_incoming=False, fail_swap=False,
                   lock_owner=None,
                   bootstrap_rc=0, job_label=None, running=("4242",)):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        target = root / "Applications/Kaku.app"
        make_app(target, "old", "0.27.1")
        new_app = root / "staged/Kaku.app"
        if new_app_exists:
            make_app(new_app, "new", "0.28.0")
        if lock_owner is not None:
            lock = root / "Applications/Kaku.app.updating"
            lock.mkdir()
            (lock / "pid").write_text(f"{lock_owner}\n")
        if stale_incoming:
            make_app(root / "Applications/Kaku.app.incoming.1.1", "stale", "0.0.1")
        work = root / "work"
        work.mkdir()
        (root / "running").write_text("".join(f"{p}\n" for p in running))
        (root / "calls").write_text("")
        bin_dir = root / "bin"
        bin_dir.mkdir()
        for tool in ("launchctl", "open", "osascript"):
            fake = bin_dir / tool
            fake.write_text(FAKE_TOOL)
            fake.chmod(0o755)

        source = (ROOT / "scripts/update_helper.sh").read_text()
        replacements = {
            "/bin/launchctl": f"{bin_dir}/launchctl",
            "/usr/bin/open": f"{bin_dir}/open",
            "/usr/bin/osascript": f"{bin_dir}/osascript",
            # Keep the log for assertions.
            '/bin/rm -rf "$WORK_DIR"': ":",
        }
        for old, new in replacements.items():
            self.assertGreaterEqual(source.count(old), 1, old)
            source = source.replace(old, new)
        if interrupt_mid_swap:
            anchor = '  /bin/mv "$TARGET_APP" "$BACKUP_APP"\n'
            self.assertEqual(source.count(anchor), 1)
            extra = '  /bin/rm -rf "$INCOMING_APP"\n' if lose_staged_copy else ""
            source = source.replace(anchor, anchor + extra + '  builtin kill -TERM $$\n')
        if fail_swap:
            anchor = '/bin/mv "$INCOMING_APP" "$TARGET_APP"\n'
            self.assertEqual(source.count(anchor), 1)
            source = source.replace(anchor, '/bin/rm -rf "$INCOMING_APP"; /bin/mv "$INCOMING_APP" "$TARGET_APP"\n')
        # Mocks go after the shebang and set lines so `ps`/`kill` resolve to them.
        script = root / "updates/apply-staged-1.sh"
        script.parent.mkdir()
        script.write_text(MOCKS + source)

        env = {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(root),
            "XDG_DATA_HOME": str(root / "data"),
            "XDG_CONFIG_HOME": str(root / "config"),
            "FIXTURE": str(root),
            "FIXTURE_TARGET": str(target),
            "FIXTURE_BOOTSTRAP_RC": str(bootstrap_rc),
            "GH_TOKEN": "fixture-secret",
            "ZDOTDIR": str(root / "zdot"),
        }
        if inline:
            env["KAKU_UPDATE_INLINE"] = "1"
        if job_label:
            env["KAKU_UPDATE_JOB_LABEL"] = job_label
        proc = subprocess.run(["/bin/bash", str(script), str(target), str(new_app), str(work)],
                              env=env, capture_output=True, text=True, timeout=60)
        log = (work / "update.log").read_text() if (work / "update.log").exists() else ""
        return proc, root, target, log

    def marker(self, target):
        return (target / "Contents/marker").read_text()

    def leftovers(self, target, kind):
        return list(target.parent.glob(f"Kaku.app.{kind}.*"))

    def test_stages_the_new_app_before_stopping_kaku(self):
        proc, root, target, log = self.run_helper()
        self.assertEqual(proc.returncode, 0, log + proc.stderr)
        self.assertEqual(self.marker(target), "new")
        self.assertIn("kill -TERM 4242 staged=yes", (root / "calls").read_text())
        self.assertLess(log.index("stage new app"), log.index("swap in new app"))
        self.assertEqual(self.leftovers(target, "incoming"), [])
        self.assertEqual(self.leftovers(target, "backup"), [])
        self.assertFalse((root / "Applications/Kaku.app.updating").exists())

    def test_a_second_helper_leaves_a_running_update_alone(self):
        proc, root, target, log = self.run_helper(lock_owner=os.getpid())
        self.assertEqual(proc.returncode, 0, log + proc.stderr)
        self.assertEqual(self.marker(target), "old")
        self.assertNotIn("kill", (root / "calls").read_text())
        self.assertIn("another update is already running", log)
        self.assertTrue((root / "Applications/Kaku.app.updating").exists())

    def test_takes_over_a_lock_left_by_a_dead_helper(self):
        proc, root, target, log = self.run_helper(lock_owner=99999999)
        self.assertEqual(proc.returncode, 0, log + proc.stderr)
        self.assertEqual(self.marker(target), "new")
        self.assertFalse((root / "Applications/Kaku.app.updating").exists())

    def test_a_signal_between_the_renames_still_leaves_an_app(self):
        proc, root, target, log = self.run_helper(interrupt_mid_swap=True)
        self.assertNotEqual(proc.returncode, 0)
        self.assertTrue(target.is_dir(), log)
        self.assertEqual(self.marker(target), "new")
        self.assertIn("finished swap on exit", log)
        self.assertEqual(self.leftovers(target, "incoming"), [])
        # A signal is usually logout or shutdown: restore, but do not reopen.
        self.assertNotIn("open", (root / "calls").read_text())

    def test_a_signal_without_a_staged_copy_restores_the_previous_app(self):
        proc, root, target, log = self.run_helper(interrupt_mid_swap=True, lose_staged_copy=True)
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(self.marker(target), "old")
        self.assertIn("restored previous app", log)
        self.assertEqual(self.leftovers(target, "backup"), [])

    def test_an_error_after_stopping_kaku_reopens_it(self):
        proc, root, target, log = self.run_helper(fail_swap=True)
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(self.marker(target), "old")
        self.assertIn(f"open {target}", (root / "calls").read_text())

    def test_removes_staged_copies_left_by_an_earlier_run(self):
        proc, root, target, log = self.run_helper(stale_incoming=True)
        self.assertEqual(proc.returncode, 0, log + proc.stderr)
        self.assertEqual(self.leftovers(target, "incoming"), [])

    def test_a_failed_copy_leaves_kaku_running_and_installed(self):
        proc, root, target, log = self.run_helper(new_app_exists=False)
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(self.marker(target), "old")
        calls = (root / "calls").read_text()
        self.assertNotIn("kill", calls)
        self.assertNotIn("open", calls)
        self.assertEqual(self.leftovers(target, "incoming"), [])
        self.assertEqual(self.leftovers(target, "backup"), [])

    def test_hands_the_update_to_a_launchd_job(self):
        proc, root, target, log = self.run_helper(inline=False)
        self.assertEqual(proc.returncode, 0, log + proc.stderr)
        # The caller only submits the job; the job does the replacement.
        self.assertEqual(self.marker(target), "old")
        self.assertNotIn("kill", (root / "calls").read_text())
        with open(root / "job.plist", "rb") as f:
            job = plistlib.load(f)
        self.assertTrue(job["Label"].startswith("fun.tw93.kaku.update."))
        self.assertTrue(job["RunAtLoad"] and job["AbandonProcessGroup"] and job["LaunchOnlyOnce"])
        lint = subprocess.run(["/usr/bin/plutil", "-lint", str(root / "job.plist")], capture_output=True)
        self.assertEqual(lint.returncode, 0, lint.stdout)
        self.assertNotIn("KeepAlive", job)
        self.assertEqual(job["ProgramArguments"][1:],
                         [str(root / "updates/apply-staged-1.sh"), str(target),
                          str(root / "staged/Kaku.app"), str(root / "work")])
        environment = job["EnvironmentVariables"]
        self.assertEqual(environment["KAKU_UPDATE_JOB_LABEL"], job["Label"])
        self.assertEqual(environment["HOME"], str(root))
        self.assertEqual(environment["XDG_DATA_HOME"], str(root / "data"))
        self.assertEqual(environment["ZDOTDIR"], str(root / "zdot"))
        self.assertNotIn("GH_TOKEN", environment)
        self.assertEqual(list((root / "work").glob("*.plist")), [])

    def test_applies_inline_when_launchd_refuses_the_job(self):
        proc, root, target, log = self.run_helper(inline=False, bootstrap_rc=5)
        self.assertEqual(proc.returncode, 0, log + proc.stderr)
        self.assertEqual(self.marker(target), "new")
        self.assertIn("launchd job refused, applying inline", log)

    def test_the_job_applies_the_update_and_unloads_itself(self):
        proc, root, target, log = self.run_helper(inline=False, job_label="fun.tw93.kaku.update.fixture")
        self.assertEqual(proc.returncode, 0, log + proc.stderr)
        self.assertEqual(self.marker(target), "new")
        calls = root / "calls"
        self.assertNotIn("bootstrap", calls.read_text())
        deadline = time.time() + 10
        while "remove fun.tw93.kaku.update.fixture" not in calls.read_text() and time.time() < deadline:
            time.sleep(0.2)
        self.assertIn("launchctl remove fun.tw93.kaku.update.fixture", calls.read_text())


if __name__ == "__main__":
    unittest.main()
