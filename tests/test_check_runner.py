"""Exercise the real CLI in isolated temporary repositories, never live evidence."""
import datetime
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest


class CheckRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        scripts = self.root / "scripts"
        scripts.mkdir()
        self.script = scripts / "check.py"
        shutil.copyfile(pathlib.Path(__file__).resolve().parents[1] / "scripts/check.py", self.script)

    def run_check(self, *args):
        return subprocess.run([sys.executable, str(self.script), *args], cwd=self.root.parent, capture_output=True, text=True, timeout=10)

    def receipt(self, name):
        return json.loads((self.root / "evidence" / (name + ".json")).read_text())

    def test_success_records_exact_argv_cwd_timing_and_combines_streams(self):
        command = [sys.executable, "-c", "import sys; print('stdout evidence'); print('stderr evidence', file=sys.stderr)"]
        result = self.run_check("success", *command)
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = self.receipt("success")
        self.assertEqual(receipt["argv"], command)
        self.assertEqual(receipt["cwd"], str(self.root))
        self.assertEqual(receipt["exit_code"], 0)
        self.assertGreaterEqual(receipt["elapsed_seconds"], 0)
        start = datetime.datetime.fromisoformat(receipt["started_utc"])
        end = datetime.datetime.fromisoformat(receipt["finished_utc"])
        self.assertEqual(start.utcoffset(), datetime.timedelta(0))
        self.assertGreaterEqual(end, start)
        log = (self.root / "evidence/success.log").read_text()
        self.assertIn("stdout evidence", log)
        self.assertIn("stderr evidence", log)
        self.assertEqual(json.loads(result.stdout.splitlines()[0]), receipt)
        self.assertEqual(set(receipt), {"argv", "cwd", "started_utc", "finished_utc", "elapsed_seconds", "exit_code"})

    def test_failure_preserves_output_and_exit_code(self):
        result = self.run_check("failure", sys.executable, "-c", "print('failure evidence'); raise SystemExit(7)")
        self.assertEqual(result.returncode, 7)
        self.assertEqual(self.receipt("failure")["exit_code"], 7)
        self.assertIn("failure evidence", (self.root / "evidence/failure.log").read_text())

    def test_invalid_invocations_create_no_evidence(self):
        for args in [(), ("name",)]:
            with self.subTest(args=args):
                result = self.run_check(*args)
                self.assertEqual(result.returncode, 2)
                self.assertIn("usage:", result.stderr)
        self.assertFalse((self.root / "evidence").exists())

    def test_invalid_names_cannot_escape_or_create_files(self):
        for name in ["", "../escape", "/absolute", "nested/path", "has space", ".hidden", "a" * 81, "line\nbreak", "name.json", "-leading"]:
            with self.subTest(name=name):
                self.assertEqual(self.run_check(name, sys.executable, "-c", "pass").returncode, 2)
        self.assertFalse((self.root / "evidence").exists())

    def test_existing_receipt_and_log_are_not_overwritten_or_reexecuted(self):
        command = [sys.executable, "-c", "from pathlib import Path; p=Path('runs'); p.write_text(p.read_text()+'x' if p.exists() else 'x')"]
        self.assertEqual(self.run_check("same", *command).returncode, 0)
        evidence = self.root / "evidence"
        before = {p.name: p.read_bytes() for p in evidence.iterdir()}
        result = self.run_check("same", *command)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(before, {p.name: p.read_bytes() for p in evidence.iterdir()})
        self.assertEqual((self.root / "runs").read_text(), "x")

    def test_orphan_log_is_preserved_and_no_receipt_is_left(self):
        evidence = self.root / "evidence"
        evidence.mkdir()
        (evidence / "orphan.log").write_bytes(b"previous incomplete run")
        self.assertEqual(self.run_check("orphan", sys.executable, "-c", "pass").returncode, 2)
        self.assertEqual((evidence / "orphan.log").read_bytes(), b"previous incomplete run")
        self.assertFalse((evidence / "orphan.json").exists())

    def test_missing_executable_produces_completed_failure_receipt(self):
        result = self.run_check("missing", str(self.root / "nonexistent-program"))
        self.assertEqual(result.returncode, 127)
        receipt = self.receipt("missing")
        self.assertEqual(receipt["exit_code"], 127)
        self.assertEqual(receipt["launch_error"]["errno"], 2)
        self.assertIn("finished_utc", receipt)
        self.assertIn("command could not start", (self.root / "evidence/missing.log").read_text())

    def test_argument_metacharacters_are_not_interpreted_by_a_shell(self):
        literal = "$(touch unexpected); `touch unexpected`"
        result = self.run_check("argv", sys.executable, "-c", "import sys; print(sys.argv[1])", literal)
        self.assertEqual(result.returncode, 0)
        self.assertIn(literal, (self.root / "evidence/argv.log").read_text())
        self.assertFalse((self.root / "unexpected").exists())


if __name__ == "__main__":
    unittest.main()
