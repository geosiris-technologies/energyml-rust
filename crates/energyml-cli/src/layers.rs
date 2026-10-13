//! Which cache layers a command turns on - **a constant per command**, and the
//! only place the command line decides it
//! (nothing is kept between two questions unless the command is on this list).
//!
//! A command lives once, so the default is [`Cache::off`]: it keeps nothing
//! between two questions, and a question that needs the package index builds it
//! for itself and drops it. The commands below turn the layers on because they
//! ask the same questions again and again over one package - and because they
//! colour: the index and the colour walks are what give the command line **the
//! colour rule of the front end** (the object, its collections, its parents, then the
//! model), where it used to answer by a shorter order that ignores the
//! collections and the model.
//!
//! Layers are lazy: turning one on costs nothing for a run that never asks.
//! This file only names layers; what a layer keeps is `energyml-cache`'s.

use energyml_cache::{Cache, Layers};

/// What colours: the index, and the walks that read it.
const COLOURING: Layers = Layers::none().with_index().with_colours();

/// The commands that turn layers on, and the layers each turns on. **A command
/// that is not here keeps nothing** ([`Cache::off`]).
///
/// - `colors` - every object's colour, by `energyml_cache::colours::of_package`;
/// - `export` - the scene's colour ramps, by `energyml_cache::colours::Colouring`;
/// - `geometry` - the same scene reading (`scene::of_package`), so the same
///   colours should it be asked for them.
pub const COMMANDS: &[(&str, Layers)] = &[
    ("colors", COLOURING),
    ("export", COLOURING),
    ("geometry", COLOURING),
];

/// The cache of a command, from the fixed list.
pub fn cache_for(command: &str) -> Cache {
    let layers = COMMANDS
        .iter()
        .find(|(name, _)| *name == command)
        .map(|(_, layers)| *layers)
        .unwrap_or(Layers::none());
    Cache::new(layers)
}
