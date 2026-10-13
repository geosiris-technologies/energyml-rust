//! Create EnergyML objects — from geometry, from a foreign file, from other
//! objects — through one door.
//!
//! By design (ADR-0017), **an operation is a pure function from a
//! bag of inputs ([`pieces::Pieces`]) to a plan of objects
//! ([`operation::Plan`]), and [`apply::apply`] is the only step that touches
//! storage.** [`operation::Registry::offers`] answers *what can I create with
//! this?*, with every refusal named; [`template`] projects what an operation
//! asks into its canonical `ActivityTemplate`, so the form and the record are
//! one declaration.
//!
//! The crate grew out of the GeoSciML GeologicUnit import (tested with SIGEOL exports), which was already this shape — a
//! plan, then a write, with derived uuids — and which is now its first
//! operation ([`ops::sigeol`]). The modules below the operations are its
//! pieces, kept apart for the reason they always were:
//!
//! # The three jobs, kept apart
//!
//! - [`geojson`] parses the **format**, RFC 7946 and nothing else. It knows no
//!   geology and no RESQML;
//! - [`sigeol`] reads the **vocabulary** — GeoSciML, tested with SIGEOL exports — and
//!   answers what a feature is about. It knows no RESQML either;
//! - the building and the writing come next, and they are the only parts that
//!   know a package exists.
//!
//! The seam matters because the first two are where a mistake is *silent*: a
//! misread ordinate places a model in the Gulf of Guinea, a misread age labels
//! a top as a bottom, and both produce a file that opens cleanly everywhere.
//! Everything above them fails loudly or not at all.

#![warn(missing_docs)]

// The derives name `::energyml_build`, so that they work in a caller's crate
// and in this one alike.
extern crate self as energyml_build;

pub mod apply;
pub mod assemble;
pub mod borehole;
pub mod build;
pub mod crs;
pub mod declare;
pub mod describe;
pub mod entries;
pub mod epc;
pub mod exemplar;
pub mod galaxy;
pub mod geojson;
pub mod mesh;
pub mod manifest;
pub mod naming;
pub mod operation;
pub mod osdu;
pub mod ops;
pub mod pieces;
pub mod read;
pub mod recipe;
pub mod selection;
pub mod sigeol;
pub mod source;
pub mod table;
pub mod tabular;
pub mod template;
pub mod typed;
pub mod types;
