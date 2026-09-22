# loxury

Two Lox interpreters in Rust, following [craftinginterpreters](https://craftinginterpreters.com/):
`tree-walk` (AST interpreter) and `vm` (bytecode compiler + VM).

- Almost no dependencies (`fxhash` for performance).
- Very slow (VM version is 2–4x slower than clox).
- Poorly written (especially the tree-walk).
- VM version passes the craftinginterpreters tests (modulo error-message wording).

## CLI

```
cargo run -p vm -- program.lox              # run a file
cargo run -p vm                             # no file: interactive prompt
LOX_DUMP=1 cargo run -p vm -- program.lox   # also dump bytecode
cargo run -p tree-walk -- program.lox       # harm yourself
```

## Playground

Test the bytecode interpreter in the browser via WebAssembly [here](https://semiflux.github.io/loxury/).
