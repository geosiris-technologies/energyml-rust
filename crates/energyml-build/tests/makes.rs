//! What an operation says it makes is a type a package can be given.
//!
//! A defect found while writing the reference model: `properties-from-table` and
//! `wells-and-properties-from-table` declared they made
//! `resqml22.CategoricalProperty`, which RESQML 2.2 does not have - a code's
//! meaning goes through `DiscreteProperty.CategoryLookup` there. The front end shows
//! that list and looks up objects of the same types in it (`modelsFor`), and
//! nothing confronted it with the schemas.
//!
//! **Asking the schema registry whether it knows the name is not enough**, and
//! that is what makes this less obvious than it looks: `resqml22.CategoricalProperty`
//! **is** in `energyml_schemas::all()` - through the `2.2_dev3` development
//! drop, registered so that files written against it still open. A check on
//! *known* would have passed over the defect. The question that holds is the
//! one the front end asks of a type somebody creates - *does a release have it, and
//! does it name a part?* - which is `energyml_schemas::may_create`, the one
//! answer the workspace gives to it.

use energyml_build::operation::Registry;

#[test]
fn every_type_an_operation_makes_is_a_part_a_release_can_hold() {
    let schemas = energyml_schemas::all();
    let mut checked = 0usize;
    let mut wrong = Vec::new();
    for operation in Registry::new().iter() {
        for made in operation.makes() {
            checked += 1;
            match schemas.by_qualified_type(made) {
                None => wrong.push(format!(
                    "`{}` makes `{made}`, which no schema of this build knows",
                    operation.name()
                )),
                // The drop owns the names its release does not have: that is
                // how `resqml22.CategoricalProperty` resolves at all.
                Some(entry) if !energyml_schemas::may_create(entry.info) => wrong.push(format!(
                    "`{}` makes `{made}`, which only {} {} holds - nothing a release writes",
                    operation.name(),
                    entry.info.package,
                    entry.info.version
                )),
                Some(_) => {}
            }
        }
    }
    // The negative control: an empty `makes` everywhere would pass the loop.
    assert!(
        checked >= 20,
        "only {checked} type(s) declared over the whole registry - the loop proved nothing"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The control the check above rests on, kept apart so that it says which of
/// its two halves broke: the name this defect was about **resolves**, and it is
/// **not** creatable. If the drop stopped being registered, the check above
/// would still catch the name - as unknown - but this one would say why the
/// first half of the argument no longer holds.
#[test]
fn the_categorical_property_of_2_2_is_a_drop_name_only() {
    let schemas = energyml_schemas::all();
    let entry = schemas.by_qualified_type("resqml22.CategoricalProperty").expect(
        "the 2.2_dev3 drop registers it, which is what made the defect invisible to a check on names",
    );
    assert!(
        !energyml_schemas::may_create(entry.info),
        "{} {} {} is offered for creation",
        entry.info.package,
        entry.info.version,
        entry.info.name
    );
    // And what replaces it is a release's own.
    let discrete = schemas
        .by_qualified_type("resqml22.DiscreteProperty")
        .expect("RESQML 2.2 has discrete properties");
    assert!(energyml_schemas::may_create(discrete.info));
}
