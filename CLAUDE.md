# Soushi (草紙) -- Rhai Scripting Engine

## Build & Test

```bash
cargo build          # compile
cargo test           # 33 unit tests + 1 doc-test
```

## Architecture

Extracts the common Rhai engine setup from ayatsuri and hikyaku. Both register builtins, load script directories, and share the same boilerplate -- this library provides a single `ScriptEngine` that handles all of it.

### Module Map

| Path | Purpose |
|------|---------|
| `src/lib.rs` | Re-exports + rhai re-export |
| `src/engine.rs` | `ScriptEngine` -- engine wrapper with builtins (33 tests) |
| `src/error.rs` | `SoushiError` -- script/IO/not-found errors |

### Key Types

- **`ScriptEngine`** -- wraps `rhai::Engine` with builtins and script loading
- **`SoushiError`** -- `ScriptError(String)`, `IoError`, `NoSuchScript(String)`

### Builtins

- `register_builtin_log()` -- `log_info(msg)`, `log_warn(msg)`, `log_error(msg)` via tracing
- `register_builtin_env()` -- `env_var(key)` and `env_exists(key)`
- `register_builtin_string()` -- `str_contains`, `str_replace`, `str_upper`, `str_lower`

### Usage Pattern

```rust
use soushi::ScriptEngine;

let mut engine = ScriptEngine::new();
engine.register_builtin_log();
engine.register_builtin_env();

let result = engine.eval("1 + 2").unwrap();
assert_eq!(result.as_int().unwrap(), 3);

// Load all .rhai files in a directory
let names = engine.load_scripts_dir(Path::new("/path/to/scripts")).unwrap();

// Compile + eval AST for reuse
let ast = engine.compile("let x = 42; x").unwrap();
let val = engine.eval_ast(&ast).unwrap();
```

## Consumers

- **ayatsuri** -- window manager automation scripts
- **hikyaku** -- email automation scripts
