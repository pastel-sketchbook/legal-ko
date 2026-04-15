#!/usr/bin/env bash
# scripts/zmd-collections.sh — Thin wrapper around `legal-ko-cli zmd`.
#
# The native Rust implementation in legal-ko-cli replaces the original bash
# pipeline with Rayon-parallel hardlink staging and batched zmd indexing.
#
# Usage:
#   ./scripts/zmd-collections.sh              # run all phases
#   ./scripts/zmd-collections.sh laws         # laws only
#   ./scripts/zmd-collections.sh precedents   # precedents only (민사+형사 대법원)
#   ./scripts/zmd-collections.sh sync         # re-pull repos + re-index
#   ./scripts/zmd-collections.sh status       # show current state
#   ./scripts/zmd-collections.sh reset        # remove everything and start fresh
#
# Environment (passed through to legal-ko-cli):
#   ZMD_BATCH_SIZE   Files per zmd update call (default: 300)
#
# Prefer `task zmd`, `task zmd:laws`, etc. — those also rebuild the binary.
set -euo pipefail

CLI="${LEGAL_KO_CLI:-legal-ko-cli}"

if ! command -v "$CLI" &>/dev/null; then
  echo "error: $CLI not found — run 'task install' first" >&2
  exit 1
fi

cmd="${1:-all}"

case "$cmd" in
  laws|precedents|all|sync|status|reset)
    exec "$CLI" zmd "$cmd"
    ;;
  -h|--help|help)
    cat <<'USAGE'
Usage: zmd-collections.sh [command]

Commands:
  all          Run all phases: laws then precedents (default)
  laws         Clone + stage + index laws (법률 only)
  precedents   Clone + stage + index precedents (민사+형사 대법원)
  sync         Pull latest from upstream repos + re-index
  status       Show current state (repos, staged files, zmd collections)
  reset        Remove collections and staged data (keeps repo clones)
  help         Show this help

Environment:
  ZMD_BATCH_SIZE     Files per zmd update call (default: 300)
  LEGAL_KO_CLI       Path to legal-ko-cli binary (default: legal-ko-cli)

This script delegates to `legal-ko-cli zmd <command>`.
Prefer using `task zmd`, `task zmd:laws`, etc. which also rebuild the binary.
USAGE
    ;;
  *)
    echo "error: unknown command: $cmd (try 'help')" >&2
    exit 1
    ;;
esac
