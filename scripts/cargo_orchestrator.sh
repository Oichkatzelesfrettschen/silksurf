#!/bin/bash
# SilkSurf Cargo Cache Orchestrator
# Goal: Maintain <30GB target directory while preserving sub-500ms startup potential.

MAX_TARGET_GB=30
TARGET_DIR="target"
CARGO_BIN=$(which cargo)

function check_bloat() {
    SIZE_GB=$(du -s "$TARGET_DIR" | cut -f1 | awk '{print int($1/1024/1024)}')
    echo "Current Target Size: ${SIZE_GB}GB"
    if [ "$SIZE_GB" -gt "$MAX_TARGET_GB" ]; then
        echo "⚠️ Target directory exceeds ${MAX_TARGET_GB}GB limit. Running garbage collection..."
        $CARGO_BIN clean gc --max-download-size=5GiB
        $CARGO_BIN clean gc --max-src-age=14days
        # If still too large, clean old debug artifacts (the primary bloat source)
        if [ "$(du -s "$TARGET_DIR" | cut -f1 | awk '{print int($1/1024/1024)}')" -gt "$MAX_TARGET_GB" ]; then
            echo "🔥 Deep cleaning debug artifacts..."
            # Remove only artifacts older than 7 days
            find "$TARGET_DIR/debug" -atime +7 -type f -delete 2>/dev/null
        fi
    fi
}

function optimize_tmp_build() {
    # If /tmp is a tmpfs (RAM disk), use it for ultra-fast, ephemeral builds
    if mount | grep -q "on /tmp type tmpfs"; then
        echo "🚀 Detected RAM disk at /tmp. Redirecting ephemeral build artifacts..."
        export CARGO_TARGET_DIR="/tmp/silksurf-target"
        mkdir -p "$CARGO_TARGET_DIR"
    fi
}

case "$1" in
    "gc")
        check_bloat
        ;;
    "fast")
        optimize_tmp_build
        $CARGO_BIN build --quiet
        ;;
    *)
        echo "Usage: $0 {gc|fast}"
        exit 1
        ;;
esac
