/// Errors produced by the Soushi scripting engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SoushiError {
    /// A Rhai script evaluation or parse error.
    #[error("script error: {0}")]
    ScriptError(String),

    /// An I/O error (e.g. reading a script file).
    #[error("io: {0}")]
    IoError(#[from] std::io::Error),

    /// The requested script was not found by name.
    #[error("no such script: {0}")]
    NoSuchScript(String),
}

impl From<Box<rhai::EvalAltResult>> for SoushiError {
    fn from(err: Box<rhai::EvalAltResult>) -> Self {
        Self::ScriptError(err.to_string())
    }
}

impl From<rhai::ParseError> for SoushiError {
    fn from(err: rhai::ParseError) -> Self {
        Self::ScriptError(err.to_string())
    }
}
