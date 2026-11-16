//! The name of a view: what a face shows instead of its technical id.

use energyml_views::ViewKinds;

#[test]
fn every_built_in_view_has_a_name_of_its_own_that_is_not_its_id() {
    let kinds = ViewKinds::with_builtins();
    let mut seen: Vec<&str> = Vec::new();
    for id in kinds.ids() {
        let name = kinds.name_of(id).expect("a view of the registry has a name");
        assert!(!name.trim().is_empty(), "`{id}` has an empty name");
        assert_ne!(name, id, "`{id}` is named by its identity: a name is for a person");
        assert!(!seen.contains(&name), "`{name}` names two views");
        seen.push(name);
    }
    assert_eq!(seen.len(), 11, "the eleven built-in views: {seen:?}");
    assert_eq!(kinds.name_of("target"), Some("Target"));
    assert_eq!(kinds.name_of("along-well"), Some("Along the well"));
    // Break it: leave a view without its `name` (the default is its id, and this test names it).
}

#[test]
fn the_name_of_a_view_the_registry_does_not_hold_is_none() {
    assert_eq!(ViewKinds::with_builtins().name_of("nowhere"), None);
    assert_eq!(ViewKinds::empty().name_of("target"), None);
    // Break it: answer the id for a view that is unknown (the face would show a name for nothing).
}
