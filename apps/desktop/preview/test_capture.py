import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from capture import capture_all, capture_plan, interactions, preview_catalogue, scenes, script_at, strip_timing


class CaptureTests(unittest.TestCase):
    def test_parallel_work_is_bounded_and_manifest_keeps_plan_order(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            active = peak = 0
            lock = threading.Lock()
            barrier = threading.Barrier(2)

            def child(command, **kwargs):
                nonlocal active, peak
                name = command[0]
                with lock:
                    active += 1
                    peak = max(peak, active)
                barrier.wait(timeout=5)
                (output / f"{name}.png").write_bytes(b"\x89PNG\r\n\x1a\nfixture")
                (output / f"{name}.json").write_text('{"fixture": true}')
                with lock:
                    active -= 1
                return subprocess.CompletedProcess(command, 0, name, "")

            plan = [(f"capture-{index}", [f"capture-{index}"], None) for index in range(8)]
            with patch("capture.subprocess.run", side_effect=child):
                capture_all(plan, output, workers=2)
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertEqual(peak, 2)
            self.assertEqual(manifest["completed"], 8)
            self.assertEqual([item["name"] for item in manifest["captures"]], [item[0] for item in plan])
            self.assertTrue(all((output / f"{item[0]}.log").read_text() == item[0] for item in plan))

    def test_failures_timeouts_and_missing_outputs_cannot_reuse_stale_success(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            plan = [
                ("failed", [sys.executable, "-c", "print('child failure'); raise SystemExit(3)"], None),
                ("timeout", [sys.executable, "-c", "import time; print('waiting', flush=True); time.sleep(30)"], None),
                ("missing", [sys.executable, "-c", "pass"], None),
            ]
            for name, _, _ in plan:
                (output / f"{name}.png").write_bytes(b"\x89PNG\r\n\x1a\nold")
                (output / f"{name}.json").write_text('{}')
            with self.assertRaisesRegex(RuntimeError, "3 captures failed"):
                capture_all(plan, output, workers=2, timeout=0.5)
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertEqual(manifest["completed"], 0)
            self.assertEqual(len(manifest["captures"]), 3)
            self.assertIn("child failure", (output / "failed.log").read_text())
            self.assertIn("exceeded", (output / "timeout.log").read_text())
            self.assertFalse(list(output.glob("*.png")))

    def test_plan_covers_both_themes_sizes_and_interaction_inputs(self):
        output = Path("preview-results")
        plan = capture_plan(Path("caelum-desktop"), output)
        names = [item[0] for item in plan]
        self.assertEqual(len(names), len(set(names)))
        for theme in ("dark", "light"):
            for suffix in ("", "-small"):
                name = f"rune-{theme}-patch-expanded{suffix}"
                _, command, events = next(item for item in plan if item[0] == name)
                self.assertEqual(command[command.index("--at") + 1], "1.8")
                self.assertEqual(command[command.index("--script") + 1], str(output / f"{name}-input.json"))
                self.assertTrue(events)

    def test_invalid_worker_counts_and_duplicate_names_fail_before_launch(self):
        with tempfile.TemporaryDirectory() as directory, patch("capture.subprocess.run") as run:
            for workers in (0, 5):
                with self.assertRaises(ValueError):
                    capture_all([], Path(directory), workers)
            with self.assertRaisesRegex(ValueError, "unique"):
                capture_all([("same", [], None)] * 2, Path(directory), 1)
            run.assert_not_called()

    def test_focused_preview_filters_theme_size_and_crop(self):
        output = Path("preview-results")
        plan = capture_plan(
            Path("caelum-desktop"), output,
            only=["composer-options"], themes=("dark",), sizes=(False,), crop="message-composer",
        )
        self.assertEqual(len(plan), 1)
        name, command, events = plan[0]
        self.assertEqual(name, "rune-dark-composer-options")
        self.assertEqual(command[command.index("--crop") + 1], "message-composer")
        self.assertEqual(command[command.index("--size") + 1], "1280x1000")
        self.assertIsNone(events)

    def test_strip_replays_become_annotated_contact_sheets(self):
        output = Path("preview-results")
        plan = capture_plan(
            Path("caelum-desktop"), output,
            only=["composer-agent"], themes=("light",), sizes=(True,), strip=True, at="mark:end",
        )
        self.assertEqual(len(plan), 1)
        name, command, events = plan[0]
        self.assertEqual(name, "rune-light-composer-agent-small")
        self.assertEqual(command[command.index("--strip") + 1], str(output / f"{name}-strip.png"))
        self.assertEqual(command[command.index("--at") + 1], "mark:end")
        self.assertEqual(command[command.index("--size") + 1], "800x620")
        self.assertIn("--keys", command)
        self.assertIn("--pointer", command)
        self.assertGreater(int(command[command.index("--frames") + 1]), 0)
        self.assertTrue(events)

    def test_strip_timing_uses_the_end_mark_or_last_event(self):
        self.assertEqual(strip_timing([{"t": 0.25}]), (20, 5))
        self.assertEqual(
            strip_timing([{"t": 0.2}, {"t": 0.5, "type": "mark", "name": "end"}]),
            (15, 4),
        )

    def test_mark_end_is_added_to_catalogue_replays(self):
        at, events = script_at("mark:end", [{"t": 0.2, "key": "live-draft-0"}])
        self.assertEqual(at, "mark:end")
        self.assertEqual(events[-1], {"t": 0.6, "type": "mark", "name": "end"})
        self.assertEqual(strip_timing(events), (18, 4))
        with self.assertRaisesRegex(ValueError, "Unknown mark"):
            script_at("mark:start", [{"t": 0.2, "key": "live-draft-0"}])

    def test_strip_crops_are_forwarded_per_frame(self):
        plan = capture_plan(
            Path("caelum-desktop"), Path("preview-results"),
            only=["composer-agent"], themes=("dark",), sizes=(False,),
            strip=True, strip_crops=[(10.0, 20.0, 30.0, 40.0)],
        )
        command = plan[0][1]
        self.assertEqual(command[command.index("--crop") + 1], "10,20,30,40")

    def test_review_options_fail_before_building(self):
        binary = Path("caelum-desktop")
        output = Path("preview-results")
        with self.assertRaisesRegex(ValueError, "exactly one"):
            capture_plan(binary, output, crop="message-composer")
        with self.assertRaisesRegex(ValueError, "needs --only"):
            capture_plan(binary, output, strip=True)
        with self.assertRaisesRegex(ValueError, "interaction"):
            capture_plan(binary, output, only=["composer-options"], strip=True)
        with self.assertRaisesRegex(ValueError, "Unknown"):
            capture_plan(binary, output, only=["not-a-scene"])
        with self.assertRaisesRegex(ValueError, "needs --strip"):
            capture_plan(binary, output, only=["composer-agent"], strip_crops=[(0, 0, 10, 10)])

    def test_catalogue_names_are_unique_and_cover_default(self):
        catalogue = preview_catalogue()
        self.assertEqual(len(catalogue), len(scenes) + len(interactions))
        self.assertIn("default", catalogue)
        self.assertIn("composer-options", catalogue)
        self.assertIn("composer-agent", catalogue)

    def test_missing_contact_sheet_fails_the_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)

            def child(command, **kwargs):
                (output / "replay.png").write_bytes(b"\x89PNG\r\n\x1a\nfixture")
                (output / "replay.json").write_text('{"fixture": true}')
                return subprocess.CompletedProcess(command, 0, "", "")

            plan = [("replay", ["replay", "--strip", str(output / "replay-strip.png")], None)]
            with patch("capture.subprocess.run", side_effect=child):
                with self.assertRaisesRegex(RuntimeError, "1 captures failed"):
                    capture_all(plan, output, workers=1)
            self.assertIn("replay-strip.png", (output / "replay.log").read_text())


if __name__ == "__main__":
    unittest.main()
