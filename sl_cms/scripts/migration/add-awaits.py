#!/usr/bin/env python3
"""Insert `.await` where the compiler says a future is being used as a value.

Once the storage traits are async, every call site is a type error, and the errors are all the
same shape: `no method named `unwrap` found for opaque type `impl Future<...>``. The compiler
gives an exact line and column — the position of the `.` that should have been preceded by
`.await` — so the fix can be applied from the diagnostics instead of by guessing at names.

Usage: add-awaits.py <package>
"""

import collections
import os
import pathlib
import re
import subprocess
import sys

DIAGNOSTIC = re.compile(
    r"^(?P<file>[^:\n]+):(?P<line>\d+):(?P<col>\d+): error\[E0599\]: "
    r"no method named `(?P<name>\w+)` found for opaque type",
    re.MULTILINE,
)


def cargo_check(package: str) -> str:
    environment = dict(os.environ)
    environment.setdefault("CARGO_HOME", "/tmp/dsh-cargo-home")
    environment["PATH"] = f"{pathlib.Path.home()}/.cargo/bin:" + environment["PATH"]
    done = subprocess.run(
        ["cargo", "check", "-p", package, "--all-targets", "--message-format=short"],
        capture_output=True,
        text=True,
        env=environment,
    )
    return done.stdout + done.stderr


def main(package: str) -> int:
    for round_number in range(1, 21):
        diagnostics = cargo_check(package)
        spots: dict[str, list[tuple[int, int]]] = collections.defaultdict(list)
        for match in DIAGNOSTIC.finditer(diagnostics):
            spots[match["file"]].append((int(match["line"]), int(match["col"])))

        if not spots:
            print(f"round {round_number}: nothing left to await")
            return 0

        total = 0
        for file, positions in spots.items():
            lines = pathlib.Path(file).read_text().split("\n")
            # Right to left, so an insertion does not move the positions still to come.
            for line, col in sorted(positions, reverse=True):
                text = lines[line - 1]
                index = col - 1
                if text[index] != ".":
                    index = text.index(".", max(0, col - 2))
                lines[line - 1] = text[:index] + ".await" + text[index:]
                total += 1
            pathlib.Path(file).write_text("\n".join(lines))
        print(f"round {round_number}: inserted {total} awaits in {len(spots)} files")

    print("gave up after 20 rounds", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
