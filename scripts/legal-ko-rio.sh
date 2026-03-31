#!/usr/bin/env bash
# Launch legal-ko in a dedicated Rio terminal with D2Coding Nerd Font.
# Usage: ./scripts/legal-ko-rio.sh
#
# Requires:
#   - Rio terminal (brew install --cask rio)
#   - D2Coding Nerd Font (brew install --cask font-d2coding-nerd-font)
#   - legal-ko binary on PATH (task install)
#
# How it works:
#   Sets RIO_CONFIG_HOME to ~/.config/legal-ko/rio which has its own
#   config.toml using D2CodingLigature Nerd Font Mono instead of your
#   default Rio font. Your main Rio config is untouched.

set -euo pipefail

RIO_CONFIG="${HOME}/.config/legal-ko/rio"

if [[ ! -f "${RIO_CONFIG}/config.toml" ]]; then
  echo "Error: ${RIO_CONFIG}/config.toml not found." >&2
  echo "Run the setup first — see AGENTS.md or README." >&2
  exit 1
fi

if ! command -v rio &>/dev/null; then
  echo "Error: rio not found. Install with: brew install --cask rio" >&2
  exit 1
fi

if ! command -v legal-ko &>/dev/null; then
  echo "Error: legal-ko not found on PATH. Run: task install" >&2
  exit 1
fi

exec env RIO_CONFIG_HOME="${RIO_CONFIG}" rio -e legal-ko
