#!/usr/bin/env bash
# CLI for Snack's live agent control plane.
#
# Prerequisites:
#   SNACK_AGENT=1 cargo run
#
# Examples:
#   scripts/agentctl.sh ping
#   scripts/agentctl.sh state
#   scripts/agentctl.sh open-palette
#   scripts/agentctl.sh set-query dev
#   scripts/agentctl.sh move 1
#   scripts/agentctl.sh submit
#   scripts/agentctl.sh screenshot tmp/agent-ui/live-palette.png
#   scripts/agentctl.sh select-channel general
#   scripts/agentctl.sh wait screen=main
#   scripts/agentctl.sh wait palette_open=true --timeout 10

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

sock_path() {
  if [[ -n "${SNACK_AGENT_SOCK:-}" ]]; then
    printf '%s\n' "$SNACK_AGENT_SOCK"
    return
  fi
  local marker="${TMPDIR:-/tmp}/snack-agent.sock.path"
  if [[ -f "$marker" ]]; then
    cat "$marker"
    return
  fi
  printf '%s\n' "${TMPDIR:-/tmp}/snack-agent.sock"
}

usage() {
  cat <<'EOF'
Usage: scripts/agentctl.sh <command> [args]

Commands:
  ping
  help
  state                         Full JSON UI snapshot
  open-palette | palette
  close-palette
  set-query <text> | type <text>
  move [delta]                  Default delta=1
  submit
  select-entry <index>
  select-channel <id|name>
  select-workspace <team_id>
  search <query>
  clear-search
  open-settings
  main-view <home|dms|activity>   Switch far-rail surface
  activity-select <index>         Open activity item in right panel
  close-settings
  open-profile <user_id>
  close-profile
  screenshot [path]
  toast <text>
  allow-destructive [true|false]
  send                          Requires allow-destructive
  wait <predicate> [--timeout N]
                                Poll state until predicate matches.
                                Predicates: screen=main|login|loading
                                            palette_open=true|false
                                            signed_in=true|false
                                            channel=<id|name>
                                            query=<text>
                                            entries>=N
  raw <json>                    Send a raw JSON request object

Env:
  SNACK_AGENT_SOCK   Unix socket path (default $TMPDIR/snack-agent.sock)
EOF
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

cmd="$1"
shift || true

REQ_ID="${AGENTCTL_ID:-$RANDOM}"
JSON=""

case "$cmd" in
  -h|--help|help)
    if [[ "$cmd" == "help" ]]; then
      JSON=$(printf '{"id":%s,"cmd":"help"}' "$REQ_ID")
    else
      usage
      exit 0
    fi
    ;;
  ping)
    JSON=$(printf '{"id":%s,"cmd":"ping"}' "$REQ_ID")
    ;;
  state)
    JSON=$(printf '{"id":%s,"cmd":"state"}' "$REQ_ID")
    ;;
  open-palette|palette)
    JSON=$(printf '{"id":%s,"cmd":"open-palette"}' "$REQ_ID")
    ;;
  close-palette)
    JSON=$(printf '{"id":%s,"cmd":"close-palette"}' "$REQ_ID")
    ;;
  set-query|type)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh $cmd <text>" >&2; exit 2; }
    text="$*"
    if [[ "$cmd" == "type" ]]; then
      JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"type", "text": sys.argv[2]}))' "$REQ_ID" "$text")
    else
      JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"set-query", "query": sys.argv[2]}))' "$REQ_ID" "$text")
    fi
    ;;
  move)
    delta="${1:-1}"
    JSON=$(printf '{"id":%s,"cmd":"move","delta":%s}' "$REQ_ID" "$delta")
    ;;
  submit)
    JSON=$(printf '{"id":%s,"cmd":"submit"}' "$REQ_ID")
    ;;
  select-entry)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh select-entry <index>" >&2; exit 2; }
    JSON=$(printf '{"id":%s,"cmd":"select-entry","index":%s}' "$REQ_ID" "$1")
    ;;
  select-channel)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh select-channel <id|name>" >&2; exit 2; }
    JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"select-channel", "channel": sys.argv[2]}))' "$REQ_ID" "$1")
    ;;
  select-workspace)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh select-workspace <team_id>" >&2; exit 2; }
    JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"select-workspace", "team": sys.argv[2]}))' "$REQ_ID" "$1")
    ;;
  search)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh search <query>" >&2; exit 2; }
    JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"search", "query": sys.argv[2]}))' "$REQ_ID" "$*")
    ;;
  clear-search)
    JSON=$(printf '{"id":%s,"cmd":"clear-search"}' "$REQ_ID")
    ;;
  open-settings)
    JSON=$(printf '{"id":%s,"cmd":"open-settings"}' "$REQ_ID")
    ;;
  close-settings)
    JSON=$(printf '{"id":%s,"cmd":"close-settings"}' "$REQ_ID")
    ;;
  open-profile)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh open-profile <user_id>" >&2; exit 2; }
    JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"open-profile", "user": sys.argv[2]}))' "$REQ_ID" "$1")
    ;;
  close-profile)
    JSON=$(printf '{"id":%s,"cmd":"close-profile"}' "$REQ_ID")
    ;;
  screenshot)
    path="${1:-}"
    if [[ -n "$path" ]]; then
      JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"screenshot", "path": sys.argv[2]}))' "$REQ_ID" "$path")
    else
      JSON=$(printf '{"id":%s,"cmd":"screenshot"}' "$REQ_ID")
    fi
    ;;
  main-view)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh main-view <home|dms|activity>" >&2; exit 2; }
    JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"main-view", "view": sys.argv[2]}))' "$REQ_ID" "$1")
    ;;
  activity-select)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh activity-select <index>" >&2; exit 2; }
    JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"activity-select", "index": int(sys.argv[2])}))' "$REQ_ID" "$1")
    ;;
  toast)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh toast <text>" >&2; exit 2; }
    JSON=$(python3 -c 'import json,sys; print(json.dumps({"id": int(sys.argv[1]), "cmd":"toast", "text": sys.argv[2]}))' "$REQ_ID" "$*")
    ;;
  allow-destructive)
    enabled="${1:-true}"
    JSON=$(printf '{"id":%s,"cmd":"allow-destructive","enabled":%s}' "$REQ_ID" "$enabled")
    ;;
  send)
    JSON=$(printf '{"id":%s,"cmd":"send"}' "$REQ_ID")
    ;;
  raw)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh raw '<json>'" >&2; exit 2; }
    JSON="$*"
    ;;
  wait)
    [[ $# -ge 1 ]] || { echo "usage: agentctl.sh wait <predicate> [--timeout N]" >&2; exit 2; }
    predicate="$1"
    shift || true
    timeout_s=20
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --timeout) timeout_s="${2:-20}"; shift 2 || true ;;
        *) echo "unknown wait arg: $1" >&2; exit 2 ;;
      esac
    done
    SOCK="$(sock_path)"
    python3 - "$SOCK" "$predicate" "$timeout_s" <<'PY'
import json, os, socket, sys, time

sock_path, predicate, timeout_s = sys.argv[1], sys.argv[2], float(sys.argv[3])
deadline = time.time() + timeout_s
req_id = 1

def call(cmd_obj):
    global req_id
    req_id += 1
    cmd_obj = dict(cmd_obj)
    cmd_obj["id"] = req_id
    payload = (json.dumps(cmd_obj) + "\n").encode()
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(5)
    s.connect(sock_path)
    s.sendall(payload)
    buf = b""
    while b"\n" not in buf:
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    line = buf.split(b"\n", 1)[0].decode()
    return json.loads(line)

def matches(state, pred):
    data = state.get("data") or {}
    if pred.startswith("screen="):
        return data.get("screen") == pred.split("=", 1)[1]
    if pred.startswith("palette_open="):
        want = pred.split("=", 1)[1].lower() == "true"
        return bool(data.get("palette_open")) is want
    if pred.startswith("signed_in="):
        want = pred.split("=", 1)[1].lower() == "true"
        return bool(data.get("signed_in")) is want
    if pred.startswith("channel="):
        want = pred.split("=", 1)[1].lstrip("#").lower()
        ch = (data.get("active_channel") or "").lower()
        name = (data.get("active_channel_name") or "").lower()
        return want in (ch, name) or want == ch or want == name
    if pred.startswith("query="):
        want = pred.split("=", 1)[1]
        palette = data.get("palette") or {}
        return (palette.get("query") or "") == want
    if pred.startswith("entries>="):
        n = int(pred.split(">=", 1)[1])
        palette = data.get("palette") or {}
        return int(palette.get("entry_count") or 0) >= n
    raise SystemExit(f"unknown predicate: {pred}")

last = None
while time.time() < deadline:
    try:
        last = call({"cmd": "state"})
    except Exception as e:
        last = {"ok": False, "error": str(e)}
        time.sleep(0.25)
        continue
    if last.get("ok") and matches(last, predicate):
        print(json.dumps(last, indent=2))
        sys.exit(0)
    time.sleep(0.2)

print(json.dumps(last or {"ok": False, "error": "timeout"}, indent=2))
sys.exit(1)
PY
    exit $?
    ;;
  *)
    echo "unknown command: $cmd" >&2
    usage >&2
    exit 2
    ;;
esac

SOCK="$(sock_path)"
if [[ ! -S "$SOCK" && ! -e "$SOCK" ]]; then
  echo "agentctl: socket not found at $SOCK" >&2
  echo "start snack with: SNACK_AGENT=1 cargo run" >&2
  exit 1
fi

python3 -c '
import json, socket, sys
sock_path, payload = sys.argv[1], sys.argv[2]
data = payload.encode()
if not data.endswith(b"\n"):
    data += b"\n"
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(35)
s.connect(sock_path)
s.sendall(data)
buf = b""
while b"\n" not in buf:
    chunk = s.recv(1 << 20)
    if not chunk:
        break
    buf += chunk
s.close()
line = buf.split(b"\n", 1)[0].decode()
try:
    obj = json.loads(line)
    print(json.dumps(obj, indent=2))
    sys.exit(0 if obj.get("ok", False) else 1)
except Exception:
    print(line)
    sys.exit(1)
' "$SOCK" "$JSON"
