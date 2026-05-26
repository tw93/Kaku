#!/usr/bin/env bash
# Verify every shipped prompt file under assets/prompts/ carries the metadata
# header expected by Kaku's prompt loader. Format mirrors Piebald's
# claude-code-system-prompts so each file is independently auditable.
#
# Required header (first non-blank lines of the file):
#   <!--
#   name: '...'
#   description: ...
#   kakuVersion: <version>
#   ...
#   -->
#
# Exit non-zero if any prompt is missing one of: <!-- opener, name:, description:,
# kakuVersion:, --> closer.
#
# Used by release gating; also safe to run locally.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROMPTS_DIR="$REPO_ROOT/assets/prompts"
EXPECTED_VERSION="$(sed -nE 's/^version = "([^"]+)".*/\1/p' "$REPO_ROOT/kaku-gui/Cargo.toml" | head -n 1)"

if [[ -z "$EXPECTED_VERSION" ]]; then
  echo "[check_prompts] Could not read kaku-gui/Cargo.toml version." >&2
  exit 1
fi

if [[ ! -d "$PROMPTS_DIR" ]]; then
  echo "[check_prompts] No assets/prompts directory; nothing to check." >&2
  exit 0
fi

failed=0
missing_files=()

while IFS= read -r -d '' f; do
  rel="${f#$REPO_ROOT/}"
  head_block="$(awk '/-->/ { print; exit } { print }' "$f")"

  ok=1
  echo "$head_block" | head -n1 | grep -q '^<!--' || ok=0
  echo "$head_block" | grep -qE '^name:[[:space:]]+'           || ok=0
  echo "$head_block" | grep -qE '^description:[[:space:]]+'    || ok=0
  echo "$head_block" | grep -qE "^kakuVersion:[[:space:]]+$EXPECTED_VERSION$" || ok=0
  echo "$head_block" | grep -q '^-->$'                         || ok=0

  if [[ $ok -ne 1 ]]; then
    failed=1
    missing_files+=("$rel")
  fi
done < <(find "$PROMPTS_DIR" -type f -name '*.txt' -print0)

if [[ $failed -ne 0 ]]; then
  echo "❌ Prompt files missing required metadata header:" >&2
  for f in "${missing_files[@]}"; do
    echo "   - $f" >&2
  done
  echo "" >&2
  echo "Every prompt must start with:" >&2
  echo "   <!--" >&2
  echo "   name: '...'" >&2
  echo "   description: ..." >&2
  echo "   kakuVersion: $EXPECTED_VERSION" >&2
  echo "   -->" >&2
  exit 1
fi

count="$(find "$PROMPTS_DIR" -type f -name '*.txt' | wc -l | tr -d ' ')"
echo "✅ All $count prompt files have metadata headers."
