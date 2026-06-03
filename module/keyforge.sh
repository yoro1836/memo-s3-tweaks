#!/system/bin/sh
# KeyForge – control script for WebUI and module management

MODDIR="${0%/*}"
BIN="$MODDIR/keyforge"
CONF="$MODDIR/keyforge.conf"
PIDFILE="$MODDIR/keyforge.pid"
LOG="$MODDIR/keyforge.log"
MANIFEST="/sdcard/.keyforge/manifest.json"
PLUGIN_DIR=/sdcard/.keyforge/plugins

log() { echo "[keyforge] $(date '+%H:%M:%S') $*" >> "$LOG"; }

is_running() {
    [ -f "$PIDFILE" ] || return 1
    _pid=""; read -r _pid < "$PIDFILE" 2>/dev/null || return 1
    [ -n "$_pid" ] && [ -d "/proc/$_pid" ] && return 0
    rm -f "$PIDFILE"; return 1
}

ensure_conf() {
    if [ ! -f "$CONF" ]; then
        cat > "$CONF" << EOF
# keyforge configuration
VID=0x045e
PID=0x028e
PLUGIN_DIR=/sdcard/.keyforge/plugins
EOF
    fi
}

case "${1:-}" in
    start)
        ensure_conf
        mkdir -p $MODDIR/plugins /sdcard/.keyforge/plugins /sdcard/.keyforge/configs 2>/dev/null
        if is_running; then echo "keyforge: already running (pid $(cat "$PIDFILE"))"; exit 0; fi
        if [ ! -f "$BIN" ]; then echo "keyforge: FATAL - binary not found: $BIN"; exit 1; fi
        chmod 755 "$BIN" 2>/dev/null || true
        log "starting daemon"
        nohup "$BIN" --config "$CONF" >> "$LOG" 2>&1 &
        _bpid=$!; echo "$_bpid" > "$PIDFILE"
        sleep 0.3
        if [ -d "/proc/$_bpid" ]; then
            log "started (pid=$_bpid)"; echo "keyforge: started (pid=$_bpid)"
        else
            rm -f "$PIDFILE"; log "died"; echo "keyforge: FAILED - see $LOG"; exit 1
        fi
        ;;

    stop)
        if is_running; then
            _pid=""; read -r _pid < "$PIDFILE" 2>/dev/null
            log "stopping (pid=$_pid)"; kill "$_pid" 2>/dev/null || true
            sleep 0.3; kill -9 "$_pid" 2>/dev/null || true
            rm -f "$PIDFILE"; echo "keyforge: stopped"
        else echo "keyforge: not running"; fi
        ;;

    restart) "$0" stop; sleep 0.5; "$0" start ;;

    status)
        if is_running; then echo "running pid=$(cat "$PIDFILE")"
        else echo "stopped"; fi
        ;;

    manifest)
        if [ -f "$MANIFEST" ]; then cat "$MANIFEST"; else echo '{"plugins":[]}'; fi
        ;;

    config)
        ensure_conf
        case "${2:-}" in
            batch) shift 2; for _pair in "$@"; do _key="${_pair%%=*}"; _value="${_pair#*=}"; if grep -q "^${_key}=" "$CONF" 2>/dev/null; then sed -i "s/^${_key}=.*/${_key}=${_value}/" "$CONF"; else echo "${_key}=${_value}" >> "$CONF"; fi; done; echo "keyforge: batch saved ${#} keys" ;;
            get) [ -n "${3:-}" ] && grep "^${3}=" "$CONF" 2>/dev/null | cut -d= -f2 || cat "$CONF" 2>/dev/null ;;
            set) _key="$3"; _value="$4"; if grep -q "^${_key}=" "$CONF" 2>/dev/null; then sed -i "s/^${_key}=.*/${_key}=${_value}/" "$CONF"; else echo "${_key}=${_value}" >> "$CONF"; fi; echo "keyforge: $_key = $_value" ;;
            *) cat "$CONF" 2>/dev/null ;;
        esac
        ;;

    devices)
        printf '{"devices":['
        _first=1
        getevent -i 2>/dev/null > /tmp/kf_devices.tmp
        _name=""; _vid=""; _pid=""; _handler=""
        while IFS= read -r line; do
            case "$line" in
                "add device"*) [ -n "$_handler" ] && [ -n "$_name" ] && { [ "$_first" = "1" ] && _first=0 || printf ','; printf '{"name":"%s","vid":"%s","pid":"%s","handler":"%s"}' "$_name" "${_vid:-0x0000}" "${_pid:-0x0000}" "$_handler"; }; _handler="${line##* }"; _handler="${_handler##*/}"; _name=""; _vid=""; _pid="" ;;
                *name:*) _name="${line#*\"}"; _name="${_name%\"*}" ;;
                *vendor*) _vid="${line##* }"; case "$_vid" in 0x*) ;; *) _vid="0x${_vid}" ;; esac ;;
                *product*) _pid="${line##* }"; case "$_pid" in 0x*) ;; *) _pid="0x${_pid}" ;; esac ;;
            esac
        done < /tmp/kf_devices.tmp
        [ -n "$_handler" ] && [ -n "$_name" ] && { [ "$_first" = "1" ] && _first=0 || printf ','; printf '{"name":"%s","vid":"%s","pid":"%s","handler":"%s"}' "$_name" "${_vid:-0x0000}" "${_pid:-0x0000}" "$_handler"; }
        rm -f /tmp/kf_devices.tmp
        printf ']}\n'
        ;;

    plugins)
        mkdir -p "$PLUGIN_DIR" 2>/dev/null
        case "${2:-}" in
            list) printf '{"plugins":['; _first=1; for f in "$PLUGIN_DIR"/*.lua; do [ -f "$f" ] || continue; _bn=$(basename "$f" .lua); [ "$_first" = "1" ] && _first=0 || printf ','; printf '"%s"' "$_bn"; done; printf ']}\n' ;;
            install) cp "${3}" "$PLUGIN_DIR/$(basename "${3}")" 2>/dev/null && echo "keyforge: installed $(basename "${3}")" || echo "keyforge: install failed" ;;
            remove) rm -f "$PLUGIN_DIR/${3}.lua" 2>/dev/null && echo "keyforge: removed ${3}" ;;
            enable|disable) _val="$([ "$2" = "enable" ] && echo 1 || echo 0)"; "$0" config set "plugin.${3}" "$_val"; "$0" restart ;;
            *) echo "usage: keyforge.sh plugins {list|install|remove|enable|disable} ..." ;;
        esac
        ;;

    calibrate)
        case "${2:-}" in
            left) [ -f /tmp/keyforge_raw_L ] && read -r cx cy < /tmp/keyforge_raw_L 2>/dev/null; [ -n "$cx" ] && "$0" config set calib_lx "$cx" && "$0" config set calib_ly "$cy" && echo "keyforge: calibrate left x=$cx y=$cy" ;;
            right) [ -f /tmp/keyforge_raw_R ] && read -r cx cy < /tmp/keyforge_raw_R 2>/dev/null; [ -n "$cx" ] && "$0" config set calib_rx "$cx" && "$0" config set calib_ry "$cy" && echo "keyforge: calibrate right x=$cx y=$cy" ;;
            reset) "$0" config set calib_lx 0; "$0" config set calib_ly 0; "$0" config set calib_rx 0; "$0" config set calib_ry 0; echo "keyforge: calibration reset" ;;
        esac
        ;;

    log) tail -20 "$LOG" 2>/dev/null ;;

    *) echo "usage: keyforge.sh {start|stop|restart|status|manifest|config|devices|plugins|calibrate|log}" ;;
esac
