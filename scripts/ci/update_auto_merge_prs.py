#!/usr/bin/env python3
"""Bring opted-in PR branches up to date using GitHub's guarded update API."""

from __future__ import annotations

import json
import os
import re
import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any


Api = Callable[..., Any]


def github_api(method: str, path: str, payload: dict | None = None) -> Any:
    command = ["gh", "api", "--method", method, path]
    if payload is not None:
        command.extend(["--input", "-"])
    result = subprocess.run(
        command,
        input=json.dumps(payload) if payload is not None else None,
        capture_output=True,
        text=True,
        timeout=60,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or f"GitHub request failed: {method} {path}")
    return json.loads(result.stdout)


def eligible(pr: dict, repository: str) -> bool:
    return (
        pr.get("state") == "open"
        and pr.get("draft") is False
        and pr.get("auto_merge") is not None
        and pr.get("base", {}).get("ref") == "main"
        and (pr.get("base", {}).get("repo") or {}).get("full_name") == repository
        and (pr.get("head", {}).get("repo") or {}).get("full_name") == repository
    )


def revision(value: str) -> str:
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", value):
        raise ValueError("GitHub returned an invalid commit ID")
    return value


def update_one(repository: str, number: int, api: Api) -> str:
    path = f"repos/{repository}/pulls/{number}"
    pr = api("GET", path)
    if not eligible(pr, repository):
        return "No longer eligible"
    head = revision(pr["head"]["sha"])
    base = revision(api("GET", f"repos/{repository}/git/ref/heads/main")["object"]["sha"])
    comparison = api("GET", f"repos/{repository}/compare/{base}...{head}")
    if comparison["behind_by"] == 0:
        return "Already up to date"
    if pr.get("mergeable") is False:
        return "Conflict: author action required"

    # Auto-merge may have been disabled or new commits pushed during the reads.
    current = api("GET", path)
    if not eligible(current, repository):
        return "No longer eligible"
    if current["head"]["sha"] != head:
        return "Head changed: newer event will recheck"
    if current.get("mergeable") is False:
        return "Conflict: author action required"
    api("PUT", f"{path}/update-branch", {"expected_head_sha": head})
    return "Branch update requested"


def update_branches(repository: str, api: Api = github_api) -> list[dict]:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Expected an owner/repository name")
    numbers = []
    page = 1
    while True:
        prs = api("GET", f"repos/{repository}/pulls?state=open&base=main&per_page=100&page={page}")
        numbers.extend(pr["number"] for pr in prs if eligible(pr, repository))
        if len(prs) < 100:
            break
        page += 1

    results = []
    for number in sorted(set(numbers)):
        try:
            status = update_one(repository, number, api)
            results.append({"number": number, "status": status, "failed": False})
        except (RuntimeError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
            results.append({"number": number, "status": str(error), "failed": True})
    return results


def main() -> None:
    results = update_branches(os.environ["GITHUB_REPOSITORY"])
    print(json.dumps(results, indent=2))
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with Path(summary).open("a") as output:
            output.write("## Auto-merge branch updates\n\n")
            for result in results:
                status = result["status"].replace("\n", " ").replace("|", "\\|")
                output.write(f"- #{result['number']}: {status}\n")
            if not results:
                output.write("No eligible auto-merge PRs.\n")
    if any(result["failed"] for result in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
