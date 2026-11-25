#!/usr/bin/env bash
# Downloads the public photos that `crates/wain-vision/eval/walls.txt` marks
# up, for scoring photo detection:
#
#   scripts/fetch-eval-photos.sh
#   cargo run --release -p wain-vision --example evaluate -- score
#
# The photos come from Wikimedia Commons, 1280 pixels wide, into
# crates/wain-vision/eval/photos (git-ignored). Ones already there are kept.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
eval_dir="$root/crates/wain-vision/eval"
mkdir -p "$eval_dir/photos"

grep '^photo ' "$eval_dir/walls.txt" | while read -r _ id title; do
	out="$eval_dir/photos/$id.jpg"
	if [ -s "$out" ]; then
		continue
	fi
	echo "$id: $title"
	# Special:FilePath redirects to the image itself, scaled to the width asked for.
	curl --fail --silent --show-error --location --get \
		--user-agent "wain-eval-photos/1.0" \
		--data-urlencode "width=1280" \
		--output "$out" \
		"https://commons.wikimedia.org/wiki/Special:FilePath/$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$title")"
	# Be gentle with the server.
	sleep 1
done
