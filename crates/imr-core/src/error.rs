//! The engine's error: a message meant for people (the same wording the Python version used).

/// What went wrong, in words to show the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A problem with the settings, a value or the input data.
    #[error("{0}")]
    Value(String),
    /// A file that doesn't exist.
    #[error("{0}")]
    NotFound(String),
    /// A column (or rule column) the data doesn't have.
    #[error("{0}")]
    Key(String),
}

impl Error {
    pub fn msg(text: impl Into<String>) -> Error {
        Error::Value(text.into())
    }

    pub fn key(text: impl Into<String>) -> Error {
        Error::Key(text.into())
    }

    /// The same error with its message changed (the kind is kept).
    pub fn map_msg(self, f: impl FnOnce(String) -> String) -> Error {
        match self {
            Error::Value(m) => Error::Value(f(m)),
            Error::NotFound(m) => Error::NotFound(f(m)),
            Error::Key(m) => Error::Key(f(m)),
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
