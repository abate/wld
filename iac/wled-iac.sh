#!/usr/bin/env bash
#
# wled-iac.sh — Infrastructure-as-Code workflow for WLED devices
#
# Usage:
#   ./wled-iac.sh init          Create config directory and snapshot all devices
#   ./wled-iac.sh snapshot      Re-export all device configs (overwrites local files)
#   ./wled-iac.sh diff          Show what changed between local files and live devices
#   ./wled-iac.sh apply         Push local configs to all devices
#   ./wled-iac.sh apply <name>  Push config to a single device
#
set -euo pipefail

CONFIG_DIR="${WLED_CONFIG_DIR:-./wled-config}"
WLD="${WLD_BIN:-wld}"

# Parse device names from ~/.wld.toml [devices] section
get_devices() {
    awk '/^\[devices\]/{found=1; next} /^\[/{found=0} found && /=/{print $1}' ~/.wld.toml
}

cmd_init() {
    mkdir -p "$CONFIG_DIR"/{devices,segments}

    echo "Snapshotting all devices into $CONFIG_DIR ..."
    for dev in $(get_devices); do
        echo "  $dev"
        $WLD config export -d "$dev" -o "$CONFIG_DIR/devices/${dev}.json"
        $WLD segment export -d "$dev" -o "$CONFIG_DIR/segments/${dev}.json"
    done

    if [ ! -d "$CONFIG_DIR/.git" ]; then
        git -C "$CONFIG_DIR" init -q
        git -C "$CONFIG_DIR" add -A
        git -C "$CONFIG_DIR" commit -q -m "Initial device snapshots"
        echo "Git repo initialized in $CONFIG_DIR"
    fi

    echo "Done. $(get_devices | wc -l) device(s) exported."
}

cmd_snapshot() {
    for dev in $(get_devices); do
        echo "  $dev"
        $WLD config export -d "$dev" -o "$CONFIG_DIR/devices/${dev}.json"
        $WLD segment export -d "$dev" -o "$CONFIG_DIR/segments/${dev}.json"
    done
    echo ""
    echo "Files updated. Review changes with:"
    echo "  git -C $CONFIG_DIR diff"
}

cmd_diff() {
    local has_diff=0
    for dev in $(get_devices); do
        local cfg="$CONFIG_DIR/devices/${dev}.json"
        if [ ! -f "$cfg" ]; then
            echo "[$dev] No local config — run 'init' or 'snapshot' first"
            continue
        fi
        echo "=== $dev ==="
        if $WLD config diff "$cfg" -d "$dev"; then
            echo "  (no differences)"
        else
            has_diff=1
        fi
        echo ""
    done
    return $has_diff
}

cmd_apply() {
    local targets
    if [ $# -gt 0 ]; then
        targets="$1"
    else
        targets=$(get_devices)
    fi

    for dev in $targets; do
        local cfg="$CONFIG_DIR/devices/${dev}.json"
        local seg="$CONFIG_DIR/segments/${dev}.json"

        if [ ! -f "$cfg" ]; then
            echo "[$dev] Skipping — no config file at $cfg"
            continue
        fi

        echo "=== $dev ==="

        # Show diff first
        $WLD config diff "$cfg" -d "$dev" && {
            echo "  (no config changes)"
        } || true

        echo "  Applying config..."
        $WLD config apply "$cfg" -d "$dev"

        if [ -f "$seg" ]; then
            echo "  Applying segments..."
            $WLD segment import "$seg" -d "$dev"
        fi

        echo "  Done."
        echo ""
    done
}

case "${1:-help}" in
    init)     cmd_init ;;
    snapshot) cmd_snapshot ;;
    diff)     cmd_diff ;;
    apply)    shift; cmd_apply "$@" ;;
    help|*)
        echo "Usage: $0 {init|snapshot|diff|apply [device]}"
        echo ""
        echo "  init              Export all devices and create git repo"
        echo "  snapshot          Re-export all devices (overwrite local files)"
        echo "  diff              Compare local files vs live device state"
        echo "  apply [device]    Push local configs to devices"
        echo ""
        echo "Set WLED_CONFIG_DIR to change config location (default: ./wled-config)"
        ;;
esac
