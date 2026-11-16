#!/usr/bin/env python3
"""Replace and relaunch a real package in an isolated, offline workspace."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import threading


def run(package: Path, app: Path | None, root: Path) -> None:
    home, public, providers = (root / name for name in ("home", "public", "bin"))
    for directory in (home, public, providers):
        directory.mkdir()
    for provider in ("codex", "opencode", "gh"):
        path = providers / provider
        path.write_text("#!/bin/sh\nexit 1\n")
        path.chmod(0o755)
    asset = public / package.name
    shutil.copy2(package, asset)
    with asset.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    (public / "SHA256SUMS").write_text(f"{digest}  {asset.name}\n")
    if app is not None:
        target = root / "Applications/Caelum.app"
        target.parent.mkdir()
        subprocess.run(["/usr/bin/ditto", str(app), str(target)], check=True)
    else:
        target = home / "data/caelum/Caelum.AppImage"
        target.parent.mkdir(parents=True)
        shutil.copy2(asset, target)
        target.chmod(0o755)
    environment = {**os.environ, "HOME": str(home), "XDG_DATA_HOME": str(home / "data"),
                   "XDG_CONFIG_HOME": str(home / "config"), "XDG_STATE_HOME": str(home / "state"),
                   "CAELUM_STATE": str(home / "workspace.json"), "PATH": f"{providers}:{os.environ['PATH']}",
                   "CAELUM_UPDATE_ROOT": str(root), "CAELUM_UPDATE_TARGET": str(target),
                   "CAELUM_UPDATE_ASSET": asset.name}
    # Compile using the caller's Cargo home/toolchain, then isolate only the test/app.
    built = subprocess.check_output(["cargo", "test", "--locked", "-p", "caelum-core", "--lib", "--no-run", "--message-format=json"], text=True)
    executables = [record["executable"] for line in built.splitlines()
                   if (record := json.loads(line)).get("executable") and record.get("profile", {}).get("test")]
    assert len(executables) == 1, executables
    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(SimpleHTTPRequestHandler, directory=str(public)))
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        environment["CAELUM_UPDATE_SOURCE"] = f"http://127.0.0.1:{server.server_port}"
        subprocess.run([executables[0], "updates::installation::packaged::replacement_relaunch_and_workspace_restoration", "--exact", "--ignored", "--nocapture", "--test-threads=1"], env=environment, check=True, timeout=180)
    finally:
        server.shutdown()
        worker.join()
        server.server_close()
    print("Verified package download, integrity, replacement, relaunch, startup acknowledgment and saved workspace")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True, help="Native DMG or AppImage")
    parser.add_argument("--app", type=Path, help="Original macOS app bundle")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="caelum-update-smoke-", dir=os.environ.get("RUNNER_TEMP")) as temporary:
        run(args.package.resolve(), args.app.resolve() if args.app else None, Path(temporary).resolve())


if __name__ == "__main__":
    main()
