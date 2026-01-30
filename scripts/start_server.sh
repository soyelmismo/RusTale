#!/bin/sh

SCRIPTPATH=$(readlink -f "$0")
DIR=$(dirname "$SCRIPTPATH")
EXE="./rustale"
ARGS="--dedicated-server --online-mode=local"

FINAL_CMD="cd '$DIR' && $EXE $ARGS"

launch_terminal() {
    for term in "$TERMINAL" x-terminal-emulator konsole gnome-terminal xfce4-terminal kitty alacritty terminator xterm; do
        if command -v "$term" >/dev/null 2>&1; then
            case "$term" in
                konsole)
                    exec konsole -e /bin/sh -c "$FINAL_CMD"
                    ;;
                gnome-terminal|xfce4-terminal|terminator)
                    exec "$term" -- /bin/sh -c "$FINAL_CMD"
                    ;;
                *)
                    exec "$term" -e /bin/sh -c "$FINAL_CMD"
                    ;;
            esac
        fi
    done

    echo "Error: could not find a terminal emulator."
    exit 1
}

if [ ! -t 0 ]; then
    launch_terminal
else
    cd "$DIR" && $EXE $ARGS
fi
