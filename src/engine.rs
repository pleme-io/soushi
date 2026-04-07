use std::path::Path;

use rhai::{Engine, Identifier, RhaiNativeFunc, Variant, AST};

use crate::SoushiError;

/// A Rhai scripting engine with convenience methods for registering
/// builtins, evaluating scripts, and loading script directories.
///
/// Wraps `rhai::Engine` with the `sync` feature enabled, so all
/// registered functions must be `Send + Sync`.
pub struct ScriptEngine {
    engine: Engine,
}

impl ScriptEngine {
    /// Create a new engine with default settings.
    ///
    /// Sets reasonable expression depth limits to prevent runaway scripts.
    #[must_use]
    pub fn new() -> Self {
        let mut engine = Engine::new();
        engine.set_max_expr_depths(64, 64);
        Self { engine }
    }

    /// Register logging builtins: `log_info(msg)`, `log_warn(msg)`, `log_error(msg)`.
    ///
    /// Each function accepts a string and emits a tracing event at the
    /// corresponding level.
    pub fn register_builtin_log(&mut self) {
        self.engine.register_fn("log_info", |msg: &str| {
            tracing::info!(script_log = msg);
        });
        self.engine.register_fn("log_warn", |msg: &str| {
            tracing::warn!(script_log = msg);
        });
        self.engine.register_fn("log_error", |msg: &str| {
            tracing::error!(script_log = msg);
        });
    }

    /// Register environment builtins: `env_var(name)` and `env_exists(name)`.
    ///
    /// `env_var` returns the variable's value, or an empty string if unset.
    /// `env_exists` returns `true` if the variable is set.
    pub fn register_builtin_env(&mut self) {
        self.engine
            .register_fn("env_var", |key: &str| -> String {
                std::env::var(key).unwrap_or_default()
            });
        self.engine
            .register_fn("env_exists", |key: &str| -> bool {
                std::env::var(key).is_ok()
            });
    }

    /// Register all built-in function families (log, env, and string).
    ///
    /// Equivalent to calling [`register_builtin_log`](Self::register_builtin_log),
    /// [`register_builtin_env`](Self::register_builtin_env), and
    /// [`register_builtin_string`](Self::register_builtin_string) individually.
    pub fn register_all_builtins(&mut self) {
        self.register_builtin_log();
        self.register_builtin_env();
        self.register_builtin_string();
    }

    /// Register string builtins: `str_contains`, `str_replace`, `str_upper`, `str_lower`.
    pub fn register_builtin_string(&mut self) {
        self.engine
            .register_fn("str_contains", |s: &str, sub: &str| -> bool {
                s.contains(sub)
            });
        self.engine.register_fn(
            "str_replace",
            |s: &str, from: &str, to: &str| -> String { s.replace(from, to) },
        );
        self.engine
            .register_fn("str_upper", |s: &str| -> String { s.to_uppercase() });
        self.engine
            .register_fn("str_lower", |s: &str| -> String { s.to_lowercase() });
    }

    /// Register a native function with the engine.
    ///
    /// Delegates to `rhai::Engine::register_fn`, accepting the same
    /// function signatures (closures taking Rhai-compatible arguments
    /// and returning a Rhai-compatible value).
    pub fn register_fn<
        A: 'static,
        const N: usize,
        const X: bool,
        R: Variant + Clone,
        const F: bool,
    >(
        &mut self,
        name: impl AsRef<str> + Into<Identifier>,
        func: impl RhaiNativeFunc<A, N, X, R, F> + Send + Sync + 'static,
    ) {
        self.engine.register_fn(name, func);
    }

    /// Evaluate a script string and return the result.
    pub fn eval(&self, script: &str) -> Result<rhai::Dynamic, SoushiError> {
        self.engine.eval(script).map_err(SoushiError::from)
    }

    /// Evaluate a script file at the given path and return the result.
    pub fn eval_file(&self, path: &Path) -> Result<rhai::Dynamic, SoushiError> {
        if !path.exists() {
            return Err(SoushiError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("script not found: {}", path.display()),
            )));
        }
        let script = std::fs::read_to_string(path)?;
        self.eval(&script)
    }

    /// Load all `.rhai` files in a directory, evaluating each one.
    ///
    /// Returns the list of loaded script names (file stems). Files are
    /// loaded in sorted order for deterministic behavior.
    pub fn load_scripts_dir(&mut self, dir: &Path) -> Result<Vec<String>, SoushiError> {
        if !dir.is_dir() {
            return Err(SoushiError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("script directory does not exist: {}", dir.display()),
            )));
        }

        let scripts = collect_rhai_paths(dir)?;

        let mut names = Vec::new();
        for script_path in &scripts {
            tracing::debug!(path = %script_path.display(), "loading script");
            let content = std::fs::read_to_string(script_path)?;
            let _result: rhai::Dynamic = self
                .engine
                .eval(&content)
                .map_err(|e| {
                    tracing::error!(path = %script_path.display(), error = %e, "script failed");
                    SoushiError::from(e)
                })?;
            let name = script_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            names.push(name);
        }

        Ok(names)
    }

    /// Compile a script string into a reusable AST.
    pub fn compile(&self, script: &str) -> Result<AST, SoushiError> {
        self.engine.compile(script).map_err(SoushiError::from)
    }

    /// Evaluate a pre-compiled AST and return the result.
    pub fn eval_ast(&self, ast: &AST) -> Result<rhai::Dynamic, SoushiError> {
        self.engine.eval_ast(ast).map_err(SoushiError::from)
    }

    /// Access the underlying `rhai::Engine` for advanced configuration.
    #[must_use]
    pub fn inner(&self) -> &Engine {
        &self.engine
    }

    /// Mutable access to the underlying `rhai::Engine`.
    pub fn inner_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

/// Collect all `.rhai` file paths from a directory, sorted for determinism.
fn collect_rhai_paths(dir: &Path) -> Result<Vec<std::path::PathBuf>, SoushiError> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("rhai") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    paths.sort();
    Ok(paths)
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    // --- Basic creation ---

    #[test]
    fn new_succeeds() {
        let _engine = ScriptEngine::new();
    }

    #[test]
    fn default_is_same_as_new() {
        let _engine = ScriptEngine::default();
    }

    // --- eval basics ---

    #[test]
    fn eval_basic_arithmetic() {
        let engine = ScriptEngine::new();
        let result = engine.eval("1 + 2").unwrap();
        assert_eq!(result.as_int().unwrap(), 3);
    }

    #[test]
    fn eval_variable_binding() {
        let engine = ScriptEngine::new();
        let result = engine.eval("let x = 10; x * 3").unwrap();
        assert_eq!(result.as_int().unwrap(), 30);
    }

    #[test]
    fn eval_syntax_error_returns_err() {
        let engine = ScriptEngine::new();
        let result = engine.eval("let = = ;");
        assert!(result.is_err());
    }

    #[test]
    fn eval_returns_int() {
        let engine = ScriptEngine::new();
        let result = engine.eval("42").unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn eval_returns_string() {
        let engine = ScriptEngine::new();
        let result = engine.eval(r#""hello world""#).unwrap();
        assert_eq!(result.into_string().unwrap(), "hello world");
    }

    #[test]
    fn eval_returns_bool() {
        let engine = ScriptEngine::new();
        let result = engine.eval("true").unwrap();
        assert!(result.as_bool().unwrap());
    }

    #[test]
    fn eval_returns_float() {
        let engine = ScriptEngine::new();
        let result = engine.eval("3.14").unwrap();
        let f = result.as_float().unwrap();
        assert!((f - 3.14).abs() < f64::EPSILON);
    }

    // --- Builtin string functions ---

    #[test]
    fn str_contains_true() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_contains("hello world", "world")"#)
            .unwrap();
        assert!(result.as_bool().unwrap());
    }

    #[test]
    fn str_contains_false() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_contains("hello world", "xyz")"#)
            .unwrap();
        assert!(!result.as_bool().unwrap());
    }

    #[test]
    fn str_replace_works() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_replace("hello world", "world", "rhai")"#)
            .unwrap();
        assert_eq!(result.into_string().unwrap(), "hello rhai");
    }

    #[test]
    fn str_upper_works() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine.eval(r#"str_upper("hello")"#).unwrap();
        assert_eq!(result.into_string().unwrap(), "HELLO");
    }

    #[test]
    fn str_lower_works() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine.eval(r#"str_lower("HELLO")"#).unwrap();
        assert_eq!(result.into_string().unwrap(), "hello");
    }

    // --- Builtin env functions ---

    #[test]
    fn env_var_reads_set_variable() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_env();

        unsafe { std::env::set_var("SOUSHI_TEST_ENV_VAR", "test_value_123") };
        let result = engine.eval(r#"env_var("SOUSHI_TEST_ENV_VAR")"#).unwrap();
        assert_eq!(result.into_string().unwrap(), "test_value_123");
        unsafe { std::env::remove_var("SOUSHI_TEST_ENV_VAR") };
    }

    #[test]
    fn env_var_returns_empty_for_missing() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_env();

        let result = engine
            .eval(r#"env_var("SOUSHI_NONEXISTENT_VAR_99999")"#)
            .unwrap();
        assert_eq!(result.into_string().unwrap(), "");
    }

    #[test]
    fn env_exists_true_when_set() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_env();

        unsafe { std::env::set_var("SOUSHI_TEST_EXISTS", "1") };
        let result = engine
            .eval(r#"env_exists("SOUSHI_TEST_EXISTS")"#)
            .unwrap();
        assert!(result.as_bool().unwrap());
        unsafe { std::env::remove_var("SOUSHI_TEST_EXISTS") };
    }

    #[test]
    fn env_exists_false_when_unset() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_env();

        let result = engine
            .eval(r#"env_exists("SOUSHI_NONEXISTENT_VAR_88888")"#)
            .unwrap();
        assert!(!result.as_bool().unwrap());
    }

    // --- Builtin log functions ---

    #[test]
    fn log_info_does_not_panic() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_log();
        let result = engine.eval(r#"log_info("info message")"#);
        assert!(result.is_ok());
    }

    #[test]
    fn log_warn_does_not_panic() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_log();
        let result = engine.eval(r#"log_warn("warn message")"#);
        assert!(result.is_ok());
    }

    #[test]
    fn log_error_does_not_panic() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_log();
        let result = engine.eval(r#"log_error("error message")"#);
        assert!(result.is_ok());
    }

    // --- eval_file ---

    #[test]
    fn eval_file_with_valid_script() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("test.rhai");
        let mut file = std::fs::File::create(&script_path).unwrap();
        writeln!(file, "let x = 10; let y = 20; x + y").unwrap();

        let engine = ScriptEngine::new();
        let result = engine.eval_file(&script_path).unwrap();
        assert_eq!(result.as_int().unwrap(), 30);
    }

    #[test]
    fn eval_file_not_found() {
        let engine = ScriptEngine::new();
        let result = engine.eval_file(Path::new("/nonexistent/path/script.rhai"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SoushiError::IoError(_)));
    }

    // --- load_scripts_dir ---

    #[test]
    fn load_scripts_dir_finds_rhai_files() {
        let dir = TempDir::new().unwrap();

        std::fs::write(dir.path().join("alpha.rhai"), "let x = 1;").unwrap();
        std::fs::write(dir.path().join("beta.rhai"), "let y = 2;").unwrap();
        std::fs::write(dir.path().join("gamma.txt"), "not a script").unwrap();

        let mut engine = ScriptEngine::new();
        let names = engine.load_scripts_dir(dir.path()).unwrap();

        assert_eq!(names.len(), 2);
        assert_eq!(names[0], "alpha");
        assert_eq!(names[1], "beta");
    }

    #[test]
    fn load_scripts_dir_empty_dir_returns_empty_vec() {
        let dir = TempDir::new().unwrap();

        let mut engine = ScriptEngine::new();
        let names = engine.load_scripts_dir(dir.path()).unwrap();

        assert!(names.is_empty());
    }

    #[test]
    fn load_scripts_dir_nonexistent_returns_err() {
        let mut engine = ScriptEngine::new();
        let result = engine.load_scripts_dir(Path::new("/nonexistent/script/dir"));
        assert!(result.is_err());
    }

    // --- compile + eval_ast ---

    #[test]
    fn compile_and_eval_ast_roundtrip() {
        let engine = ScriptEngine::new();
        let ast = engine.compile("let a = 5; let b = 7; a + b").unwrap();
        let result = engine.eval_ast(&ast).unwrap();
        assert_eq!(result.as_int().unwrap(), 12);
    }

    #[test]
    fn compile_syntax_error() {
        let engine = ScriptEngine::new();
        let result = engine.compile("let = = ;");
        assert!(result.is_err());
    }

    // --- register_fn ---

    #[test]
    fn register_fn_custom_function() {
        let mut engine = ScriptEngine::new();
        engine.register_fn("double", |x: i64| x * 2);

        let result = engine.eval("double(21)").unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    // --- inner / inner_mut ---

    #[test]
    fn inner_access() {
        let engine = ScriptEngine::new();
        let _inner: &rhai::Engine = engine.inner();
    }

    #[test]
    fn inner_mut_access() {
        let mut engine = ScriptEngine::new();
        let _inner: &mut rhai::Engine = engine.inner_mut();
    }

    // --- Error messages ---

    #[test]
    fn error_message_contains_useful_info() {
        let engine = ScriptEngine::new();
        let err = engine.eval("nonexistent_fn()").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("script error"),
            "error should contain 'script error': {msg}"
        );
    }

    #[test]
    fn script_error_from_parse_error() {
        let engine = ScriptEngine::new();
        let err = engine.compile("fn {}").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("script error"),
            "compile error should contain 'script error': {msg}"
        );
    }

    // --- eval edge cases ---

    #[test]
    fn eval_empty_string_returns_unit() {
        let engine = ScriptEngine::new();
        let result = engine.eval("").unwrap();
        assert!(result.is_unit(), "empty script should return unit/()");
    }

    #[test]
    fn eval_whitespace_only_returns_unit() {
        let engine = ScriptEngine::new();
        let result = engine.eval("   \n\t  ").unwrap();
        assert!(result.is_unit());
    }

    #[test]
    fn eval_semicolon_only_returns_unit() {
        let engine = ScriptEngine::new();
        let result = engine.eval(";").unwrap();
        assert!(result.is_unit());
    }

    #[test]
    fn eval_multiline_script() {
        let engine = ScriptEngine::new();
        let script = r"
            let a = 10;
            let b = 20;
            let c = a + b;
            c * 2
        ";
        let result = engine.eval(script).unwrap();
        assert_eq!(result.as_int().unwrap(), 60);
    }

    #[test]
    fn eval_string_concatenation() {
        let engine = ScriptEngine::new();
        let result = engine.eval(r#""hello" + " " + "world""#).unwrap();
        assert_eq!(result.into_string().unwrap(), "hello world");
    }

    #[test]
    fn eval_if_else_expression() {
        let engine = ScriptEngine::new();
        let result = engine.eval("if 10 > 5 { 1 } else { 0 }").unwrap();
        assert_eq!(result.as_int().unwrap(), 1);
    }

    #[test]
    fn eval_loop_with_break() {
        let engine = ScriptEngine::new();
        let script = r"
            let sum = 0;
            for i in 1..=10 {
                sum += i;
            }
            sum
        ";
        let result = engine.eval(script).unwrap();
        assert_eq!(result.as_int().unwrap(), 55);
    }

    #[test]
    fn eval_function_definition_and_call() {
        let engine = ScriptEngine::new();
        let script = r"
            fn add(a, b) { a + b }
            add(3, 4)
        ";
        let result = engine.eval(script).unwrap();
        assert_eq!(result.as_int().unwrap(), 7);
    }

    #[test]
    fn eval_division_by_zero_returns_err() {
        let engine = ScriptEngine::new();
        let result = engine.eval("1 / 0");
        assert!(result.is_err());
    }

    #[test]
    fn eval_undefined_variable_returns_err() {
        let engine = ScriptEngine::new();
        let result = engine.eval("undefined_variable");
        assert!(result.is_err());
    }

    #[test]
    fn eval_type_mismatch_returns_err() {
        let engine = ScriptEngine::new();
        let result = engine.eval(r#""hello" - 5"#);
        assert!(result.is_err());
    }

    #[test]
    fn eval_nested_arithmetic() {
        let engine = ScriptEngine::new();
        let result = engine.eval("((2 + 3) * (4 - 1)) / 5").unwrap();
        assert_eq!(result.as_int().unwrap(), 3);
    }

    #[test]
    fn eval_array_literal() {
        let engine = ScriptEngine::new();
        let result = engine.eval("[1, 2, 3].len()").unwrap();
        assert_eq!(result.as_int().unwrap(), 3);
    }

    #[test]
    fn eval_map_literal() {
        let engine = ScriptEngine::new();
        let result = engine.eval(r#"let m = #{x: 42}; m.x"#).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    // --- String builtin edge cases ---

    #[test]
    fn str_contains_empty_needle_always_true() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_contains("anything", "")"#)
            .unwrap();
        assert!(result.as_bool().unwrap());
    }

    #[test]
    fn str_contains_empty_haystack() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_contains("", "something")"#)
            .unwrap();
        assert!(!result.as_bool().unwrap());
    }

    #[test]
    fn str_replace_no_match_returns_original() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_replace("hello world", "xyz", "abc")"#)
            .unwrap();
        assert_eq!(result.into_string().unwrap(), "hello world");
    }

    #[test]
    fn str_replace_multiple_occurrences() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_replace("aaa", "a", "bb")"#)
            .unwrap();
        assert_eq!(result.into_string().unwrap(), "bbbbbb");
    }

    #[test]
    fn str_upper_empty_string() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine.eval(r#"str_upper("")"#).unwrap();
        assert_eq!(result.into_string().unwrap(), "");
    }

    #[test]
    fn str_lower_empty_string() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine.eval(r#"str_lower("")"#).unwrap();
        assert_eq!(result.into_string().unwrap(), "");
    }

    #[test]
    fn str_upper_unicode() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine.eval(r#"str_upper("cafe\u0301")"#).unwrap();
        let s = result.into_string().unwrap();
        assert_eq!(s, "CAFE\u{0301}");
    }

    #[test]
    fn str_contains_unicode() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine
            .eval(r#"str_contains("日本語テスト", "本語")"#)
            .unwrap();
        assert!(result.as_bool().unwrap());
    }

    // --- Builtin registration combinations ---

    #[test]
    fn register_all_builtins_together() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_log();
        engine.register_builtin_env();
        engine.register_builtin_string();

        // All three families work together
        let _ = engine.eval(r#"log_info("test")"#).unwrap();
        let result = engine
            .eval(r#"str_upper(env_var("SOUSHI_NONEXISTENT_987654"))"#)
            .unwrap();
        assert_eq!(result.into_string().unwrap(), "");
    }

    #[test]
    fn register_builtins_idempotent() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        engine.register_builtin_string(); // double register

        let result = engine.eval(r#"str_upper("ok")"#).unwrap();
        assert_eq!(result.into_string().unwrap(), "OK");
    }

    // --- eval_file edge cases ---

    #[test]
    fn eval_file_empty_script() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("empty.rhai");
        std::fs::write(&script_path, "").unwrap();

        let engine = ScriptEngine::new();
        let result = engine.eval_file(&script_path).unwrap();
        assert!(result.is_unit());
    }

    #[test]
    fn eval_file_with_syntax_error() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("bad.rhai");
        std::fs::write(&script_path, "let = = ;").unwrap();

        let engine = ScriptEngine::new();
        let result = engine.eval_file(&script_path);
        assert!(result.is_err());
    }

    #[test]
    fn eval_file_returns_last_expression() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("multi.rhai");
        std::fs::write(&script_path, "let a = 1;\nlet b = 2;\na + b").unwrap();

        let engine = ScriptEngine::new();
        let result = engine.eval_file(&script_path).unwrap();
        assert_eq!(result.as_int().unwrap(), 3);
    }

    // --- load_scripts_dir edge cases ---

    #[test]
    fn load_scripts_dir_ignores_subdirectories() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rhai"), "let x = 1;").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        std::fs::write(
            dir.path().join("subdir").join("nested.rhai"),
            "let y = 2;",
        )
        .unwrap();

        let mut engine = ScriptEngine::new();
        let names = engine.load_scripts_dir(dir.path()).unwrap();
        assert_eq!(names, vec!["main"]);
    }

    #[test]
    fn load_scripts_dir_sorted_order() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("zebra.rhai"), "let z = 1;").unwrap();
        std::fs::write(dir.path().join("alpha.rhai"), "let a = 1;").unwrap();
        std::fs::write(dir.path().join("middle.rhai"), "let m = 1;").unwrap();

        let mut engine = ScriptEngine::new();
        let names = engine.load_scripts_dir(dir.path()).unwrap();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn load_scripts_dir_stops_on_error() {
        let dir = TempDir::new().unwrap();
        // "aaa" sorts before "bbb", so the error script runs first
        std::fs::write(dir.path().join("aaa_bad.rhai"), "let = = ;").unwrap();
        std::fs::write(dir.path().join("bbb_good.rhai"), "let x = 1;").unwrap();

        let mut engine = ScriptEngine::new();
        let result = engine.load_scripts_dir(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn load_scripts_dir_ignores_non_rhai_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("script.rhai"), "let x = 1;").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a script").unwrap();
        std::fs::write(dir.path().join("data.json"), "{}").unwrap();
        std::fs::write(dir.path().join("readme.md"), "# hi").unwrap();
        std::fs::write(dir.path().join("no_ext"), "random").unwrap();

        let mut engine = ScriptEngine::new();
        let names = engine.load_scripts_dir(dir.path()).unwrap();
        assert_eq!(names, vec!["script"]);
    }

    // --- compile + eval_ast edge cases ---

    #[test]
    fn ast_can_be_evaluated_multiple_times() {
        let engine = ScriptEngine::new();
        let ast = engine.compile("40 + 2").unwrap();

        let r1 = engine.eval_ast(&ast).unwrap();
        let r2 = engine.eval_ast(&ast).unwrap();
        assert_eq!(r1.as_int().unwrap(), 42);
        assert_eq!(r2.as_int().unwrap(), 42);
    }

    #[test]
    fn compile_empty_script() {
        let engine = ScriptEngine::new();
        let ast = engine.compile("").unwrap();
        let result = engine.eval_ast(&ast).unwrap();
        assert!(result.is_unit());
    }

    #[test]
    fn compile_with_builtins() {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();

        let ast = engine.compile(r#"str_upper("test")"#).unwrap();
        let result = engine.eval_ast(&ast).unwrap();
        assert_eq!(result.into_string().unwrap(), "TEST");
    }

    // --- register_fn edge cases ---

    #[test]
    fn register_fn_multiple_custom_functions() {
        let mut engine = ScriptEngine::new();
        engine.register_fn("add", |a: i64, b: i64| a + b);
        engine.register_fn("mul", |a: i64, b: i64| a * b);

        let result = engine.eval("add(3, mul(4, 5))").unwrap();
        assert_eq!(result.as_int().unwrap(), 23);
    }

    #[test]
    fn register_fn_returning_string() {
        let mut engine = ScriptEngine::new();
        engine.register_fn("greet", |name: &str| -> String {
            format!("Hello, {name}!")
        });

        let result = engine.eval(r#"greet("world")"#).unwrap();
        assert_eq!(result.into_string().unwrap(), "Hello, world!");
    }

    #[test]
    fn register_fn_returning_bool() {
        let mut engine = ScriptEngine::new();
        engine.register_fn("is_positive", |x: i64| -> bool { x > 0 });

        let t = engine.eval("is_positive(1)").unwrap();
        let f = engine.eval("is_positive(-1)").unwrap();
        assert!(t.as_bool().unwrap());
        assert!(!f.as_bool().unwrap());
    }

    // --- SoushiError coverage ---

    #[test]
    fn no_such_script_error_display() {
        let err = SoushiError::NoSuchScript("missing.rhai".to_string());
        assert_eq!(err.to_string(), "no such script: missing.rhai");
    }

    #[test]
    fn io_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = SoushiError::IoError(io_err);
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn script_error_display() {
        let err = SoushiError::ScriptError("something went wrong".to_string());
        assert_eq!(err.to_string(), "script error: something went wrong");
    }

    #[test]
    fn error_from_eval_alt_result() {
        let engine = ScriptEngine::new();
        let err = engine.eval("throw \"boom\"").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("script error"), "got: {msg}");
        assert!(msg.contains("boom"), "got: {msg}");
    }

    #[test]
    fn error_from_parse_error() {
        let engine = ScriptEngine::new();
        let err = engine.compile("fn (").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("script error"), "got: {msg}");
    }

    #[test]
    fn error_debug_impl() {
        let err = SoushiError::NoSuchScript("test".to_string());
        let debug = format!("{err:?}");
        assert!(debug.contains("NoSuchScript"), "got: {debug}");
    }

    // --- Expression depth limits ---

    #[test]
    fn deeply_nested_expression_rejected() {
        let engine = ScriptEngine::new();
        // Build a deeply nested expression exceeding the depth limit of 64
        let mut script = String::from("1");
        for _ in 0..100 {
            script = format!("({script} + 1)");
        }
        let result = engine.eval(&script);
        assert!(result.is_err(), "deeply nested expression should be rejected");
    }

    // --- inner_mut mutation ---

    #[test]
    fn inner_mut_can_configure_engine() {
        let mut engine = ScriptEngine::new();
        engine.inner_mut().set_max_expr_depths(10, 10);

        // A moderately nested expression should now fail with tighter limits
        let mut script = String::from("1");
        for _ in 0..20 {
            script = format!("({script} + 1)");
        }
        let result = engine.eval(&script);
        assert!(result.is_err(), "should fail with reduced depth limit");
    }

    // --- Rhai re-export from lib.rs ---

    #[test]
    fn rhai_dynamic_from_int() {
        let val = rhai::Dynamic::from(42_i64);
        assert_eq!(val.as_int().unwrap(), 42);
    }

    // --- register_all_builtins ---

    #[test]
    fn register_all_builtins_registers_every_family() {
        let mut engine = ScriptEngine::new();
        engine.register_all_builtins();

        engine.eval(r#"log_info("ok")"#).unwrap();
        let upper = engine.eval(r#"str_upper("abc")"#).unwrap();
        assert_eq!(upper.into_string().unwrap(), "ABC");
        let env = engine
            .eval(r#"env_var("SOUSHI_NOPE_999")"#)
            .unwrap();
        assert_eq!(env.into_string().unwrap(), "");
    }

    // --- collect_rhai_paths ---

    #[test]
    fn collect_rhai_paths_returns_sorted_paths() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("z.rhai"), "1;").unwrap();
        std::fs::write(dir.path().join("a.rhai"), "1;").unwrap();
        std::fs::write(dir.path().join("m.rhai"), "1;").unwrap();
        std::fs::write(dir.path().join("readme.md"), "hi").unwrap();

        let paths = collect_rhai_paths(dir.path()).unwrap();
        assert_eq!(paths.len(), 3);
        let stems: Vec<&str> = paths
            .iter()
            .map(|p| p.file_stem().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(stems, vec!["a", "m", "z"]);
    }

    #[test]
    fn collect_rhai_paths_empty_dir() {
        let dir = TempDir::new().unwrap();
        let paths = collect_rhai_paths(dir.path()).unwrap();
        assert!(paths.is_empty());
    }

    // --- load_scripts_dir with script-level errors ---

    #[test]
    fn load_scripts_dir_returns_script_error_variant() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("bad.rhai"), "let = ;").unwrap();

        let mut engine = ScriptEngine::new();
        let err = engine.load_scripts_dir(dir.path()).unwrap_err();
        assert!(
            matches!(err, SoushiError::ScriptError(_)),
            "expected ScriptError, got: {err:?}"
        );
    }

    // --- eval_file with builtins ---

    #[test]
    fn eval_file_uses_registered_builtins() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("use_builtin.rhai");
        std::fs::write(&path, r#"str_upper("test")"#).unwrap();

        let mut engine = ScriptEngine::new();
        engine.register_builtin_string();
        let result = engine.eval_file(&path).unwrap();
        assert_eq!(result.into_string().unwrap(), "TEST");
    }

    // --- Default trait ---

    #[test]
    fn default_engine_can_eval() {
        let engine = ScriptEngine::default();
        let result = engine.eval("2 + 2").unwrap();
        assert_eq!(result.as_int().unwrap(), 4);
    }

    // --- Error conversion coverage ---

    #[test]
    fn io_error_from_trait() {
        let io = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
        let err = SoushiError::from(io);
        assert!(matches!(err, SoushiError::IoError(_)));
        assert!(err.to_string().contains("pipe broken"));
    }

    #[test]
    fn eval_runtime_error_produces_script_error() {
        let engine = ScriptEngine::new();
        let err = engine.eval("throw \"runtime boom\"").unwrap_err();
        assert!(matches!(err, SoushiError::ScriptError(_)));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn compile_error_produces_script_error() {
        let engine = ScriptEngine::new();
        let err = engine.compile("fn {").unwrap_err();
        assert!(matches!(err, SoushiError::ScriptError(_)));
    }
}
