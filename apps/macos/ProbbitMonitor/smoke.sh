#!/bin/sh
# Launches a built "probbit Monitor.app", checks that its engine serves the page on 127.0.0.1, quits the app, and checks that
# no engine process is left. Needs a logged-in macOS session (a desktop, or a GitHub macOS runner).
#
#   smoke.sh APP [--quit term|osascript|kill9] [-- --demo week | -- PATH...]     (default: --demo week)
#
#   --quit term       SIGTERM to the app (default; what CI uses)
#   --quit osascript  quit it the way the menu does (needs Automation permission for the terminal)
#   --quit kill9      SIGKILL the app, the crash case: its engine must still go away on its own
set -eu
app=$(cd "$1" && pwd -P); shift
quit=term
while [ $# -gt 0 ]; do
  case "$1" in
    --quit) quit=$2; shift 2 ;;
    --) shift; break ;;
    *) echo "smoke.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done
[ $# -gt 0 ] || set -- --demo week
shell="$app/Contents/MacOS/ProbbitMonitor"
engine="$app/Contents/MacOS/probbit"

fail() { echo "smoke: FAIL: $*" >&2; pkill -f "$engine monitor" 2>/dev/null || true; pkill -f "$shell" 2>/dev/null || true; exit 1; }
wait_for() { i=0; while [ $i -lt "$2" ]; do if eval "$1"; then return 0; fi; sleep 0.25; i=$((i + 1)); done; return 1; }

pkill -f "$shell" 2>/dev/null || true
# flags go to the app as arguments; paths are opened the way Finder opens them (an open-documents Apple Event)
case "$1" in -*) open -n -a "$app" --args "$@" ;; *) open -n -a "$app" "$@" ;; esac
wait_for 'app_pid=$(pgrep -f "^$shell" | head -1); [ -n "$app_pid" ]' 80 || fail "the app did not start"
wait_for 'eng_pid=$(pgrep -f "^$engine monitor" | head -1); [ -n "$eng_pid" ]' 80 || fail "no engine process"
wait_for 'port=$(lsof -nP -a -p "$eng_pid" -iTCP -sTCP:LISTEN 2>/dev/null | awk '"'"'NR>1{sub(/.*:/,"",$9); print $9; exit}'"'"'); [ -n "$port" ]' 80 || fail "the engine listens on no port"
listen=$(lsof -nP -a -p "$eng_pid" -iTCP -sTCP:LISTEN | awk 'NR>1{print $9}')
echo "app pid $app_pid; engine pid $eng_pid ($(ps -o args= -p "$eng_pid" | sed "s|$app/||")); listening on $listen"
case "$listen" in 127.0.0.1:*) ;; *) fail "the engine listens on $listen, not 127.0.0.1 only" ;; esac
code=$(curl -s -o /tmp/probbit-smoke-page.html -w '%{http_code}' "http://127.0.0.1:$port/")
[ "$code" = 200 ] || fail "GET / answered $code"
echo "GET http://127.0.0.1:$port/ -> $code, $(wc -c < /tmp/probbit-smoke-page.html | tr -d ' ') bytes: $(head -c 120 /tmp/probbit-smoke-page.html | tr '\n' ' ')"
rm -f /tmp/probbit-smoke-page.html

case "$quit" in
  term) kill -TERM "$app_pid" ;;
  osascript) osascript -e 'quit app "probbit Monitor"' ;;
  kill9) kill -KILL "$app_pid" ;;
  *) fail "--quit expects term, osascript or kill9" ;;
esac
wait_for '! kill -0 "$app_pid" 2>/dev/null' 40 || fail "the app is still running after --quit $quit"
wait_for '! pgrep -f "^$engine monitor" > /dev/null' 40 || fail "an engine process survived the app: $(pgrep -fl "^$engine monitor")"
wait_for '! pgrep -f "$engine" > /dev/null' 20 || fail "a guard process survived the app: $(pgrep -fl "$engine")"
echo "quit ($quit): the app and every engine process are gone"
echo "smoke: ok"
