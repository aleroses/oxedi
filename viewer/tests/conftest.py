"""Fixtures for the browser tests: a local server for viewer/ and a page that fails on CSP trouble."""

from __future__ import annotations

import functools
import http.server
import os
import pathlib
import threading
from collections.abc import Mapping

import pytest
from playwright.sync_api import expect

ROOT = pathlib.Path(__file__).resolve().parents[2]
VIEWER = ROOT / "viewer"
FIXTURES = ROOT / "crates" / "oxedi_core" / "tests" / "fixtures"
SAMPLES = ROOT / "crates" / "oxedi_core" / "tests" / "samples"

CSP = (
    "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; "
    "img-src 'self' data:; connect-src 'none'; base-uri 'none'; form-action 'none'"
)

# Records every CSP violation the page reports, before any page script runs.
WATCH_CSP = """
window.__violations = [];
document.addEventListener('securitypolicyviolation', (e) => {
  window.__violations.push(`${e.violatedDirective} blocked ${e.blockedURI}`);
});
"""


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:
        pass


def chromium_launch_args(launch_args: dict, environ: Mapping[str, str]) -> dict:
    """Adds --no-sandbox only when OXEDI_CHROMIUM_NO_SANDBOX is exactly "1", which only the sandbox
    image sets: the container is the sandbox there. Existing args are kept."""
    if environ.get("OXEDI_CHROMIUM_NO_SANDBOX") != "1":
        return launch_args
    return {**launch_args, "args": [*launch_args.get("args", []), "--no-sandbox"]}


@pytest.fixture(scope="session")
def browser_type_launch_args(browser_type_launch_args: dict) -> dict:
    return chromium_launch_args(browser_type_launch_args, os.environ)


@pytest.fixture(scope="session")
def base_url() -> str:
    """Serves viewer/ on a free port for the whole session."""
    handler = functools.partial(QuietHandler, directory=str(VIEWER))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{server.server_port}"
    server.shutdown()
    server.server_close()


class Viewer:
    """The open page, with helpers for the intake and the errors it collected."""

    def __init__(self, page, url: str) -> None:
        self.page = page
        self.url = url
        self.console_errors: list[str] = []
        self.page_errors: list[str] = []
        self.expect_violations = False
        page.on("console", self._console)
        page.on("pageerror", lambda err: self.page_errors.append(str(err)))
        page.add_init_script(WATCH_CSP)

    def _console(self, message) -> None:
        if message.type == "error":
            self.console_errors.append(message.text)

    def open(self) -> None:
        self.page.goto(f"{self.url}/index.html")
        # Locator waits, not string predicates: those are evaluated as script, which the CSP forbids.
        expect(self.page.locator("#sample")).to_be_enabled()

    def load(self, name_or_path, buffer: bytes | None = None) -> None:
        """Gives the file input a path or an in-memory file."""
        if buffer is None:
            self.page.set_input_files("#file", str(name_or_path))
        else:
            self.page.set_input_files(
                "#file", {"name": name_or_path, "mimeType": "application/octet-stream", "buffer": buffer}
            )

    def drop(self, name: str, buffer: bytes) -> None:
        """Dispatches a drop of one in-memory file on the page, as the browser would."""
        self.page.evaluate(
            """([name, bytes]) => {
              const transfer = new DataTransfer();
              transfer.items.add(new File([new Uint8Array(bytes)], name));
              window.dispatchEvent(new DragEvent('drop', { dataTransfer: transfer, bubbles: true, cancelable: true }));
            }""",
            [name, list(buffer)],
        )

    def wait_loaded(self) -> None:
        self.page.wait_for_selector("#results:not([hidden])")

    def load_sample(self) -> None:
        self.page.click("#sample")
        self.wait_loaded()

    def tab(self, name: str) -> None:
        self.page.click(f"#tab-{name}")

    def violations(self) -> list[str]:
        return self.page.evaluate("window.__violations")


@pytest.fixture
def viewer(page, base_url):
    """A fresh page on the viewer; the test fails on a console error or a CSP violation."""
    v = Viewer(page, base_url)
    yield v
    assert v.page_errors == [], f"uncaught errors: {v.page_errors}"
    if not v.expect_violations:
        assert v.violations() == [], f"CSP violations: {v.violations()}"
        assert v.console_errors == [], f"console errors: {v.console_errors}"
