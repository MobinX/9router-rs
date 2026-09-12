#!/bin/sh
set -eu
echo "differential: boot original 9router + 9router-rs, replay fixtures, normalize ids/timestamps, diff (requires original installed)"
cargo test --workspace
