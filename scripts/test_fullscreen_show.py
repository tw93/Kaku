#!/usr/bin/env python3
"""Exercise Show and update confirmation in an isolated macOS app bundle.

Run after make app:
    python3 scripts/test_fullscreen_show.py dist/Kaku.app

Requires a logged-in macOS desktop. Only the test instance enters fullscreen.
No update is downloaded or applied; every update overlay is cancelled.
"""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest


LUA = r"""
local wezterm = require 'wezterm'
local mode = os.getenv('KAKU_SHOW_TEST_MODE')
local function record(message)
  local file = assert(io.open(os.getenv('KAKU_SHOW_TEST_RESULT'), 'a'))
  file:write(message .. '\n')
  file:close()
end
local function wait_until(label, predicate)
  for attempt = 1, 100 do
    if predicate() then
      record(label)
      return
    end
    wezterm.sleep_ms(100)
  end
  error('timeout: ' .. label)
end
wezterm.on('gui-startup', function()
  local tab, pane, mux_window = wezterm.mux.spawn_window {
    args = { '/bin/cat' },
  }
  local window = mux_window:gui_window()
  wezterm.time.call_after(1, function()
    local ok, failure = pcall(function()
      record('started')
      local fullscreen = mode ~= 'windowed'
      if fullscreen then
        window:perform_action(wezterm.action.ToggleFullScreen, pane)
        window:perform_action(wezterm.action.Show, pane)
      end
      wait_until('initial-state', function()
        return window:get_dimensions().is_full_screen == fullscreen
      end)
      wezterm.sleep_ms(1500)
      local original = window:get_dimensions()
      local function check_geometry()
        local current = window:get_dimensions()
        assert(current.is_full_screen == fullscreen, 'Show changed fullscreen state')
        assert(current.pixel_width == original.pixel_width and
               current.pixel_height == original.pixel_height,
               'Show changed content dimensions')
      end
      record('update-begin')
      window:perform_action(wezterm.action.EmitEvent 'restart-to-update', pane)
      wait_until('update-overlay', function()
        return window:active_pane():pane_id() ~= pane:pane_id()
      end)
      local overlay = window:active_pane()
      wait_until('update-prompt', function()
        return overlay:get_lines_as_text():find('Update Kaku now?', 1, true) ~= nil
      end)
      window:perform_action(wezterm.action.SendKey { key = 'Escape' }, overlay)
      wait_until('update-cancelled', function()
        return window:active_pane():pane_id() == pane:pane_id()
      end)
      check_geometry()
      for attempt = 1, 3 do
        record('show-' .. attempt .. '-begin')
        window:perform_action(wezterm.action.Show, pane)
        check_geometry()
        record('show-' .. attempt .. '-done')
      end
      window:perform_action(wezterm.action.SendString 'show-regression-input\n', pane)
      wait_until('input-responsive', function()
        return pane:get_lines_as_text():find('show-regression-input', 1, true) ~= nil
      end)
      if fullscreen then
        window:perform_action(wezterm.action.ToggleFullScreen, pane)
        window:perform_action(wezterm.action.Show, pane)
        wait_until('exited-fullscreen', function()
          return not window:get_dimensions().is_full_screen
        end)
        wezterm.sleep_ms(1500)
        window:perform_action(wezterm.action.Show, pane)
        assert(not window:get_dimensions().is_full_screen)
      end
      record('PASS')
    end)
    if not ok then
      record('FAIL: ' .. tostring(failure))
    end
  end)
end)
return {
  check_for_updates = false,
  restore_previous_session = false,
  remote = { enabled = false, tunnel = false },
  native_macos_fullscreen_mode = mode == 'native',
  window_close_confirmation = 'NeverPrompt',
  window_decorations = mode == 'simple' and 'TITLE|RESIZE' or 'INTEGRATED_BUTTONS|RESIZE',
  use_fancy_tab_bar = false,
  front_end = 'WebGpu',
  initial_cols = 90,
  initial_rows = 24,
}
"""


class FullscreenShowTests(unittest.TestCase):
    def run_mode(self, mode):
        with tempfile.TemporaryDirectory(prefix="kaku-show-", dir="/tmp") as temporary:
            root = Path(temporary)
            home = root / "home"
            scratch = root / "tmp"
            home.mkdir()
            scratch.mkdir()
            config = root / "kaku.lua"
            config.write_text(LUA)
            result = root / "result.txt"
            env = {
                key: value for key, value in os.environ.items()
                if not key.startswith(("KAKU_", "WEZTERM_", "XDG_"))
                and key not in ("SSH_AUTH_SOCK", "SSH_AGENT_PID")
            }
            env.update({
                "HOME": str(home),
                "TMPDIR": str(scratch) + "/",
                "XDG_CONFIG_HOME": str(home / ".config"),
                "XDG_DATA_HOME": str(home / ".local/share"),
                "XDG_CACHE_HOME": str(home / ".cache"),
                "KAKU_SHOW_TEST_MODE": mode,
                "KAKU_SHOW_TEST_RESULT": str(result),
                "KAKU_UPDATE_PROVIDER": "disabled-for-show-test",
                "KAKU_UPDATE_TARGET_APP": str(root / "updates-disabled"),
                "RUST_BACKTRACE": "1",
            })
            with (root / "stderr.log").open("w+") as output:
                process = subprocess.Popen(
                    [str(BINARY), "--config-file", str(config), "start", "--always-new-process"],
                    env=env, stdout=output, stderr=output,
                )
                try:
                    deadline = time.monotonic() + 45
                    events = []
                    while time.monotonic() < deadline:
                        events = result.read_text().splitlines() if result.exists() else []
                        if "PASS" in events or any(event.startswith("FAIL:") for event in events):
                            break
                        if process.poll() is not None:
                            break
                        time.sleep(0.1)
                    output.seek(0)
                    log = output.read()
                    self.assertIn(
                        "PASS", events,
                        f"{mode}: exit={process.poll()}, events={events}\n{log[-6000:]}",
                    )
                    self.assertIsNone(process.poll(), "The app exited before verification completed")
                    self.assertNotIn("panicked at", log)
                    self.assertNotIn("NSWindowStyleMaskFullScreen cleared", log)
                    self.assertIn("update-prompt", events)
                    self.assertIn("update-cancelled", events)
                    self.assertIn("input-responsive", events)
                finally:
                    if process.poll() is None:
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait(timeout=5)

    def test_show_windowed(self):
        self.run_mode("windowed")

    def test_show_native_fullscreen(self):
        self.run_mode("native")

    def test_show_simple_fullscreen(self):
        self.run_mode("simple")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", nargs="?", default="dist/Kaku.app")
    options, remaining = parser.parse_known_args()
    BINARY = Path(options.app).absolute() / "Contents/MacOS/kaku-gui"
    if not BINARY.is_file():
        parser.error(f"build the app first: {BINARY}")
    unittest.main(argv=[__file__, *remaining])
