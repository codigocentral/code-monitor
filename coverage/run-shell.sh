#!/bin/bash
set -euo pipefail
source coverage/environment.sh
export UNIT_SHELL_OUTPUT="${1:?absolute evidence path}"
bash_binary="$campaign/cache/session-13/bash/bin/bash"
python3 coverage/record-command.py "$UNIT_SHELL_OUTPUT" ruby coverage/bashcov-adapter.rb --root "$PWD" --bash-path "$bash_binary" tests/shell/unit.sh
python3 coverage/shell-evidence.py "$UNIT_SHELL_OUTPUT"
