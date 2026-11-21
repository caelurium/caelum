"""Run inside Boat's command workdir. Only the physical checkout path goes to stdout."""
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from urllib.parse import urlsplit


def resolve(path):
    try:
        return Path(path).resolve(strict=True)
    except (OSError, RuntimeError):
        return None


def git(directory, *args):
    return subprocess.check_output(
        ["git", "-C", str(directory), *args],
        stderr=subprocess.DEVNULL,
        encoding="utf-8",
        errors="replace",
    ).removesuffix("\n")


def github_origin(directory):
    directory = resolve(directory)
    if directory is None or not directory.is_dir() or not (directory / ".git").exists():
        return None
    try:
        if resolve(git(directory, "rev-parse", "--show-toplevel")) != directory:
            return None
        url = git(directory, "remote", "get-url", "origin")
        if url.startswith("git@github.com:"):
            slug = url.split(":", 1)[1]
        else:
            remote = urlsplit(url)
            if remote.scheme not in ("https", "ssh") or remote.hostname != "github.com":
                return None
            slug = remote.path.lstrip("/")
        return slug.rstrip("/").removesuffix(".git").lower() if slug.isascii() else None
    except (OSError, RuntimeError, subprocess.CalledProcessError, ValueError):
        return None


def scan(base, candidates):
    pending = [(base, 0)]
    visited = set()
    while pending and len(visited) < 4000:
        path, depth = pending.pop()
        actual = resolve(path)
        if actual is None or not actual.is_dir():
            continue
        try:
            stat = actual.stat()
        except OSError:
            continue
        identity = (stat.st_dev, stat.st_ino)
        if identity in visited:
            continue
        visited.add(identity)
        candidates.add(actual)
        if depth >= 7 or (actual / ".git").exists():
            continue
        try:
            entries = list(os.scandir(actual))
        except OSError:
            continue
        for entry in entries:
            if entry.name.startswith("."):
                continue
            child = Path(entry.path)
            try:
                if entry.is_dir(follow_symlinks=True):
                    pending.append((child, depth + 1))
            except OSError:
                continue


def locate(repo, pinned, new_base):
    if pinned:
        path = Path(pinned)
        resolved = resolve(path)
        if resolved is None or not resolved.is_dir():
            sys.exit(40)
        if github_origin(resolved) != repo.lower():
            sys.exit(41)
        return resolved

    name = repo.split("/")[1]
    home = Path.home().resolve()
    workdir = Path.cwd().resolve()
    if workdir == workdir.parent:
        workdir = home
    if workdir == workdir.parent:
        sys.exit(40)
    destination = workdir / name
    candidates = set()
    try:
        containing = resolve(git(workdir, "rev-parse", "--show-toplevel"))
    except subprocess.CalledProcessError:
        containing = None
    if containing and github_origin(containing) == repo.lower():
        return containing

    for base in {workdir, home}:
        if base == base.parent:
            continue
        candidates.add(base)
        inside_repo = (base / ".git").exists() or (
            containing is not None and base.is_relative_to(containing)
        )
        named = base / name
        if os.path.lexists(named) and (not inside_repo or (named / ".git").exists()):
            actual = resolve(named)
            if actual is None or github_origin(actual) != repo.lower():
                sys.exit(41)
            candidates.add(actual)
        if not inside_repo:
            scan(base, candidates)

    matches = [path for path in candidates if github_origin(path) == repo.lower()]
    if len(matches) == 1:
        return matches[0]
    if matches:
        sys.exit(42)

    if containing or os.path.lexists(destination):
        sys.exit(41)
    if not new_base:
        sys.exit(40)
    with TemporaryDirectory(prefix=".caelum-clone-", dir=workdir) as temporary:
        staged = Path(temporary) / name
        clone = subprocess.run(
            ["git", "clone", "--branch", new_base, "--", f"https://github.com/{repo}.git", staged],
            env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if clone.returncode:
            detail = clone.stderr.strip()
            if detail:
                print(detail[-4096:], file=sys.stderr)
            sys.exit(43)
        if github_origin(staged) != repo.lower():
            sys.exit(41)
        try:
            destination.mkdir()
        except FileExistsError:
            if destination.is_symlink() or github_origin(destination) != repo.lower():
                sys.exit(41)
        else:
            staged.rename(destination)
    return destination


print(locate(*sys.argv[1:]))
