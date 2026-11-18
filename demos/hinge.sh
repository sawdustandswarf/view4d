#!/usr/bin/env bash
# Demo: a hinge opening and closing every three seconds while the eye goes
# round it every nine, in a pane of this termmd. hinge.prim is the hinge of
# primlib's hinged_lid assembly alone: its two leaves and the joint.
# Prints the command it runs, starts it in the background and returns; PLACE
# gives other placement keys, DURATION stops it after that many seconds.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
cmd=(view4d hinge.prim --animate open_angle=0:-150:3 --orbit 9
    --focus=no --place "${PLACE:-fixed=bottom-right w=50% h=50%}")
printf '\e[1m$ %s\e[0m\n' "${cmd[*]}"
"${cmd[@]}" &
if [[ -n ${DURATION:-} ]]; then
    pid=$!
    (sleep "$DURATION"; kill "$pid" 2>/dev/null) &
fi
