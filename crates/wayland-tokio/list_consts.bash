#!/usr/bin/env bash

cmd=(
	"cargo" "test" "list_consts" "--" "--nocapture"
)

printf '$ %s \n' "${cmd[*]}"
"${cmd[@]}"
