"""Collect read-only CLI diagnostics from an installed game, without game data.

Usage: python tests/read_only_cli_smoke.py GAME_DIR docs/windows-cli-2026-10-21.txt
The output contains local paths and mod names. No game files are copied.
"""

import pathlib
import subprocess
import sys

if len(sys.argv) != 3:
    raise SystemExit("usage: python tests/read_only_cli_smoke.py GAME_DIR OUTPUT.txt")

root = pathlib.Path(__file__).resolve().parents[1]
exe = root / "target" / "debug" / "bocw_mod_tools.exe"
game_dir = pathlib.Path(sys.argv[1])
out_path = pathlib.Path(sys.argv[2])
commands = [
    ("doctor",),
    ("status",),
    ("info",),
    ("server", "status"),
    ("proxy", "status"),
    ("verify",),
    ("inspect", "zm_common"),
    ("inspect", "ww_zm_tungsten"),
]

lines = []
failed = False
for command in commands:
    args = [str(exe), "--game-dir", str(game_dir), *command]
    proc = subprocess.run(args, capture_output=True, check=False)
    lines.append(f"> {exe} --game-dir '{game_dir}' " + " ".join(command))
    lines.append(proc.stdout.decode("utf-8", errors="replace").rstrip())
    lines.append(proc.stderr.decode("utf-8", errors="replace").rstrip())
    lines.append(f"exit={proc.returncode}\n")
    failed |= proc.returncode != 0

out_path.write_text("\n".join(lines), encoding="utf-8")
print(f"Read-only CLI log: {out_path}; commands={len(commands)}; failures={int(failed)}")
raise SystemExit(1 if failed else 0)
