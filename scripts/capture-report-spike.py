#!/usr/bin/env python3
"""Capture live vs foreignObject PNG for the report poster spike.

Requires the Vite dev server at http://localhost:1420 and Playwright browsers.
This is a local spike helper, not a CI test.
"""
from __future__ import annotations

import base64
import pathlib
import sys

from playwright.sync_api import sync_playwright

URL = "http://localhost:1420/report-spike.html"
OUT_DIR = pathlib.Path(__file__).resolve().parents[1] / "docs" / "spikes" / "report-poster"


def save_data_url(data_url: str, path: pathlib.Path) -> None:
    marker = "base64,"
    index = data_url.find(marker)
    if index < 0:
        raise RuntimeError(f"unexpected data URL for {path.name}")
    path.write_bytes(base64.b64decode(data_url[index + len(marker) :]))


def capture(browser_name: str) -> None:
    with sync_playwright() as playwright:
        browser_type = getattr(playwright, browser_name)
        launch_kwargs = {"headless": True}
        if browser_name == "chromium":
            launch_kwargs["channel"] = "chrome"
        browser = browser_type.launch(**launch_kwargs)
        page = browser.new_page(viewport={"width": 1600, "height": 1800})
        page.goto(URL, wait_until="networkidle")
        page.evaluate("() => document.fonts.ready")
        poster = page.locator("#report-poster")
        poster.wait_for()
        live_path = OUT_DIR / f"live-{browser_name}.png"
        poster.screenshot(path=str(live_path))
        page.locator('[data-spike="capture-btn"]').click()
        page.wait_for_function("() => window.__SPIKE_READY__ === true")
        error = page.evaluate("() => window.__SPIKE_ERROR__ || null")
        if error:
            raise RuntimeError(f"{browser_name} capture failed: {error}")
        data_url = page.evaluate("() => window.__SPIKE_PNG__ || null")
        if not data_url:
            raise RuntimeError(f"{browser_name} produced an empty PNG")
        save_data_url(data_url, OUT_DIR / f"capture-{browser_name}.png")
        browser.close()
        print(f"wrote {browser_name} live + capture PNGs")


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    engines = sys.argv[1:] or ["chromium", "webkit"]
    for name in engines:
        capture(name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
