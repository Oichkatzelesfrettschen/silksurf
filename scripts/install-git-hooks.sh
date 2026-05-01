#!/usr/bin/env bash
# Install silksurf git hooks: pre-commit (fast gate) + pre-push (full gate).
#
# WHY: cloud CI is intentionally disabled (ADR-009). The local-gate is the
# merge gate, and these hooks make running it the default path -- failures
# are caught before commits/pushes leave the working tree.
#
# WHAT: symlinks .git/hooks/pre-commit and .git/hooks/pre-push at the
# versioned scripts under scripts/hooks/. Idempotent.
#
# HOW: scripts/install-git-hooks.sh
#      scripts/install-git-hooks.sh --force   # overwrite existing hooks

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if [ ! -d ".git/hooks" ]; then
    echo "ERROR: $REPO_ROOT is not a git checkout (no .git/hooks)." >&2
    exit 1
fi

FORCE=0
if [ "${1:-}" = "--force" ]; then
    FORCE=1
fi

install_hook() {
    local name="$1"
    local src="scripts/hooks/${name}"
    local dst=".git/hooks/${name}"

    if [ ! -x "${src}" ]; then
        echo "ERROR: ${src} missing or not executable." >&2
        exit 1
    fi

    if [ -e "${dst}" ] || [ -L "${dst}" ]; then
        if [ "${FORCE}" = "1" ]; then
            rm -f "${dst}"
        else
            # If already pointing at our script, leave it alone.
            if [ "$(readlink -f "${dst}" 2>/dev/null || true)" = "$(readlink -f "${src}")" ]; then
                echo "OK: .git/hooks/${name} already points at ${src}"
                return
            fi
            echo "WARN: .git/hooks/${name} exists and is not our script."
            echo "      Use --force to overwrite, or install manually."
            return
        fi
    fi

    ln -s "../../${src}" "${dst}"
    echo "OK: installed .git/hooks/${name} -> ${src}"
}

install_hook pre-commit
install_hook pre-push

echo
echo "Hooks installed. They will run on every git commit and git push."
echo "To bypass once (rare; document why in the commit): git commit --no-verify"
