#!/usr/bin/env python3
"""Regression contract: Nolune has no passive screen capture stack."""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


class NoContinuousScreenRecordingTest(unittest.TestCase):
    def assert_tokens_absent(self, relative_path: str, tokens: tuple[str, ...]) -> None:
        source = (ROOT / relative_path).read_text()
        for token in tokens:
            with self.subTest(path=relative_path, token=token):
                self.assertNotIn(token, source)

    def test_legacy_capture_protocol_is_absent_from_production_files(self) -> None:
        contracts = {
            "desktop/src-tauri/src/computer_use_bridge.rs": (
                "ScreenFrame",
                '"screen_frame"',
                "start_recording",
                "stop_recording",
            ),
            "desktop/src-tauri/src/cua_runtime.rs": (
                "ScreenFrame",
                '"screen_frame"',
                "start_recording",
                "stop_recording",
            ),
            "desktop/src-tauri/src/lib.rs": ("screen_recorder",),
            "server/src/services/machine_registry.rs": (
                "ScreenFrame",
                "screen_frame",
                "screen_recording_allowed",
            ),
            "server/src/routes/machine_agents.rs": (
                "live-frame",
                "save_screen_observation",
                "screen_frame",
            ),
            "server/src/services/tools/mod.rs": (
                "CollectScreenRecordingTool",
                "SaveScreenObservationTool",
                "tools::screen",
            ),
            "server/src/services/heartbeat.rs": ("builtin_observer",),
        }
        for path, tokens in contracts.items():
            self.assert_tokens_absent(path, tokens)

    def test_recording_routes_and_overlays_are_absent(self) -> None:
        self.assert_tokens_absent(
            "client/src/routes/[slug]/+layout.svelte",
            ("/observations", "/live", "Observations", "Live screen"),
        )
        for overlay in (
            "client/src/routes/overlay/[slug]/+page.svelte",
            "desktop/src/routes/overlay/+page.svelte",
        ):
            self.assert_tokens_absent(
                overlay,
                ("recording", "pip-rec", "rec-pulse", ">REC<", " REC "),
            )

        removed_paths = (
            ROOT / "desktop/src-tauri/src/screen_recorder.rs",
            # The legacy executor (#19): enigo automation and the app's own
            # screenshots capture with its scaling and scale cache.
            ROOT / "desktop/src-tauri/src/computer_use.rs",
            ROOT / "desktop/src/lib/computer-use.ts",
            ROOT / "server/src/services/tools/screen.rs",
            ROOT / "client/src/lib/components/observations/ObservationsView.svelte",
            ROOT / "client/src/routes/[slug]/observations/+page.svelte",
            ROOT / "client/src/routes/[slug]/live/+page.svelte",
        )
        for path in removed_paths:
            with self.subTest(path=path):
                self.assertFalse(path.exists())

    def test_explicit_computer_use_and_remote_tools_remain(self) -> None:
        desktop_bridge = (ROOT / "desktop/src-tauri/src/computer_use_bridge.rs").read_text()
        desktop_cua = (ROOT / "desktop/src-tauri/src/cua_runtime.rs").read_text()
        desktop_manifest = (ROOT / "desktop/src-tauri/Cargo.toml").read_text()
        server_tools = (ROOT / "server/src/services/tools/mod.rs").read_text()
        typed_tools = (ROOT / "server/src/services/tools/cua.rs").read_text()
        # The desktop captures nothing on its own (#19): no screenshot
        # toolcall, no enigo, no screenshots crate. The only capture it takes
        # part in is the window snapshot a typed `get_window_state` asks for,
        # and the toolcalls left are the shell and file ones.
        self.assertNotIn('"screenshot" =>', desktop_bridge)
        for executor in ("enigo", "screenshots::", "cached_scale", "computer_use::"):
            self.assertNotIn(executor, desktop_bridge)
        for crate in ("enigo", "screenshots", "image"):
            self.assertNotIn(f"\n{crate} =", desktop_manifest)
        self.assertIn('"get_window_state"', desktop_bridge)
        for toolcall in ('"bash" =>', '"file_read" =>', '"file_write" =>', '"file_list" =>'):
            self.assertIn(toolcall, desktop_bridge)
        # The desktop's Cua driver (#17) is the driver's one-shot MCP surface:
        # the only subcommand it ever runs is `mcp`, and every capture is a
        # window snapshot the server asked for by name.
        self.assertIn('.arg("mcp")', desktop_cua)
        for subcommand in ('"serve"', '"record"', '"stream"', '"watch"'):
            self.assertNotIn(f".arg({subcommand})", desktop_cua)
        self.assertIn("get_window_state", desktop_cua)
        # The model's only capture is the one-shot `get_window_state` takes
        # when asked (#18): the typed tools are registered, the coordinate
        # `computer_use` tool is gone with its type (#19), and the remote
        # shell and file tools remain.
        self.assertIn("GetWindowStateTool::new", server_tools)
        self.assertIn("ActTool::new", server_tools)
        self.assertNotIn("ComputerUseTool::new", server_tools)
        self.assertNotIn("ComputerUseTool", (ROOT / "server/src/services/tools/computer.rs").read_text())
        self.assertIn("include_screenshot", typed_tools)
        for continuous in ("start_recording", "stop_recording", "screen_frame"):
            self.assertNotIn(continuous, typed_tools)
        self.assertIn("RemoteBashTool::new", server_tools)
        self.assertIn("RemoteFilesTool::new", server_tools)

    def test_permission_onboarding_asks_for_one_shot_capture_only(self) -> None:
        # The desktop's permission onboarding (#20) reads the driver's own
        # report and runs the driver's grant flow; it never starts anything
        # that captures on its own.
        onboarding = (ROOT / "desktop/src-tauri/src/cua_permissions.rs").read_text()
        self.assertIn('.arg("permissions")', onboarding)
        self.assertIn('.arg("grant")', onboarding)
        for subcommand in ('"serve"', '"record"', '"recording"', '"stream"', '"watch"', '"update"'):
            self.assertNotIn(f".arg({subcommand})", onboarding)
        self.assert_tokens_absent(
            "desktop/src-tauri/src/cua_permissions.rs",
            ("ScreenFrame", "start_recording", "stop_recording", "CGDisplayStream"),
        )
        # The copy: one-shot capture during an action, in every state.
        for path in (
            "desktop/src/lib/cua-permissions.js",
            "desktop/src/routes/settings/+page.svelte",
        ):
            copy = (ROOT / path).read_text().lower()
            with self.subTest(path=path):
                self.assertIn("one-shot", copy)
                for phrase in (
                    "continuous",
                    "always on",
                    "always-on",
                    "records your screen",
                    "record your screen",
                    "watches your screen",
                    "live screen",
                ):
                    self.assertNotIn(phrase, copy)
        docs = (ROOT / "docs/computer-use.md").read_text()
        self.assertIn("## Permissions (#20)", docs)
        permissions = docs.split("## Permissions (#20)", 1)[1].split("\n## ", 1)[0]
        self.assertIn("com.trycua.driver", permissions)
        self.assertIn("one-shot", permissions)
        self.assertNotIn("continuous capture", permissions.replace("no continuous capture", ""))

    # Every file that speaks to, describes or drives a Cua driver (#21). A
    # file that moves must move here too: a missing path fails the test
    # rather than silently leaving the guard.
    CUA_FILES = (
        "cua-protocol/src/lib.rs",
        "cua-protocol/src/driver_mcp.rs",
        "cua-protocol/src/cua_driver_pin.rs",
        "server/src/services/cua/daemon.rs",
        "server/src/services/cua/desktop.rs",
        "server/src/services/cua/discovery.rs",
        "server/src/services/cua/driver.rs",
        "server/src/services/cua/host.rs",
        "server/src/services/cua/install.rs",
        "server/src/services/cua/mod.rs",
        "server/src/services/cua/orchestrator.rs",
        "server/src/services/cua/runtime.rs",
        "server/src/services/cua/session.rs",
        "server/src/services/cua/transport.rs",
        "server/src/services/tools/cua.rs",
        "server/src/services/tools/computer.rs",
        "server/src/services/machine_registry.rs",
        "server/src/routes/machine_agents.rs",
        "desktop/src-tauri/src/cua_runtime.rs",
        "desktop/src-tauri/src/cua_permissions.rs",
        "desktop/src-tauri/src/computer_use_bridge.rs",
        "desktop/src/lib/cua-permissions.js",
        "client/src/lib/computers/spaces.js",
        "scripts/cua-driver.sh",
        "scripts/release-check-computer-use.sh",
    )

    def test_every_cua_file_takes_one_shot_captures_only(self) -> None:
        # No Cua file names a recorder, a frame stream or a passive capture
        # API: the only capture anywhere is the window snapshot a
        # `get_window_state` asks for by name, and the driver is only ever
        # run as `mcp` (the desktop and the server), `serve` (the macOS
        # daemon `nolune cua status` starts by path) or `permissions grant`.
        cua_dir = ROOT / "server/src/services/cua"
        listed = {path for path in self.CUA_FILES if path.startswith("server/src/services/cua/")}
        present = {
            f"server/src/services/cua/{path.name}" for path in cua_dir.glob("*.rs")
        }
        self.assertEqual(present, listed, "every module under services/cua is listed")
        protocol_dir = ROOT / "cua-protocol/src"
        self.assertEqual(
            {f"cua-protocol/src/{path.name}" for path in protocol_dir.glob("*.rs")},
            {path for path in self.CUA_FILES if path.startswith("cua-protocol/src/")},
            "every module of cua-protocol is listed",
        )
        for relative in self.CUA_FILES:
            self.assertTrue((ROOT / relative).is_file(), f"{relative} is missing")
            self.assert_tokens_absent(
                relative,
                (
                    "start_recording",
                    "stop_recording",
                    "begin_recording",
                    "screen_frame",
                    "ScreenFrame",
                    "screen_recorder",
                    "CGDisplayStream",
                    "SCStream",
                    "AVCaptureScreen",
                    "CollectScreenRecording",
                    "live-frame",
                    ".arg(\"record\")",
                    ".arg(\"recording\")",
                    ".arg(\"stream\")",
                    ".arg(\"watch\")",
                    ".arg(\"update\")",
                ),
            )
        # The protocol names no recording or replay action, so no runtime can
        # send one: every tool name the driver mapping emits is a discovery,
        # observation, action, verification, session or health call.
        mapping = (ROOT / "cua-protocol/src/driver_mcp.rs").read_text()
        emitted = set(re.findall(r'\.finish\("([a-z_]+)"\)', mapping))
        self.assertTrue(emitted, "the driver mapping names its tools")
        for name in emitted:
            with self.subTest(tool=name):
                for forbidden in ("record", "replay", "stream", "watch", "kill", "clipboard"):
                    self.assertNotIn(forbidden, name)
        self.assertIn("get_window_state", emitted)


if __name__ == "__main__":
    unittest.main()
