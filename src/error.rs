/// Errors produced by the Soushi scripting engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SoushiError {
    /// A Rhai script failed at runtime (e.g. undefined variable, type
    /// mismatch, explicit `throw`).
    #[error("script error: {0}")]
    ScriptError(String),

    /// A Rhai script failed to compile (syntax / parse error).
    #[error("compile error: {0}")]
    CompileError(String),

    /// An I/O error (e.g. reading a script file).
    #[error("io: {0}")]
    IoError(#[from] std::io::Error),

    /// The requested script was not found by name.
    #[error("no such script: {0}")]
    NoSuchScript(String),

    /// A script file could not be found at the given path.
    #[error("script file not found: {0}")]
    ScriptFileNotFound(std::path::PathBuf),

    /// A script directory could not be found at the given path.
    #[error("script dir not found: {0}")]
    ScriptDirNotFound(std::path::PathBuf),
}

impl SoushiError {
    /// Returns `true` if this is a runtime script error.
    #[must_use]
    pub fn is_script_error(&self) -> bool {
        matches!(self, Self::ScriptError(_))
    }

    /// Returns `true` if this is a compile/parse error.
    #[must_use]
    pub fn is_compile_error(&self) -> bool {
        matches!(self, Self::CompileError(_))
    }

    /// Returns `true` if this is an I/O error.
    #[must_use]
    pub fn is_io_error(&self) -> bool {
        matches!(self, Self::IoError(_))
    }

    /// Returns `true` if this is a not-found error (file, dir, or name).
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::NoSuchScript(_) | Self::ScriptFileNotFound(_) | Self::ScriptDirNotFound(_)
        )
    }
}

impl From<Box<rhai::EvalAltResult>> for SoushiError {
    fn from(err: Box<rhai::EvalAltResult>) -> Self {
        Self::ScriptError(err.to_string())
    }
}

impl From<rhai::ParseError> for SoushiError {
    fn from(err: rhai::ParseError) -> Self {
        Self::CompileError(err.to_string())
    }
}
