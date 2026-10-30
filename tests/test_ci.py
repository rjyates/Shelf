import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("ci", Path(__file__).parents[1] / "scripts" / "ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


class SelectionTests(unittest.TestCase):
    def test_server_does_not_build_mobile(self):
        self.assertEqual(ci.affected(["server/src/main.rs", "web/app.css"]), {"server": True, "mobile": False, "android": False, "ios": False})

    def test_mobile_does_not_build_server(self):
        self.assertEqual(ci.affected(["mobile/lib/main.dart"]), {"server": False, "mobile": True, "android": True, "ios": True})
        self.assertEqual(ci.affected([".flutter-version"]), {"server": False, "mobile": True, "android": True, "ios": True})

    def test_platform_changes_keep_the_shared_mobile_version(self):
        self.assertEqual(ci.affected(["mobile/android/app/build.gradle.kts"]), {"server": False, "mobile": True, "android": True, "ios": True})

    def test_contract_and_ci_affect_both(self):
        for path in ("protocol/openapi.json", ".github/workflows/ci.yml"):
            self.assertTrue(all(ci.affected([path]).values()))

    def test_docs_do_not_ship(self):
        self.assertFalse(any(ci.affected(["docs/architecture.md", "README.md"]).values()))

    def test_web_tests_do_not_rebuild_mobile(self):
        self.assertFalse(ci.affected(["tests/web.test.mjs", "scripts/smoke.py", "scripts/compose_smoke.py"])["mobile"])

    def test_independent_versions_and_internal_patch(self):
        self.assertEqual(ci.bump("server-v1.2.3", ["feat(server): add uploads"]), "1.3.0")
        self.assertEqual(ci.bump("mobile-v2.4.1", ["chore(mobile): update dependency"]), "2.4.2")
        self.assertEqual(ci.bump("server-v1.2.3", ["feat(server)!: replace protocol"]), "2.0.0")
        self.assertEqual(ci.bump("mobile-v1.2.3", ["fix: change\n\nBREAKING CHANGE: remove auth"]), "2.0.0")
        self.assertEqual(ci.bump(None, []), "0.1.0")


class SemanticTests(unittest.TestCase):
    def test_squash_title_cannot_drop_a_breaking_footer_or_feature(self):
        for message, invalid, valid in [
            ("feat(server): add reader", "fix: add reader", "feat: add reader"),
            ("refactor(server): replace API\n\nBREAKING CHANGE: remove v1", "refactor: replace API", "refactor!: replace API"),
        ]:
            with patch.object(ci, "git", side_effect=lambda *args: "abc123" if args[0] == "rev-list" else message):
                with self.assertRaisesRegex(ValueError, "PR title must preserve"):
                    ci.check_semantics("base", "head", invalid)
                ci.check_semantics("base", "head", valid)


class HistoryTests(unittest.TestCase):
    def test_only_relevant_commits_bump_a_component(self):
        with tempfile.TemporaryDirectory() as folder:
            def git(*args):
                return subprocess.check_output(["git", "-C", folder, *args], text=True, encoding="utf-8").strip()
            git("init", "--quiet")
            def commit(path, message):
                file = Path(folder) / path
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_text(message)
                git("add", path)
                git("-c", "user.name=CI Test", "-c", "user.email=ci@example.invalid", "commit", "-qm", message)
            commit("server/src/lib.rs", "feat(server): initial")
            git("tag", "server-v1.0.0")
            base = git("rev-parse", "HEAD")
            commit("mobile/lib/main.dart", "feat(mobile)!: change mobile")
            commit("server/src/lib.rs", "chore(server): update dependency")
            with patch.object(ci, "git", side_effect=git):
                plan = ci.make_plan(base, "HEAD")
            self.assertEqual(plan["components"]["server"]["version"], "1.0.1")
            self.assertEqual(len(plan["components"]["server"]["commits"]), 1)
            self.assertTrue(plan["selection"]["mobile"])
            git("tag", "server-v1.0.1")
            git("tag", "mobile-v0.1.0")
            before_move = git("rev-parse", "HEAD")
            with patch.object(ci, "git", side_effect=git):
                retry = ci.make_plan(base, "HEAD")
            self.assertFalse(any(retry["selection"].values()), "A published revision must not release again")
            git("mv", "mobile/lib/main.dart", "server/src/moved.dart")
            git("-c", "user.name=CI Test", "-c", "user.email=ci@example.invalid", "commit", "-qm", "refactor: move shared source")
            with patch.object(ci, "git", side_effect=git):
                moved = ci.make_plan(before_move, "HEAD")
            self.assertTrue(moved["selection"]["server"])
            self.assertTrue(moved["selection"]["mobile"], "Removing a mobile file must still validate mobile")
            before_unicode = git("rev-parse", "HEAD")
            commit("mobile/assets/café.txt", "chore(mobile): add localized asset")
            with patch.object(ci, "git", side_effect=git):
                paths = ci.changed(before_unicode, "HEAD")
            self.assertIn("mobile/assets/café.txt", paths)
            self.assertTrue(ci.affected(paths)["mobile"])
            self.assertFalse(ci.affected(paths)["server"])


if __name__ == "__main__":
    unittest.main()
