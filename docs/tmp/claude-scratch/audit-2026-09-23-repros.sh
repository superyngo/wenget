#!/usr/bin/env bash
# Reproductions for docs/audit/2026-09-23-implementation-simplicity-audit.md.
# Every run is sandboxed under WENGET_ROOT (bin dir included), so the real
# ~/.wenget and ~/.local/bin are never touched.
#
# Usage: cargo build && bash docs/tmp/claude-scratch/audit-2026-09-23-repros.sh
set -u
BIN=${BIN:-./target/debug/wenget}
TMP=$(mktemp -d /tmp/wenget-audit.XXXXXX)
echo "sandbox: $TMP"

echo; echo "== IM-1: failed install exits 0 =="
WENGET_ROOT=$TMP/r1 "$BIN" add -y https://wenget-audit.invalid/tool-linux-x86_64.tar.gz >/dev/null 2>&1
echo "exit=$?   (expected non-zero)"

echo; echo "== IM-2: error cause chain dropped =="
touch "$TMP/notadir"
WENGET_ROOT=$TMP/notadir/root "$BIN" bucket list
echo "(expected to also show: Not a directory (os error 20))"

echo; echo "== IM-3: --verbose / RUST_LOG have no effect =="
echo -n "DEBUG lines with --verbose: "
WENGET_ROOT=$TMP/r3 "$BIN" --verbose add -y https://wenget-audit.invalid/x.tar.gz 2>&1 | grep -c DEBUG
echo -n "INFO lines with RUST_LOG=error: "
RUST_LOG=error WENGET_ROOT=$TMP/r3 "$BIN" add -y https://wenget-audit.invalid/x.tar.gz 2>&1 | grep -c INFO

echo; echo "== IM-4: rename of a non-bash script package fails on Unix =="
mkdir -p "$TMP/src"
printf '#!/usr/bin/env python3\nprint("hi")\n' > "$TMP/src/hello.py"
WENGET_ROOT=$TMP/r4 "$BIN" add -y "$TMP/src/hello.py" >/dev/null 2>&1
WENGET_ROOT=$TMP/r4 "$BIN" rename hello hey
echo "exit=$?   (expected 0 and a 'hey' launcher)"
ls "$TMP/r4/bin"
