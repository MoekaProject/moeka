#!/usr/bin/env bash
# Real localhost SSH integration using only disposable keys, HOME and config.
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp_base=${TMPDIR:-/tmp}
tmp_base=${tmp_base%/}
fixture=$(mktemp -d "$tmp_base/moeka-loopback.XXXXXX")
sshd_pid=

cleanup() {
    if [[ -n "$sshd_pid" ]] && kill -0 "$sshd_pid" 2>/dev/null; then
        kill "$sshd_pid"
        wait "$sshd_pid" 2>/dev/null || true
    fi
    case "$fixture" in
        "$tmp_base"/moeka-loopback.*) rm -rf -- "$fixture";;
        *) echo "Refusing to remove unexpected loopback fixture: $fixture" >&2;;
    esac
}
trap cleanup EXIT INT TERM

for command in cargo python3 ssh ssh-keygen ssh-keyscan; do
    command -v "$command" >/dev/null || {
        echo "$command is required for the loopback SSH tests" >&2
        exit 1
    }
done
sshd=$(command -v sshd 2>/dev/null || true)
if [[ -z "$sshd" && -x /usr/sbin/sshd ]]; then
    sshd=/usr/sbin/sshd
fi
[[ -n "$sshd" ]] || { echo "sshd is required for the loopback SSH tests" >&2; exit 1; }

mkdir -p "$fixture/home" "$fixture/run" "$fixture/test-config/config" "$fixture/test-config/cache"
chmod 700 "$fixture" "$fixture/home"
ssh-keygen -q -t ed25519 -N '' -f "$fixture/host_key"
ssh-keygen -q -t ed25519 -N '' -f "$fixture/client_key"
port=$(python3 - <<'PY'
import socket
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    print(listener.getsockname()[1])
PY
)
user=$(id -un)

cat >"$fixture/forced-command.sh" <<EOF
#!/bin/sh
export HOME='$fixture/home'
exec /bin/sh -c "\$SSH_ORIGINAL_COMMAND"
EOF
chmod 755 "$fixture/forced-command.sh"
printf 'command="%s",no-agent-forwarding,no-port-forwarding,no-pty,no-user-rc,no-X11-forwarding %s\n' \
    "$fixture/forced-command.sh" "$(awk '{print $1, $2}' "$fixture/client_key.pub")" \
    >"$fixture/authorized_keys"

cat >"$fixture/sshd_config" <<EOF
Port $port
ListenAddress 127.0.0.1
HostKey $fixture/host_key
PidFile $fixture/run/sshd.pid
AuthorizedKeysFile $fixture/authorized_keys
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
PermitRootLogin no
UsePAM no
StrictModes no
LogLevel ERROR
AllowUsers $user
EOF
cat >"$fixture/client_config" <<EOF
Host moeka-loopback-isolated
    HostName 127.0.0.1
    Port $port
    User $user
    IdentityFile $fixture/client_key
    IdentitiesOnly yes
    UserKnownHostsFile $fixture/known_hosts
EOF
chmod 600 "$fixture"/*_key "$fixture/authorized_keys" "$fixture/sshd_config" "$fixture/client_config"

"$sshd" -t -f "$fixture/sshd_config"
"$sshd" -D -e -f "$fixture/sshd_config" &
sshd_pid=$!
for _ in 1 2 3 4 5; do
    if ssh-keyscan -p "$port" 127.0.0.1 >"$fixture/known_hosts" 2>/dev/null; then
        break
    fi
    sleep 1
done
ssh -F "$fixture/client_config" -o BatchMode=yes -o StrictHostKeyChecking=yes \
    moeka-loopback-isolated 'test "$HOME" = '"'"$fixture/home"'"''

cd "$root_dir"
cargo build --release --locked -p moeka-remote-server
cargo build --release --locked
bundle=$(find target/release/build -path '*/out/servers.mrb' -type f -exec ls -t {} + | head -1)
[[ -n "$bundle" ]] || { echo "Local development bundle was not generated" >&2; exit 1; }

export XDG_CONFIG_HOME="$fixture/test-config/config"
export XDG_CACHE_HOME="$fixture/test-config/cache"
export MOEKA_LOOPBACK_CONFIG="$fixture/client_config"
export MOEKA_LOOPBACK_HOME="$fixture/home"
export MOEKA_LOOPBACK_DESTINATION=moeka-loopback-isolated
export MOEKA_REMOTE_BUNDLE="$root_dir/$bundle"
export MOEKA_LOOPBACK_SERVER="$root_dir/target/release/moeka-remote-server"

cargo test --release --locked -p moeka-ssh --test loopback \
    loopback_install_reuse_files_search_watch_and_restart_outcomes -- --ignored --nocapture
cargo test --release --locked --features gpui-test-support \
    loopback_editor_opens_saves_reloads_and_preserves_disconnected_drafts \
    -- --ignored --nocapture --test-threads=1
