//! What can be refused, and by whom.
//!
//! Three families, because they are repaired in three places: what **we** refuse
//! before sending anything, what the **server** refuses, and what breaks **on
//! the way**. Confusing them would send someone to reread their query when the
//! network is what dropped.

/// The result of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Why it did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// What the caller asked for makes no sense here: a collection that does
    /// not exist, a filter the collection does not declare, a required filter
    /// missing. **No request was sent.**
    Refused(String),
    /// The server answered, and its answer is a refusal.
    ///
    /// The text is **its own**, repeated as is: it is the one who knows why, and
    /// rewording it would lose the only sentence somebody can act on. Measured
    /// on real services: a WFS answers its refusals as an `ows:ExceptionReport`,
    /// in XML, with a 400 — even when JSON is asked for.
    Server {
        /// The HTTP code.
        status: u16,
        /// The exception's code, when the document carries one.
        code: Option<String>,
        /// What the server says.
        says: String,
    },
    /// The round trip did not complete: no network, a name that does not
    /// resolve, a cut in the middle of a read.
    Transport(String),
    /// The answer arrived and cannot be read.
    Unreadable(String),
    /// A ceiling set by the caller was reached. **This is not a failure**: it is
    /// the protection that did its job, and it names itself so that it can be
    /// lifted.
    TooMuch(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Refused(why) => write!(f, "{why}"),
            Error::Server { status, code, says } => match code {
                Some(code) => write!(f, "the server answered {status} ({code}): {says}"),
                None => write!(f, "the server answered {status}: {says}"),
            },
            Error::Transport(why) => write!(f, "{why}"),
            Error::Unreadable(why) => write!(f, "{why}"),
            Error::TooMuch(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Error {}
