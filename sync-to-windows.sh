#!/usr/bin/env bash
# Incremental sync of this repo to the Windows mount (WSL → /mnt/c/...).
# Uses rsync so unchanged files are skipped.
#
# Usage:
#   ./sync-to-windows.sh              # sync to default dest
#   ./sync-to-windows.sh /mnt/c/...   # sync to a custom dest
#   ./sync-to-windows.sh --dry-run    # show what would change
#   ./sync-to-windows.sh --delete     # also remove dest files missing from source
#
# Env:
#   SYNC_DEST   Override default destination

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="${SYNC_DEST:-/mnt/c/Users/tjhit/aion2}"

DRY_RUN=()
DELETE=()
EXTRA=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run|-n)
      DRY_RUN=(--dry-run)
      shift
      ;;
    --delete)
      DELETE=(--delete)
      shift
      ;;
    -h|--help)
      sed -n '2,14p' "$0"
      exit 0
      ;;
    --)
      shift
      EXTRA+=("$@")
      break
      ;;
    -*)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
    *)
      DEST="$1"
      shift
      ;;
  esac
done

if ! command -v rsync >/dev/null 2>&1; then
  echo "rsync is required but not installed." >&2
  exit 1
fi

if [[ ! -d "$DEST" ]]; then
  echo "Creating destination: $DEST"
  mkdir -p "$DEST"
fi

# Archive mode + human sizes. --modify-window=1 helps NTFS/drvfs timestamp skew.
# Excludes regenerable / platform-local artifacts so Windows can keep its own
# node_modules and Rust target/.
rsync -avh \
  --modify-window=1 \
  --info=stats1,progress2 \
  "${DRY_RUN[@]}" \
  "${DELETE[@]}" \
  --exclude '.git/' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'dist/' \
  --exclude 'dist-ssr/' \
  --exclude '*.log' \
  --exclude '.DS_Store' \
  "${EXTRA[@]}" \
  "$ROOT/" \
  "$DEST/"

echo
echo "Synced: $ROOT/ → $DEST/"
