#!/usr/bin/env bash
# Dry-run the freeze-authority sample's instructions via the SPEL CLI.
#
# Fully offline: dry-run resolves accounts, PDAs, and instruction data and
# prints the transaction without submitting. No node, no built guest binary
# (a placeholder program id is enough).
#
# Usage:  scripts/dry-run.sh [path-to-spel-repo]
# Output: prints to stdout; CI or docs can redirect to a file.
#
# Enum arguments (candidate: FreezeCandidate) use the CLI's defined-type
# syntax: a bare variant name for unit variants (Signer), or a one-key
# JSON object for payload variants ({"Pda": {"program_id": "..", "seed": ".."}}).

set -uo pipefail

SPEL_REPO="${1:-$(dirname "$0")/../../spel}"
SAMPLE_SRC="$(dirname "$0")/../freeze-authority-sample/src/main.rs"
PROG_ID="$(printf 'ab%.0s' {1..32})"          # placeholder, fine for dry-run
CALLER="$(printf '11%.0s' {1..32})"
NEW_AUTH="$(printf '22%.0s' {1..32})"
TARGET="$(printf '33%.0s' {1..32})"
IDL="$(mktemp --suffix .idl.json)"
trap 'rm -f "$IDL"' EXIT

echo "== Building spel CLI =="
(cd "$SPEL_REPO" && RISC0_SKIP_BUILD=1 cargo build -q -p spel 2>/dev/null)
SPEL_BIN="$SPEL_REPO/target/debug/spel"

echo "== Generating IDL from sample =="
"$SPEL_BIN" generate-idl "$SAMPLE_SRC" 2>/dev/null > "$IDL"

run() {
    echo
    echo "── $* ──────────────────────────────"
    "$SPEL_BIN" --idl "$IDL" --program "$PROG_ID" --dry-run -- "$@" 2>&1
}

# Auto-gated consumer instruction: framework injects freeze_config,
# freeze_account, and caller alongside the source-declared `config`.
# Reviewer point 2a. Confirms IDL resolution sees every gate account.
run update-value --caller "$CALLER" --new-value 42

# Freeze-exempt consumer instruction: no gate accounts, escape-hatch read.
run read-value

# Admin-gated freeze management instructions.
run freeze-initialize --caller "$CALLER"
run freeze-program --caller "$CALLER"
run freeze-program-release --caller "$CALLER"

# Signer candidate: hand freeze authority off to a new signer.
run freeze-authority-transfer \
    --caller "$CALLER" \
    --new-account "$NEW_AUTH" \
    --candidate Signer

# Pda candidate: hand freeze authority to a program-derived address.
PDA_PROGRAM="$(printf 'cd%.0s' {1..32})"
PDA_SEED="$(printf 'ef%.0s' {1..32})"
run freeze-authority-transfer \
    --caller "$CALLER" \
    --new-account "$NEW_AUTH" \
    --candidate "{\"Pda\": {\"program_id\": \"$PDA_PROGRAM\", \"seed\": \"$PDA_SEED\"}}"

# Dual-path renounce (admin OR current holder per ADR-0004).
run freeze-authority-renounce --caller "$CALLER"

# Per-account freeze operations, target passed as an argument.
run freeze-account --caller "$CALLER" --target "$TARGET"
run freeze-account-release --caller "$CALLER" --target "$TARGET"

echo
echo "Done."
