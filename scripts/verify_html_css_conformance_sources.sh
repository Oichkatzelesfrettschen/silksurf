#!/bin/sh
# Verify the retained HTML and CSS conformance source bundle.

set -eu

bundle_dir="${1:-docs/external_sources/html_css_conformance_2026-07-02}"
manifest="$bundle_dir/SHA256SUMS"

if [ ! -f "$manifest" ]; then
    echo "missing checksum manifest: $manifest" >&2
    exit 1
fi

(
    cd "$bundle_dir"
    sha256sum -c SHA256SUMS
)
