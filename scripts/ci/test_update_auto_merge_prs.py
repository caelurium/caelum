import copy
import json
import subprocess
import unittest
from unittest.mock import patch

from update_auto_merge_prs import github_api, update_branches


REPOSITORY = "example/project"
HEAD = "a" * 40
BASE = "b" * 40


def pull_request(number=1):
    return {
        "number": number,
        "state": "open",
        "draft": False,
        "auto_merge": {"merge_method": "squash"},
        "base": {"ref": "main", "repo": {"full_name": REPOSITORY}},
        "head": {"sha": HEAD, "repo": {"full_name": REPOSITORY}},
        "mergeable": True,
    }


class GitHub:
    def __init__(self, prs=None):
        self.prs = prs if prs is not None else [pull_request()]
        self.calls = []
        self.behind = 1
        self.change = None
        self.error = None

    def __call__(self, method, path, payload=None):
        self.calls.append((method, path, payload))
        if "?" in path:
            page = int(path.rsplit("=", 1)[1])
            return copy.deepcopy(self.prs[(page - 1) * 100:page * 100])
        if path.endswith("/main"):
            return {"object": {"sha": BASE}}
        if "/compare/" in path:
            if self.change:
                self.change(self.prs[0])
            return {"behind_by": self.behind}
        if method == "PUT":
            if self.error:
                raise RuntimeError(self.error)
            return {"message": "Updating pull request branch."}
        number = int(path.rsplit("/", 1)[1])
        return copy.deepcopy(next(pr for pr in self.prs if pr["number"] == number))

    def updates(self):
        return [call for call in self.calls if call[0] == "PUT"]


class AutoMergeUpdatesTests(unittest.TestCase):
    def test_updates_behind_branch_with_the_expected_head(self):
        api = GitHub()
        results = update_branches(REPOSITORY, api)
        self.assertEqual(api.updates(), [("PUT", "repos/example/project/pulls/1/update-branch", {"expected_head_sha": HEAD})])
        self.assertEqual(results, [{"number": 1, "status": "Branch update requested", "failed": False}])
        self.assertIn(("GET", f"repos/{REPOSITORY}/compare/{BASE}...{HEAD}", None), api.calls)

    def test_already_current_branch_is_idempotent(self):
        api = GitHub()
        api.behind = 0
        self.assertEqual(update_branches(REPOSITORY, api)[0]["status"], "Already up to date")
        self.assertFalse(api.updates())

    def test_only_open_ready_same_repository_main_prs_are_eligible(self):
        prs = [pull_request(i) for i in range(1, 8)]
        prs[0]["auto_merge"] = None
        prs[1]["draft"] = True
        prs[2]["state"] = "closed"
        prs[3]["head"]["repo"]["full_name"] = "contributor/project"
        prs[4]["head"]["repo"] = None
        prs[5]["base"]["ref"] = "release"
        prs[6]["base"]["repo"]["full_name"] = "other/project"
        api = GitHub(prs)
        self.assertEqual(update_branches(REPOSITORY, api), [])
        self.assertEqual(len(api.calls), 1)

    def test_conflict_is_reported_without_attempting_an_update(self):
        api = GitHub()
        api.prs[0]["mergeable"] = False
        self.assertIn("Conflict", update_branches(REPOSITORY, api)[0]["status"])
        self.assertFalse(api.updates())

    def test_unknown_mergeability_uses_the_guarded_server_side_update(self):
        api = GitHub()
        api.prs[0]["mergeable"] = None
        self.assertFalse(update_branches(REPOSITORY, api)[0]["failed"])
        self.assertEqual(len(api.updates()), 1)

    def test_head_changed_during_comparison_is_not_overwritten(self):
        api = GitHub()
        api.change = lambda pr: pr["head"].update(sha="c" * 40)
        self.assertIn("Head changed", update_branches(REPOSITORY, api)[0]["status"])
        self.assertFalse(api.updates())

    def test_disabling_auto_merge_during_reads_prevents_update(self):
        api = GitHub()
        api.change = lambda pr: pr.update(auto_merge=None)
        self.assertEqual(update_branches(REPOSITORY, api)[0]["status"], "No longer eligible")
        self.assertFalse(api.updates())

    def test_new_conflict_during_reads_prevents_update(self):
        api = GitHub()
        api.change = lambda pr: pr.update(mergeable=False)
        self.assertIn("Conflict", update_branches(REPOSITORY, api)[0]["status"])
        self.assertFalse(api.updates())

    def test_rejected_update_is_a_failure_and_other_prs_are_still_examined(self):
        api = GitHub([pull_request(1), pull_request(2)])
        api.error = "GitHub rejected expected_head_sha (HTTP 422)"
        results = update_branches(REPOSITORY, api)
        self.assertEqual([result["number"] for result in results], [1, 2])
        self.assertTrue(all(result["failed"] for result in results))
        self.assertEqual(len(api.updates()), 2)

    def test_paginates_before_updating_branches(self):
        prs = [pull_request(i) for i in range(1, 102)]
        for pr in prs[:-1]:
            pr["auto_merge"] = None
        api = GitHub(prs)
        self.assertEqual(update_branches(REPOSITORY, api)[0]["number"], 101)
        self.assertIn("page=2", api.calls[1][1])
        self.assertEqual(len(api.updates()), 1)

    def test_invalid_repository_or_revision_never_causes_a_write(self):
        api = GitHub()
        with self.assertRaises(ValueError):
            update_branches("example/project/../../other", api)
        self.assertFalse(api.calls)
        api.prs[0]["head"]["sha"] = "feature/name"
        self.assertTrue(update_branches(REPOSITORY, api)[0]["failed"])
        self.assertFalse(api.updates())

    def test_api_sends_head_guard_as_json_and_surfaces_errors(self):
        with patch("update_auto_merge_prs.subprocess.run") as run:
            run.return_value = subprocess.CompletedProcess([], 0, '{"message":"ok"}', "")
            github_api("PUT", "repos/example/project/pulls/1/update-branch", {"expected_head_sha": HEAD})
            args, kwargs = run.call_args
            self.assertEqual(args[0][-2:], ["--input", "-"])
            self.assertEqual(json.loads(kwargs["input"]), {"expected_head_sha": HEAD})
            run.return_value = subprocess.CompletedProcess([], 1, "{}", "Forbidden (HTTP 403)")
            with self.assertRaisesRegex(RuntimeError, "Forbidden"):
                github_api("GET", "repos/example/project/pulls/1")


if __name__ == "__main__":
    unittest.main()
