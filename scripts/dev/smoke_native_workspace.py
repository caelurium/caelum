#!/usr/bin/env python3
"""Exercise Codex, SFTP and Git over OpenSSH with an isolated VM home."""
import argparse
import asyncio
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
from contextlib import nullcontext

import asyncssh  # Only needed on the fixture host.
from native_responses import model


ROOT = Path(__file__).resolve().parents[2]


async def run(home, real_codex, api, calls, approval):
    project = home / "Boat's checkout"
    project.mkdir()
    binary = home / ".local/bin"
    binary.mkdir(parents=True)
    remote = {"HOME": str(home), "PATH": f"{binary}:/usr/bin:/bin", "LANG": "C",
              "XDG_DATA_HOME": str(home / "data"), "XDG_CONFIG_HOME": str(home / "config"),
              "XDG_CACHE_HOME": str(home / "cache"), "XDG_STATE_HOME": str(home / "state")}
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        remote["CAELUM_CODEX_PORT"] = str(sock.getsockname()[1])
    for name in ("caelum", "opencode"):
        assert shutil.which(name, path=remote["PATH"]) is None, f"{name} is available on the fixture VM"
    if real_codex:
        (binary / "codex").symlink_to(Path(real_codex).resolve())
        config = home / ".codex"
        config.mkdir()
        (config / "config.toml").write_text(f'''model_provider = "fixture"
model = "gpt-5"
cli_auth_credentials_store = "file"
[model_providers.fixture]
name = "SSH fixture"
base_url = "{api}"
wire_api = "responses"
env_key = "CAELUM_TEST_KEY"
''')
        remote.update(CODEX_HOME=str(config), CAELUM_TEST_KEY="test-key")
        subprocess.run([binary / "codex", "login", "--with-api-key"], env=remote, input=b"test-key\n", check=True)

    def git(*args):
        subprocess.run(["git", *args], cwd=project, env=remote, check=True, stdout=subprocess.DEVNULL)

    git("init", "-b", "feat/native")
    git("remote", "add", "origin", "https://github.com/example/project.git")
    (project / "note.txt").write_text("base\n")
    git("add", "note.txt")
    git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "commit", "-m", "fixture")
    (project / "note.txt").write_text("VM edit\n")
    (project / "outside").symlink_to(home)
    (binary / "gh").write_text('#!/bin/sh\n[ "$1 $2" = "pr list" ] && test -f ssh-smoke || exit 1\nprintf "[]" \n')
    (binary / "gh").chmod(0o700)
    (project / "ssh-smoke").touch()

    login = asyncssh.generate_private_key("ssh-ed25519")
    login.write_private_key(home / "client")
    (home / "client").chmod(0o600)
    login.write_public_key(home / "client.pub")
    phases = ["native_new_session_over_openssh", "native_resume_after_vm_stop"] if real_codex else ["native_workspace_over_openssh"]
    if approval:
        phases = ["native_pending_approval_over_openssh"]
    for phase in phases:
        await exercise(home, project, remote, phase)
        if real_codex:
            assert calls == ["turn"], f"expected one explicit turn and no replay, got {calls}"
        if approval:
            assert not (project / "approval-fixture").exists()


async def exercise(home, project, remote, phase):
    host = asyncssh.generate_private_key("ssh-ed25519")

    groups = set()
    async def attach(channel):
        child = await asyncio.create_subprocess_exec(
            "/bin/sh", "-c", channel.command, cwd=home, env=remote,
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, start_new_session=True,
        )
        if "nohup " in channel.command:
            groups.add(child.pid)
        async def input_stream():
            try:
                while data := await channel.stdin.read(65536):
                    child.stdin.write(data)
                    await child.stdin.drain()
            finally:
                child.stdin.close()

        incoming = asyncio.create_task(input_stream())
        try:
            while data := await child.stdout.read(65536):
                channel.stdout.write(data)
                await channel.stdout.drain()
            channel.exit(await child.wait())
        finally:
            incoming.cancel()
            await asyncio.gather(incoming, return_exceptions=True)
            if child.returncode is None:
                child.kill()
                await child.wait()

    class VM(asyncssh.SSHServer):
        def connection_requested(self, dest_host, dest_port, orig_host, orig_port):
            return dest_host in ("127.0.0.1", "::1")

    server = await asyncssh.create_server(
        VM, "127.0.0.1", 0, server_host_keys=[host],
        authorized_client_keys=str(home / "client.pub"), process_factory=attach, encoding=None,
        sftp_factory=asyncssh.SFTPServer,
    )
    port = server.sockets[0].getsockname()[1]
    (home / "endpoint.json").write_text(json.dumps({
        "ok": True, "type": "sshkey", "sshEndpoint": f"127.0.0.1:{port}",
        "sshUser": "user", "hostKey": host.export_public_key().decode().strip(),
    }))
    try:
        test = await asyncio.create_subprocess_exec(
            "cargo", "test", "--locked", "-p", "caelum-core", phase, "--", "--ignored",
            cwd=ROOT, env={**os.environ, "CAELUM_NATIVE_SSH_FIXTURE": str(home),
                           "CAELUM_NATIVE_SSH_ROOT": str(project)},
        )
        if await test.wait():
            raise RuntimeError("native workspace smoke failed")
    finally:
        server.close()
        await server.wait_closed()
        # Simulate a stopped VM: keep the disk, terminate only fixture processes.
        for pgid in groups:
            try:
                os.killpg(pgid, signal.SIGTERM)
            except ProcessLookupError:
                pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--approval", action="store_true", help="interrupt SSH while Codex awaits permission")
    parser.add_argument("--real-codex", metavar="BINARY", help="stream and cold-resume a real Codex thread using a loopback Responses fixture")
    args = parser.parse_args()
    if args.approval and not args.real_codex:
        parser.error("--approval requires --real-codex BINARY")
    parent = Path(tempfile.gettempdir()).resolve()
    with tempfile.TemporaryDirectory(prefix="cw-", dir=parent) as temp:
        with model(args.approval, Path(temp) / "Boat's checkout") if args.real_codex else nullcontext((None, [])) as (api, calls):
            asyncio.run(run(Path(temp).resolve(), args.real_codex, api, calls, args.approval))
