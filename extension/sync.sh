#!/bin/sh
# The two manifests differ; their implementation is shared.
set -eu
cd "$(dirname "$0")"
for file in background.js options.html options.js; do
  cp "chromium/$file" "firefox/$file"
done
