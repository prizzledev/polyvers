//! Single-macro schema versioning. See the crate README for the canonical
//! example and rationale.

pub use polyvers_macros::versioned;

/// Errors returned by the generated `parse_at_version` function.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The requested version string did not match any declared version.
    #[error("unknown version `{requested}`; known versions: {known:?}")]
    UnknownVersion {
        requested: String,
        known: &'static [&'static str],
    },
    /// The underlying serde format failed to parse the input.
    #[error("format error: {0}")]
    Format(Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// Construct an `UnknownVersion` error. Used by generated code.
    pub fn unknown_version(requested: &str, known: &'static [&'static str]) -> Self {
        Self::UnknownVersion {
            requested: requested.to_owned(),
            known,
        }
    }

    /// Wrap any `std::error::Error` as a `Format` variant. Used by generated code.
    pub fn format<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Format(Box::new(err))
    }
}
