#!/usr/bin/env bash
# Dry-run the embedded sample's instructions via the SPEL CLI.
#
# Both authority slots live inside the consumer's program_config account:
# admin at byte offset 32, freeze at 64. Neither initializer exists. The
# admin slot is born initialized by the sample's own initialize, the
# freeze slot is born vacant until the admin appoints a holder. The
# management instructions of both extensions resolve the shared embedding
# account once, and no offset appears anywhere in a transaction.
#
# Usage:  scripts/dry-run-embedded.sh [path-to-spel-repo]
# Output: prints to stdout; CI or docs can redirect to a file.

set -uo pipefail

SPEL_REPO="${1:-$(dirname "$0")/../../spel}"
SAMPLE_SRC="$(dirname "$0")/../freeze-authority-sample-embedded/src/main.rs"
PROG_ID="$(printf 'ab%.0s' {1..32})"          # placeholder, fine for dry-run
CALLER="$(printf '11%.0s' {1..32})"
NEW_ADMIN="$(printf '22%.0s' {1..32})"
HOLDER="$(printf '33%.0s' {1..32})"
TARGET="$(printf '44%.0s' {1..32})"
IDL="$(mktemp --suffix .idl.json)"
trap 'rm -f "$IDL"' EXIT

echo "== Building spel CLI =="
(cd "$SPEL_REPO" && RISC0_SKIP_BUILD=1 cargo build -q -p spel 2>/dev/null)
SPEL_BIN="$SPEL_REPO/target/debug/spel"

echo "== Generating IDL from embedded sample =="
"$SPEL_BIN" generate-idl "$SAMPLE_SRC" 2>/dev/null > "$IDL"

run() {
    echo
    echo "── $* ──────────────────────────────"
    "$SPEL_BIN" --idl "$IDL" --program "$PROG_ID" --dry-run -- "$@" 2>&1
}

run initialize --signer "$CALLER"
run update-value --caller "$CALLER" --new-value 42
run read-value
run withdraw --caller "$CALLER"

run admin-transfer --caller "$CALLER" --new-admin-account "$NEW_ADMIN" --new-admin Signer
run freeze-authority-transfer --caller "$CALLER" --new-account "$HOLDER" --candidate Signer
run freeze-program --caller "$HOLDER"
run freeze-program-release --caller "$HOLDER"
run freeze-account --caller "$HOLDER" --target "$TARGET"
run freeze-account-release --caller "$HOLDER" --target "$TARGET"
run freeze-authority-renounce --caller "$HOLDER"
run admin-renounce --caller "$CALLER"

echo
echo "Done."
