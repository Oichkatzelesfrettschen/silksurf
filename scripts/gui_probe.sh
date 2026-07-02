#!/usr/bin/env bash
# gui_probe.sh -- run the winit browser GUI probe and require clean exit.
#
# The probe opens the native GUI, renders the default page or supplied URL,
# feeds address-bar input through the winit event loop, waits for the final
# input frame to present, then exits from inside the application.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

usage() {
    cat <<'EOF'
Usage: scripts/gui_probe.sh [--release|--debug|--o0] [--backend auto|wayland|x11] [--presenter auto|shm|softbuffer] [--shm] [--fixture ai-chat|form-submit|post-submit] [--probe smoke|address|address-caret|chrome|hover|page-input|form-submit|reload|scroll|stop] [--runs N] [--timeout-seconds N] [--max-input-ns N] [--max-any-input-ns N] [--max-buffer-ns N] [--max-any-buffer-ns N] [--max-render-ns N] [--max-overhead-ns N] [--max-total-ns N] [--max-focus-total-ns N] [--trace-app-frame] [URL]

Runs the silksurf winit GUI probe with SILKSURF_PROBE_EXIT_AFTER_INPUT=1.
The process exits successfully only after the final synthetic address input
has produced a presented frame.

Options:
  --release              Build and run target/release/silksurf-app.
  --debug                Build and run target/debug/silksurf-app. Default.
  --o0                   Build and run target/dev-o0/silksurf-app.
  --backend VALUE        Select auto, wayland, or x11. Default: auto.
  --presenter VALUE      Select auto, shm, or softbuffer. Default: auto.
  --shm                  Select the Wayland SHM presenter.
  --fixture VALUE        Serve a local probe fixture. Supported: ai-chat, form-submit, post-submit.
  --probe VALUE          Select smoke, address, address-caret, chrome, hover, page-input, form-submit, reload, scroll, or stop input sequence. Default: address.
  --runs N               Run the probe N times after building. Default: 1.
  --timeout-seconds N    Kill one app run after N seconds. Default: 10.
  --max-input-ns N       Fail when final_input_to_present_ns is greater than N.
  --max-any-input-ns N   Fail when any input frame input_to_present is greater than N.
  --max-buffer-ns N      Fail when final_buffer_ns is greater than N.
  --max-any-buffer-ns N  Fail when any input frame buffer is greater than N.
  --max-render-ns N      Fail when final_render_ns is greater than N.
  --max-overhead-ns N    Fail when final_overhead_ns is greater than N.
  --max-total-ns N       Fail when final_total_ns is greater than N.
  --max-focus-total-ns N Fail when page-input focus_total_ns is greater than N.
  --trace-app-frame      Include app-frame diagnostic logs inside the measured render callback.
  -h, --help             Show this help text.
EOF
}

profile=debug
display_backend=auto
wayland_presenter=auto
runs=1
probe_timeout_seconds=10
max_input_ns=
max_any_input_ns=
max_buffer_ns=
max_any_buffer_ns=
max_render_ns=
max_overhead_ns=
max_total_ns=
max_focus_total_ns=
trace_app_frame=0
url=https://example.com
fixture=
probe=address

while [ "$#" -gt 0 ]; do
    case "$1" in
        --release)
            profile=release
            shift
            ;;
        --debug)
            profile=debug
            shift
            ;;
        --o0)
            profile=dev-o0
            shift
            ;;
        --backend)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --backend requires a value" >&2
                exit 2
            fi
            display_backend="$2"
            shift 2
            ;;
        --backend=*)
            display_backend="${1#--backend=}"
            shift
            ;;
        --shm)
            wayland_presenter=shm
            shift
            ;;
        --presenter)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --presenter requires a value" >&2
                exit 2
            fi
            wayland_presenter="$2"
            shift 2
            ;;
        --presenter=*)
            wayland_presenter="${1#--presenter=}"
            shift
            ;;
        --fixture)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --fixture requires a value" >&2
                exit 2
            fi
            fixture="$2"
            shift 2
            ;;
        --fixture=*)
            fixture="${1#--fixture=}"
            shift
            ;;
        --probe)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --probe requires a value" >&2
                exit 2
            fi
            probe="$2"
            shift 2
            ;;
        --probe=*)
            probe="${1#--probe=}"
            shift
            ;;
        --runs)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --runs requires a value" >&2
                exit 2
            fi
            runs="$2"
            shift 2
            ;;
        --runs=*)
            runs="${1#--runs=}"
            shift
            ;;
        --timeout-seconds)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --timeout-seconds requires a value" >&2
                exit 2
            fi
            probe_timeout_seconds="$2"
            shift 2
            ;;
        --timeout-seconds=*)
            probe_timeout_seconds="${1#--timeout-seconds=}"
            shift
            ;;
        --max-input-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-input-ns requires a value" >&2
                exit 2
            fi
            max_input_ns="$2"
            shift 2
            ;;
        --max-input-ns=*)
            max_input_ns="${1#--max-input-ns=}"
            shift
            ;;
        --max-any-input-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-any-input-ns requires a value" >&2
                exit 2
            fi
            max_any_input_ns="$2"
            shift 2
            ;;
        --max-any-input-ns=*)
            max_any_input_ns="${1#--max-any-input-ns=}"
            shift
            ;;
        --max-buffer-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-buffer-ns requires a value" >&2
                exit 2
            fi
            max_buffer_ns="$2"
            shift 2
            ;;
        --max-buffer-ns=*)
            max_buffer_ns="${1#--max-buffer-ns=}"
            shift
            ;;
        --max-any-buffer-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-any-buffer-ns requires a value" >&2
                exit 2
            fi
            max_any_buffer_ns="$2"
            shift 2
            ;;
        --max-any-buffer-ns=*)
            max_any_buffer_ns="${1#--max-any-buffer-ns=}"
            shift
            ;;
        --max-render-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-render-ns requires a value" >&2
                exit 2
            fi
            max_render_ns="$2"
            shift 2
            ;;
        --max-render-ns=*)
            max_render_ns="${1#--max-render-ns=}"
            shift
            ;;
        --max-overhead-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-overhead-ns requires a value" >&2
                exit 2
            fi
            max_overhead_ns="$2"
            shift 2
            ;;
        --max-overhead-ns=*)
            max_overhead_ns="${1#--max-overhead-ns=}"
            shift
            ;;
        --max-total-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-total-ns requires a value" >&2
                exit 2
            fi
            max_total_ns="$2"
            shift 2
            ;;
        --max-total-ns=*)
            max_total_ns="${1#--max-total-ns=}"
            shift
            ;;
        --max-focus-total-ns)
            if [ "$#" -lt 2 ]; then
                echo "gui_probe: --max-focus-total-ns requires a value" >&2
                exit 2
            fi
            max_focus_total_ns="$2"
            shift 2
            ;;
        --max-focus-total-ns=*)
            max_focus_total_ns="${1#--max-focus-total-ns=}"
            shift
            ;;
        --trace-app-frame)
            trace_app_frame=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --*)
            echo "gui_probe: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
        *)
            url="$1"
            shift
            ;;
    esac
done

case "${display_backend}" in
    auto|wayland|x11) ;;
    *)
        echo "gui_probe: backend must be auto, wayland, or x11" >&2
        exit 2
        ;;
esac

case "${wayland_presenter}" in
    auto|shm|softbuffer) ;;
    *)
        echo "gui_probe: presenter must be auto, shm, or softbuffer" >&2
        exit 2
        ;;
esac

case "${fixture}" in
    ""|ai-chat|form-submit|post-submit) ;;
    *)
        echo "gui_probe: fixture must be ai-chat, form-submit, or post-submit" >&2
        exit 2
        ;;
esac

case "${probe}" in
    smoke|address|address-caret|chrome|hover|page-input|form-submit|reload|scroll|stop) ;;
    *)
        echo "gui_probe: probe must be smoke, address, address-caret, chrome, hover, page-input, form-submit, reload, scroll, or stop" >&2
        exit 2
        ;;
esac
if [ "${probe}" = "stop" ] && [ -z "${fixture}" ]; then
    echo "gui_probe: --probe stop requires --fixture" >&2
    exit 2
fi
if [ "${probe}" = "form-submit" ] && [ "${fixture}" != "form-submit" ] && [ "${fixture}" != "post-submit" ]; then
    echo "gui_probe: --probe form-submit requires --fixture form-submit or --fixture post-submit" >&2
    exit 2
fi
if [ "${probe}" = "reload" ] && [ -z "${fixture}" ]; then
    echo "gui_probe: --probe reload requires --fixture" >&2
    exit 2
fi
if [ "${probe}" = "scroll" ] && [ -z "${fixture}" ]; then
    echo "gui_probe: --probe scroll requires --fixture" >&2
    exit 2
fi
if [ "${probe}" = "hover" ] && [ "${fixture}" != "ai-chat" ]; then
    echo "gui_probe: --probe hover requires --fixture ai-chat" >&2
    exit 2
fi

validate_threshold() {
    local name="$1"
    local value="$2"

    case "${value}" in
        ""|*[!0-9]*)
            echo "gui_probe: ${name} must be an integer nanosecond value" >&2
            exit 2
            ;;
    esac
}

if [ -n "${max_input_ns}" ]; then
    validate_threshold "--max-input-ns" "${max_input_ns}"
fi
if [ -n "${max_any_input_ns}" ]; then
    validate_threshold "--max-any-input-ns" "${max_any_input_ns}"
fi
if [ -n "${max_buffer_ns}" ]; then
    validate_threshold "--max-buffer-ns" "${max_buffer_ns}"
fi
if [ -n "${max_any_buffer_ns}" ]; then
    validate_threshold "--max-any-buffer-ns" "${max_any_buffer_ns}"
fi
if [ -n "${max_render_ns}" ]; then
    validate_threshold "--max-render-ns" "${max_render_ns}"
fi
if [ -n "${max_overhead_ns}" ]; then
    validate_threshold "--max-overhead-ns" "${max_overhead_ns}"
fi
if [ -n "${max_total_ns}" ]; then
    validate_threshold "--max-total-ns" "${max_total_ns}"
fi
if [ -n "${max_focus_total_ns}" ]; then
    validate_threshold "--max-focus-total-ns" "${max_focus_total_ns}"
fi
validate_threshold "--runs" "${runs}"
validate_threshold "--timeout-seconds" "${probe_timeout_seconds}"
if [ "${runs}" -lt 1 ]; then
    echo "gui_probe: --runs must be greater than zero" >&2
    exit 2
fi
if [ "${probe_timeout_seconds}" -lt 1 ]; then
    echo "gui_probe: --timeout-seconds must be greater than zero" >&2
    exit 2
fi

wayland_socket_path() {
    local wayland_display="${WAYLAND_DISPLAY:-}"

    if [ -z "${wayland_display}" ]; then
        return 1
    fi
    case "${wayland_display}" in
        /*)
            printf '%s\n' "${wayland_display}"
            ;;
        *)
            if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
                return 1
            fi
            printf '%s/%s\n' "${XDG_RUNTIME_DIR}" "${wayland_display}"
            ;;
    esac
}

wayland_display_available() {
    local socket_path

    socket_path="$(wayland_socket_path)" || return 1
    [ -S "${socket_path}" ]
}

x11_display_available() {
    [ -n "${DISPLAY:-}" ]
}

resolve_probe_display_backend() {
    case "${display_backend}" in
        wayland)
            if ! wayland_display_available; then
                echo "gui_probe: --backend wayland requires a live WAYLAND_DISPLAY socket" >&2
                exit 1
            fi
            printf '%s\n' wayland
            ;;
        x11)
            if ! x11_display_available; then
                echo "gui_probe: --backend x11 requires DISPLAY" >&2
                exit 1
            fi
            printf '%s\n' x11
            ;;
        auto)
            if wayland_display_available; then
                printf '%s\n' wayland
            elif x11_display_available; then
                printf '%s\n' x11
            else
                echo "gui_probe: no live display found; set WAYLAND_DISPLAY, set DISPLAY, or wrap with xvfb-run" >&2
                exit 1
            fi
            ;;
    esac
}

resolved_display_backend="$(resolve_probe_display_backend)"

case "${profile}" in
    release)
        RUSTFLAGS='-D warnings' cargo build --release -p silksurf-app --bin silksurf-app
        binary=target/release/silksurf-app
        ;;
    debug)
        RUSTFLAGS='-D warnings' cargo build -p silksurf-app --bin silksurf-app
        binary=target/debug/silksurf-app
        ;;
    dev-o0)
        RUSTFLAGS='-D warnings' cargo build --profile dev-o0 -p silksurf-app --bin silksurf-app
        binary=target/dev-o0/silksurf-app
        ;;
    *)
        echo "gui_probe: invalid profile: ${profile}" >&2
        exit 2
        ;;
esac

env_args=(
    "SILKSURF_TRACE_FRAME=1"
    "SILKSURF_PROBE_INPUT=${probe}"
    "SILKSURF_PROBE_EXIT_AFTER_INPUT=1"
    "SILKSURF_WAYLAND_PRESENTER=${wayland_presenter}"
)
if [ "${trace_app_frame}" -eq 1 ] || [ "${probe}" = "scroll" ]; then
    env_args+=("SILKSURF_TRACE_APP_FRAME=1")
fi
if [ "${fixture}" = "ai-chat" ] && [ "${probe}" = "page-input" ]; then
    env_args+=("SILKSURF_TRACE_NAV_BUILD=1")
fi

work_dir="$(mktemp -d)"
if [ -n "${fixture}" ]; then
    env_args+=("XDG_CACHE_HOME=${work_dir}/cache")
fi
metrics_file="${work_dir}/metrics.tsv"
fixture_server_pid=
fixture_server_log_file=
cleanup() {
    if [ -n "${fixture_server_pid}" ]; then
        kill "${fixture_server_pid}" 2>/dev/null || true
        wait "${fixture_server_pid}" 2>/dev/null || true
    fi
    rm -rf "${work_dir}"
}
trap cleanup EXIT

write_ai_chat_fixture() {
    local fixture_dir="$1"
    local item

    mkdir -p "${fixture_dir}"
    cat >"${fixture_dir}/index.html" <<'EOF'
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>SilkSurf AI Chat Fixture</title>
  <link rel="stylesheet" href="/style.css">
  <link rel="modulepreload" href="/module.js">
  <script type="module" src="/module.js"></script>
</head>
<body>
  <main class="shell">
    <nav class="rail">
      <a href="/chat/overview">Overview</a>
      <a href="/chat/research">Research</a>
      <a href="/chat/code">Code</a>
      <a href="/chat/settings">Settings</a>
    </nav>
    <section class="thread">
      <h1>AI Workbench</h1>
      <img class="avatar" src="/avatar.png" width="64" height="32" alt="fixture image">
      <p class="lede">A deterministic local page models dense chat, citations, controls, code, and composer chrome.</p>
      <form class="composer">
        <textarea>Summarize the current trace and propose the next falsifier.</textarea>
        <button>Send</button>
      </form>
      <div class="toolbar">
        <button>New chat</button>
        <button>Attach</button>
        <button>Search</button>
        <input value="profile: low latency browser">
      </div>
      <div class="messages">
EOF
    item=0
    while [ "${item}" -lt 96 ]; do
        cat >>"${fixture_dir}/index.html" <<EOF
        <article class="message ${item}">
          <h2>Turn ${item}</h2>
          <p>Assistant output includes markdown, citations, code notes, status rows, and compact controls for repeated scanning.</p>
          <pre><code>fn latency_term_${item}(state: &BrowserState) -> u32 { state.frame.raster_height }</code></pre>
          <a href="/source/${item}">source ${item}</a>
          <input value="editable field ${item}">
        </article>
EOF
        item=$((item + 1))
    done
    cat >>"${fixture_dir}/index.html" <<'EOF'
      </div>
      <script src="/app.js"></script>
    </section>
  </main>
</body>
</html>
EOF
    cat >"${fixture_dir}/style.css" <<'EOF'
body { margin: 0; font-family: system-ui, sans-serif; background: #f8fafc; color: #111827; }
.shell { display: flex; min-height: 1600px; }
.rail { width: 192px; padding: 16px; background: #111827; }
.rail a { display: block; color: #f9fafb; padding: 8px; margin: 4px 0; }
.thread { flex: 1; padding: 20px; }
body[data-fixture="ai-chat"] .thread { border-left: 3px solid #2563eb; }
.lede { color: #4b5563; }
.toolbar { display: flex; gap: 8px; padding: 12px 0; }
.toolbar input { width: 360px; }
.messages { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
.message { background: #ffffff; border: 1px solid #d1d5db; padding: 12px; }
.message h2 { font-size: 16px; margin: 0 0 8px; }
.message p { margin: 6px 0; }
.message pre { background: #111827; color: #f9fafb; padding: 8px; overflow: hidden; }
.message input { width: 90%; }
.composer { margin: 20px 0; display: flex; gap: 8px; }
.composer textarea { width: 80%; height: 72px; }
EOF
    cat >"${fixture_dir}/app.js" <<'EOF'
document.body.setAttribute('data-fixture', 'ai-chat');
var dynamicScript = document.createElement('script');
dynamicScript.src = '/dynamic.js';
dynamicScript.innerHTML = 'document.body.setAttribute("data-dynamic-script", "queued");';
document.head.appendChild(dynamicScript);
EOF
    cat >"${fixture_dir}/dynamic.js" <<'EOF'
document.body.setAttribute("data-dynamic-script", "fetched");
EOF
cat >"${fixture_dir}/module.js" <<'EOF'
import { fixtureGraph } from "/module-child.js";
document.body.setAttribute("data-module-graph", fixtureGraph);
export const fixtureKind = "ai-chat";
export const fixtureGraphKind = fixtureGraph;
EOF
    cat >"${fixture_dir}/module-child.js" <<'EOF'
export const fixtureGraph = "module-child";
EOF
    python3 - "${fixture_dir}/avatar.png" <<'PY'
import base64
import pathlib
import sys

pathlib.Path(sys.argv[1]).write_bytes(base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR4nGP4z8Dwn+E/"
    "QwMAEHkDfiHA/Y0AAAAASUVORK5CYII="
))
PY
    mkdir -p "${fixture_dir}/start"
    cp "${fixture_dir}/index.html" "${fixture_dir}/start/index.html"
}

write_form_submit_fixture() {
    local fixture_dir="$1"

    mkdir -p "${fixture_dir}/results"
    cat >"${fixture_dir}/index.html" <<'EOF'
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>SilkSurf Form Submit Fixture</title>
  <link rel="stylesheet" href="/style.css">
</head>
<body>
  <main class="shell">
    <h1>Search</h1>
    <form action="/results/" method="get">
      <input name="q" value="silk">
      <input name="mode" value="fast">
      <input type="checkbox" name="opt" checked>
      <input type="radio" name="tier" value="basic">
      <input type="radio" name="tier" value="pro" checked>
      <select name="sort">
        <option value="recent" selected>Recent</option>
        <option value="popular">Popular</option>
      </select>
      <button>Go</button>
    </form>
  </main>
</body>
</html>
EOF
    cat >"${fixture_dir}/results/index.html" <<'EOF'
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>SilkSurf Form Result</title>
</head>
<body>
  <main class="shell">
    <h1>Result</h1>
    <p>Form navigation reached the result page.</p>
  </main>
</body>
</html>
EOF
    cat >"${fixture_dir}/style.css" <<'EOF'
body { margin: 0; font-family: system-ui, sans-serif; background: #f8fafc; color: #111827; }
.shell { padding: 24px; }
input, select { width: 320px; margin: 8px; }
button { margin: 8px; }
EOF
}

write_post_submit_fixture() {
    local fixture_dir="$1"

    mkdir -p "${fixture_dir}"
    cat >"${fixture_dir}/index.html" <<'EOF'
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>SilkSurf POST Submit Fixture</title>
  <link rel="stylesheet" href="/style.css">
</head>
<body>
  <main class="shell">
    <h1>Post Search</h1>
    <form action="/posted/" method="post">
      <input name="q" value="silk">
      <input name="mode" value="fast">
      <input type="checkbox" name="opt" checked>
      <input type="radio" name="tier" value="basic">
      <input type="radio" name="tier" value="pro" checked>
      <select name="sort">
        <option value="recent" selected>Recent</option>
        <option value="popular">Popular</option>
      </select>
      <button>Go</button>
    </form>
  </main>
</body>
</html>
EOF
    cat >"${fixture_dir}/style.css" <<'EOF'
body { margin: 0; font-family: system-ui, sans-serif; background: #f8fafc; color: #111827; }
.shell { padding: 24px; }
input, select { width: 320px; margin: 8px; }
button { margin: 8px; }
EOF
}

start_fixture_server() {
    local fixture_name="$1"
    local fixture_dir="${work_dir}/${fixture_name}"
    local port_file="${work_dir}/fixture-port"
    local log_file="${work_dir}/fixture-server.log"

    case "${fixture_name}" in
        ai-chat)
            write_ai_chat_fixture "${fixture_dir}"
            ;;
        form-submit)
            write_form_submit_fixture "${fixture_dir}"
            ;;
        post-submit)
            write_post_submit_fixture "${fixture_dir}"
            ;;
    esac
    fixture_server_log_file="${log_file}"
    python3 - "${fixture_dir}" "${port_file}" >"${log_file}" 2>&1 <<'PY' &
import http.server
import pathlib
import socketserver
import sys
import time

directory = pathlib.Path(sys.argv[1])
port_file = pathlib.Path(sys.argv[2])
class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(directory), **kwargs)

    def do_GET(self):
        if self.path.startswith("/slow/"):
            time.sleep(2.0)
            body = b"<!doctype html><title>slow</title><p>slow fixture</p>"
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        super().do_GET()

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        sys.stdout.write(f"POST {self.path} {body.decode('ascii', 'replace')}\n")
        sys.stdout.flush()
        if self.path != "/posted/" or body != b"q=silk%21&mode=fast&opt=on&tier=pro":
            self.send_response(400)
            response = b"<!doctype html><title>bad post</title><p>bad post</p>"
        else:
            self.send_response(200)
            response = b"<!doctype html><title>posted</title><p>post navigation reached the result page.</p>"
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

with socketserver.TCPServer(("127.0.0.1", 0), Handler) as server:
    port_file.write_text(str(server.server_address[1]), encoding="ascii")
    server.serve_forever()
PY
    fixture_server_pid="$!"
    while [ ! -s "${port_file}" ]; do
        if ! kill -0 "${fixture_server_pid}" 2>/dev/null; then
            cat "${log_file}" >&2
            echo "gui_probe: fixture server failed" >&2
            exit 1
        fi
        sleep 0.05
    done
    url="http://127.0.0.1:$(cat "${port_file}")/"
    if [ "${probe}" = "chrome" ]; then
        env_args+=("SILKSURF_HOME_URL=${url}")
        url="${url}start/"
    elif [ "${probe}" = "stop" ]; then
        env_args+=("SILKSURF_PROBE_NAVIGATE_URL=${url}slow/")
    fi
    echo "==> GUI probe fixture: ${fixture_name}, url=${url}"
}

if [ -n "${fixture}" ]; then
    start_fixture_server "${fixture}"
fi

if [ "${resolved_display_backend}" = "wayland" ]; then
    echo "==> GUI probe display: configured=${display_backend}, resolved=wayland, socket=$(wayland_socket_path)"
else
    echo "==> GUI probe display: configured=${display_backend}, resolved=x11, DISPLAY=${DISPLAY}"
fi
echo "==> GUI probe: ${profile}, backend=${display_backend}, presenter=${wayland_presenter}, runs=${runs}, url=${url}"

metric_value() {
    local timing="$1"
    local name="$2"

    printf '%s\n' "${timing}" |
        awk -F= -v metric="gui_probe: ${name}" '$1 == metric { print $2; found = 1 } END { exit(found ? 0 : 1) }'
}

check_max_metric() {
    local timing="$1"
    local metric="$2"
    local limit="$3"
    local value

    if [ -z "${limit}" ]; then
        return 0
    fi
    value="$(metric_value "${timing}" "${metric}")"
    if [ "${value}" -lt 0 ]; then
        echo "gui_probe: ${metric} is unavailable" >&2
        exit 1
    fi
    if [ "${value}" -gt "${limit}" ]; then
        echo "gui_probe: ${metric} ${value} exceeds ${limit}" >&2
        exit 1
    fi
}

record_metrics() {
    local run_index="$1"
    local timing="$2"
    local metric
    local value

    for metric in \
        final_blit_ns \
        final_chrome_ns \
        final_input_to_present_ns \
        max_input_to_present_ns \
        final_predraw_ns \
        final_buffer_ns \
        max_input_buffer_ns \
        final_render_ns \
        final_present_ns \
        final_draw_ns \
        final_draw_overhead_ns \
        final_overhead_ns \
        final_handler_overhead_ns \
        final_total_ns
    do
        value="$(metric_value "${timing}" "${metric}")"
        if [ "${value}" -ge 0 ]; then
            printf '%s\t%s\t%s\n' "${run_index}" "${metric}" "${value}" >>"${metrics_file}"
        fi
    done
}

extract_final_timing() {
    local log_file="$1"
    local app_mode_regex="$2"

    awk -v app_mode_regex="${app_mode_regex}" '
        function to_ns(value, unit) {
            if (unit == "ns") {
                return value
            }
            if (unit == "us") {
                return value * 1000
            }
            if (unit == "ms") {
                return value * 1000000
            }
            if (unit == "s") {
                return value * 1000000000
            }
            if (unit ~ /s$/) {
                return value * 1000
            }
            return -1
        }

        function capture_metric(line, name, fields, count, field_index, metric) {
            count = split(line, fields, /[ ,]+/)
            for (field_index = 1; field_index <= count; field_index++) {
                if (fields[field_index] == name && field_index < count) {
                    metric = fields[field_index + 1]
                    if (match(metric, /^([0-9.]+)([^0-9.]+)$/, parts)) {
                        return int(to_ns(parts[1] + 0, parts[2]) + 0.5)
                    }
                }
            }
            return -1
        }

        function frame_overhead(buffer_ns, render_ns, present_ns, total_ns) {
            if (buffer_ns < 0 || render_ns < 0 || present_ns < 0 || total_ns < 0) {
                return -1
            }
            return total_ns - buffer_ns - render_ns - present_ns
        }

        function draw_overhead(buffer_ns, render_ns, present_ns, draw_ns) {
            if (buffer_ns < 0 || render_ns < 0 || present_ns < 0 || draw_ns < 0) {
                return -1
            }
            return draw_ns - buffer_ns - render_ns - present_ns
        }

        function handler_overhead(predraw_ns, draw_ns, total_ns) {
            if (predraw_ns < 0 || draw_ns < 0 || total_ns < 0) {
                return -1
            }
            return total_ns - predraw_ns - draw_ns
        }

        $0 ~ app_mode_regex {
            app = $0
            blit_ns = capture_metric($0, "blit")
            chrome_ns = capture_metric($0, "chrome")
            want_frame = 1
            next
        }
        /frame: .*input_to_present .*total/ {
            line_input_to_present_ns = capture_metric($0, "input_to_present")
            line_buffer_ns = capture_metric($0, "buffer")
            if (line_input_to_present_ns > 0) {
                if (max_input_to_present_ns == "" || line_input_to_present_ns > max_input_to_present_ns) {
                    max_input_to_present_ns = line_input_to_present_ns
                }
                if (max_input_buffer_ns == "" || line_buffer_ns > max_input_buffer_ns) {
                    max_input_buffer_ns = line_buffer_ns
                }
            }
        }
        want_frame && /frame: .*total/ {
            frame = $0
            input_to_present_ns = capture_metric($0, "input_to_present")
            predraw_ns = capture_metric($0, "predraw")
            buffer_ns = capture_metric($0, "buffer")
            render_ns = capture_metric($0, "render")
            present_ns = capture_metric($0, "present")
            draw_ns = capture_metric($0, "draw")
            total_ns = capture_metric($0, "total")
            draw_overhead_ns = draw_overhead(buffer_ns, render_ns, present_ns, draw_ns)
            overhead_ns = frame_overhead(buffer_ns, render_ns, present_ns, total_ns)
            handler_overhead_ns = handler_overhead(predraw_ns, draw_ns, total_ns)
            want_frame = 0
        }
        END {
            if (app != "" && frame != "") {
                print "gui_probe: final_app=" app
                print "gui_probe: final_frame=" frame
                print "gui_probe: final_blit_ns=" blit_ns
                print "gui_probe: final_chrome_ns=" chrome_ns
                print "gui_probe: final_input_to_present_ns=" input_to_present_ns
                print "gui_probe: max_input_to_present_ns=" max_input_to_present_ns
                print "gui_probe: final_predraw_ns=" predraw_ns
                print "gui_probe: final_buffer_ns=" buffer_ns
                print "gui_probe: max_input_buffer_ns=" max_input_buffer_ns
                print "gui_probe: final_render_ns=" render_ns
                print "gui_probe: final_present_ns=" present_ns
                print "gui_probe: final_draw_ns=" draw_ns
                print "gui_probe: final_draw_overhead_ns=" draw_overhead_ns
                print "gui_probe: final_overhead_ns=" overhead_ns
                print "gui_probe: final_handler_overhead_ns=" handler_overhead_ns
                print "gui_probe: final_total_ns=" total_ns
            }
        }
    ' "${log_file}"
}

extract_final_input_frame_timing() {
    local log_file="$1"

    awk '
        function to_ns(value, unit) {
            if (unit == "ns") {
                return value
            }
            if (unit == "us") {
                return value * 1000
            }
            if (unit == "ms") {
                return value * 1000000
            }
            if (unit == "s") {
                return value * 1000000000
            }
            if (unit ~ /s$/) {
                return value * 1000
            }
            return -1
        }

        function capture_metric(line, name, fields, count, field_index, metric) {
            count = split(line, fields, /[ ,]+/)
            for (field_index = 1; field_index <= count; field_index++) {
                if (fields[field_index] == name && field_index < count) {
                    metric = fields[field_index + 1]
                    if (match(metric, /^([0-9.]+)([^0-9.]+)$/, parts)) {
                        return int(to_ns(parts[1] + 0, parts[2]) + 0.5)
                    }
                }
            }
            return -1
        }

        function frame_overhead(buffer_ns, render_ns, present_ns, total_ns) {
            if (buffer_ns < 0 || render_ns < 0 || present_ns < 0 || total_ns < 0) {
                return -1
            }
            return total_ns - buffer_ns - render_ns - present_ns
        }

        function draw_overhead(buffer_ns, render_ns, present_ns, draw_ns) {
            if (buffer_ns < 0 || render_ns < 0 || present_ns < 0 || draw_ns < 0) {
                return -1
            }
            return draw_ns - buffer_ns - render_ns - present_ns
        }

        function handler_overhead(predraw_ns, draw_ns, total_ns) {
            if (predraw_ns < 0 || draw_ns < 0 || total_ns < 0) {
                return -1
            }
            return total_ns - predraw_ns - draw_ns
        }

        /frame: .*input_to_present .*total/ {
            line_input_to_present_ns = capture_metric($0, "input_to_present")
            line_buffer_ns = capture_metric($0, "buffer")
            if (line_input_to_present_ns <= 0) {
                next
            }
            if (max_input_to_present_ns == "" || line_input_to_present_ns > max_input_to_present_ns) {
                max_input_to_present_ns = line_input_to_present_ns
            }
            if (max_input_buffer_ns == "" || line_buffer_ns > max_input_buffer_ns) {
                max_input_buffer_ns = line_buffer_ns
            }
            frame = $0
            input_to_present_ns = line_input_to_present_ns
            predraw_ns = capture_metric($0, "predraw")
            buffer_ns = line_buffer_ns
            render_ns = capture_metric($0, "render")
            present_ns = capture_metric($0, "present")
            draw_ns = capture_metric($0, "draw")
            total_ns = capture_metric($0, "total")
        }
        END {
            if (frame != "") {
                draw_overhead_ns = draw_overhead(buffer_ns, render_ns, present_ns, draw_ns)
                overhead_ns = frame_overhead(buffer_ns, render_ns, present_ns, total_ns)
                handler_overhead_ns = handler_overhead(predraw_ns, draw_ns, total_ns)
                print "gui_probe: final_app="
                print "gui_probe: final_frame=" frame
                print "gui_probe: final_blit_ns=-1"
                print "gui_probe: final_chrome_ns=-1"
                print "gui_probe: final_input_to_present_ns=" input_to_present_ns
                print "gui_probe: max_input_to_present_ns=" max_input_to_present_ns
                print "gui_probe: final_predraw_ns=" predraw_ns
                print "gui_probe: final_buffer_ns=" buffer_ns
                print "gui_probe: max_input_buffer_ns=" max_input_buffer_ns
                print "gui_probe: final_render_ns=" render_ns
                print "gui_probe: final_present_ns=" present_ns
                print "gui_probe: final_draw_ns=" draw_ns
                print "gui_probe: final_draw_overhead_ns=" draw_overhead_ns
                print "gui_probe: final_overhead_ns=" overhead_ns
                print "gui_probe: final_handler_overhead_ns=" handler_overhead_ns
                print "gui_probe: final_total_ns=" total_ns
            }
        }
    ' "${log_file}"
}

extract_page_input_focus_timing() {
    local log_file="$1"

    awk '
        function to_ns(value, unit) {
            if (unit == "ns") {
                return value
            }
            if (unit == "us") {
                return value * 1000
            }
            if (unit == "ms") {
                return value * 1000000
            }
            if (unit == "s") {
                return value * 1000000000
            }
            if (unit ~ /s$/) {
                return value * 1000
            }
            return -1
        }

        function capture_metric(line, name, fields, count, field_index, metric) {
            count = split(line, fields, /[ ,]+/)
            for (field_index = 1; field_index <= count; field_index++) {
                if (fields[field_index] == name && field_index < count) {
                    metric = fields[field_index + 1]
                    if (match(metric, /^([0-9.]+)([^0-9.]+)$/, parts)) {
                        return int(to_ns(parts[1] + 0, parts[2]) + 0.5)
                    }
                }
            }
            return -1
        }

        /probe input: FocusNextPageInput/ {
            want_focus_frame = 1
            next
        }
        want_focus_frame && /frame: .*input_to_present .*total/ {
            frame = $0
            input_to_present_ns = capture_metric($0, "input_to_present")
            predraw_ns = capture_metric($0, "predraw")
            buffer_ns = capture_metric($0, "buffer")
            render_ns = capture_metric($0, "render")
            draw_ns = capture_metric($0, "draw")
            total_ns = capture_metric($0, "total")
            want_focus_frame = 0
        }
        END {
            if (frame != "") {
                print "gui_probe: focus_frame=" frame
                print "gui_probe: focus_input_to_present_ns=" input_to_present_ns
                print "gui_probe: focus_predraw_ns=" predraw_ns
                print "gui_probe: focus_buffer_ns=" buffer_ns
                print "gui_probe: focus_render_ns=" render_ns
                print "gui_probe: focus_draw_ns=" draw_ns
                print "gui_probe: focus_total_ns=" total_ns
            }
        }
    ' "${log_file}"
}

summarize_metrics() {
    if [ "${runs}" -le 1 ]; then
        return 0
    fi
    if [ ! -s "${metrics_file}" ]; then
        return 0
    fi

    awk '
        {
            metric = $2
            value = $3 + 0
            count[metric]++
            sum[metric] += value
            if (!(metric in min) || value < min[metric]) {
                min[metric] = value
            }
            if (!(metric in max) || value > max[metric]) {
                max[metric] = value
            }
        }
        END {
            order[1] = "final_blit_ns"
            order[2] = "final_chrome_ns"
            order[3] = "final_input_to_present_ns"
            order[4] = "max_input_to_present_ns"
            order[5] = "final_predraw_ns"
            order[6] = "final_buffer_ns"
            order[7] = "max_input_buffer_ns"
            order[8] = "final_render_ns"
            order[9] = "final_present_ns"
            order[10] = "final_draw_ns"
            order[11] = "final_draw_overhead_ns"
            order[12] = "final_overhead_ns"
            order[13] = "final_handler_overhead_ns"
            order[14] = "final_total_ns"
            print "gui_probe: summary_runs=" count["final_total_ns"]
            for (metric_index = 1; metric_index <= 14; metric_index++) {
                metric = order[metric_index]
                if (count[metric] > 0) {
                    avg = int((sum[metric] / count[metric]) + 0.5)
                    printf "gui_probe: summary_%s=min:%d avg:%d max:%d\n",
                        metric, min[metric], avg, max[metric]
                }
            }
        }
    ' "${metrics_file}"
}

run_probe_once() {
    local run_index="$1"
    local log_file="${work_dir}/run-${run_index}.log"
    local final_timing

    if [ "${runs}" -gt 1 ]; then
        echo "==> GUI probe run ${run_index}/${runs}"
    fi

    if ! timeout "${probe_timeout_seconds}s" env "${env_args[@]}" \
        "${binary}" \
        --backend=winit \
        "--display-backend=${display_backend}" \
        "${url}" >"${log_file}" 2>&1; then
        cat "${log_file}"
        echo "gui_probe: app did not exit cleanly" >&2
        exit 1
    fi

    cat "${log_file}"

    if [ "${probe}" = "chrome" ]; then
        check_chrome_probe_log "${log_file}"
        return 0
    fi
    if [ "${probe}" = "hover" ]; then
        check_hover_probe_log "${log_file}"
        return 0
    fi
    if [ "${probe}" = "stop" ]; then
        check_stop_probe_log "${log_file}"
        return 0
    fi
    if [ "${probe}" = "page-input" ]; then
        check_page_input_probe_log "${log_file}"
        focus_timing="$(extract_page_input_focus_timing "${log_file}")"
        if [ -z "${focus_timing}" ]; then
            echo "gui_probe: missing page input focus frame timing" >&2
            exit 1
        fi
        check_max_metric "${focus_timing}" focus_total_ns "${max_focus_total_ns}"
        printf '%s\n' "${focus_timing}"
        final_timing="$(extract_final_input_frame_timing "${log_file}")"
        if [ -n "${final_timing}" ]; then
            check_max_metric "${final_timing}" final_input_to_present_ns "${max_input_ns}"
            check_max_metric "${final_timing}" max_input_to_present_ns "${max_any_input_ns}"
            check_max_metric "${final_timing}" final_buffer_ns "${max_buffer_ns}"
            check_max_metric "${final_timing}" max_input_buffer_ns "${max_any_buffer_ns}"
            check_max_metric "${final_timing}" final_render_ns "${max_render_ns}"
            check_max_metric "${final_timing}" final_overhead_ns "${max_overhead_ns}"
            check_max_metric "${final_timing}" final_total_ns "${max_total_ns}"
            record_metrics "${run_index}" "${final_timing}"
            printf '%s\n' "${final_timing}"
        fi
        return 0
    fi
    if [ "${probe}" = "form-submit" ]; then
        check_form_submit_probe_log "${log_file}"
        return 0
    fi
    if [ "${probe}" = "reload" ]; then
        check_reload_probe_log "${log_file}"
        return 0
    fi
    if [ "${probe}" = "scroll" ]; then
        check_scroll_probe_log "${log_file}"
        return 0
    fi
    if [ "${probe}" = "smoke" ]; then
        check_smoke_probe_log "${log_file}"
        return 0
    fi
    if [ "${probe}" = "address-caret" ]; then
        check_address_caret_probe_log "${log_file}"
    fi

    if ! grep -q 'frame: .*input_to_present .*total' "${log_file}"; then
        echo "gui_probe: missing final address text frame timing" >&2
        exit 1
    fi

    final_timing="$(extract_final_input_frame_timing "${log_file}")"
    if [ -z "${final_timing}" ]; then
        echo "gui_probe: missing final address text frame timing" >&2
        exit 1
    fi

    check_max_metric "${final_timing}" final_input_to_present_ns "${max_input_ns}"
    check_max_metric "${final_timing}" max_input_to_present_ns "${max_any_input_ns}"
    check_max_metric "${final_timing}" final_buffer_ns "${max_buffer_ns}"
    check_max_metric "${final_timing}" max_input_buffer_ns "${max_any_buffer_ns}"
    check_max_metric "${final_timing}" final_render_ns "${max_render_ns}"
    check_max_metric "${final_timing}" final_overhead_ns "${max_overhead_ns}"
    check_max_metric "${final_timing}" final_total_ns "${max_total_ns}"
    record_metrics "${run_index}" "${final_timing}"
    printf '%s\n' "${final_timing}"
}

check_reload_probe_log() {
    local log_file="$1"

    if [ -z "${fixture}" ]; then
        echo "gui_probe: reload probe requires --fixture" >&2
        exit 1
    fi
    if ! grep -q "probe input: Reload" "${log_file}"; then
        echo "gui_probe: missing reload probe input" >&2
        exit 1
    fi
    if ! grep -q "Navigation complete: ${url}" "${log_file}"; then
        echo "gui_probe: missing reload navigation completion" >&2
        exit 1
    fi
    if ! grep -q "Image cache hit:" "${log_file}"; then
        echo "gui_probe: missing decoded image cache hit" >&2
        exit 1
    fi
    if grep -q "Image .*: fetch error:" "${log_file}"; then
        echo "gui_probe: reload image fetch failed" >&2
        exit 1
    fi
    echo "gui_probe: reload image cache OK"
}

check_smoke_probe_log() {
    local log_file="$1"

    if ! grep -Eq "\[SilkSurf\] Navigation (cache hit|fetched|fetched via|posted):" "${log_file}"; then
        echo "gui_probe: missing smoke navigation load" >&2
        exit 1
    fi
    if grep -q "Navigation error:" "${log_file}"; then
        echo "gui_probe: smoke navigation failed" >&2
        exit 1
    fi
    if grep -q "Image .*: fetch error:" "${log_file}"; then
        echo "gui_probe: smoke image fetch failed" >&2
        exit 1
    fi
    if ! grep -q "frame: .*damage Full" "${log_file}"; then
        echo "gui_probe: smoke frame did not present" >&2
        exit 1
    fi

    echo "gui_probe: smoke navigation OK"
}

check_address_caret_probe_log() {
    local log_file="$1"

    if ! grep -q "probe input: MoveCaretLeft" "${log_file}"; then
        echo "gui_probe: missing address caret move input" >&2
        exit 1
    fi
    if ! grep -q "probe input: TextInput('b')" "${log_file}"; then
        echo "gui_probe: missing address middle-insert input" >&2
        exit 1
    fi

    echo "gui_probe: address caret OK"
}

check_scroll_probe_log() {
    local log_file="$1"

    if [ -z "${fixture}" ]; then
        echo "gui_probe: scroll probe requires --fixture" >&2
        exit 1
    fi
    if ! grep -q "probe input: ScrollPixels(96.0)" "${log_file}"; then
        echo "gui_probe: missing scroll-down probe input" >&2
        exit 1
    fi
    if ! grep -q "probe input: ScrollPixels(-48.0)" "${log_file}"; then
        echo "gui_probe: missing scroll-up probe input" >&2
        exit 1
    fi
    if grep -q "bitmap refresh: ScrollReuse" "${log_file}"; then
        echo "gui_probe: scroll reuse OK"
        return
    fi

    local retained_scroll_frames
    retained_scroll_frames="$(awk '
        /probe input: ScrollPixels/ { armed = 1; next }
        armed && /\[SilkSurf\] frame:/ {
            if ($0 ~ /render 0ns/) {
                count++
            }
            armed = 0
        }
        END { print count + 0 }
    ' "${log_file}")"
    if [ "${retained_scroll_frames}" -lt 2 ]; then
        echo "gui_probe: scroll did not use retained presentation or retained bitmap rows" >&2
        exit 1
    fi

    echo "gui_probe: scroll retained presenter OK"
}

check_chrome_probe_log() {
    local log_file="$1"
    local root_url
    local start_url
    local root_completions

    if [ -z "${fixture}" ]; then
        echo "gui_probe: chrome probe requires --fixture" >&2
        exit 1
    fi

    root_url="${url%start/}"
    start_url="${url}"

    for click in 'x: 55.0' 'x: 15.0' 'x: 35.0' 'x: 75.0'; do
        if ! grep -q "probe input: PrimaryClick { ${click}, y: 22.0 }" "${log_file}"; then
            echo "gui_probe: missing chrome probe click ${click}" >&2
            exit 1
        fi
    done

    root_completions="$(
        grep -Fxc "[SilkSurf] Navigation complete: ${root_url}" "${log_file}" || true
    )"
    if [ "${root_completions}" -lt 3 ]; then
        echo "gui_probe: expected home, forward, and reload completions for ${root_url}" >&2
        exit 1
    fi
    if ! grep -Fxq "[SilkSurf] Navigation complete: ${start_url}" "${log_file}"; then
        echo "gui_probe: expected back completion for ${start_url}" >&2
        exit 1
    fi

    echo "gui_probe: chrome navigation OK"
}

check_hover_probe_log() {
    local log_file="$1"

    if [ "${fixture}" != "ai-chat" ]; then
        echo "gui_probe: hover probe requires --fixture ai-chat" >&2
        exit 1
    fi
    if ! grep -Fq "probe input: CursorMoved { x: 48.0, y: 220.0 }" "${log_file}"; then
        echo "gui_probe: missing hover-over-link cursor move" >&2
        exit 1
    fi
    if ! grep -Fq "probe input: CursorMoved { x: 420.0, y: 420.0 }" "${log_file}"; then
        echo "gui_probe: missing hover-away cursor move" >&2
        exit 1
    fi
    if ! grep -Fq "damage Rect(WinitDamageRect { x: 1000, y: 8, width: 170, height: 28 })" "${log_file}" \
        && ! grep -Fq "damage Rect(WinitDamageRect { x: 1010, y: 14, width: 160, height: 7 })" "${log_file}"; then
        echo "gui_probe: hover did not use status-only damage" >&2
        exit 1
    fi

    echo "gui_probe: hover status damage OK"
}

check_stop_probe_log() {
    local log_file="$1"
    local slow_url

    if [ -z "${fixture}" ]; then
        echo "gui_probe: stop probe requires --fixture" >&2
        exit 1
    fi

    slow_url="${url}slow/"
    if ! grep -q "probe input: SubmitAddress" "${log_file}"; then
        echo "gui_probe: missing stop probe submit" >&2
        exit 1
    fi
    if ! grep -q "probe input: PrimaryClick { x: 95.0, y: 22.0 }" "${log_file}"; then
        echo "gui_probe: missing stop probe click" >&2
        exit 1
    fi
    if ! grep -Fxq "[SilkSurf] Navigation stopped" "${log_file}"; then
        echo "gui_probe: missing stop confirmation" >&2
        exit 1
    fi
    if grep -Fxq "[SilkSurf] Navigation complete: ${slow_url}" "${log_file}"; then
        echo "gui_probe: slow navigation completed after stop" >&2
        exit 1
    fi
    if grep -q "frame: Wayland buffers busy" "${log_file}"; then
        echo "gui_probe: stop probe hit Wayland SHM buffer exhaustion" >&2
        exit 1
    fi

    echo "gui_probe: stop navigation OK"
}

check_page_input_probe_log() {
    local log_file="$1"

    if ! grep -q "probe input: FocusNextPageInput" "${log_file}"; then
        echo "gui_probe: missing page input focus probe" >&2
        exit 1
    fi
    if ! grep -q "probe input: TextInput('!')" "${log_file}"; then
        echo "gui_probe: missing page input text probe" >&2
        exit 1
    fi
    if ! grep -q "Page input focused: node=" "${log_file}"; then
        echo "gui_probe: page input focus did not reach DOM target" >&2
        exit 1
    fi
    if ! grep -q "Page input updated: node=" "${log_file}"; then
        echo "gui_probe: page input edit did not repaint" >&2
        exit 1
    fi
    if [ "${fixture}" = "ai-chat" ]; then
        if ! grep -q "Modulepreload .*module.js:" "${log_file}"; then
            echo "gui_probe: ai-chat modulepreload was not fetched" >&2
            exit 1
        fi
        if ! grep -q "Modulepreload .*module-child.js:" "${log_file}"; then
            echo "gui_probe: ai-chat module graph child was not fetched" >&2
            exit 1
        fi
        if ! grep -q "Navigation DOM body data-module-graph=module-child" "${log_file}"; then
            echo "gui_probe: ai-chat module graph did not execute" >&2
            exit 1
        fi
        if ! grep -q "Script .*app.js:" "${log_file}"; then
            echo "gui_probe: ai-chat external script was not fetched" >&2
            exit 1
        fi
        if ! grep -q "Navigation script 0 done:" "${log_file}"; then
            echo "gui_probe: ai-chat script did not execute" >&2
            exit 1
        fi
        if ! grep -q "Navigation DOM body data-fixture=ai-chat" "${log_file}"; then
            echo "gui_probe: ai-chat script did not mutate body data-fixture" >&2
            exit 1
        fi
        if ! grep -q "Navigation DOM script src=/dynamic.js text_bytes=" "${log_file}"; then
            echo "gui_probe: ai-chat dynamic script node was not preserved" >&2
            exit 1
        fi
        if ! grep -q "Navigation dynamic script 0.0 done:" "${log_file}"; then
            echo "gui_probe: ai-chat dynamic script did not execute" >&2
            exit 1
        fi
        if ! grep -q "Navigation DOM body data-dynamic-script=fetched" "${log_file}"; then
            echo "gui_probe: ai-chat dynamic script did not mutate body" >&2
            exit 1
        fi
    fi

    echo "gui_probe: page input OK"
}

check_form_submit_probe_log() {
    local log_file="$1"
    local result_url

    if [ "${fixture}" != "form-submit" ] && [ "${fixture}" != "post-submit" ]; then
        echo "gui_probe: form-submit probe requires --fixture form-submit or --fixture post-submit" >&2
        exit 1
    fi

    if [ "${fixture}" = "post-submit" ]; then
        result_url="${url}posted/"
    else
        result_url="${url}results/?q=silk%21&mode=fast&opt=on&tier=pro&sort=recent"
    fi
    if ! grep -q "probe input: FocusNextPageInput" "${log_file}"; then
        echo "gui_probe: missing form input focus probe" >&2
        exit 1
    fi
    if ! grep -q "probe input: TextInput('!')" "${log_file}"; then
        echo "gui_probe: missing form text probe" >&2
        exit 1
    fi
    if ! grep -q "probe input: SubmitAddress" "${log_file}"; then
        echo "gui_probe: missing form submit probe" >&2
        exit 1
    fi
    if ! grep -Fxq "[SilkSurf] Navigating: ${result_url}" "${log_file}"; then
        echo "gui_probe: missing form navigation target" >&2
        exit 1
    fi
    if ! grep -Fxq "[SilkSurf] Navigation complete: ${result_url}" "${log_file}"; then
        echo "gui_probe: missing form navigation completion" >&2
        exit 1
    fi
    if [ "${fixture}" = "post-submit" ]; then
        if ! grep -q "Navigation posted:" "${log_file}"; then
            echo "gui_probe: missing POST navigation fetch" >&2
            exit 1
        fi
        if ! grep -Fxq "POST /posted/ q=silk%21&mode=fast&opt=on&tier=pro&sort=recent" "${fixture_server_log_file}"; then
            echo "gui_probe: fixture server did not receive expected POST body" >&2
            exit 1
        fi
    fi

    echo "gui_probe: form submit OK"
}

run_index=1
while [ "${run_index}" -le "${runs}" ]; do
    run_probe_once "${run_index}"
    run_index=$((run_index + 1))
done

summarize_metrics
echo "gui_probe: OK"
