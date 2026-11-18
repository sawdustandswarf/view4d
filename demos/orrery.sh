#!/usr/bin/env bash
# Demo: the planets moving, a week a second, in a pane of this termmd.
# Prints the command it runs, starts it in the background and returns; PLACE
# gives other placement keys, DURATION stops it after that many seconds.
set -euo pipefail
cmd=(view4d --orrery --speed 4 --focus=no --place "${PLACE:-fixed=top-right w=50% h=50%}")
printf '\e[1m$ %s\e[0m\n' "${cmd[*]}"
"${cmd[@]}" &
if [[ -n ${DURATION:-} ]]; then
    pid=$!
    (sleep "$DURATION"; kill "$pid" 2>/dev/null) &
fi
