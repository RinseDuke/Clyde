"""Browser regression check against built dist (mock Tauri IPC, no live hooks).

Run from any directory after npm run build. The server uses a temporary port
and closes automatically, without spawning any background server process.
Requires Python Playwright and Microsoft Edge. This verifies rendering and IPC
ordering; it does not replace Windows/multiple-monitor native drag acceptance.
"""
import json
from contextlib import contextmanager
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import gettempdir
from threading import Thread
from playwright.sync_api import sync_playwright

MOCK = r"""
window.mockCalls = [];
let next = 0;
const callbacks = new Map();
const events = new Map();
window.mockEmit = (event, payload) => {
  for (const id of events.get(event) || []) callbacks.get(id)({ event, id, payload });
};
window.__TAURI_INTERNALS__ = {
  transformCallback(callback) { callbacks.set(++next, callback); return next; },
  async invoke(command, args) {
    window.mockCalls.push({ command, args });
    if (command === 'plugin:event|listen') {
      events.set(args.event, [...(events.get(args.event) || []), args.handler]);
      return args.handler;
    }
    if (command === 'get_interaction_state') return { position_locked: false, click_through: false };
    if (command === 'get_pet_config') return { opacity: 1 };
    if (command === 'get_current_hit_layout') {
      const d = window.devicePixelRatio;
      return { width: 120*d, height: 150*d, regions: [
        { x: 0, y: 0, width: 120*d, height: 108*d },
        { x: 12*d, y: 90*d, width: 96*d, height: 60*d }
      ] };
    }
    if (command === 'drag_start') return ++next;
    if (command === 'plugin:window|cursor_position') return { x: 1880, y: 720 };
    return null;
  }
};
window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
"""

@contextmanager
def serve_dist():
    class QuietHandler(SimpleHTTPRequestHandler):
        def log_message(self, *args):
            pass
    dist = Path(__file__).resolve().parents[2] / "dist"
    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietHandler, directory=str(dist)))
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


with serve_dist() as base_url, sync_playwright() as p:
    browser = p.chromium.launch(channel="msedge", headless=True)
    results = []
    for dpi in [1, 1.25, 1.5, 2]:
        context = browser.new_context(viewport={"width": 120, "height": 150}, device_scale_factor=dpi)
        context.add_init_script(MOCK)
        page = context.new_page()
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto(f"{base_url}/src/windows/hit/index.html")
        page.wait_for_load_state("networkidle")
        zones = page.locator(".hit-zone")
        assert zones.count() == 2
        box = zones.nth(1).bounding_box()
        for key, expected in {"x": 12, "y": 90, "width": 96, "height": 60}.items():
            assert abs(box[key] - expected) < 0.1, (dpi, box)
        page.evaluate("mockEmit('snap-preview', {active:true, side:'right'})")
        assert zones.first.evaluate("el => getComputedStyle(el, '::after').content") == "none"
        page.mouse.move(20, 30)
        page.mouse.down()
        assert zones.first.evaluate("el => getComputedStyle(el).outlineStyle") == "none"
        page.mouse.move(60, 90)
        page.mouse.up()
        page.wait_for_function("mockCalls.some(c => c.command === 'drag_end')")
        released = page.evaluate("mockCalls.find(c => c.command === 'drag_end').args")
        assert released["x"] == 1880 and released["y"] == 720 and not released["cancelled"]
        page.evaluate("mockCalls.length = 0")
        page.mouse.move(20, 30)
        page.mouse.down()
        page.wait_for_function("mockCalls.some(c => c.command === 'drag_start')")
        zones.first.dispatch_event("pointercancel", {"pointerId": 1, "button": 0})
        page.wait_for_function("mockCalls.some(c => c.command === 'drag_end')")
        cancelled = page.evaluate("mockCalls.find(c => c.command === 'drag_end').args")
        assert cancelled["cancelled"] and cancelled["x"] is None
        page.mouse.up()
        page.screenshot(path=str(Path(gettempdir()) / f"clyde-hit-fixed-{dpi}.png"))
        assert not errors, errors
        context.close()
        results.append({"dpi": dpi, "hit_layout": "pass", "blue_outline": "absent", "release_cancel": "pass"})
    page = browser.new_page(viewport={"width": 200, "height": 200})
    page.add_init_script(MOCK)
    page.goto(f"{base_url}/src/windows/pet/index.html")
    page.wait_for_load_state("networkidle")
    page.evaluate("mockEmit('snap-preview', {active:true, side:'right'})")
    assert page.locator("#pet-container").evaluate("el => getComputedStyle(el).transform") == "none"
    assert page.locator("#pet-container").evaluate("el => getComputedStyle(el).opacity") == "1"
    page.screenshot(path=str(Path(gettempdir()) / "clyde-pet-fixed.png"))
    browser.close()
    print(json.dumps({"hit": results, "pet_scale_opacity": "unchanged"}))
