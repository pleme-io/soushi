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

        let mut scripts: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
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

        scripts.sort();

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
                .unwrap_or("")
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
}
