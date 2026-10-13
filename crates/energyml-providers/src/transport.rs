//! The round trip, and nothing else.
//!
//! The impure half, held behind a trait for the same reason as the ETP client:
//! **wasm has no blocking call**, and the front end does its own `fetch`. What is
//! here serves the CLI, the Python bridge and the tests.
//!
//! # The body arrives as a stream, not all at once
//!
//! Measured on the boreholes WFS: `Transfer-Encoding: chunked`, no
//! `Content-Length`, first byte at **2.56 s** and last at **11.67 s** for
//! 50.8 MB. A reader that waits for the end does nothing during nine tenths of
//! the answer — hence [`Answer::body`], which is a `Read` and not a `Vec`.

use std::io::Read;

use crate::error::{Error, Result};

/// A round trip to make — and nothing that makes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The complete address, parameters included.
    pub url: String,
    /// What is accepted in return.
    pub accept: String,
    /// What this request is for, for a readable trace.
    pub about: String,
}

/// What came back.
///
/// The body is a **reader**, never a string: deciding the format before having
/// read condemns one to say *"this is not JSON"* of a refusal the server
/// explained perfectly well in XML.
pub struct Answer {
    /// The HTTP code.
    pub status: u16,
    /// What the server says it sent.
    pub content_type: Option<String>,
    /// The body, to be read as it comes.
    pub body: Box<dyn Read>,
}

impl Answer {
    /// The whole body, when it is small **and one knows it is**.
    ///
    /// For a quote or a refusal — a few kilobytes. Never for a harvest: it is
    /// exactly the read this crate exists to avoid, hence the ceiling, which is
    /// there for the typo and not for the data.
    pub fn text(mut self, most: usize) -> Result<String> {
        let mut out = Vec::new();
        let read = self
            .body
            .by_ref()
            .take(most as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|why| Error::Transport(format!("reading the answer: {why}")))?;
        if read > most {
            return Err(Error::TooMuch(format!(
                "the answer is longer than {most} bytes, which is more than this call expects"
            )));
        }
        String::from_utf8(out).map_err(|why| Error::Unreadable(format!("the answer is not text: {why}")))
    }
}

/// Whoever makes the round trip.
pub trait Transport {
    /// A GET, and the body as a stream.
    fn fetch(&self, request: &Request) -> Result<Answer>;
}

/// The native transport: `ureq`, blocking, the body streamed.
#[cfg(feature = "native")]
pub mod native {
    use super::{Answer, Request, Transport};
    use crate::error::{Error, Result};

    /// How many bytes an answer is allowed to weigh.
    ///
    /// **A protection against a wrong address, not a judgement on the data.**
    /// Without it, a mistyped address that serves a video is a machine filling
    /// its memory instead of a refusal. It is named in the refusal, because a
    /// ceiling nobody sees is a ceiling nobody can lift.
    pub const MOST: u64 = 2 * 1024 * 1024 * 1024;

    /// A GET through `ureq`.
    pub struct Native {
        most: u64,
    }

    impl Default for Native {
        fn default() -> Self {
            Native { most: MOST }
        }
    }

    impl Native {
        /// The transport, with the default ceiling.
        pub fn new() -> Self {
            Self::default()
        }

        /// The same, with another ceiling in bytes.
        pub fn with_limit(most: u64) -> Self {
            Native { most }
        }
    }

    impl Transport for Native {
        fn fetch(&self, request: &Request) -> Result<Answer> {
            // `ureq` refuses a 4xx/5xx before a byte of the body is read, and
            // that is exactly what we do not want here: the body of a 400
            // **is** the explanation. Hence `http_status_as_error(false)`.
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into();
            let response = agent
                .get(&request.url)
                .header("Accept", &request.accept)
                .call()
                .map_err(|why| Error::Transport(format!("fetching {}: {why}", request.url)))?;

            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            // **The body is not read here.** `into_reader` gives a `'static`
            // reader — detached from the response, hence returnable — and the
            // ceiling lives *on that reader*: it stops the read instead of
            // being discovered afterwards, which is what the CLI's `fetch` used
            // to do, downloading 512 MB before saying it was too much.
            let (_, body) = response.into_parts();
            let reader = body.into_with_config().limit(self.most).reader();
            Ok(Answer {
                status,
                content_type,
                body: Box::new(reader),
            })
        }
    }
}

#[cfg(feature = "native")]
pub use native::Native;
