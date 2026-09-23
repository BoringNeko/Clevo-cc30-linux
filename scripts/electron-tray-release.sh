#!/usr/bin/env bash
#
# electron-tray-release.sh - verify closing the window releases the renderer
# while the tray and backend stay alive, and that reopening works.
#
# Scheme 2 behaviour: the title bar's close button destroys the BrowserWindow
# (and its Chromium renderer/GPU processes); the tray keeps the app alive and
# "open" recreates the window. This script measures it for real:
#
#   1. launch the app with a debug port,
#   2. count electron processes and RSS with the window up,
#   3. drive the *frontend's own* close path over CDP (hideMainWindow),
#   4. confirm renderer processes and their RSS are gone, backend still up,
#   5. reopen over IPC and confirm a renderer comes back.
#
# Requires: the Electron runtime, a display, and node with the `ws` package
# (present in ui/node_modules via electron's deps).
#
# Usage: scripts/electron-tray-release.sh

set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ELECTRON="$ROOT/ui/node_modules/electron/dist/electron"
WS="$ROOT/ui/node_modules/.pnpm/ws@8.21.3/node_modules/ws"
PORT=9231

fail=0
pass() { printf '  ok   %s\n' "$1"; }
bad()  { printf '  FAIL %s\n' "$1" >&2; fail=1; }

[[ -x "$ELECTRON" ]] || { printf '  skip Electron runtime not installed\n'; exit 0; }
[[ -d "$WS" ]] || { printf '  skip ws module missing (%s)\n' "$WS"; exit 0; }
if [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
    printf '  skip no display (tray/window test needs a session)\n'; exit 0
fi

cleanup() {
    pkill -9 -f 'electron/dist/electron' 2>/dev/null
    pkill -9 -f 'clevo-cc-ui --serve' 2>/dev/null
}
trap cleanup EXIT
cleanup; sleep 2

# Count renderer processes and total electron RSS.
renderers() { ps -eo args | grep -c '[e]lectron/dist/electron.*--type=renderer'; }
electron_rss() { ps -eo rss,args | grep '[e]lectron/dist/electron' | awk '{s+=$1} END {printf "%d", s/1024}'; }
backend_up() { pgrep -f 'clevo-cc-ui --serve' >/dev/null && echo yes || echo no; }

# --- launch -----------------------------------------------------------------
# CLEVO_CC_E2E_REOPEN_MS makes the main process reopen the window 3 s after it is
# closed, so we can verify the recreate path without a real tray click.
( cd ui && CLEVO_CC_E2E_REOPEN_MS=3000 "$ELECTRON" . --no-sandbox --remote-debugging-port=$PORT >/tmp/opencode/tray-release.log 2>&1 ) &
sleep 8

before_r=$(renderers)
before_m=$(electron_rss)
[[ "$before_r" -ge 1 ]] && pass "window up: $before_r renderer process(es), ${before_m} MB" \
    || bad "no renderer process with the window up"
[[ "$(backend_up)" == "yes" ]] && pass "backend running" || bad "backend not running"

# --- drive the frontend's own close path over CDP ---------------------------
cat > /tmp/opencode/tray-cdp.cjs <<'EOF'
const path=require("path"),http=require("http");
const WebSocket=require(path.join(process.argv[3]));
const port=process.argv[2], action=process.argv[4];
function tg(){return new Promise((res,rej)=>{http.get(`http://127.0.0.1:${port}/json`,r=>{let b="";r.on("data",c=>b+=c);r.on("end",()=>res(JSON.parse(b)))}).on("error",rej)})}
(async()=>{
  const t=await tg();const page=t.find(x=>x.type==="page");
  const ws=new WebSocket(page.webSocketDebuggerUrl,{maxPayload:1<<28});
  let id=0;const p=new Map();
  const send=(m,pr={})=>new Promise(r=>{const i=++id;p.set(i,r);ws.send(JSON.stringify({id:i,method:m,params:pr}))});
  ws.on("message",d=>{const m=JSON.parse(d.toString());if(m.id&&p.has(m.id)){p.get(m.id)(m.result);p.delete(m.id)}});
  await new Promise(r=>ws.on("open",r));
  const expr = action==="hide"
    ? "window.__CLEVO_ELECTRON__.window.hide().then(()=>'hid')"
    : "window.__CLEVO_ELECTRON__.window.show().then(()=>'shown')";
  const r=await send("Runtime.evaluate",{expression:expr,awaitPromise:true,returnByValue:true});
  console.log(action+":"+(r.result?.value ?? JSON.stringify(r.result)));
  ws.close();process.exit(0);
})().catch(e=>{console.error("cdp:"+e.message);process.exit(1)});
EOF

node /tmp/opencode/tray-cdp.cjs "$PORT" "$WS" hide >/dev/null 2>&1 \
    && pass "drove the UI close path (window.hide)" \
    || bad "could not drive the close path"
sleep 3

after_r=$(renderers)
after_m=$(electron_rss)
[[ "$after_r" == "0" ]] && pass "renderer released after close (0 processes)" \
    || bad "renderer still running after close ($after_r processes)"
[[ "$after_m" -lt "$before_m" ]] && pass "electron RSS dropped: ${before_m} -> ${after_m} MB" \
    || bad "RSS did not drop: ${before_m} -> ${after_m} MB"
[[ "$(backend_up)" == "yes" ]] && pass "backend survived the close" || bad "backend exited on close"

# --- reopen -----------------------------------------------------------------
# The test hook reopens the window 3 s after it closes; wait past that and check
# a renderer came back and the frontend re-rendered.
sleep 8
reopen_r=$(renderers)
[[ "$reopen_r" -ge 1 ]] && pass "window recreated: $reopen_r renderer process(es)" \
    || bad "window was not recreated after close"

# Confirm the recreated page actually loaded (a page target exists again).
if curl -s "http://127.0.0.1:$PORT/json" | grep -q '"type": "page"'; then
    pass "recreated page is live (CDP target present)"
else
    bad "no page target after reopen"
fi

printf '  -- %s\n' "before: ${before_r} renderer / ${before_m} MB; after close: ${after_r} / ${after_m} MB; after reopen: ${reopen_r} renderer"
exit "$fail"
