#!/usr/bin/env bash
# Unified build script for Radiumical workspace.
#
# Usage:
#   ./build.sh                          # dev profile, all packages
#   ./build.sh -p release               # release profile
#   ./build.sh -p release-small -t x86_64-unknown-linux-gnu
#   ./build.sh --package tui,rpc        # specific packages
#   ./build.sh --clean                  # clean before build

set -euo pipefail

PROFILE="dev"
TARGET=""
PACKAGES="all"
CLEAN=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -p|--profile)   PROFILE="$2"; shift 2 ;;
        -t|--target)    TARGET="$2"; shift 2 ;;
        --package)      PACKAGES="$2"; shift 2 ;;
        --clean)        CLEAN=true; shift ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Resolve packages
if [[ "$PACKAGES" == "all" ]]; then
    PKG_LIST=(tui rpc tauri)
else
    IFS=',' read -ra PKG_LIST <<< "$PACKAGES"
fi

# Profile dir
case "$PROFILE" in
    dev)            PROFILE_DIR="debug" ;;
    release)        PROFILE_DIR="release" ;;
    release-small)  PROFILE_DIR="release-small" ;;
    *) echo "Invalid profile: $PROFILE"; exit 1 ;;
esac

# Target
TARGET_FLAG=""
TARGET_SUBDIR=""
if [[ -n "$TARGET" ]]; then
    TARGET_FLAG="--target $TARGET"
    TARGET_SUBDIR="$TARGET"
fi

# Output root
if [[ -n "$TARGET_SUBDIR" ]]; then
    OUT_ROOT="target/$TARGET_SUBDIR/$PROFILE_DIR"
else
    OUT_ROOT="target/$PROFILE_DIR"
fi

# Crate map
declare -A CRATE_MAP=(
    [tui]="radiumical"
    [rpc]="radiumical-rpc"
    [tauri]="radiumical-tauri"
)

# Clean
if $CLEAN; then
    for pkg in "${PKG_LIST[@]}"; do
        OUT_DIR="$OUT_ROOT/$pkg"
        if [[ -d "$OUT_DIR" ]]; then
            echo "Cleaning $OUT_DIR"
            rm -rf "$OUT_DIR"
        fi
    done
fi

# Build
CRATE_ARGS=""
for pkg in "${PKG_LIST[@]}"; do
    CRATE_ARGS="$CRATE_ARGS --package ${CRATE_MAP[$pkg]}"
done

PROFILE_ARG=""
if [[ "$PROFILE" != "dev" ]]; then
    PROFILE_ARG="--profile $PROFILE"
fi

echo ""
echo "▸ cargo build $PROFILE_ARG $TARGET_FLAG $CRATE_ARGS"
cargo build $PROFILE_ARG $TARGET_FLAG $CRATE_ARGS

# Copy binaries
for pkg in "${PKG_LIST[@]}"; do
    BIN="${CRATE_MAP[$pkg]}"
    SRC="$OUT_ROOT/$BIN"
    DST_DIR="$OUT_ROOT/$pkg"
    DST="$DST_DIR/$BIN"

    if [[ ! -f "$SRC" ]]; then
        echo "⚠ Binary not found: $SRC"
        continue
    fi

    mkdir -p "$DST_DIR"
    cp "$SRC" "$DST"
    SIZE=$(du -h "$DST" | cut -f1)
    echo "✔ $pkg → $DST ($SIZE)"
done

echo ""
echo "Done. Output in: $OUT_ROOT/{tui,rpc,tauri}/"
