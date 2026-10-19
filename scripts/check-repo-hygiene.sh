#!/bin/sh
# Repo hygiene gate — refuse hidden / broken files before they are committed.
#
# A local + CI gate. Run it yourself before pushing:
#
#     sh scripts/check-repo-hygiene.sh
#
# It scans the TRACKED tree (what a commit actually carries), so it catches the
# classes of file that pass code review unseen and break a consumer later:
#
#   1. committed symlinks — an absolute or relative symlink in git breaks
#      Windows/Composer/zip extraction and is a supply-chain footgun (this is
#      exactly what shipped the broken .confd_* links once);
#   2. dangling symlinks in the working tree — a link to nowhere;
#   3. zero-byte tracked files — the usual shape of a truncated or half-staged
#      commit;
#   4. junk / editor / OS / merge-leftover files that should never be tracked
#      (.DS_Store, *.swp/*.swo, *~, *.bak, *.orig, *.rej), plus a STRAY
#      .tina4-metrics.json (per-project run-history data) left anywhere other
#      than the one committed ratchet baseline at src/.tina4-metrics.json, which
#      the Metrics regression gate reads and is allowed;
#   5. NUL bytes inside a text-source file — a corrupt or truly-binary file
#      masquerading as source.
#
# Exit 0 clean, exit 1 naming every offender. Zero dependencies beyond git and
# perl (both present locally and in CI).
set -eu

cd "$(git rev-parse --show-toplevel)"
status=0

emit() { printf '%s\n' "$1" >&2; }
fail() { status=1; emit "ERROR: $1"; shift; for f in "$@"; do emit "  $f"; done; }

# 1) committed symlinks (git stores them with mode 120000)
symlinks=$(git ls-files -s | awk '$1=="120000"{print $4}')
[ -n "$symlinks" ] && fail "committed symlinks are not allowed (break Windows/Composer extraction; supply-chain risk):" $symlinks

# 2) dangling symlinks anywhere in the tracked tree
dangling=""
for l in $(git ls-files); do
  if [ -h "$l" ] && [ ! -e "$l" ]; then dangling="$dangling $l"; fi
done
[ -n "$dangling" ] && fail "dangling symlinks (point to a missing target):" $dangling

# 3) zero-byte tracked files
empty=""
for f in $(git ls-files); do
  if [ -f "$f" ] && [ ! -h "$f" ] && [ ! -s "$f" ]; then empty="$empty $f"; fi
done
[ -n "$empty" ] && fail "zero-byte tracked files (truncated or half-staged?):" $empty

# 4) junk / editor / OS / merge-leftover files that must never be tracked
# The committed ratchet baseline src/.tina4-metrics.json is deliberately tracked
# (the Metrics gate diffs against it); exclude exactly that path, so a stray
# .tina4-metrics.json left anywhere else is still caught.
junk=$(git ls-files | grep -E '(^|/)\.DS_Store$|(^|/)\.tina4-metrics\.json$|\.(swp|swo|orig|rej|bak)$|~$' | grep -vxF 'src/.tina4-metrics.json' || true)
[ -n "$junk" ] && fail "junk / hidden / merge-leftover files must not be tracked:" $junk

# 5) NUL bytes in a text-source file (corrupt or mislabelled binary)
nul=""
for f in $(git ls-files '*.rs' '*.toml' '*.md' '*.sh' '*.ps1' '*.json' '*.yml' '*.yaml' '*.py' '*.txt'); do
  [ -f "$f" ] && [ ! -h "$f" ] || continue
  if perl -ne 'if (/\x00/){exit 1}' "$f"; then :; else nul="$nul $f"; fi
done
[ -n "$nul" ] && fail "NUL byte in a text-source file (corrupt or binary-as-source):" $nul

if [ "$status" -eq 0 ]; then
  echo "OK: no hidden or broken files (symlinks / dangling / empty / junk / NUL)"
fi
exit "$status"
