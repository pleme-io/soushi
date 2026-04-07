//! Integration tests for the script loader against fixture scripts.

use std::path::Path;

use soushi::ScriptEngine;

/// Path to the checked-in fixture scripts.
fn fixtures_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

#[test]
fn load_fixtures_discovers_only_rhai_files() {
    let mut engine = ScriptEngine::new();
    engine.register_all_builtins();

    let names = engine.load_scripts_dir(fixtures_dir()).unwrap();

    assert_eq!(names.len(), 3, "expected 3 .rhai fixtures, got: {names:?}");
    assert!(!names.contains(&"not_a_script".to_string()));
}

#[test]
fn load_fixtures_returns_sorted_stems() {
    let mut engine = ScriptEngine::new();
    engine.register_all_builtins();

    let names = engine.load_scripts_dir(fixtures_dir()).unwrap();

    assert_eq!(names, vec!["01_math", "02_strings", "03_functions"]);
}

#[test]
fn eval_math_fixture() {
    let engine = ScriptEngine::new();
    let result = engine
        .eval_file(fixtures_dir().join("01_math.rhai"))
        .unwrap();
    assert_eq!(result.as_int().unwrap(), 30);
}

#[test]
fn eval_string_fixture_requires_builtins() {
    let engine = ScriptEngine::new();
    let result = engine.eval_file(fixtures_dir().join("02_strings.rhai"));
    assert!(result.is_err(), "should fail without string builtins");
}

#[test]
fn eval_string_fixture_with_builtins() {
    let mut engine = ScriptEngine::new();
    engine.register_builtin_string();

    let result = engine
        .eval_file(fixtures_dir().join("02_strings.rhai"))
        .unwrap();
    assert_eq!(result.into_string().unwrap(), "HI");
}

#[test]
fn eval_function_fixture() {
    let engine = ScriptEngine::new();
    let result = engine
        .eval_file(fixtures_dir().join("03_functions.rhai"))
        .unwrap();
    assert_eq!(result.as_int().unwrap(), 23);
}

#[test]
fn register_all_builtins_enables_all_families() {
    let mut engine = ScriptEngine::new();
    engine.register_all_builtins();

    let _ = engine.eval(r#"log_info("test")"#).unwrap();

    let upper = engine.eval(r#"str_upper("hello")"#).unwrap();
    assert_eq!(upper.into_string().unwrap(), "HELLO");

    let exists = engine
        .eval(r#"env_exists("SOUSHI_UNLIKELY_VAR_ABCDEF")"#)
        .unwrap();
    assert!(!exists.as_bool().unwrap());
}

#[test]
fn load_scripts_dir_with_builtins_succeeds() {
    let mut engine = ScriptEngine::new();
    engine.register_all_builtins();

    let names = engine.load_scripts_dir(fixtures_dir()).unwrap();
    assert_eq!(names.len(), 3);
}

#[test]
fn compile_and_reuse_fixture_script() {
    let engine = ScriptEngine::new();
    let source = std::fs::read_to_string(fixtures_dir().join("01_math.rhai")).unwrap();
    let ast = engine.compile(&source).unwrap();

    let r1 = engine.eval_ast(&ast).unwrap();
    let r2 = engine.eval_ast(&ast).unwrap();
    assert_eq!(r1.as_int().unwrap(), 30);
    assert_eq!(r2.as_int().unwrap(), 30);
}

#[test]
fn eval_file_nonexistent_produces_io_error() {
    let engine = ScriptEngine::new();
    let err = engine
        .eval_file(Path::new("/tmp/soushi_no_such_file.rhai"))
        .unwrap_err();
    assert!(
        matches!(err, soushi::SoushiError::ScriptFileNotFound(_)),
        "expected ScriptFileNotFound, got: {err:?}"
    );
}

#[test]
fn load_scripts_dir_nonexistent_produces_io_error() {
    let mut engine = ScriptEngine::new();
    let err = engine
        .load_scripts_dir(Path::new("/tmp/soushi_no_such_dir"))
        .unwrap_err();
    assert!(
        matches!(err, soushi::SoushiError::ScriptDirNotFound(_)),
        "expected ScriptDirNotFound, got: {err:?}"
    );
}
