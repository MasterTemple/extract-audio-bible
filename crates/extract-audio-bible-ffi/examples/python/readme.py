"""Runs every Python example in ../../README.md, in order, as one program, and checks what each
prints against its `# → ` comments (one for each line printed, in order; `…` stands for any text).

Run after `pip install dist/python/wheelhouse/*.whl` (see ../../README.md):
    python examples/python/readme.py [--show]

The examples run in a new folder holding the files in ../fixtures.
"""

import contextlib
import io
import os
import pathlib
import re
import shutil
import sys
import tempfile

here = pathlib.Path(__file__).resolve().parent
show = "--show" in sys.argv
fixtures = here.parent / "fixtures"
readme = (here.parent.parent / "README.md").read_text()
blocks = re.findall(r"^```python\n(.*?)^```$", readme, flags=re.M | re.S)

folder = pathlib.Path(tempfile.mkdtemp(prefix="extract-audio-bible-readme-"))
for file in fixtures.iterdir():
    shutil.copy(file, folder / file.name)
os.chdir(folder)

namespace: dict = {"__name__": "__readme__"}
printed = []
for block in blocks:
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        exec(compile(block, "README.md", "exec"), namespace)
    # A line at a time, blank lines left out
    printed.append([line for line in out.getvalue().splitlines() if line.strip()])


def flat(text: str) -> str:
    return " ".join(text.split())


def matches(expected: str, actual: str) -> bool:
    if expected == actual:
        return True
    parts = expected.split("…")
    if len(parts) == 1 or not actual.startswith(parts[0]) or not actual.endswith(parts[-1]):
        return False
    at = len(parts[0])
    for part in parts[1:-1]:
        found = actual.find(part, at)
        if found < 0:
            return False
        at = found + len(part)
    return at <= len(actual) - len(parts[-1])


failed = 0
for block, actual in zip(blocks, printed):
    expected = [flat(e) for e in re.findall(r"# → (.*)$", block, flags=re.M)]
    actual = [flat(a) for a in actual]
    ok = len(expected) == len(actual) and all(matches(e, a) for e, a in zip(expected, actual))
    failed += not ok
    if not ok or show:
        line = readme[: readme.index(block)].count("\n") + 1
        print(f"{'ok' if ok else 'FAILED'}: the example at README.md:{line}")
        for j in range(max(len(expected), len(actual))):
            print(f"  {expected[j] if j < len(expected) else '(nothing)'}")
            print(f"  {actual[j] if j < len(actual) else '(nothing printed)'}")
os.chdir(here)
if failed:
    print(f"{failed} of {len(blocks)} examples print otherwise than README.md says (run in {folder})")
else:
    shutil.rmtree(folder)
    print(f"all {len(blocks)} examples print what README.md says")
sys.exit(1 if failed else 0)
