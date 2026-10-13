//! The level changes while the program runs - the desktop application's switch.

use std::io;
use std::sync::{Arc, Mutex};

use energyml_log::{Format, dispatch};
use tracing::{debug, info, info_span};
use tracing_subscriber::fmt::MakeWriter;

/// An in-memory stderr.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

impl Sink {
    fn take(&self) -> String {
        String::from_utf8(std::mem::take(&mut *self.0.lock().unwrap())).unwrap()
    }
}

fn emit() {
    // One callsite, hit repeatedly: its cached interest must be rebuilt when
    // the filter changes, or the second pass stays silent.
    debug!("a debug line");
}

#[test]
fn the_level_changes_while_the_program_runs() {
    let sink = Sink::default();
    // Starts off, as a desktop face does.
    let (d, handle) = dispatch("off", Format::Text, sink.clone(), false).unwrap();
    tracing::dispatcher::with_default(&d, || {
        emit();
        assert_eq!(sink.take(), "", "off: nothing");

        handle.set_filter("debug").unwrap();
        emit();
        assert!(sink.take().contains("a debug line"), "debug: the line appears");
        assert_eq!(handle.filter().as_deref(), Some("debug"));

        handle.set_filter("off").unwrap();
        emit();
        assert_eq!(sink.take(), "", "off again: nothing");
    });
}

#[test]
fn a_span_says_how_long_it_took_and_a_bad_filter_is_refused() {
    let sink = Sink::default();
    let (d, handle) = dispatch("info", Format::Text, sink.clone(), false).unwrap();
    tracing::dispatcher::with_default(&d, || {
        {
            let _s = info_span!("step", rows = 3).entered();
            info!("inside");
        }
        let out = sink.take();
        assert!(out.contains("inside"));
        assert!(
            out.contains("time.busy"),
            "span close carries the elapsed time: {out}"
        );
        assert!(out.contains("rows=3"));
        assert!(handle.set_filter("energyml=[").is_err());
        // The failed change left the filter alone.
        assert_eq!(handle.filter().as_deref(), Some("info"));
    });
}

#[test]
fn json_lines_are_json() {
    let sink = Sink::default();
    let (d, _h) = dispatch("info", Format::Json, sink.clone(), false).unwrap();
    tracing::dispatcher::with_default(&d, || info!(n = 2, "hello"));
    let out = sink.take();
    let line = out.lines().next().unwrap();
    assert!(
        line.starts_with('{') && line.ends_with('}') && line.contains("\"hello\""),
        "{line}"
    );
}

#[test]
fn nothing_installed_refuses_a_change() {
    // No filter asked, none in the environment of a test run: inactive.
    if std::env::var(energyml_log::ENV_FILTER).is_ok() {
        return;
    }
    let h = energyml_log::init(None, None).unwrap();
    assert!(!h.is_installed());
    assert!(h.set_filter("debug").is_err());
}
