#!/bin/sh
# Fetch primary HTML and CSS conformance sources into the retained bundle.

set -eu

bundle_dir="${1:-docs/external_sources/html_css_conformance_2026-07-02}"
ua="${SILKSURF_WGET_UA:-Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0}"
tmp_dir="$(mktemp -d)"

cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

fetch_source() {
    file="$1"
    url="$2"
    wget --user-agent="$ua" -O "$tmp_dir/$file" "$url"
}

mkdir -p "$bundle_dir"

fetch_source css21.pdf https://www.w3.org/TR/CSS21/css2.pdf
fetch_source css22.pdf https://www.w3.org/TR/CSS22/css2.pdf
fetch_source html40.pdf https://www.w3.org/TR/1998/REC-html40-19980424/html40.pdf
fetch_source html-living-standard.html https://html.spec.whatwg.org/multipage/

for file in css21.pdf css22.pdf html40.pdf html-living-standard.html; do
    mv "$tmp_dir/$file" "$bundle_dir/$file"
done

(
    cd "$bundle_dir"
    sha256sum css21.pdf css22.pdf html40.pdf html-living-standard.html >SHA256SUMS
    sha256sum -c SHA256SUMS
)
