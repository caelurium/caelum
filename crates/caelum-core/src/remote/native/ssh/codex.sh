set -eu
PATH="$HOME/.local/bin:$HOME/.nix-profile/bin:$HOME/.opencode/bin:$HOME/.npm-global/bin:$HOME/.volta/bin:$HOME/.bun/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:$HOME/.asdf/shims:/opt/homebrew/bin:/usr/local/bin:/run/current-system/sw/bin:/usr/bin:/bin:$PATH"
for node_bin in "$HOME"/.nvm/versions/node/*/bin; do
    [ -d "$node_bin" ] && PATH="$node_bin:$PATH"
done
export PATH
if [ "${1:-}" = "--side" ]; then
    exec codex app-server --listen stdio://
fi
port=${CAELUM_CODEX_PORT:-}
case "$port" in ''|*[!0-9]*) [ -z "$port" ] || { echo "Invalid Codex port." >&2; exit 1; };; esac
if ! command -v codex >/dev/null 2>&1; then
    echo "Codex CLI was not found in the VM PATH." >&2
    exit 1
fi
if ! codex login status >/dev/null 2>&1; then
    echo "CAELUM_CODEX_AUTH_REQUIRED: Sign in to Codex on the VM with codex login." >&2
    exit 42
fi
state_root=${XDG_RUNTIME_DIR:-"$HOME/.cache"}
state_root="$state_root/caelum"
mkdir -p "$state_root"
chmod 700 "$state_root"
checkout_key=$(printf '%s' "$PWD" | cksum | cut -d ' ' -f 1)
state="$state_root/codex-$checkout_key"
mkdir -p "$state"
chmod 700 "$state"
lock="$state/lock"
i=0
while ! mkdir "$lock" 2>/dev/null; do
    old_pid=
    if [ -r "$lock/owner" ]; then
        read -r old_pid < "$lock/owner" || true
    fi
    if [ -n "$old_pid" ] && ! kill -0 "$old_pid" 2>/dev/null; then
        rm -rf "$lock"
        continue
    fi
    [ "$i" -lt 150 ] || { echo "Timed out waiting for the Codex launch lock." >&2; exit 1; }
    i=$((i + 1))
    sleep 0.1
done
trap 'rm -rf "$lock"' EXIT HUP INT TERM
printf '%s\n' "$$" > "$lock/owner"
pid_file="$state/server.pid"
port_file="$state/server.port"
log_file="$state/server.log"

identity_matches() {
    pid=$1
    listen_port=$2
    kill -0 "$pid" 2>/dev/null || return 1
    ps -p "$pid" -o args= 2>/dev/null |
        grep -F "codex app-server --listen ws://127.0.0.1:$listen_port" >/dev/null
}

ready() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsS --noproxy '*' --max-time 1 "http://127.0.0.1:$1/readyz" >/dev/null 2>&1
    elif command -v python3 >/dev/null 2>&1; then
        python3 -c 'import sys,urllib.request; urllib.request.urlopen("http://127.0.0.1:"+sys.argv[1]+"/readyz", timeout=1)' "$1" >/dev/null 2>&1
    else
        echo "Codex readiness requires curl or Python 3 on the VM." >&2
        return 1
    fi
}

if [ -r "$pid_file" ] && [ -r "$port_file" ]; then
    old_pid=$(cat "$pid_file")
    old_port=$(cat "$port_file")
    if { [ -z "$port" ] || [ "$old_port" = "$port" ]; } &&
        identity_matches "$old_pid" "$old_port" && ready "$old_port"; then
        printf '%s\n' "$old_port"
        exit 0
    fi
fi

if [ -z "$port" ]; then
    port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()') || {
        echo "Could not allocate a Codex listener port on the VM." >&2
        exit 1
    }
fi
case "$port" in ''|*[!0-9]*) echo "Invalid Codex port." >&2; exit 1;; esac
[ "$port" -gt 0 ] && [ "$port" -le 65535 ] || { echo "Invalid Codex port." >&2; exit 1; }

: > "$log_file"
nohup codex app-server --listen "ws://127.0.0.1:$port" </dev/null >>"$log_file" 2>&1 &
pid=$!
printf '%s\n' "$pid" > "$pid_file"
printf '%s\n' "$port" > "$port_file"
i=0
while [ "$i" -lt 100 ]; do
    if identity_matches "$pid" "$port" && ready "$port"; then
        printf '%s\n' "$port"
        exit 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
        break
    fi
    i=$((i + 1))
    sleep 0.2
done
kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
rm -f "$pid_file" "$port_file"
if [ -s "$log_file" ]; then
    tail -n 12 "$log_file" >&2
else
    echo "Codex app-server did not become ready on 127.0.0.1:$port." >&2
fi
exit 1
