//! The export run: the one sequence every face calls.
//!
//! Given scene objects that are already read, an exporter and its
//! [`Options`], [`run`] does what the command line and the Python bridge each
//! wrote for themselves: prepare every object, ask the exporter whether it
//! writes it, write what it does, and **say what it did not**.
//!
//! # Skip or refuse is the caller's word
//!
//! An object an exporter cannot write is [`OnUnhandled::Refuse`] (the run fails,
//! naming it, before anything is written) or [`OnUnhandled::Skip`] (the run
//! goes on and the object is named in [`Report::skipped`]). The two used to be
//! two behaviours of two faces; now it is a field of the request, and a face
//! that skips shows what it skipped, never hides it.
//!
//! # Where the output goes
//!
//! A [`Sink`]: a directory (one file per object, named by [`file_name`]), one
//! file, or any writer. In a directory one object failing is not the run
//! failing: its stub is removed and it is named in [`Report::failed`].

use std::io::Write;
use std::path::Path;

use energyml_geometry::SceneObject;

use crate::error::{Error, Result};
use crate::registry::{Exporter, Options, prepare};

/// What to do with an object the exporter does not write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnUnhandled {
    /// Leave it out and name it in [`Report::skipped`].
    Skip,
    /// Fail the run, naming the object, before anything is written.
    Refuse,
}

/// Where the bytes go.
pub enum Sink<'a> {
    /// A directory, created if need be: one file per object, named by [`file_name`].
    Directory(&'a Path),
    /// One file holding every object.
    File(&'a Path),
    /// Any writer (a buffer, standard output), every object in one document.
    Stream(&'a mut dyn Write),
}

/// An object the exporter does not write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The object's uuid.
    pub uuid: String,
    /// Its ETP uri.
    pub uri: String,
    /// Its type, as the scene object carries it.
    pub qualified_type: String,
    /// Its title.
    pub title: String,
    /// Why, in a sentence.
    pub reason: String,
}

/// What a run did.
#[derive(Debug, Default)]
pub struct Report {
    /// How many objects were written.
    pub written: usize,
    /// The objects the exporter does not write (only with [`OnUnhandled::Skip`]).
    pub skipped: Vec<Skipped>,
    /// Objects the exporter accepted and then failed on, one line each
    /// (directory sink only: any other sink fails the run).
    pub failed: Vec<String>,
}

/// The type of an object as a file name wants it: `resqml22.TriangulatedSetRepresentation`.
///
/// Read out of the ETP URI rather than from `SceneObject::qualified_type`,
/// which carries only the bare type name and is written into every export's
/// metadata (so it is left alone).
pub fn qualified_type_of(object: &SceneObject) -> String {
    energyml_core::identity::ObjectId::parse(&object.uri)
        .map(|(_, id)| id.qualified_type())
        .unwrap_or_else(|_| object.qualified_type.clone())
}

/// The file name of one object in a directory: `qualifiedType_uuid.ext`.
///
/// The uuid makes it unique (two objects share a title far more often than
/// anyone expects), the qualified type makes it readable and carries the
/// release (`resqml20.` against `resqml22.`). Anything a file system objects
/// to is replaced.
pub fn file_name(object: &SceneObject, exporter: &dyn Exporter) -> String {
    format!(
        "{}_{}.{}",
        file_safe(&qualified_type_of(object)),
        object.uuid,
        exporter.extension()
    )
}

fn file_safe(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

/// The CRS that give no EPSG code, among the objects the exporter does not write.
///
/// `"uuid (kind)"`, sorted and without repeats: the hint a reader needs when
/// GeoJSON skips objects over a package that plainly contains a CRS.
pub fn crs_without_epsg(
    objects: &[SceneObject],
    exporter: &dyn Exporter,
    options: &Options<'_>,
) -> Vec<String> {
    let mut named: Vec<String> = objects
        .iter()
        .filter(|o| !exporter.handles_with(o, options))
        .filter_map(|o| o.crs.as_ref())
        .filter(|crs| crs.projected.epsg().is_none())
        .map(|crs| format!("{} ({})", crs.uuid, crs.kind))
        .collect();
    named.sort();
    named.dedup();
    named
}

/// Write `objects` with `exporter` into `sink`.
///
/// Every object is prepared ([`prepare`]) and asked of
/// [`Exporter::handles_with`]; the ones it does not write are refused or
/// skipped by `on_unhandled`. A shape rule an object cannot take is the same
/// case: refused by name, or skipped with the reason.
///
/// # Errors
///
/// [`Error::Unsuitable`] (or the preparation's own error) for the first
/// unhandled object with [`OnUnhandled::Refuse`]; the exporter's or the
/// sink's error for anything that goes wrong while writing to a file or a
/// stream.
pub fn run(
    objects: &[SceneObject],
    exporter: &dyn Exporter,
    options: &Options<'_>,
    on_unhandled: OnUnhandled,
    sink: Sink<'_>,
) -> Result<Report> {
    let mut report = Report::default();
    let mut writable: Vec<&SceneObject> = Vec::with_capacity(objects.len());
    for object in objects {
        let why = match prepare(object, options) {
            Err(e) => {
                let reason = format!("{e}");
                Some((e, reason))
            }
            Ok(prepared) => {
                if exporter.handles_with(&prepared, options) {
                    None
                } else {
                    let what = format!("{} ({})", prepared.qualified_type, object.uri);
                    Some((
                        Error::unsuitable(exporter.name(), what),
                        format!("`{}` cannot write it", exporter.name()),
                    ))
                }
            }
        };
        match (why, on_unhandled) {
            (None, _) => writable.push(object),
            (Some((error, _)), OnUnhandled::Refuse) => return Err(error),
            (Some((_, reason)), OnUnhandled::Skip) => report.skipped.push(Skipped {
                uuid: object.uuid.clone(),
                uri: object.uri.clone(),
                qualified_type: object.qualified_type.clone(),
                title: object.title.clone(),
                reason,
            }),
        }
    }

    match sink {
        Sink::Directory(dir) => {
            std::fs::create_dir_all(dir)?;
            for object in writable {
                let target = dir.join(file_name(object, exporter));
                let file = std::fs::File::create(&target)?;
                let mut out = std::io::BufWriter::new(file);
                // One object failing is not the run failing: a batch that stops
                // at the first refusal leaves a folder that looks finished.
                match exporter.write(object, &mut out, options) {
                    Ok(()) => match out.flush() {
                        Ok(()) => report.written += 1,
                        Err(e) => {
                            drop(out);
                            let _ = std::fs::remove_file(&target);
                            report
                                .failed
                                .push(format!("{}  {}  {e}", object.uuid, object.qualified_type));
                        }
                    },
                    Err(e) => {
                        // Take the stub with it: a zero-byte file reads as a
                        // representation with nothing in it.
                        drop(out);
                        let _ = std::fs::remove_file(&target);
                        report
                            .failed
                            .push(format!("{}  {}  {e}", object.uuid, object.qualified_type));
                    }
                }
            }
        }
        Sink::File(path) => {
            let file = std::fs::File::create(path)?;
            let mut out = std::io::BufWriter::new(file);
            write_many(exporter, &writable, &mut out, options)?;
            out.flush()?;
            report.written = writable.len();
        }
        Sink::Stream(out) => {
            write_many(exporter, &writable, out, options)?;
            report.written = writable.len();
        }
    }
    Ok(report)
}

fn write_many(
    exporter: &dyn Exporter,
    objects: &[&SceneObject],
    out: &mut dyn Write,
    options: &Options<'_>,
) -> Result<()> {
    // `write_all` takes a slice of owned objects: the format decides whether
    // that is one document or a concatenation.
    let owned: Vec<SceneObject> = objects.iter().map(|o| (*o).clone()).collect();
    exporter.write_all(&owned, out, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_safe_replaces_what_a_file_system_refuses() {
        assert_eq!(file_safe("a/b:c*d"), "a_b_c_d");
        assert_eq!(file_safe("resqml22.Foo"), "resqml22.Foo");
        // Break it: drop one of the characters from the match arm.
    }
}
