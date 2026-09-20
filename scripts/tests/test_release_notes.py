#!/usr/bin/env python3
import importlib.util
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("release_notes", ROOT / "scripts/release_notes.py")
release_notes = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release_notes
SPEC.loader.exec_module(release_notes)


class ReleaseNotesTests(unittest.TestCase):
    def test_openai_request_does_not_follow_redirect_with_authorization(self):
        received_authorization = []

        class DestinationHandler(BaseHTTPRequestHandler):
            def _record(self):
                received_authorization.append(self.headers.get("Authorization"))
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"{}")

            do_GET = _record
            do_POST = _record

            def log_message(self, format, *args):
                pass

        destination = ThreadingHTTPServer(("127.0.0.1", 0), DestinationHandler)

        class RedirectHandler(BaseHTTPRequestHandler):
            def do_POST(self):
                self.send_response(302)
                self.send_header(
                    "Location",
                    f"http://127.0.0.1:{destination.server_port}/stolen",
                )
                self.end_headers()

            def log_message(self, format, *args):
                pass

        redirect = ThreadingHTTPServer(("127.0.0.1", 0), RedirectHandler)
        threads = [
            threading.Thread(target=server.serve_forever)
            for server in (destination, redirect)
        ]
        for thread in threads:
            thread.start()
        original_url = release_notes.OPENAI_URL
        release_notes.OPENAI_URL = f"http://127.0.0.1:{redirect.server_port}/v1/chat/completions"
        try:
            with self.assertRaises(OSError):
                release_notes._request_openai(
                    release_notes.OPENAI_URL,
                    {"Authorization": "Bearer secret"},
                    {},
                )
        finally:
            release_notes.OPENAI_URL = original_url
            for server in (redirect, destination):
                server.shutdown()
                server.server_close()
            for thread in threads:
                thread.join()

        self.assertEqual([], received_authorization)

    def test_successful_ai_output_is_rendered_from_trusted_metadata(self):
        pull_requests = [
            release_notes.PullRequest(
                number=12,
                title="Add moon themes",
                body="A friendlier look.",
                labels=("feature",),
                author="alice",
                author_is_bot=False,
                url="https://github.com/triangle-int/nolune/pull/12",
            ),
            release_notes.PullRequest(
                number=14,
                title="Fix startup",
                body="Avoid a crash.",
                labels=("bug",),
                author="P5ina",
                author_is_bot=False,
                url="https://github.com/triangle-int/nolune/pull/14",
            ),
        ]
        response = {
            "overview": "A friendlier and more reliable release.",
            "pull_requests": [
                {"number": 12, "category": "New features", "summary": "Adds a friendly moon theme."},
                {"number": 14, "category": "Bug fixes", "summary": "Prevents a startup crash."},
            ],
        }

        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="P5ina",
            api_key="secret",
            request_json=lambda _url, _headers, _payload: {
                "choices": [{"message": {"content": json.dumps(response)}}]
            },
        )

        self.assertIn("A friendlier and more reliable release.", notes)
        self.assertIn("## New features", notes)
        self.assertIn("Adds a friendly moon theme. [#12](https://github.com/triangle-int/nolune/pull/12)", notes)
        self.assertIn("## Bug fixes", notes)
        self.assertIn("Prevents a startup crash. [#14](https://github.com/triangle-int/nolune/pull/14)", notes)
        self.assertIn("## Thanks", notes)
        self.assertIn("[@alice](https://github.com/alice)", notes)
        self.assertNotIn("@P5ina", notes)

    def test_unknown_duplicate_or_omitted_pr_numbers_use_fallback(self):
        pull_requests = [
            release_notes.PullRequest(12, "Add feature", "", ("feature",), "alice", False, "https://github.com/o/r/pull/12"),
            release_notes.PullRequest(14, "Fix bug", "", ("bug",), "bob", False, "https://github.com/o/r/pull/14"),
        ]
        invalid_sets = ([12], [12, 12], [12, 99])
        for numbers in invalid_sets:
            with self.subTest(numbers=numbers):
                response = {
                    "overview": "Untrusted overview",
                    "pull_requests": [
                        {"number": number, "category": "New features", "summary": "Untrusted summary"}
                        for number in numbers
                    ],
                }
                notes = release_notes.generate_notes(
                    pull_requests,
                    [],
                    repository_owner="owner",
                    api_key="secret",
                    request_json=lambda _url, _headers, _payload, response=response: {
                        "choices": [{"message": {"content": json.dumps(response)}}]
                    },
                )
                self.assertNotIn("Untrusted", notes)
                self.assertIn("Add feature [#12](https://github.com/o/r/pull/12)", notes)
                self.assertIn("Fix bug [#14](https://github.com/o/r/pull/14)", notes)

    def test_direct_commits_are_preserved_in_other_changes(self):
        commit = release_notes.DirectCommit("abc1234", "Update packaging")
        notes = release_notes.generate_notes(
            [],
            [commit],
            repository_owner="owner",
            api_key="",
            request_json=lambda *_args: self.fail("missing key must not call OpenAI"),
        )

        self.assertIn("## Other changes", notes)
        self.assertIn("- Update packaging (`abc1234`)", notes)

    def test_fallback_and_direct_change_text_remove_untrusted_urls(self):
        pull_requests = [
            release_notes.PullRequest(
                12,
                "Fix updater https://evil.example/path [click](https://other.example)",
                "",
                ("bug",),
                "owner",
                False,
                "https://github.com/o/r/pull/12",
            )
        ]
        direct = [
            release_notes.DirectCommit(
                "abc1234",
                "Update docs <https://third.example> ftp://fourth.example/file mailto:attacker@evil.example",
            )
        ]

        notes = release_notes.generate_notes(
            pull_requests,
            direct,
            repository_owner="owner",
            api_key="",
            request_json=lambda *_args: self.fail("missing key must not call OpenAI"),
        )

        self.assertNotIn("evil.example", notes)
        self.assertNotIn("other.example", notes)
        self.assertNotIn("third.example", notes)
        self.assertNotIn("fourth.example", notes)
        self.assertIn("Fix updater", notes)
        self.assertIn("Update docs", notes)

    def test_fallback_removes_controls_bidi_and_bare_domains(self):
        pull_requests = [
            release_notes.PullRequest(
                12,
                "Fix\x00 updater evil.example/path 127.0.0.1:8080/admin\u202espoof",
                "",
                ("bug",),
                "owner",
                False,
                "https://github.com/o/r/pull/12",
            )
        ]
        direct = [release_notes.DirectCommit("abc1234", "Docs\x1b bad.test/path\u2066hidden")]

        notes = release_notes.generate_notes(
            pull_requests,
            direct,
            repository_owner="owner",
            api_key="",
            request_json=lambda *_args: self.fail("missing key must not call OpenAI"),
        )

        for unsafe in ("\x00", "\x1b", "\u202e", "\u2066", "evil.example", "127.0.0.1", "bad.test"):
            self.assertNotIn(unsafe, notes)
        self.assertIn("Fix updater", notes)
        self.assertIn("Docs", notes)

    def test_model_text_with_markdown_or_mentions_is_rejected(self):
        pull_requests = [
            release_notes.PullRequest(7, "Safe title", "", (), "dependabot[bot]", True, "https://github.com/o/r/pull/7")
        ]
        response = {
            "overview": "See [site](https://evil.example)",
            "pull_requests": [
                {"number": 7, "category": "Improvements", "summary": "## Surprise @everyone"}
            ],
        }
        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="owner",
            api_key="secret",
            request_json=lambda *_args: {"choices": [{"message": {"content": json.dumps(response)}}]},
        )

        self.assertNotIn("evil.example", notes)
        self.assertNotIn("@everyone", notes)
        self.assertIn("Safe title [#7](https://github.com/o/r/pull/7)", notes)
        self.assertNotIn("## Thanks", notes)

    def test_model_text_with_line_separators_or_controls_is_rejected(self):
        pull_requests = [
            release_notes.PullRequest(
                7,
                "Trusted fallback",
                "",
                (),
                "owner",
                False,
                "https://github.com/o/r/pull/7",
            )
        ]
        for unsafe in ("\r", "\v", "\f", "\x1c", "\x85", "\u2028", "\u2029"):
            with self.subTest(unsafe=ascii(unsafe)):
                response = {
                    "overview": f"First{unsafe}Injected overview",
                    "pull_requests": [
                        {
                            "number": 7,
                            "category": "Improvements",
                            "summary": f"Safe{unsafe}Injected summary",
                        }
                    ],
                }
                notes = release_notes.generate_notes(
                    pull_requests,
                    [],
                    repository_owner="owner",
                    api_key="secret",
                    request_json=lambda *_args, response=response: {
                        "choices": [{"message": {"content": json.dumps(response)}}]
                    },
                )

                self.assertNotIn("Injected", notes)
                self.assertIn("Trusted fallback", notes)

    def test_model_text_with_bare_domain_or_oversized_fields_is_rejected(self):
        pull_requests = [
            release_notes.PullRequest(7, "Trusted fallback", "", (), "owner", False, "https://github.com/o/r/pull/7")
        ]
        unsafe_responses = (
            {"overview": "Visit evil.example/path", "pull_requests": [{"number": 7, "category": "Improvements", "summary": "Safe summary"}]},
            {"overview": "Normal overview", "pull_requests": [{"number": 7, "category": "Improvements", "summary": "x" * (release_notes.MAX_SUMMARY_CHARS + 1)}]},
            {"overview": "x" * (release_notes.MAX_OVERVIEW_CHARS + 1), "pull_requests": [{"number": 7, "category": "Improvements", "summary": "Safe summary"}]},
        )
        for response in unsafe_responses:
            with self.subTest(response=response):
                notes = release_notes.generate_notes(
                    pull_requests,
                    [],
                    repository_owner="owner",
                    api_key="secret",
                    request_json=lambda *_args, response=response: {
                        "choices": [{"message": {"content": json.dumps(response)}}]
                    },
                )
                self.assertIn("Trusted fallback", notes)
                self.assertNotIn("evil.example", notes)

    def test_openai_payload_is_bounded_before_request(self):
        pull_requests = [
            release_notes.PullRequest(
                7,
                "T" * (release_notes.MAX_AI_TITLE_CHARS + 100),
                "B" * (release_notes.MAX_AI_BODY_CHARS + 100),
                tuple(f"label-{index}" for index in range(50)),
                "owner",
                False,
                "https://github.com/o/r/pull/7",
            )
        ]
        captured = {}

        def request(_url, _headers, payload):
            captured.update(payload)
            return {
                "choices": [{"message": {"content": json.dumps({
                    "overview": "Bounded notes.",
                    "pull_requests": [{"number": 7, "category": "Improvements", "summary": "Bounded change."}],
                })}}]
            }

        release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="owner",
            api_key="secret",
            request_json=request,
        )

        user_data = json.loads(captured["messages"][1]["content"])
        self.assertLessEqual(len(user_data[0]["title"]), release_notes.MAX_AI_TITLE_CHARS)
        self.assertLessEqual(len(user_data[0]["body"]), release_notes.MAX_AI_BODY_CHARS)
        self.assertLessEqual(len(captured["messages"][1]["content"].encode()), release_notes.MAX_AI_REQUEST_BYTES)
        self.assertEqual(release_notes.MAX_COMPLETION_TOKENS, captured["max_completion_tokens"])
        self.assertEqual("gpt-5.6-terra", captured["model"])

    def test_too_many_pull_requests_skip_ai_and_use_bounded_fallback(self):
        pull_requests = [
            release_notes.PullRequest(
                number,
                f"feat: change {number}",
                "body",
                (),
                "owner",
                False,
                f"https://github.com/o/r/pull/{number}",
            )
            for number in range(1, release_notes.MAX_AI_PULL_REQUESTS + 2)
        ]

        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="owner",
            api_key="secret",
            request_json=lambda *_args: self.fail("oversized input must not call OpenAI"),
        )

        self.assertIn(f"[#{release_notes.MAX_AI_PULL_REQUESTS + 1}]", notes)
        self.assertLessEqual(len(notes.encode()), release_notes.MAX_RELEASE_NOTES_BYTES)

    def test_api_failure_uses_deterministic_fallback(self):
        pull_requests = [
            release_notes.PullRequest(8, "Repair updater", "", ("bug",), "carol", False, "https://github.com/o/r/pull/8")
        ]

        def fail(*_args):
            raise OSError("offline")

        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="owner",
            api_key="secret",
            request_json=fail,
        )

        self.assertIn("## Bug fixes", notes)
        self.assertIn("Repair updater [#8](https://github.com/o/r/pull/8)", notes)

    def test_fallback_categorizes_conventional_pull_request_titles(self):
        pull_requests = [
            release_notes.PullRequest(1, "feat(cli): add onboarding", "", (), "owner", False, "https://github.com/o/r/pull/1"),
            release_notes.PullRequest(2, "[verified] fix: prevent startup crash", "", (), "owner", False, "https://github.com/o/r/pull/2"),
            release_notes.PullRequest(3, "security: rotate browser sessions", "", (), "owner", False, "https://github.com/o/r/pull/3"),
            release_notes.PullRequest(4, "docs: explain native setup", "", (), "owner", False, "https://github.com/o/r/pull/4"),
            release_notes.PullRequest(5, "ci(release): test artifacts", "", (), "owner", False, "https://github.com/o/r/pull/5"),
            release_notes.PullRequest(6, "refactor: simplify navigation", "", (), "owner", False, "https://github.com/o/r/pull/6"),
        ]

        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="owner",
            api_key="",
            request_json=lambda *_args: self.fail("missing key must not call OpenAI"),
        )

        sections = {}
        current = None
        for line in notes.splitlines():
            if line.startswith("## "):
                current = line[3:]
                sections[current] = []
            elif current and line.startswith("- "):
                sections[current].append(line)
        self.assertTrue(any("#1" in line for line in sections["New features"]))
        self.assertTrue(any("#2" in line for line in sections["Bug fixes"]))
        self.assertTrue(any("#3" in line for line in sections["Security"]))
        self.assertTrue(any("#4" in line for line in sections["Documentation"]))
        self.assertTrue(any("#5" in line for line in sections["Developer experience"]))
        self.assertTrue(any("#6" in line for line in sections["Improvements"]))

    def test_explicit_maintainers_are_not_thanked_as_contributors(self):
        pull_requests = [
            release_notes.PullRequest(1, "Feature", "", (), "P5ina", False, "https://github.com/o/r/pull/1"),
            release_notes.PullRequest(2, "Another feature", "", (), "alice", False, "https://github.com/o/r/pull/2"),
        ]

        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="triangle-int",
            excluded_contributors=("P5ina",),
            api_key="",
            request_json=lambda *_args: self.fail("missing key must not call OpenAI"),
        )

        self.assertNotIn("@P5ina", notes)
        self.assertIn("[@alice](https://github.com/alice)", notes)

    def test_collection_deduplicates_associated_prs_and_keeps_direct_commits(self):
        commits = [
            release_notes.DirectCommit("aaa1111", "Feature commit"),
            release_notes.DirectCommit("bbb2222", "Merge follow-up"),
            release_notes.DirectCommit("ccc3333", "Direct maintenance"),
        ]
        pr_json = {
            "number": 20,
            "title": "Ship feature",
            "body": "Details",
            "labels": [{"name": "feature"}],
            "user": {"login": "alice", "type": "User"},
            "html_url": "https://github.com/o/r/pull/20",
            "state": "closed",
            "merged_at": "2026-09-19T12:00:00Z",
        }
        calls = []

        def associated(sha):
            calls.append(sha)
            return [pr_json] if sha in {"aaa1111", "bbb2222"} else []

        prs, direct = release_notes.collect_changes(commits, associated, repository="o/r")

        self.assertEqual([20], [pr.number for pr in prs])
        self.assertEqual(["ccc3333"], [commit.sha for commit in direct])
        self.assertEqual(["aaa1111", "bbb2222", "ccc3333"], calls)

    def test_collection_ignores_unmerged_associated_pull_requests(self):
        commits = [
            release_notes.DirectCommit("aaa1111", "Open PR commit"),
            release_notes.DirectCommit("bbb2222", "Closed PR commit"),
            release_notes.DirectCommit("ccc3333", "Merged PR commit"),
        ]

        def metadata(number, *, state, merged_at):
            return {
                "number": number,
                "title": f"PR {number}",
                "body": "",
                "labels": [],
                "user": {"login": "alice", "type": "User"},
                "html_url": f"https://github.com/o/r/pull/{number}",
                "state": state,
                "merged_at": merged_at,
            }

        associated = {
            "aaa1111": [metadata(20, state="open", merged_at=None)],
            "bbb2222": [metadata(21, state="closed", merged_at=None)],
            "ccc3333": [metadata(22, state="closed", merged_at="2026-09-19T12:00:00Z")],
        }

        prs, direct = release_notes.collect_changes(
            commits,
            associated.__getitem__,
            repository="o/r",
        )

        self.assertEqual([22], [pr.number for pr in prs])
        self.assertEqual(["aaa1111", "bbb2222"], [commit.sha for commit in direct])

    def test_collection_constructs_canonical_github_urls(self):
        commits = [release_notes.DirectCommit("aaa1111", "Merged PR commit")]
        metadata = {
            "number": 23,
            "title": "Safe change",
            "body": "",
            "labels": [],
            "user": {"login": "alice", "type": "User"},
            "html_url": "https://evil.example/stolen",
            "state": "closed",
            "merged_at": "2026-09-19T12:00:00Z",
        }

        prs, direct = release_notes.collect_changes(
            commits,
            lambda _sha: [metadata],
            repository="o/r",
        )
        notes = release_notes.generate_notes(
            prs,
            direct,
            repository_owner="owner",
            api_key="",
            request_json=lambda *_args: self.fail("missing key must not call OpenAI"),
        )

        self.assertIn("[#23](https://github.com/o/r/pull/23)", notes)
        self.assertIn("[@alice](https://github.com/alice)", notes)
        self.assertNotIn("evil.example", notes)

    def test_collection_rejects_invalid_github_logins(self):
        commit = release_notes.DirectCommit("aaa1111", "Merged PR commit")
        for login in ("alice/evil", "-alice", "alice-", "alice--evil"):
            with self.subTest(login=login):
                metadata = {
                    "number": 23,
                    "title": "Safe change",
                    "body": "",
                    "labels": [],
                    "user": {"login": login, "type": "User"},
                    "state": "closed",
                    "merged_at": "2026-09-19T12:00:00Z",
                }
                with self.assertRaises(ValueError):
                    release_notes.collect_changes(
                        [commit],
                        lambda _sha, metadata=metadata: [metadata],
                        repository="o/r",
                    )

    def test_commit_collection_uses_exact_tag_range(self):
        calls = []

        def run(command):
            calls.append(command)
            return "aaa111122223333\x1fFeature\n"

        commits = release_notes.load_commits("v1.1.0", "v1.2.0", run)

        self.assertEqual(
            [["git", "log", "--format=%H%x1f%s", "v1.1.0..v1.2.0"]],
            calls,
        )
        self.assertEqual([release_notes.DirectCommit("aaa111122223333", "Feature")], commits)

    def test_cli_generates_notes_offline_from_release_range_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            calls = temp / "calls"
            output = temp / "notes.md"
            git = temp / "git"
            git.write_text(
                "#!/bin/sh\n"
                "printf 'git %s\\n' \"$*\" >> \"$CALLS\"\n"
                "case \"$1\" in\n"
                "  describe) printf 'v1.1.0\\n' ;;\n"
                "  log) printf 'aaa111122223333\\037Feature commit\\nccc333344445555\\037Direct commit\\n' ;;\n"
                "esac\n"
            )
            gh = temp / "gh"
            gh.write_text(
                "#!/bin/sh\n"
                "printf 'gh %s\\n' \"$*\" >> \"$CALLS\"\n"
                "case \"$*\" in\n"
                "  *commits/aaa111122223333/pulls*) printf '%s\\n' '[{\"number\":30,\"title\":\"Feature PR\",\"body\":\"Body\",\"labels\":[{\"name\":\"feature\"}],\"user\":{\"login\":\"alice\",\"type\":\"User\"},\"html_url\":\"https://evil.example/stolen\",\"state\":\"closed\",\"merged_at\":\"2026-09-19T12:00:00Z\"}]' ;;\n"
                "  *commits/ccc333344445555/pulls*) printf '[]\\n' ;;\n"
                "  *repos/o/r*) printf '%s\\n' '{\"owner\":{\"login\":\"owner\"}}' ;;\n"
                "esac\n"
            )
            git.chmod(0o755)
            gh.chmod(0o755)
            env = os.environ.copy()
            env.update({"PATH": f"{temp}:{env['PATH']}", "CALLS": str(calls)})
            env.pop("OPENAI_API_KEY", None)

            completed = subprocess.run(
                [sys.executable, str(ROOT / "scripts/release_notes.py"), "--repository", "o/r", "--release-tag", "v1.2.0", "--output", str(output)],
                text=True,
                capture_output=True,
                env=env,
            )

            self.assertEqual(0, completed.returncode, completed.stderr)
            notes = output.read_text()
            self.assertIn("Feature PR [#30](https://github.com/o/r/pull/30)", notes)
            self.assertIn("Direct commit (`ccc3333`)", notes)
            calls_text = calls.read_text()
            self.assertIn("git describe --tags --match v* --abbrev=0 v1.2.0^", calls_text)
            self.assertIn("git log --format=%H%x1f%s v1.1.0..v1.2.0", calls_text)

    def test_response_with_extra_fields_is_rejected(self):
        pull_requests = [
            release_notes.PullRequest(9, "Trusted fallback", "", (), "owner", False, "https://github.com/o/r/pull/9")
        ]
        response = {
            "overview": "Looks valid.",
            "pull_requests": [
                {"number": 9, "category": "Improvements", "summary": "Model summary.", "url": "https://evil.example"}
            ],
        }
        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="owner",
            api_key="secret",
            request_json=lambda *_args: {"choices": [{"message": {"content": json.dumps(response)}}]},
        )

        self.assertIn("Trusted fallback", notes)
        self.assertNotIn("Model summary", notes)

    def test_inline_markdown_in_model_summary_is_rejected(self):
        pull_requests = [
            release_notes.PullRequest(10, "Plain fallback", "", (), "owner", False, "https://github.com/o/r/pull/10")
        ]
        response = {
            "overview": "A normal overview.",
            "pull_requests": [
                {"number": 10, "category": "Improvements", "summary": "A **bold** change."}
            ],
        }
        notes = release_notes.generate_notes(
            pull_requests,
            [],
            repository_owner="owner",
            api_key="secret",
            request_json=lambda *_args: {"choices": [{"message": {"content": json.dumps(response)}}]},
        )

        self.assertIn("Plain fallback", notes)
        self.assertNotIn("**bold**", notes)


if __name__ == "__main__":
    unittest.main()
