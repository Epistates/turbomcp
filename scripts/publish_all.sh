#!/usr/bin/env bash
set -euo pipefail

# Compatibility entry point. The justfile owns the dependency order and gates.
# Dry-run checks metadata/file lists; it does not upload or claim registry builds.
cd "$(dirname "$0")/.."
if [[ $# -ne 0 ]]; then
  echo "Usage: DRY_RUN=true|false scripts/publish_all.sh (no positional arguments)" >&2
  exit 2
fi
case "${DRY_RUN:-true}" in
  true) exec just publish-check ;;
  false) exec env CONFIRM=yes just publish ;;
  *) echo "DRY_RUN must be true or false" >&2; exit 2 ;;
esac
