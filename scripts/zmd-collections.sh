#!/usr/bin/env bash
# scripts/zmd-collections.sh — Clone, filter, stage, and index Korean law/precedent
# collections into zmd in manageable batches.
#
# Strategy:
#   1. Clone the upstream repos (shallow) into a local cache dir
#   2. Stage files incrementally using hardlinks (zero extra disk space)
#   3. Call `zmd update` after each batch of N files
#      - zmd skips already-indexed docs, so only new files are processed
#      - Each batch completes in ~1-3 min, making progress visible and
#        allowing safe interruption (just re-run to resume)
#
# Note: zmd's walkdir does not follow symlinks, so hardlinks are required.
#
# Batching:
#   Laws:       ~1,711 법률.md files, indexed in batches of BATCH_SIZE
#   Precedents: 민사/대법원 + 형사/대법원, indexed in batches of BATCH_SIZE
#
# Usage:
#   ./scripts/zmd-collections.sh              # run all phases
#   ./scripts/zmd-collections.sh laws         # laws only
#   ./scripts/zmd-collections.sh precedents   # precedents only (민사 + 형사)
#   ./scripts/zmd-collections.sh sync         # re-pull repos + re-index
#   ./scripts/zmd-collections.sh status       # show current state
#   ./scripts/zmd-collections.sh reset        # remove everything and start fresh
#
# Environment:
#   ZMD_CACHE_DIR   — cache root (default: ~/.cache/legal-ko/zmd)
#   ZMD_BATCH_SIZE  — files per zmd update call (default: 100)
#
set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────

LAWS_REPO="https://github.com/legalize-kr/legalize-kr.git"
PRECEDENT_REPO="https://github.com/legalize-kr/precedent-kr.git"

CACHE_DIR="${ZMD_CACHE_DIR:-$HOME/.cache/legal-ko/zmd}"
REPOS_DIR="$CACHE_DIR/repos"
STAGE_DIR="$CACHE_DIR/stage"

LAWS_CLONE="$REPOS_DIR/legalize-kr"
PRECEDENT_CLONE="$REPOS_DIR/precedent-kr"

LAWS_STAGE="$STAGE_DIR/laws"
PRECEDENT_STAGE="$STAGE_DIR/precedents"

BATCH_SIZE="${ZMD_BATCH_SIZE:-100}"

# Case types to include for precedents (add more here to expand scope)
PRECEDENT_CASE_TYPES=("민사" "형사")

# Court levels to include (add "하급심" here to expand scope)
PRECEDENT_COURTS=("대법원")

# ── Helpers ────────────────────────────────────────────────────────────────

log()  { printf "\033[1;34m[zmd]\033[0m %s\n" "$*" >&2; }
ok()   { printf "\033[1;32m[zmd]\033[0m %s\n" "$*" >&2; }
warn() { printf "\033[1;33m[zmd]\033[0m %s\n" "$*" >&2; }
err()  { printf "\033[1;31m[zmd]\033[0m %s\n" "$*" >&2; }
die()  { err "$@"; exit 1; }

count_files() {
  find "$1" -name "*.md" -type f 2>/dev/null | wc -l | tr -d ' '
}

# ── Clone / Pull ───────────────────────────────────────────────────────────

clone_or_pull() {
  local url="$1" dir="$2" name="$3"

  if [[ -d "$dir/.git" ]]; then
    log "Pulling latest $name..."
    git -C "$dir" pull --ff-only --depth 1 2>&1 | tail -1 >&2
  else
    log "Cloning $name (shallow)..."
    mkdir -p "$(dirname "$dir")"
    git clone --depth 1 "$url" "$dir" 2>&1 | tail -1 >&2
  fi
}

# ── Register with zmd ─────────────────────────────────────────────────────

register_collection() {
  local name="$1" path="$2"

  if zmd collection list 2>/dev/null | grep -q "^  $name:"; then
    log "Collection '$name' already registered"
    return 0
  fi

  log "Registering collection '$name' → $path"
  zmd collection add "$name" "$path" >&2
}

# ── Index one batch ────────────────────────────────────────────────────────

index_batch() {
  local label="$1"
  local start=$SECONDS
  zmd update >/dev/null 2>&1
  local elapsed=$(( SECONDS - start ))
  local total
  total=$(zmd status 2>/dev/null | grep "^Documents:" | awk '{print $2}')
  ok "  $label — ${elapsed}s (total indexed: $total)"
}

# ── Batch stage + index loop ──────────────────────────────────────────────
#
# Reads file paths from stdin, hardlinks them into the stage dir, and calls
# zmd update every BATCH_SIZE files.
#
# Arguments:
#   $1 — source root (prefix to strip for relative path)
#   $2 — stage root (destination for hardlinks)
#   $3 — label for logging
#
batch_stage_and_index() {
  local src_root="$1"
  local stage_root="$2"
  local label="$3"

  mkdir -p "$stage_root"

  local batch_count=0
  local total_staged=0
  local total_new=0
  local batch_num=0

  while IFS= read -r src; do
    # Compute relative path and destination
    local rel="${src#"$src_root/"}"
    local dst="$stage_root/$rel"

    # Skip if already staged
    if [[ -f "$dst" ]]; then
      total_staged=$((total_staged + 1))
      continue
    fi

    mkdir -p "$(dirname "$dst")"
    ln "$src" "$dst"
    total_new=$((total_new + 1))
    total_staged=$((total_staged + 1))
    batch_count=$((batch_count + 1))

    # Index when batch is full
    if (( batch_count >= BATCH_SIZE )); then
      batch_num=$((batch_num + 1))
      index_batch "batch $batch_num: +$batch_count files (${total_staged} staged)"
      batch_count=0
    fi
  done

  # Index remaining files in the last partial batch
  if (( batch_count > 0 )); then
    batch_num=$((batch_num + 1))
    index_batch "batch $batch_num: +$batch_count files (${total_staged} staged)"
  fi

  if (( total_new == 0 )); then
    ok "$label: all $total_staged files already indexed — nothing to do"
  else
    ok "$label: $total_new new files indexed ($total_staged total staged)"
  fi
}

# ── Laws ──────────────────────────────────────────────────────────────────

cmd_laws() {
  clone_or_pull "$LAWS_REPO" "$LAWS_CLONE" "legalize-kr (laws)"

  local total
  total=$(find "$LAWS_CLONE/kr" -maxdepth 2 -name "법률.md" -type f | wc -l | tr -d ' ')
  log "Found $total 법률.md files"

  register_collection "laws" "$LAWS_STAGE"

  find "$LAWS_CLONE/kr" -maxdepth 2 -name "법률.md" -type f | sort \
    | batch_stage_and_index "$LAWS_CLONE" "$LAWS_STAGE" "laws"

  echo >&2
  zmd status >&2
}

# ── Precedents ────────────────────────────────────────────────────────────

cmd_precedents() {
  clone_or_pull "$PRECEDENT_REPO" "$PRECEDENT_CLONE" "precedent-kr (precedents)"

  register_collection "precedents" "$PRECEDENT_STAGE"

  for ct in "${PRECEDENT_CASE_TYPES[@]}"; do
    for court in "${PRECEDENT_COURTS[@]}"; do
      local src_dir="$PRECEDENT_CLONE/$ct/$court"
      if [[ ! -d "$src_dir" ]]; then
        warn "$ct/$court not found in repo — skipping"
        continue
      fi

      local court_total
      court_total=$(find "$src_dir" -maxdepth 1 -name "*.md" -type f | wc -l | tr -d ' ')
      log "$ct/$court: $court_total files"

      find "$src_dir" -maxdepth 1 -name "*.md" -type f | sort \
        | batch_stage_and_index "$PRECEDENT_CLONE" "$PRECEDENT_STAGE" "$ct/$court"

      echo >&2
    done
  done

  zmd status >&2
}

# ── All ───────────────────────────────────────────────────────────────────

cmd_all() {
  log "Phase 1/2: Laws (법률 only)"
  log "========================================="
  cmd_laws

  echo >&2
  log "Phase 2/2: Precedents (${PRECEDENT_CASE_TYPES[*]} / ${PRECEDENT_COURTS[*]})"
  log "========================================="
  cmd_precedents
}

# ── Sync ──────────────────────────────────────────────────────────────────

cmd_sync() {
  log "Syncing: pulling latest from upstream + re-indexing..."

  if [[ -d "$LAWS_CLONE/.git" ]]; then
    cmd_laws
  fi

  if [[ -d "$PRECEDENT_CLONE/.git" ]]; then
    cmd_precedents
  fi
}

# ── Status ────────────────────────────────────────────────────────────────

cmd_status() {
  echo "=== Cache ==="
  echo "  Cache dir: $CACHE_DIR"
  echo

  echo "=== Repos ==="
  if [[ -d "$LAWS_CLONE/.git" ]]; then
    echo "  laws: $(git -C "$LAWS_CLONE" log -1 --format='%h %s (%ci)' 2>/dev/null || echo 'unknown')"
  else
    echo "  laws: not cloned"
  fi
  if [[ -d "$PRECEDENT_CLONE/.git" ]]; then
    echo "  precedents: $(git -C "$PRECEDENT_CLONE" log -1 --format='%h %s (%ci)' 2>/dev/null || echo 'unknown')"
  else
    echo "  precedents: not cloned"
  fi
  echo

  echo "=== Staged Files ==="
  if [[ -d "$LAWS_STAGE" ]]; then
    echo "  laws: $(count_files "$LAWS_STAGE") files"
  else
    echo "  laws: not staged"
  fi
  if [[ -d "$PRECEDENT_STAGE" ]]; then
    for ct in "${PRECEDENT_CASE_TYPES[@]}"; do
      for court in "${PRECEDENT_COURTS[@]}"; do
        local d="$PRECEDENT_STAGE/$ct/$court"
        if [[ -d "$d" ]]; then
          echo "  precedents/$ct/$court: $(count_files "$d") files"
        fi
      done
    done
    echo "  precedents total: $(count_files "$PRECEDENT_STAGE") files"
  else
    echo "  precedents: not staged"
  fi
  echo

  echo "=== zmd ==="
  zmd collection list 2>/dev/null || echo "  No collections"
  echo
  zmd status 2>/dev/null || echo "  Database not initialized"
}

# ── Reset ─────────────────────────────────────────────────────────────────

cmd_reset() {
  warn "Removing all zmd collections and staged data..."

  for name in laws precedents; do
    if zmd collection list 2>/dev/null | grep -q "^  $name:"; then
      zmd collection remove "$name" 2>/dev/null || true
    fi
  done

  rm -rf "$STAGE_DIR"

  zmd cleanup 2>/dev/null || true
  zmd update 2>/dev/null || true

  ok "Reset complete. Repo clones preserved at $REPOS_DIR"
  ok "To also remove clones: rm -rf $CACHE_DIR"
}

# ── Main ───────────────────────────────────────────────────────────────────

main() {
  local cmd="${1:-all}"

  case "$cmd" in
    laws)       cmd_laws ;;
    precedents) cmd_precedents ;;
    all)        cmd_all ;;
    sync)       cmd_sync ;;
    status)     cmd_status ;;
    reset)      cmd_reset ;;
    -h|--help|help)
      cat <<'USAGE'
Usage: zmd-collections.sh [command]

Commands:
  all          Run all phases: laws then precedents (default)
  laws         Clone + stage + index laws (법률 only, ~1,711 docs)
  precedents   Clone + stage + index precedents (민사+형사 대법원, ~35K docs)
  sync         Pull latest from upstream repos + re-stage + re-index
  status       Show current state (repos, staged files, zmd collections)
  reset        Remove collections and staged data (keeps repo clones)
  help         Show this help

Configuration (env vars):
  ZMD_CACHE_DIR    Cache root (default: ~/.cache/legal-ko/zmd)
  ZMD_BATCH_SIZE   Files per zmd update call (default: 100)

Scope (edit arrays at top of script to expand):
  PRECEDENT_CASE_TYPES  Case types to include (default: 민사 형사)
  PRECEDENT_COURTS      Court levels to include (default: 대법원)

To add 하급심 (lower courts) later:
  1. Edit PRECEDENT_COURTS to add "하급심"
  2. Run: ./scripts/zmd-collections.sh precedents

To add more case types (세무, 일반행정, etc.):
  1. Edit PRECEDENT_CASE_TYPES to add the desired types
  2. Run: ./scripts/zmd-collections.sh precedents

Resumable: if interrupted, re-run the same command. Already-indexed
files are skipped automatically.
USAGE
      ;;
    *)
      die "Unknown command: $cmd (try 'help')"
      ;;
  esac
}

main "$@"
