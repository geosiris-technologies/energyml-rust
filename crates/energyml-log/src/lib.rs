//! Where the library's `tracing` output goes — installed once, by a face.
//!
//! **The libraries emit, they never configure.** `energyml-build`,
//! `energyml-storage` and the rest open spans and send events; none of them
//! installs a subscriber. The command line, the desktop application or an
//! embedding program calls [`init`] (or [`init_reloadable`]) once, and decides
//! there whether anything is shown, how, and at which level.
//!
//! - **Off by default.** With no filter given and no `ENERGYML_LOG` in the
//!   environment, [`init`] installs nothing: no subscriber, no cost, and the
//!   output of every command is what it was.
//! - **The filter** is `tracing_subscriber::EnvFilter` syntax: `info`,
//!   `debug`, `energyml_build=debug,energyml_storage=info`, `off`.
//! - **Environment**: `ENERGYML_LOG` (the filter) and `ENERGYML_LOG_FORMAT`
//!   (`text` or `json`). An explicit argument wins over the variable.
//! - **Always stderr**, never stdout: stdout carries results a script reads.
//! - **A span says how long it took**: closing a span writes a line with
//!   `time.busy` and `time.idle`.
//! - **The level can change while the program runs.** [`LogHandle::set_filter`]
//!   swaps the filter live — from off to `debug` and back — which is what a
//!   desktop application's "log level" menu calls. It works even when the
//!   program started with logs off, provided it installed through
//!   [`init_reloadable`].
//! - **`log` records are bridged.** The ETP crates still use the `log` crate;
//!   once a subscriber is installed their records go through the same filter.
//!
//! Pure Rust, no native dependency.

use std::fmt;
use std::io::IsTerminal;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use tracing::Dispatch;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// The environment variable holding the filter.
pub const ENV_FILTER: &str = "ENERGYML_LOG";
/// The environment variable holding the format, `text` or `json`.
pub const ENV_FORMAT: &str = "ENERGYML_LOG_FORMAT";

/// How a line is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Human-readable lines.
    #[default]
    Text,
    /// One JSON object per line, for a tool to read.
    Json,
}

impl FromStr for Format {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        match s.trim().to_ascii_lowercase().as_str() {
            "text" | "" => Ok(Format::Text),
            "json" => Ok(Format::Json),
            other => Err(Error::Format(other.to_string())),
        }
    }
}

impl Format {
    /// The format named by `ENERGYML_LOG_FORMAT`, `Text` when it is unset.
    pub fn from_env() -> Result<Format, Error> {
        match std::env::var(ENV_FORMAT) {
            Ok(v) => v.parse(),
            Err(_) => Ok(Format::Text),
        }
    }
}

/// What went wrong installing or changing the logs.
#[derive(Debug)]
pub enum Error {
    /// The filter is not valid `EnvFilter` syntax.
    Filter(String),
    /// The format is neither `text` nor `json`.
    Format(String),
    /// A global subscriber is already installed.
    AlreadyInstalled,
    /// [`LogHandle::set_filter`] on a handle that installed nothing.
    NotInstalled,
    /// The subscriber behind the handle is gone.
    Gone,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Filter(e) => write!(f, "invalid log filter: {e}"),
            Error::Format(e) => write!(f, "invalid log format `{e}` (expected `text` or `json`)"),
            Error::AlreadyInstalled => write!(f, "a log subscriber is already installed"),
            Error::NotInstalled => write!(
                f,
                "logging was not installed (no filter at start, and `init_reloadable` was not used)"
            ),
            Error::Gone => write!(f, "the log subscriber no longer exists"),
        }
    }
}

impl std::error::Error for Error {}

type FilterHandle = reload::Handle<EnvFilter, Registry>;

/// A hold on the installed subscriber, to change its filter later.
#[derive(Clone)]
pub struct LogHandle {
    inner: Option<(FilterHandle, Arc<Mutex<String>>)>,
}

impl fmt::Debug for LogHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogHandle")
            .field("filter", &self.filter())
            .finish()
    }
}

impl LogHandle {
    fn inactive() -> LogHandle {
        LogHandle { inner: None }
    }

    /// Whether a subscriber is installed behind this handle.
    pub fn is_installed(&self) -> bool {
        self.inner.is_some()
    }

    /// The filter in force, as last given; `None` when nothing is installed.
    pub fn filter(&self) -> Option<String> {
        self.inner
            .as_ref()
            .map(|(_, s)| s.lock().map(|g| g.clone()).unwrap_or_default())
    }

    /// Change the level while the program runs: `"debug"`, `"off"`,
    /// `"energyml_build=trace,info"`. The new filter takes effect for every
    /// span and event emitted from now on, callsites already seen included.
    /// An invalid filter is refused and leaves the current one in force.
    pub fn set_filter(&self, spec: &str) -> Result<(), Error> {
        let (handle, current) = self.inner.as_ref().ok_or(Error::NotInstalled)?;
        let spec = normalise(spec);
        let filter = parse(&spec)?;
        handle.reload(filter).map_err(|_| Error::Gone)?;
        if let Ok(mut c) = current.lock() {
            *c = spec;
        }
        Ok(())
    }
}

fn normalise(spec: &str) -> String {
    let s = spec.trim();
    if s.is_empty() {
        "off".to_string()
    } else {
        s.to_string()
    }
}

fn parse(spec: &str) -> Result<EnvFilter, Error> {
    EnvFilter::try_new(spec).map_err(|e| Error::Filter(e.to_string()))
}

/// The filter a face asked for: the argument, else `ENERGYML_LOG`; `None` when
/// neither says anything (blank counts as nothing).
pub fn requested(spec: Option<&str>) -> Option<String> {
    let from = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
    spec.and_then(from)
        .or_else(|| std::env::var(ENV_FILTER).ok().and_then(|v| from(&v)))
}

/// Install logging **if it was asked for**, otherwise install nothing.
///
/// `spec` is the face's explicit filter (a `--log` flag); it wins over
/// `ENERGYML_LOG`. `format` likewise wins over `ENERGYML_LOG_FORMAT`. With
/// neither a filter nor the variable, nothing is installed and the returned
/// handle is inactive ([`LogHandle::is_installed`] is false) — a program that
/// wants to switch logs on later uses [`init_reloadable`] instead.
pub fn init(spec: Option<&str>, format: Option<Format>) -> Result<LogHandle, Error> {
    match requested(spec) {
        None => Ok(LogHandle::inactive()),
        Some(s) => install(&s, format),
    }
}

/// Install logging always, starting at the requested filter or **off**, so a
/// running program (a desktop application) can raise the level later with
/// [`LogHandle::set_filter`].
pub fn init_reloadable(spec: Option<&str>, format: Option<Format>) -> Result<LogHandle, Error> {
    install(&requested(spec).unwrap_or_else(|| "off".to_string()), format)
}

fn install(spec: &str, format: Option<Format>) -> Result<LogHandle, Error> {
    let format = match format {
        Some(f) => f,
        None => Format::from_env()?,
    };
    let (dispatch, handle) = dispatch(spec, format, std::io::stderr, std::io::stderr().is_terminal())?;
    tracing::dispatcher::set_global_default(dispatch).map_err(|_| Error::AlreadyInstalled)?;
    // The ETP crates speak `log`: carry their records into the subscriber. A
    // logger already installed is left alone — the face's own, by its choice.
    let _ = tracing_log::LogTracer::init();
    Ok(handle)
}

/// Build a reloadable subscriber writing to `writer`, **without installing it**.
///
/// What [`init`] uses, and what a test or an embedding program uses to hold a
/// subscriber of its own (`tracing::dispatcher::with_default`) without
/// touching the process-wide one.
pub fn dispatch<W>(spec: &str, format: Format, writer: W, ansi: bool) -> Result<(Dispatch, LogHandle), Error>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let spec = normalise(spec);
    let (filter, handle) = reload::Layer::new(parse(&spec)?);
    let events = FmtSpan::CLOSE;
    let layer: Box<dyn Layer<_> + Send + Sync> = match format {
        Format::Text => tracing_subscriber::fmt::layer()
            .with_writer(writer)
            .with_ansi(ansi)
            .with_span_events(events)
            .boxed(),
        Format::Json => tracing_subscriber::fmt::layer()
            .json()
            .with_writer(writer)
            .with_span_events(events)
            .boxed(),
    };
    let subscriber = tracing_subscriber::registry().with(filter).with(layer);
    let handle = LogHandle {
        inner: Some((handle, Arc::new(Mutex::new(spec)))),
    };
    Ok((Dispatch::new(subscriber), handle))
}
