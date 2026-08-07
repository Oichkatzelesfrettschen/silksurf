#!/usr/bin/env bash
# install-git-hooks: point the pre-commit and pre-push slots at the local gate.
#
# AD-009 keeps cloud CI off, so scripts/local_gate.sh is the only merge gate and
# these hooks are what make it run without being remembered. Installation
# symlinks the git hook slot at the versioned script under scripts/hooks/ and is
# idempotent.
#
# Git runs exactly one script per hook name. A slot already holding a foreign
# hook -- git-lfs writes pre-push, post-checkout, post-commit, and post-merge --
# moves to <name>.local, and the installed gate hook runs it first with stdin
# and arguments intact. Preserving rather than replacing keeps an LFS checkout
# pushing its objects.
#
#   scripts/install-git-hooks.sh
#   scripts/install-git-hooks.sh --force   # discard the occupying hook instead

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# core.hooksPath moves the slot away from .git/hooks, so git reports the
# directory rather than the layout being assumed.
HOOK_DIR="$(git rev-parse --git-path hooks)"

if [ ! -d "${HOOK_DIR}" ]; then
    echo "ERROR: $REPO_ROOT has no git hook directory at ${HOOK_DIR}." >&2
    exit 1
fi

FORCE=0
if [ "${1:-}" = "--force" ]; then
    FORCE=1
fi

install_hook() {
    local name="$1"
    local src="scripts/hooks/${name}"
    local dst="${HOOK_DIR}/${name}"

    if [ ! -x "${src}" ]; then
        echo "ERROR: ${src} missing or not executable." >&2
        exit 1
    fi

    if [ -e "${dst}" ] || [ -L "${dst}" ]; then
        if [ "$(readlink -f "${dst}" 2>/dev/null || true)" = "$(readlink -f "${src}")" ]; then
            echo "OK: ${dst} already points at ${src}"
            return
        fi
        if [ "${FORCE}" = "1" ]; then
            rm -f "${dst}"
        elif [ -e "${dst}.local" ] || [ -L "${dst}.local" ]; then
            # Two foreign hooks and one chain slot. Choosing between them
            # silently would drop whichever loses.
            echo "ERROR: ${dst} holds a foreign hook and ${dst}.local is taken." >&2
            echo "       Merge them into ${dst}.local, or re-run with --force to" >&2
            echo "       discard ${dst}." >&2
            exit 1
        else
            mv "${dst}" "${dst}.local"
            chmod +x "${dst}.local"
            echo "OK: preserved the existing ${name} as ${name}.local"
        fi
    fi

    # core.hooksPath and linked worktrees both put the slot at a depth a fixed
    # relative prefix guesses wrong, so the link target is absolute. It lives
    # under .git and is never committed.
    ln -s "${REPO_ROOT}/${src}" "${dst}"
    echo "OK: installed ${dst} -> ${src}"
}

install_hook pre-commit
install_hook pre-push

echo
echo "Hooks installed. make check runs on every commit, make full on every push."
echo "To bypass once (rare; document why in the commit): git commit --no-verify"
