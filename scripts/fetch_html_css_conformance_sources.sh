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
fetch_source css2-1998.pdf https://www.w3.org/TR/1998/REC-CSS2-19980512/css2.pdf
fetch_source html40.pdf https://www.w3.org/TR/1998/REC-html40-19980424/html40.pdf
fetch_source html401.pdf.gz https://www.w3.org/TR/1999/REC-html401-19991224/html40.pdf.gz
fetch_source html-living-standard.html https://html.spec.whatwg.org/multipage/
fetch_source css-syntax-3.html https://www.w3.org/TR/css-syntax-3/
fetch_source selectors-4.html https://www.w3.org/TR/selectors-4/
fetch_source css-cascade-5.html https://www.w3.org/TR/css-cascade-5/
fetch_source css-values-4.html https://www.w3.org/TR/css-values-4/
fetch_source css-color-4.html https://www.w3.org/TR/css-color-4/
fetch_source css-backgrounds-3.html https://www.w3.org/TR/css-backgrounds-3/
fetch_source css-flexbox-1.html https://www.w3.org/TR/css-flexbox-1/
fetch_source css-display-3.html https://www.w3.org/TR/css-display-3/
fetch_source css-text-3.html https://www.w3.org/TR/css-text-3/
fetch_source cssom-1.html https://www.w3.org/TR/cssom-1/
fetch_source cssom-view-1.html https://www.w3.org/TR/cssom-view-1/
fetch_source css-sizing-3.html https://www.w3.org/TR/css-sizing-3/
fetch_source css-overflow-3.html https://www.w3.org/TR/css-overflow-3/
fetch_source css-position-3.html https://www.w3.org/TR/css-position-3/
fetch_source css-align-3.html https://www.w3.org/TR/css-align-3/
fetch_source css-fonts-4.html https://www.w3.org/TR/css-fonts-4/
fetch_source css-images-3.html https://www.w3.org/TR/css-images-3/

for html_file in "$tmp_dir"/*.html
do
    [ -e "$html_file" ] || continue
    sed -i 's/\r$//' "$html_file"
done

for file in \
    css21.pdf \
    css22.pdf \
    css2-1998.pdf \
    html40.pdf \
    html401.pdf.gz \
    html-living-standard.html \
    css-syntax-3.html \
    selectors-4.html \
    css-cascade-5.html \
    css-values-4.html \
    css-color-4.html \
    css-backgrounds-3.html \
    css-flexbox-1.html \
    css-display-3.html \
    css-text-3.html \
    cssom-1.html \
    cssom-view-1.html \
    css-sizing-3.html \
    css-overflow-3.html \
    css-position-3.html \
    css-align-3.html \
    css-fonts-4.html \
    css-images-3.html
do
    mv "$tmp_dir/$file" "$bundle_dir/$file"
done

(
    cd "$bundle_dir"
    sha256sum \
        css21.pdf \
        css22.pdf \
        css2-1998.pdf \
        html40.pdf \
        html401.pdf.gz \
        html-living-standard.html \
        css-syntax-3.html \
        selectors-4.html \
        css-cascade-5.html \
        css-values-4.html \
        css-color-4.html \
        css-backgrounds-3.html \
        css-flexbox-1.html \
        css-display-3.html \
        css-text-3.html \
        cssom-1.html \
        cssom-view-1.html \
        css-sizing-3.html \
        css-overflow-3.html \
        css-position-3.html \
        css-align-3.html \
        css-fonts-4.html \
        css-images-3.html >SHA256SUMS
    sha256sum -c SHA256SUMS
)
