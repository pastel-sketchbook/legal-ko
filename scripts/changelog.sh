#!/usr/bin/env bash
# Generate CHANGELOG.md from git tags and conventional commits.
# Usage: ./scripts/changelog.sh > CHANGELOG.md
set -euo pipefail

header() {
  cat <<'EOF'
# Changelog

All notable changes to this project are documented in this file.
Generated from conventional commits via `task changelog`.
EOF
}

# Classify a conventional-commit subject into a section heading.
section_for() {
  case "$1" in
    feat*)     echo "Added" ;;
    fix*)      echo "Fixed" ;;
    perf*)     echo "Changed" ;;
    refactor*) echo "Refactored" ;;
    docs*)     echo "Docs" ;;
    chore*|ci*|build*|style*|test*) echo "Chore" ;;
    *)         echo "Other" ;;
  esac
}

# Strip the conventional-commit prefix and optional scope, leaving the
# human-readable description.
strip_prefix() {
  # Remove type(scope): or type: prefix
  echo "$1" | sed -E 's/^[a-z]+(\([^)]*\))?[!]?:\s*//'
}

# ── main ──────────────────────────────────────────────────────
header

tags=($(git tag --sort=-version:refname))

for i in "${!tags[@]}"; do
  tag="${tags[$i]}"
  date=$(git log -1 --format='%ai' "$tag" | cut -d' ' -f1)
  version="${tag#v}"

  # Range: from previous tag (or root) to this tag.
  if (( i + 1 < ${#tags[@]} )); then
    range="${tags[$((i+1))]}..${tag}"
  else
    range="${tag}"
  fi

  # Collect commits in this range.
  mapfile -t subjects < <(git log --format='%s' "$range")
  if (( ${#subjects[@]} == 0 )); then
    continue
  fi

  echo ""
  echo "## [${version}] — ${date}"

  # Bucket subjects by section.
  declare -A buckets=()
  for subj in "${subjects[@]}"; do
    sec=$(section_for "$subj")
    desc=$(strip_prefix "$subj")
    buckets["$sec"]+="- ${desc}"$'\n'
  done

  # Print sections in a stable order.
  for sec in Added Changed Refactored Fixed Docs Chore Other; do
    if [[ -n "${buckets[$sec]:-}" ]]; then
      echo ""
      echo "### ${sec}"
      printf '%s' "${buckets[$sec]}"
    fi
  done

  unset buckets
done
