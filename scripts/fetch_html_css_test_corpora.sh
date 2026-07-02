#!/bin/sh
# fetch_html_css_test_corpora.sh keeps external HTML/CSS test corpora in the ignored extras tree.

set -eu

extras_dir="${1:-silksurf-extras}"
html5lib_dir="$extras_dir/html5lib-tests"
wpt_dir="$extras_dir/wpt-css-parser-subset"

clone_or_update_branch() {
    target_dir="$1"
    repo_url="$2"
    branch="$3"
    if [ -d "$target_dir/.git" ]; then
        git -C "$target_dir" fetch --depth 1 origin "$branch"
        git -C "$target_dir" checkout --detach FETCH_HEAD
        return
    fi
    rm -rf "$target_dir"
    git clone --depth 1 --branch "$branch" "$repo_url" "$target_dir"
}

clone_or_update_wpt_subset() {
    target_dir="$1"
    repo_url="$2"
    branch="$3"
    if [ -d "$target_dir/.git" ]; then
        git -C "$target_dir" fetch --depth 1 origin "$branch"
        git -C "$target_dir" checkout --detach FETCH_HEAD
    else
        rm -rf "$target_dir"
        git clone --depth 1 --filter=blob:none --sparse --branch "$branch" "$repo_url" "$target_dir"
    fi
    git -C "$target_dir" sparse-checkout set \
        css/CSS2/syntax \
        css/css-syntax \
        css/selectors/parsing
}

mkdir -p "$extras_dir"

clone_or_update_branch \
    "$html5lib_dir" \
    https://github.com/html5lib/html5lib-tests.git \
    master

clone_or_update_wpt_subset \
    "$wpt_dir" \
    https://github.com/web-platform-tests/wpt.git \
    master

{
    echo "html5lib-tests $(git -C "$html5lib_dir" rev-parse HEAD) https://github.com/html5lib/html5lib-tests.git master"
    echo "wpt-css-parser-subset $(git -C "$wpt_dir" rev-parse HEAD) https://github.com/web-platform-tests/wpt.git master"
} >"$extras_dir/html-css-test-corpora-revisions.txt"

printf '%s\n' "HTML/CSS test corpora fetched under $extras_dir"
