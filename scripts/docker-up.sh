#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${PROJECT_ROOT}"

docker compose build
docker compose up -d dev
(docker exec fujihime-re rea doctor --json || true) | jq -e '
  {
    cli_package_version: .identity.cli_package_version,
    node: (.checks[] | select(.name=="node")),
    ghidra: (.providerInspections[] | select(.id=="ghidra") | {available, providerVersion, checks})
  }
  | select(.node.ok == true and .ghidra.available == true)
'
