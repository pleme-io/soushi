//! Soushi (草紙) --- Rhai scripting engine.
//!
//! Extracts the common Rhai engine setup from karakuri and hikyaku.
//! Both register builtins, load script directories, and share the
//! same boilerplate --- this library provides a single `ScriptEngine`
//! that handles all of it.
//!
//! # Quick Start
//!
//! ```rust
//! use soushi::ScriptEngine;
//!
//! let mut engine = ScriptEngine::new();
//! engine.register_builtin_log();
//! engine.register_builtin_env();
//!
//! let result = engine.eval("1 + 2").unwrap();
//! assert_eq!(result.as_int().unwrap(), 3);
//! ```

mod engine;
mod error;

pub use engine::{ScriptEngine, ScriptEngineBuilder};
pub use error::SoushiError;

pub use rhai;
