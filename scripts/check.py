#!/usr/bin/env python3
"""Run a reproducible check, saving argv/timestamps/exit code without env dumps."""
import datetime
import json
import os
import pathlib
import re
import subprocess
import sys
import time

NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,79}\Z")


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def main(argv=None):
    args = sys.argv[1:] if argv is None else argv
    if len(args) < 2 or not NAME.fullmatch(args[0]):
        print("usage: check.py NAME COMMAND [ARG ...]; NAME must be 1-80 letters, digits, underscores or hyphens, beginning with a letter or digit", file=sys.stderr)
        return 2
    name, *cmd = args
    root = pathlib.Path(__file__).resolve().parents[1]
    out = root / "evidence"
    out.mkdir(exist_ok=True)
    path = out / (name + ".json")
    log = out / (name + ".log")
    # Reserve both files exclusively. Never overwrite an earlier run, including
    # incomplete evidence or dangling symlinks. Choose a new name for each run.
    try:
        receipt_file = path.open("x", encoding="utf-8")
    except FileExistsError:
        print("refusing to overwrite existing receipt; choose a new NAME", file=sys.stderr)
        return 2
    try:
        log_file = log.open("xb")
    except FileExistsError:
        receipt_file.close()
        path.unlink()
        print("refusing to overwrite existing log; choose a new NAME", file=sys.stderr)
        return 2
    env = os.environ.copy()
    local = root.parent / "TangleClaw-build-tools"
    if (local / "cargo/bin/cargo").exists():
        env["CARGO_HOME"] = str(local / "cargo")
        env["RUSTUP_HOME"] = str(local / "rustup")
        env["PATH"] = str(local / "cargo/bin") + os.pathsep + env.get("PATH", "")
    start = time.monotonic()
    receipt = {"argv": cmd, "cwd": str(root), "started_utc": utc_now()}
    with receipt_file, log_file:
        receipt_file.write(json.dumps(receipt, indent=2) + "\n")
        receipt_file.flush()
        try:
            result = subprocess.run(cmd, cwd=root, env=env, stdout=log_file, stderr=subprocess.STDOUT)
            code = result.returncode
        except OSError as error:
            code = 127 if error.errno == 2 else 126
            # Record the launch failure, not environment values or credential discovery.
            receipt["launch_error"] = {"errno": error.errno, "message": error.strerror}
            log_file.write(("command could not start: " + str(error.strerror) + "\n").encode())
        receipt.update(exit_code=code, elapsed_seconds=time.monotonic() - start, finished_utc=utc_now())
        receipt_file.seek(0)
        receipt_file.truncate()
        receipt_file.write(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt))
    print(log.read_text(errors="replace")[-6000:])
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    sys.exit(main())
