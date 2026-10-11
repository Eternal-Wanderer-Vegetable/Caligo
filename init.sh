#!/usr/bin/env bash
set -euo pipefail

# Windows Git Bash entrypoint; verification is implemented once in init.ps1.
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if command -v pwsh >/dev/null 2>&1; then
    exec pwsh -NoProfile -File "$repo_root/init.ps1" "$@"
fi
exec powershell.exe -NoProfile -File "$repo_root/init.ps1" "$@"
