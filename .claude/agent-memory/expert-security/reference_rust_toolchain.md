---
name: reference-rust-toolchain
description: cargo/rustc are only reachable via ~/.asdf/shims on this machine — not on the default PATH
metadata:
  type: reference
---

`cargo`, `rustc`, `clippy`, `rustfmt` are installed through **asdf**, and the shims
directory is NOT on the default PATH used by tool invocations.

Prefix build commands with:

```
export PATH="$HOME/.asdf/shims:$PATH"; cargo test -p <crate>
```

Symptoms if you forget: `command not found: cargo`, and `~/.cargo/bin` does not exist
(only `~/.cargo/registry` + `git` caches are present, so it looks like Rust is missing
entirely). It is not — check `~/.asdf/shims` before concluding you cannot compile.
