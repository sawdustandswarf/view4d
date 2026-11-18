#!/usr/bin/env bash
# Demo: a model that follows its file. A copy of hinge.prim is shown, then
# rewritten every 1.5 seconds with another hinge length; view4d reads it
# again each time. Prints each call, starts in the background and returns;
# PLACE gives other placement keys, DURATION stops it (and removes the copy)
# after that many seconds.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
dir=$(mktemp -d)
cp "$here/hinge.prim" "$dir/hinge.prim"
cd "$dir"
cmd=(view4d hinge.prim --focus=no --place "${PLACE:-fixed=bottom-right w=50% h=50%}")
printf '\e[1m$ %s\e[0m\n' "${cmd[*]}"
"${cmd[@]}" &
pid=$!
# Rewrite the file's hinge_length while view4d runs; then tidy up.
(
    lengths=(60 25 45 40)
    i=0
    while kill -0 "$pid" 2>/dev/null; do
        sleep 1.5
        kill -0 "$pid" 2>/dev/null || break
        n=${lengths[i++ % ${#lengths[@]}]}
        printf '\e[1m$ # hinge.prim rewritten: hinge_length = %s\e[0m\n' "$n"
        python3 -c '
import json, sys
p, n = sys.argv[1], sys.argv[2]
d = json.load(open(p))
for q in d["params"]:
    if q["name"] == "hinge_length":
        q["default"] = n
json.dump(d, open(p + ".new", "w"), indent=1)
' hinge.prim "$n" && mv hinge.prim.new hinge.prim
    done
    rm -rf "$dir"
) &
if [[ -n ${DURATION:-} ]]; then
    (sleep "$DURATION"; kill "$pid" 2>/dev/null) &
fi
