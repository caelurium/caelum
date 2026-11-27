#!/usr/bin/env python3
"""Exercise packaged Linux UI, activation and persistence in an isolated desktop."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
import time
import urllib.parse
from pathlib import Path


def wait_for(check, description, process=None):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            raise RuntimeError(f"Desktop exited before {description}: {process.returncode}")
        value = check()
        if value:
            return value
        time.sleep(0.1)
    raise RuntimeError(f"Timed out: {description}")


def run(image: Path, backend: str, output: Path, root: Path) -> None:
    home, runtime, bin_dir = (root / name for name in ("home", "runtime", "bin"))
    for directory in (home, runtime, bin_dir, output):
        directory.mkdir(parents=True, exist_ok=True)
    runtime.chmod(0o700)
    for provider in ("codex", "opencode"):
        path = bin_dir / provider
        path.write_text("#!/bin/sh\nexit 1\n")
        path.chmod(0o755)
    environment = {**os.environ, "HOME": str(home), "XDG_DATA_HOME": str(home / "data"),
                   "XDG_CONFIG_HOME": str(home / "config"), "XDG_STATE_HOME": str(home / "state"),
                   "XDG_RUNTIME_DIR": str(runtime), "XDG_CURRENT_DESKTOP": "GNOME",
                   "PATH": f"{bin_dir}:/usr/bin:/bin", "WINIT_UNIX_BACKEND": backend,
                   "CAELUM_STATE": str(home / "workspace.json")}
    icds = list(Path("/usr/share/vulkan/icd.d").glob("lvp_icd*.json"))
    if len(icds) != 1:
        raise RuntimeError("Install the native Mesa software Vulkan driver")
    environment["VK_ICD_FILENAMES"] = str(icds[0])
    command = [str(image), "--appimage-extract-and-run"]
    children = []
    with (output / "desktop.log").open("w") as log:
        try:
            if backend == "wayland":
                environment.pop("DISPLAY", None)
                environment["WAYLAND_DISPLAY"] = "caelum-test"
                compositor = subprocess.Popen(["weston", "--backend=headless-backend.so", "--use-pixman", "--socket=caelum-test", "--idle-time=0"], env=environment, stdout=log, stderr=log)
                children.append(compositor)
                wait_for(lambda: (runtime / "caelum-test").is_socket(), "Wayland compositor", compositor)
            else:
                environment.pop("WAYLAND_DISPLAY", None)
                children.append(subprocess.Popen(["openbox"], env=environment, stdout=log, stderr=log))
            subprocess.run(["dbus-update-activation-environment", "DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "XDG_CURRENT_DESKTOP", "HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME"], env=environment, check=True, stdout=log, stderr=log)
            for theme in ("light", "dark"):
                for scene in ("home", "settings"):
                    name = f"{scene}-{theme}"
                    subprocess.run([*command, "--design-fixture", "--scene", scene, "--theme", theme, "--size", "1280x1000", "--scale", "1", "--at", "1", "--snap", str(output / f"{name}.png")], env=environment, check=True, timeout=60, stdout=log, stderr=log)
                    if (output / f"{name}.png").stat().st_size < 1000:
                        raise RuntimeError("Empty Linux preview")
            trace_environment = {**environment, "RUNE_CPU_FIRST": "off"}
            subprocess.run([*command, "--design-fixture", "--scene", "home", "--native", "--frames", "3", "--trace", str(output / "native.json")], env=trace_environment, check=True, timeout=60, stdout=log, stderr=log)
            if not (output / "native.json").is_file():
                raise RuntimeError("Native renderer did not produce its trace")
            subprocess.run([*command, "--install"], env=environment, check=True, timeout=30, stdout=log, stderr=log)
            entry = home / "data/applications/dev.caelum.desktop.desktop"
            subprocess.run(["desktop-file-validate", str(entry)], check=True)
            handler = subprocess.check_output(["xdg-mime", "query", "default", "x-scheme-handler/caelum"], env=environment, text=True).strip()
            if handler != entry.name:
                raise RuntimeError(f"Project URL handler was not registered: {handler!r}")
            if backend == "x11":
                exercise_x11(command, environment, root, output, log)
            print(f"Verified {backend} AppImage startup, native rendering, light/dark views and user installation")
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                child.wait(timeout=10)


def exercise_x11(command, environment, root, output, log):
    projects = [root / "first café #1", root / "second & 2%+?"]
    for project in projects:
        project.mkdir()
    state = Path(environment["CAELUM_STATE"])
    sessions = [dict(scope=dict(project=str(project), provider="codex"), remote_id=None,
                     title=f"Draft {index}", options={}, draft=f"Preserved draft {index}",
                     attachments=[], failed_prompt=None, pending_submission=None,
                     queue=[], archived=False, reading=-1.0) for index, project in enumerate(projects)]
    state.write_text(json.dumps(dict(version=1, projects=[dict(name=p.name, path=str(p)) for p in projects],
                                    sessions=sessions, selected=0, preferences={},
                                    remembered={str(p): i for i, p in enumerate(projects)}, project_roots={})))
    urls = ["caelum://open?" + urllib.parse.urlencode({"path": str(project)}) for project in projects]

    def selected(index):
        return json.loads(state.read_text())["selected"] == index

    def key(window, *keys):
        subprocess.run(["xdotool", "windowfocus", "--sync", window, "key", "--clearmodifiers", *keys], env=environment, check=True, timeout=10)

    app = subprocess.Popen([*command, urls[1]], env=environment, stdout=log, stderr=log)
    try:
        window = subprocess.check_output(["xdotool", "search", "--sync", "--onlyvisible", "--class", "dev.caelum.desktop"], env=environment, text=True, timeout=30).splitlines()[0]
        wait_for(lambda: selected(1), "cold project URL", app)
        subprocess.run([*command, urls[0]], env=environment, check=True, timeout=15, stdout=log, stderr=log)
        wait_for(lambda: selected(0), "warm project URL", app)
        assert app.poll() is None
        draft = "Linux clipboard café & draft"
        subprocess.run(["xclip", "-selection", "clipboard"], input=draft, text=True, env=environment, check=True, stdout=log, stderr=log)
        key(window, "ctrl+l", "ctrl+a", "ctrl+v")
        wait_for(lambda: json.loads(state.read_text())["sessions"][0]["draft"] == draft, "native clipboard draft", app)
        key(window, "ctrl+o")
        dialog = subprocess.check_output(["xdotool", "search", "--sync", "--onlyvisible", "--name", "^Open a project$"], env=environment, text=True, timeout=30).splitlines()[0]
        key(dialog, "Escape")
        key(window, "ctrl+w")
        assert app.wait(timeout=30) == 0, "closing the last window must quit cleanly"
        assert json.loads(state.read_text())["sessions"][0]["draft"] == draft
        subprocess.run(["xdg-open", urls[1]], env={**environment, "APPIMAGE_EXTRACT_AND_RUN": "1"}, check=True, timeout=15, stdout=log, stderr=log)
        window = subprocess.check_output(["xdotool", "search", "--sync", "--onlyvisible", "--class", "dev.caelum.desktop"], env=environment, text=True, timeout=30).splitlines()[0]
        wait_for(lambda: selected(1), "installed desktop URL")
        key(window, "ctrl+q")
        wait_for(lambda: subprocess.run(["xdotool", "getwindowname", window], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode != 0, "installed app quit")
        snapshot = json.loads(state.read_text())
        assert [s["draft"] for s in snapshot["sessions"]] == [draft, sessions[1]["draft"]]
        print("Verified X11 cold/warm URLs, portal dialog, clipboard, drafts, window close and installed-app relaunch")
    finally:
        if app.poll() is None:
            app.terminate()
            app.wait(timeout=10)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--backend", choices=("x11", "wayland"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="caelum-linux-smoke-", dir=os.environ.get("RUNNER_TEMP")) as temporary:
        root = Path(temporary)
        try:
            run(args.image.resolve(), args.backend, args.output.resolve(), root)
        finally:
            documents = root / "runtime/doc"
            if documents.is_mount():
                subprocess.run(["fusermount3", "-u", str(documents)], check=True, timeout=10)


if __name__ == "__main__":
    main()
