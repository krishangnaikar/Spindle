#!/usr/bin/env python3
"""Run a reproducible check, saving argv/timestamps/exit code without env dumps."""
import datetime, json, os, pathlib, subprocess, sys, time
root = pathlib.Path(__file__).resolve().parents[1]
name, *cmd = sys.argv[1:]
assert name and cmd
out = root / 'evidence'
out.mkdir(exist_ok=True)
env = os.environ.copy()
local = root.parent / 'TangleClaw-build-tools'
if (local / 'cargo/bin/cargo').exists():
    env['CARGO_HOME'] = str(local / 'cargo')
    env['RUSTUP_HOME'] = str(local / 'rustup')
    env['PATH'] = str(local / 'cargo/bin') + os.pathsep + env.get('PATH', '')
start = time.monotonic()
receipt = {'argv': cmd, 'cwd': str(root), 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
path = out / (name + '.json')
path.write_text(json.dumps(receipt, indent=2) + '\n')
with (out / (name + '.log')).open('wb') as f:
    result = subprocess.run(cmd, cwd=root, env=env, stdout=f, stderr=subprocess.STDOUT)
receipt.update(exit_code=result.returncode, elapsed_seconds=time.monotonic()-start, finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
path.write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt))
print((out / (name + '.log')).read_text(errors='replace')[-6000:])
sys.exit(result.returncode)
