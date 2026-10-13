//! A cache that keeps nothing answers what a cache that keeps everything
//! answers - **the audit: cache on, cache off, on Lutetia**.
//!
//! There is one implementation (ADR-0010): a layer that is off runs the same code
//! over a slot thrown away after the call. What differs is what is *held*, and
//! that is counted, never timed.

use energyml_cache::colours::{self, Wanted};
use energyml_cache::{Cache, Held, Layers};

use crate::lutetia;

fn rows(cache: &Cache, store: &energyml_storage::EpcStore) -> Vec<(String, String)> {
    let report = colours::of_package(cache, store, &Wanted::default()).expect("the colours answer");
    report
        .rows
        .iter()
        .map(|row| (row.summary.uuid.clone(), format!("{:?}", row.decision)))
        .collect()
}

#[test]
fn the_colours_are_the_same_with_every_layer_on_and_with_none() {
    let on = Cache::new(Layers::all());
    let off = Cache::off();
    let Some(store) = lutetia::store() else { return };
    let (with, without) = (rows(&on, &store), rows(&off, &store));
    assert!(!with.is_empty(), "Lutetia has representations and properties");
    assert!(
        with.iter().any(|(_, decision)| decision != "None"),
        "an audit over objects none of which has a colour proves nothing"
    );
    assert_eq!(with, without);
}

#[test]
fn the_ladders_and_the_file_colours_are_the_same_on_and_off() {
    let Some(store) = lutetia::store() else { return };
    let uuids: Vec<String> = store
        .resources()
        .iter()
        .filter(|summary| summary.qualified_type.contains("Representation"))
        .map(|summary| summary.uuid.clone())
        .collect();
    assert!(uuids.len() > 5, "Lutetia has representations");

    let on = Cache::new(Layers::all());
    let off = Cache::off();
    let (with, without) = (
        on.file_colours(&store, &uuids).unwrap(),
        off.file_colours(&store, &uuids).unwrap(),
    );
    assert_eq!(with, without);

    let asked: Vec<colours::Row<'_>> = uuids
        .iter()
        .map(|uuid| colours::Row {
            key: uuid.to_ascii_lowercase(),
            kinds: Vec::new(),
            property: None,
        })
        .collect();
    let (with, without) = (
        on.ladders(&store, &asked).unwrap(),
        off.ladders(&store, &asked).unwrap(),
    );
    let (with, without) = (
        with.expect("Lutetia holds a set"),
        without.expect("Lutetia holds a set"),
    );
    assert_eq!(with.len(), asked.len());
    assert_eq!(format!("{with:?}"), format!("{without:?}"));
}

#[test]
fn the_attachments_are_the_same_on_and_off() {
    let Some(store) = lutetia::store() else { return };
    let uuids: Vec<String> = store
        .resources()
        .iter()
        .map(|summary| summary.uuid.clone())
        .collect();
    let asked: Vec<&str> = uuids.iter().map(String::as_str).collect();

    let with = Cache::new(Layers::all()).attachments_of(&store, &asked);
    let without = Cache::off().attachments_of(&store, &asked);
    assert_eq!(with.len(), asked.len(), "one listing per object asked");
    assert_eq!(format!("{with:?}"), format!("{without:?}"));
}

#[test]
fn an_off_cache_keeps_nothing_and_an_on_cache_keeps_what_it_was_asked() {
    let Some(store) = lutetia::store() else { return };
    let uuids: Vec<String> = store
        .resources()
        .iter()
        .filter(|summary| summary.qualified_type.contains("Representation"))
        .map(|summary| summary.uuid.clone())
        .collect();
    let asked: Vec<&str> = uuids.iter().map(String::as_str).collect();

    let off = Cache::off();
    colours::of_package(&off, &store, &Wanted::default()).unwrap();
    off.file_colours(&store, &uuids).unwrap();
    off.attachments_of(&store, &asked);
    off.with_index(&store, |index| {
        assert!(
            !index.is_empty(),
            "a question is still answered: from an index built for it"
        )
    });
    assert_eq!(
        off.held(),
        Held::default(),
        "…and nothing of it is held afterwards"
    );
    assert_eq!(
        off.colour_walk_stats(),
        Default::default(),
        "no walk was kept, so none is counted"
    );
    assert_eq!(off.attachment_stats(), Default::default());
    assert_eq!(
        off.index_step(&store, 100).total,
        0,
        "there is no pass to advance"
    );
    assert!(off.index_slot().revision().is_none(), "the slot was never filled");

    let on = Cache::new(Layers::all());
    on.file_colours(&store, &uuids).unwrap();
    on.attachments_of(&store, &asked);
    let held = on.held();
    assert!(held.index_nodes >= uuids.len(), "{held:?}");
    assert!(held.colour_walks > 0, "{held:?}");
    assert!(held.listings > 0, "{held:?}");
}

#[test]
fn a_layer_that_stands_on_the_index_turns_it_on() {
    let colours_only = Cache::new(Layers::none().with_colours());
    assert!(
        colours_only.layers().index && colours_only.layers().colours && !colours_only.layers().attachments
    );
    let attachments_only = Cache::new(Layers::none().with_attachments());
    assert!(attachments_only.layers().index);
    assert_eq!(Cache::off().layers(), Layers::none());
}

#[test]
fn a_second_question_does_not_walk_again_when_the_layer_is_on() {
    let Some(store) = lutetia::store() else { return };
    let uuids: Vec<String> = store
        .resources()
        .iter()
        .filter(|summary| summary.qualified_type.contains("Representation"))
        .map(|summary| summary.uuid.clone())
        .collect();
    let on = Cache::new(Layers::all());
    on.file_colours(&store, &uuids).unwrap();
    let begun = on.colour_walk_stats().walks_begun;
    assert!(begun > 0);
    on.file_colours(&store, &uuids).unwrap();
    assert_eq!(on.colour_walk_stats().walks_begun, begun, "the walks were kept");

    let off = Cache::off();
    off.file_colours(&store, &uuids).unwrap();
    off.file_colours(&store, &uuids).unwrap();
    assert_eq!(
        off.colour_walk_stats().walks_begun,
        0,
        "an off cache holds no slot, so there is no walk of its own to count"
    );
}
