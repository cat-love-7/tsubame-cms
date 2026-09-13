#!/usr/bin/env python3
"""Rewrite `self.runtime.block_on(async move { ... })` wrappers into the enclosing async fn.

The AWS adapter was written async from the start, with a one-line wrapper per trait method that
handed the future to a runtime thread (the `BlockingRuntime` bridge). Now that the traits
themselves are async, the wrapper is what has to go: the method body is inlined and the method
becomes `async fn`.

The wrapper is always the last expression of its method, so the job is:

    fn f(&self, ...) -> T {          async fn f(&self, ...) -> T {
        setup();                         setup();
        self.runtime.block_on(async       BODY            <- dedented by 4
            move { BODY })           }
    }

Finding BODY means matching braces, which the earlier attempts got wrong because `{}` appears
inside `format!` strings. This scanner tracks double-quoted strings, escapes and line comments,
so braces inside them are ignored. It refuses to touch a file it does not fully understand
rather than leaving half a rewrite behind.
"""

import pathlib
import sys

WRAPPER = "self.runtime.block_on(async move {"


def skip_string(text: str, i: int) -> int:
    """`i` is at a `"`; return the index just past the closing one."""
    i += 1
    while i < len(text):
        c = text[i]
        if c == "\\":
            i += 2
            continue
        if c == '"':
            return i + 1
        i += 1
    raise ValueError("unterminated string")


def matching_brace(text: str, open_index: int) -> int:
    """`text[open_index] == '{'`; return the index of its `}`."""
    depth = 0
    i = open_index
    while i < len(text):
        c = text[i]
        if c == '"':
            i = skip_string(text, i)
            continue
        if c == "/" and text.startswith("//", i):
            i = text.find("\n", i)
            if i == -1:
                break
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError("unbalanced braces")


def unwrap(path: pathlib.Path, expected: int) -> int:
    text = path.read_text()
    done = 0
    while WRAPPER in text:
        start = text.index(WRAPPER)
        brace = text.index("{", start + len(WRAPPER) - 1)
        end = matching_brace(text, brace)

        # The wrapper has to be the last expression: only whitespace and `)` may follow the
        # closing brace, and then the method's own closing brace.
        rest = text[end + 1 :]
        if not rest.startswith(")\n"):
            raise ValueError(f"{path}: the wrapper is not the last expression")

        body = text[brace + 1 : end]
        dedented = (
            "\n".join(line[4:] if line.startswith("    ") else line for line in body.split("\n"))
            .strip("\n")
            .rstrip()
        )
        # The wrapper line, the body's extra indentation and the `})` line go; what is left is
        # the method body, closed by the method's own brace. The wrapper's own indentation goes
        # with it, or it would end up in front of the body's first line.
        text = text[:start].rstrip() + "\n" + dedented + "\n" + text[end + 3 :]
        done += 1

    if done != expected:
        raise ValueError(f"{path}: rewrote {done} wrappers, expected {expected}")

    # Every method that had a wrapper becomes async.
    for name in [
        "get_user_from_id",
        "get_user_from_username",
        "add_user",
        "update_user",
        "get_all_users",
        "delete_user",
    ]:
        old = f"\n    fn {name}("
        if old in text:
            text = text.replace(old, f"\n    async fn {name}(", 1)

    path.write_text(text)
    return done


if __name__ == "__main__":
    path = pathlib.Path(sys.argv[1])
    expected = int(sys.argv[2])
    print(f"{path}: {unwrap(path, expected)} wrappers unwrapped")
