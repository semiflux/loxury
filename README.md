# loxury

Two complete Lox interpreters in Rust, following [craftinginterpreters](https://craftinginterpreters.com/):
`tree-walk` (AST interpreter) and `vm` (bytecode compiler + VM).

- Almost no dependencies (`fxhash` for performance).
- Only mildly slow (VM version is ~1.3x clox's runtime).
- Poorly written (especially the tree-walk).
- Original Crafting Interpreters tests re-encoded and verified (modulo error-message wording).

## CLI

```sh
cargo run -p vm -- program.lox              # run a file
cargo run -p vm                             # no file: interactive prompt
LOX_DUMP=1 cargo run -p vm -- program.lox   # also dump bytecode
cargo run -p tree-walk -- program.lox       # harm yourself
```

## Tests
```sh
cargo test -p vm                            # lexer, compiler, VM
cargo test -p vm --lib compiler::tests      # compilation
cargo test -p vm --lib vm::tests            # execution
cargo test -p vm --no-default-features      # tagged values instead of NaN boxing
```

## Playground

Test the bytecode interpreter in the browser via WebAssembly [here](https://semiflux.github.io/loxury/).
