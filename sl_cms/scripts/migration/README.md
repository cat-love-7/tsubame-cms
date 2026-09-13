# Tools used while moving the storage layer from synchronous to asynchronous

Both exist because the change is wide and mechanical, and doing it by regex is what made the
three earlier attempts fail. They are kept here as the record of how it was done.

## `unwrap-bridge.py <file> <expected-count>`

The AWS adapter was written `async` from the start, with a one-line wrapper per trait method
that handed the future to a runtime thread (`BlockingRuntime`), which is what made synchronous
traits possible. Once the traits themselves are async, the wrapper is what has to go:

```rust
fn f(&self) -> T {                      async fn f(&self) -> T {
    setup();                                setup();
    self.runtime.block_on(async move        BODY          <- dedented by 4
        { BODY })                       }
}
```

Finding `BODY` means matching braces, which is where the earlier attempts broke: `{}` appears
inside `format!` strings. The scanner tracks double-quoted strings, escapes and line comments,
so braces inside them are ignored. It counts the rewrites it made and refuses to write a file
whose count does not match, rather than leaving half a rewrite behind.

## `add-awaits.py <package>`

Once a trait is async, every call site is the same error: `no method named `unwrap` found for
opaque type `impl Future<...>``. The compiler reports the exact line and column of the `.` that
should have been preceded by `.await`, so the fix can be applied from the diagnostics instead of
by matching names — which is what went wrong before, when `to_response` and `notify` existed as
both sync and async methods and a name-based rewrite called the wrong one.
