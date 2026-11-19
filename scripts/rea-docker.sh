#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "$(docker inspect -f '{{.State.Running}}' fujihime-re 2>/dev/null || true)" != "true" ]]; then
  docker compose -f "${PROJECT_ROOT}/docker-compose.yml" up -d dev >&2
fi

if [[ -t 0 && -t 1 ]]; then
  exec docker exec -it fujihime-re rea "$@"
else
  exec docker exec -i fujihime-re rea "$@"
fi
