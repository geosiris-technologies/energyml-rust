//! The generated relations tables, held against the reflection of the same types.
//!
//! # What this checks, and what it cannot
//!
//! Each generated crate carries a `relations.rs`: for every top-level type, the
//! places where it holds another data object by value (`ContainedSlot`) and,
//! for WITSML, where it writes a reference (`DorSlot`). The same facts can be
//! read a second way, by walking [`TypeInfo::fields`] from the type: a field
//! whose type is a top-level data object that is not a reference is a slot, a
//! nested struct lengthens the path, a choice is entered per variant.
//!
//! Both routes start from the generator's IR, so this suite catches what the
//! generator **forgot to emit** or emitted at the wrong place, not a wrong
//! reading of the XSD. That one is caught by the generator's own test against
//! an oracle that parses the schemas by itself. It is the second half of the
//! same guarantee, and neither half replaces the other.
//!
//! No test here reads a schema or a package: the types are the generated ones.
//!
//! # Making it fail
//!
//! - Delete a `ContainedSlot` from a generated `relations.rs` (or stop the
//!   generator emitting it), and `every_table_agrees_with_the_reflection`
//!   names the type and the slot.
//! - The checker itself is held by `a_removed_slot_is_reported` and its two
//!   siblings, which feed it a table with one entry removed, added or
//!   altered.

use std::collections::{BTreeSet, HashMap};

use energyml_core::naming;
use energyml_core::reflect::{Binding, FieldInfo, FieldType, TypeInfo, TypeKind};
use energyml_core::registry::Registry;
use energyml_core::relations::{ContainedSlot, DorSlot, Step, TypeRelations, dotted};

/// The packages the generator computes reference slots for. A fact about the
/// generator's configuration, written here because the tables cannot say which
/// of their empty `refers` mean "none" and which "not computed".
const WITH_REFERENCES: &[&str] = &["witsml"];

fn registry() -> Registry {
    energyml_schemas::all()
}

fn tables(registry: &Registry) -> Vec<&'static TypeRelations> {
    registry.relations().collect()
}

// ---------------------------------------------------------------------------
// The second route: the facts, from reflection
// ---------------------------------------------------------------------------

/// The qualified type of any type, nested ones included, as the generator
/// writes it.
fn qualified(info: &TypeInfo) -> String {
    let local = if info.kind == TypeKind::Choice {
        info.xml_name
    } else {
        info.object_type
    };
    format!(
        "{}{}.{}",
        info.package,
        naming::compact_version(info.schema_version),
        local
    )
}

fn key_of(info: &TypeInfo) -> usize {
    std::ptr::from_ref(info) as usize
}

/// The struct types a choice can hold.
fn payloads(choice: &TypeInfo) -> Vec<&'static TypeInfo> {
    choice
        .variants
        .iter()
        .filter_map(|v| v.ty)
        .map(|get| get())
        .collect()
}

/// What a field means for the walk.
enum Target {
    Dor,
    /// A data object held by value, by the qualified type the field declares;
    /// what else the field accepts is read again from the field itself when
    /// the slot is checked (`accepted`).
    Contained {
        declared: String,
    },
    Descend(&'static TypeInfo),
}

/// Read one field, the way the generator does and from the other end.
fn targets(field: &'static FieldInfo) -> Vec<Target> {
    let FieldType::Object(get) = field.ty else {
        return Vec::new();
    };
    if field.binding != Binding::Element {
        return Vec::new();
    }
    let t = get();
    match t.kind {
        TypeKind::Enumeration => Vec::new(),
        TypeKind::Struct => {
            if t.is_object_ref {
                vec![Target::Dor]
            } else if t.is_top_level {
                vec![Target::Contained {
                    declared: qualified(t),
                }]
            } else {
                vec![Target::Descend(t)]
            }
        }
        TypeKind::Choice => {
            let held = payloads(t);
            let mut out = Vec::new();
            if held.iter().any(|p| p.is_object_ref) {
                out.push(Target::Dor);
            }
            let tops: BTreeSet<String> = held
                .iter()
                .filter(|p| p.is_top_level && !p.is_object_ref)
                .map(|p| qualified(p))
                .collect();
            if !tops.is_empty() {
                out.push(Target::Contained {
                    declared: qualified(t),
                });
            }
            for p in held {
                if !p.is_top_level && !p.is_object_ref && p.kind == TypeKind::Struct {
                    out.push(Target::Descend(p));
                }
            }
            out
        }
    }
}

/// For every struct reachable from a registered type, whether a containment
/// slot (first) or a reference slot (second) can be reached from it. Computed
/// once; the model is cyclic, hence a fixed point.
fn reach(registry: &Registry) -> HashMap<usize, (bool, bool)> {
    let mut index: HashMap<usize, usize> = HashMap::new();
    let mut nodes: Vec<&'static TypeInfo> = Vec::new();
    let mut stack: Vec<&'static TypeInfo> = registry.types().collect();
    while let Some(t) = stack.pop() {
        if index.contains_key(&key_of(t)) {
            continue;
        }
        index.insert(key_of(t), nodes.len());
        nodes.push(t);
        for f in t.fields {
            for target in targets(f) {
                if let Target::Descend(d) = target {
                    stack.push(d);
                }
            }
        }
    }
    let edges: Vec<Vec<Target>> = nodes
        .iter()
        .map(|t| t.fields.iter().flat_map(targets).collect())
        .collect();
    let mut flags = vec![(false, false); nodes.len()];
    loop {
        let mut changed = false;
        for (i, edge) in edges.iter().enumerate() {
            let mut now = flags[i];
            for target in edge {
                match target {
                    Target::Dor => now.1 = true,
                    Target::Contained { .. } => now.0 = true,
                    Target::Descend(d) => {
                        if let Some(j) = index.get(&key_of(d)) {
                            now.0 |= flags[*j].0;
                            now.1 |= flags[*j].1;
                        }
                    }
                }
            }
            if now != flags[i] {
                flags[i] = now;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    nodes
        .iter()
        .enumerate()
        .map(|(i, t)| (key_of(t), flags[i]))
        .collect()
}

/// What reflection says one type holds.
#[derive(Default)]
struct Expected {
    /// `(dotted path, declared type)`.
    contained: BTreeSet<(String, String)>,
    /// `(dotted path, effective requiredness)`.
    refers: BTreeSet<(String, bool)>,
}

struct Collector<'r> {
    reach: &'r HashMap<usize, (bool, bool)>,
    want_refs: bool,
    path: Vec<&'static str>,
    visiting: Vec<usize>,
    out: Expected,
}

impl Collector<'_> {
    fn collect(&mut self, info: &'static TypeInfo, required: bool) {
        for field in info.fields {
            let targets = targets(field);
            if targets.is_empty() {
                continue;
            }
            let required_here = required && field.is_required();
            self.path.push(field.xml_name);
            let here = self.path.join(".");
            for target in targets {
                match target {
                    Target::Dor => {
                        if self.want_refs {
                            self.out.refers.insert((here.clone(), required_here));
                        }
                    }
                    Target::Contained { declared, .. } => {
                        self.out.contained.insert((here.clone(), declared));
                    }
                    Target::Descend(d) => {
                        let (contains, dor) = self.reach.get(&key_of(d)).copied().unwrap_or_default();
                        if !(contains || (self.want_refs && dor)) || self.visiting.contains(&key_of(d)) {
                            continue;
                        }
                        self.visiting.push(key_of(d));
                        self.collect(d, required_here);
                        self.visiting.pop();
                    }
                }
            }
            self.path.pop();
        }
    }
}

fn expected_for(info: &'static TypeInfo, want_refs: bool, reach: &HashMap<usize, (bool, bool)>) -> Expected {
    let mut collector = Collector {
        reach,
        want_refs,
        path: Vec::new(),
        visiting: vec![key_of(info)],
        out: Expected::default(),
    };
    collector.collect(info, true);
    collector.out
}

// ---------------------------------------------------------------------------
// Does a slot point at a real field, of the type it says?
// ---------------------------------------------------------------------------

/// The fields (with their types) a path ends at, following `when_type` through
/// choices. Empty when the path does not exist.
fn resolve(info: &'static TypeInfo, path: &[Step]) -> Vec<(&'static FieldInfo, &'static TypeInfo)> {
    let mut current = vec![info];
    for (i, step) in path.iter().enumerate() {
        let last = i + 1 == path.len();
        let mut next = Vec::new();
        let mut leaf = Vec::new();
        for cur in &current {
            let Some(field) = cur
                .fields
                .iter()
                .find(|f| f.xml_name == step.name && f.binding == Binding::Element)
            else {
                continue;
            };
            let FieldType::Object(get) = field.ty else {
                continue;
            };
            let t = get();
            if last {
                leaf.push((field, t));
                continue;
            }
            match (t.kind, step.when_type) {
                (TypeKind::Choice, Some(wanted)) => {
                    next.extend(payloads(t).into_iter().filter(|p| qualified(p) == wanted));
                }
                (TypeKind::Choice, None) => next.extend(payloads(t)),
                (_, Some(_)) => {}
                (_, None) => next.push(t),
            }
        }
        if last {
            return leaf;
        }
        if next.is_empty() {
            return Vec::new();
        }
        current = next;
    }
    Vec::new()
}

fn describe(slot: &ContainedSlot) -> String {
    format!("{} -> {}", dotted(slot.path), slot.declared)
}

/// The type names a slot's field accepts, per reflection: the concrete data
/// objects behind the field's type.
fn accepted(t: &TypeInfo) -> BTreeSet<String> {
    if t.kind == TypeKind::Choice {
        payloads(t)
            .into_iter()
            .filter(|p| p.is_top_level && !p.is_object_ref)
            .map(qualified)
            .collect()
    } else {
        BTreeSet::from([qualified(t)])
    }
}

fn check_contained_slot(info: &'static TypeInfo, slot: &ContainedSlot, out: &mut Vec<String>) {
    let who = format!("{}: slot `{}`", info.package_version(), describe(slot));
    if slot.path.is_empty() {
        out.push(format!("{who}: an empty path"));
        return;
    }
    let ends = resolve(info, slot.path);
    if ends.is_empty() {
        out.push(format!("{who}: no such field on {}", qualified(info)));
        return;
    }
    // At least one route to the field must agree with the slot on every point.
    let agrees = ends.iter().any(|&(field, t)| {
        let accepted = accepted(t);
        let declared_matches = slot.declared == qualified(t);
        let concrete_declared = accepted.contains(slot.declared);
        let mut allowed: BTreeSet<String> = slot.alternatives.iter().map(|s| (*s).to_string()).collect();
        if !slot.abstract_bound {
            allowed.insert(slot.declared.to_string());
        }
        declared_matches
            && slot.abstract_bound == !concrete_declared
            && allowed == accepted
            && field.occurs.min == slot.min_occurs
            && match field.occurs.max {
                energyml_core::reflect::MaxOccurs::Bounded(n) => slot.max_occurs == Some(n),
                energyml_core::reflect::MaxOccurs::Unbounded => slot.max_occurs.is_none(),
            }
    });
    if !agrees {
        out.push(format!(
            "{who}: the field exists but its type or cardinality is not what the slot says"
        ));
    }
}

fn check_dor_slot(info: &'static TypeInfo, slot: &DorSlot, out: &mut Vec<String>) {
    let who = format!(
        "{}: reference slot `{}`",
        info.package_version(),
        dotted(slot.path)
    );
    let ends = resolve(info, slot.path);
    if ends.is_empty() {
        out.push(format!("{who}: no such field on {}", qualified(info)));
        return;
    }
    let is_ref = |t: &TypeInfo| {
        t.is_object_ref || (t.kind == TypeKind::Choice && payloads(t).iter().any(|p| p.is_object_ref))
    };
    if !ends.iter().any(|&(_, t)| is_ref(t)) {
        out.push(format!("{who}: the field is not a reference"));
    }
}

// ---------------------------------------------------------------------------
// The check itself
// ---------------------------------------------------------------------------

type Key = (String, String, String);

fn key_of_table(t: &TypeRelations) -> Key {
    (
        t.package.to_ascii_lowercase(),
        t.version.to_string(),
        t.name.to_ascii_lowercase(),
    )
}

/// Everything wrong with `tables`, held against the reflection of the types in
/// `registry`. Empty when they agree.
fn problems(registry: &Registry, tables: &[&'static TypeRelations]) -> Vec<String> {
    let reach = reach(registry);
    let mut out = Vec::new();
    let by_key: HashMap<Key, &'static TypeRelations> = tables.iter().map(|t| (key_of_table(t), *t)).collect();

    // 1. Each table: a registered top-level type; each slot a real field of the
    //    type it says; and the slots are exactly the ones reflection finds.
    for table in tables {
        let Some(entry) = registry.by_name(table.package, table.version, table.name) else {
            out.push(format!("{}: not a registered type", table.qualified_type));
            continue;
        };
        let info = entry.info;
        if !info.is_top_level || info.is_object_ref {
            out.push(format!("{}: not a top-level data object", table.qualified_type));
            continue;
        }
        for slot in table.contains {
            check_contained_slot(info, slot, &mut out);
        }
        for slot in table.refers {
            check_dor_slot(info, slot, &mut out);
        }

        let want_refs = WITH_REFERENCES.contains(&table.package);
        let want = expected_for(info, want_refs, &reach);
        let got_contained: BTreeSet<(String, String)> = table
            .contains
            .iter()
            .map(|c| (dotted(c.path), c.declared.to_string()))
            .collect();
        for (path, declared) in want.contained.difference(&got_contained) {
            out.push(format!(
                "{}: reflection finds the slot `{path} -> {declared}` and the table does not",
                table.qualified_type
            ));
        }
        for (path, declared) in got_contained.difference(&want.contained) {
            out.push(format!(
                "{}: the table has the slot `{path} -> {declared}` and reflection does not",
                table.qualified_type
            ));
        }
        if want_refs {
            let got_refs: BTreeSet<(String, bool)> = table
                .refers
                .iter()
                .map(|d| (dotted(d.path), d.required))
                .collect();
            for (path, required) in want.refers.difference(&got_refs) {
                out.push(format!(
                    "{}: reflection finds the reference `{path}` (required: {required}) and the table does not",
                    table.qualified_type
                ));
            }
            for (path, required) in got_refs.difference(&want.refers) {
                out.push(format!(
                    "{}: the table has the reference `{path}` (required: {required}) and reflection does not",
                    table.qualified_type
                ));
            }
        } else if !table.refers.is_empty() {
            out.push(format!(
                "{}: reference slots in a package that does not emit them",
                table.qualified_type
            ));
        }
    }

    // 2. Every top-level type that reflection says holds something has a table.
    for info in registry.types().filter(|i| i.is_top_level && !i.is_object_ref) {
        let key = (
            info.package.to_ascii_lowercase(),
            info.version.to_string(),
            info.name.to_ascii_lowercase(),
        );
        if by_key.contains_key(&key) {
            continue;
        }
        let want_refs = WITH_REFERENCES.contains(&info.package);
        let want = expected_for(info, want_refs, &reach);
        if !want.contained.is_empty() || want_refs {
            out.push(format!(
                "{}: no table, and reflection finds {} containment slot(s)",
                qualified(info),
                want.contained.len()
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// **The generated tables are the facts reflection finds**, for every
/// generated crate at once: no slot missing, none invented, each pointing at a
/// real field of the type it names, with that field's cardinality, and every
/// type that holds something has a table.
///
/// Break it: delete one `ContainedSlot` from a generated `relations.rs`.
#[test]
fn every_table_agrees_with_the_reflection() {
    let registry = registry();
    let tables = tables(&registry);
    assert!(
        tables.iter().any(|t| t.qualified_type == "witsml21.Log"),
        "no table for witsml21.Log: relations are not registered"
    );
    assert!(tables.len() > 100, "only {} tables", tables.len());
    let found = problems(&registry, &tables);
    assert!(
        found.is_empty(),
        "{} problem(s):\n{}",
        found.len(),
        found.join("\n")
    );
}

fn leak<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}

fn replace(
    tables: &[&'static TypeRelations],
    qualified_type: &str,
    change: impl Fn(&TypeRelations) -> TypeRelations,
) -> Vec<&'static TypeRelations> {
    let mut found = false;
    let out = tables
        .iter()
        .map(|t| {
            if t.qualified_type == qualified_type {
                found = true;
                let changed: &'static TypeRelations = Box::leak(Box::new(change(t)));
                changed
            } else {
                *t
            }
        })
        .collect();
    assert!(found, "no table {qualified_type} to change");
    out
}

/// The checker must be able to fail: one slot removed from `witsml21.Log` is
/// named. Break it: make `problems` skip the `difference` of the expected set.
#[test]
fn a_removed_slot_is_reported() {
    let registry = registry();
    let tables = replace(&tables(&registry), "witsml21.Log", |t| {
        let kept: Vec<ContainedSlot> = t
            .contains
            .iter()
            .filter(|c| dotted(c.path) != "ChannelSet")
            .copied()
            .collect();
        assert!(
            kept.len() < t.contains.len(),
            "Log has no ChannelSet slot to remove"
        );
        TypeRelations {
            contains: leak(kept),
            ..*t
        }
    });
    let found = problems(&registry, &tables);
    assert!(
        found.iter().any(|p| p.contains("witsml21.Log")
            && p.contains("ChannelSet")
            && p.contains("the table does not")),
        "{found:?}"
    );
}

/// A slot the schema does not have is named, whether the path is wrong or the
/// declared type is. Break it: stop checking `declared` in
/// `check_contained_slot`.
#[test]
fn an_invented_or_altered_slot_is_reported() {
    let registry = registry();
    let base = tables(&registry);

    let invented = replace(&base, "witsml21.Log", |t| {
        let mut slots = t.contains.to_vec();
        slots.push(ContainedSlot {
            path: leak(vec![Step::new("NoSuchElement")]),
            ..t.contains[0]
        });
        TypeRelations {
            contains: leak(slots),
            ..*t
        }
    });
    let found = problems(&registry, &invented);
    assert!(
        found
            .iter()
            .any(|p| p.contains("NoSuchElement") && p.contains("no such field")),
        "{found:?}"
    );

    let altered = replace(&base, "witsml21.Log", |t| {
        let slots: Vec<ContainedSlot> = t
            .contains
            .iter()
            .map(|c| ContainedSlot {
                declared: "witsml21.Wellbore",
                ..*c
            })
            .collect();
        TypeRelations {
            contains: leak(slots),
            ..*t
        }
    });
    let found = problems(&registry, &altered);
    assert!(
        found.iter().any(|p| p.contains("witsml21.Log")),
        "a slot declared as another type must be reported: {found:?}"
    );
}

/// A type that holds something and has no table is named. Break it: skip
/// part 2 of `problems`.
#[test]
fn a_missing_table_is_reported() {
    let registry = registry();
    let tables: Vec<&'static TypeRelations> = tables(&registry)
        .into_iter()
        .filter(|t| t.qualified_type != "witsml21.Log")
        .collect();
    let found = problems(&registry, &tables);
    assert!(
        found
            .iter()
            .any(|p| p.contains("witsml21.Log") && p.contains("no table")),
        "{found:?}"
    );
}

/// The questions the registry answers, on real types.
/// Break it: register no relations in `witsml-2-1`'s `register`.
#[test]
fn the_registry_answers_the_container_questions() {
    let registry = registry();
    assert!(registry.is_container("witsml21.Log"));
    assert!(registry.is_container("witsml21.ChannelSet"));
    assert!(
        !registry.is_container("witsml21.Wellbore"),
        "Wellbore holds no data object by value"
    );
    assert!(registry.may_be_contained("witsml21.Channel"));
    assert!(registry.may_be_contained("witsml21.PPFGChannel"));
    assert!(!registry.may_be_contained("witsml21.Log"));
    assert!(!registry.may_be_contained("witsml21.Wellbore"));

    let log = registry
        .types()
        .find(|i| i.package == "witsml" && i.version == "2.1" && i.name == "Log")
        .expect("witsml 2.1 Log is registered");
    let table = registry.relations_of(log).expect("a table for Log");
    assert_eq!(table.qualified_type, "witsml21.Log");
    let slot = table
        .contains
        .iter()
        .find(|c| dotted(c.path) == "ChannelSet")
        .expect("Log holds ChannelSet");
    assert_eq!(slot.declared, "witsml21.ChannelSet");
    assert_eq!(slot.alternatives, &["witsml21.PPFGChannelSet"]);
    assert!(!slot.abstract_bound);
    assert!(slot.admits("witsml21.PPFGChannelSet"));
}

/// A type that a table of its **own package** says is contained somewhere is
/// flagged `nestable`: within one package, the table is a subset of what
/// `mark_nestable` finds. (The converse does not hold: `nestable`
/// over-approximates, for instance for a type held only by a type that has no
/// element of its own. And across packages it says nothing: `nestable` is
/// computed per crate, so `eml22.TimeSeries`, held by value by RESQML
/// 2.2_dev3's `LocalGridSet.Activation`, is not flagged in eml 2.2.) Break it:
/// add a slot of a package for a type of that package that is not nestable.
#[test]
fn what_may_be_contained_is_nestable() {
    let registry = registry();
    // Keyed by package and version too: a release and its development drop
    // share their qualified types (`resqml22.RockVolumeFeature`), and only the
    // drop has the dictionary that holds the feature by value.
    let nestable: HashMap<(String, String, String), bool> = registry
        .types()
        .filter(|i| i.is_top_level)
        .map(|i| {
            (
                (i.package.to_string(), i.version.to_string(), qualified(i)),
                i.is_nestable,
            )
        })
        .collect();
    let mut checked = 0;
    for table in tables(&registry) {
        let package = table.qualified_type.split('.').next().unwrap_or("");
        for slot in table.contains {
            let mut types: Vec<&str> = slot.alternatives.to_vec();
            if !slot.abstract_bound {
                types.push(slot.declared);
            }
            for contained in types {
                if contained.split('.').next() != Some(package) {
                    continue;
                }
                checked += 1;
                let key = (
                    table.package.to_string(),
                    table.version.to_string(),
                    contained.to_string(),
                );
                assert_eq!(
                    nestable.get(&key),
                    Some(&true),
                    "{contained}, held by {} at {}, is not flagged nestable",
                    table.qualified_type,
                    slot.dotted_path()
                );
            }
        }
    }
    assert!(checked > 0, "no same-package containment was checked");
}

/// For WITSML 2.0 the targets of the tables are **exactly** the nestable
/// types of the crate (11 of 11, checked by hand against the schemas). Held as an
/// equality, because it is the one package where the two can be compared
/// whole. Break it: drop a slot of WITSML 2.0.
#[test]
fn witsml_2_0_targets_are_exactly_its_nestable_types() {
    let registry = registry();
    let mut targets: BTreeSet<String> = BTreeSet::new();
    for table in registry
        .relations()
        .filter(|t| t.package == "witsml" && t.version == "2.0")
    {
        for slot in table.contains {
            targets.insert(slot.declared.to_string());
            targets.extend(slot.alternatives.iter().map(|s| (*s).to_string()));
        }
    }
    let nestable: BTreeSet<String> = registry
        .types()
        .filter(|i| i.package == "witsml" && i.version == "2.0" && i.is_top_level && i.is_nestable)
        .map(qualified)
        .collect();
    assert_eq!(targets, nestable);
}

/// RESQML 2.0.1 holds nothing by value: its table is empty and no RESQML
/// 2.0.1 type may be contained, so reading such a package never needs a part
/// more. Break it: let the
/// generator count a nested struct as a data object.
#[test]
fn resqml_2_0_1_has_no_containment() {
    let registry = registry();
    assert_eq!(
        registry
            .relations()
            .filter(|t| t.package == "resqml" && t.version == "2.0.1")
            .map(|t| t.contains.len())
            .sum::<usize>(),
        0
    );
    for info in registry
        .types()
        .filter(|i| i.package == "resqml" && i.version == "2.0.1" && i.is_top_level)
    {
        assert!(
            !registry.may_be_contained(&qualified(info)),
            "{}",
            qualified(info)
        );
        assert!(!registry.is_container(&qualified(info)), "{}", qualified(info));
    }
}

/// A development drop shares its release's qualified types and keeps tables of
/// its own: they differ, and the release answers by qualified type. Break it:
/// key the index on the qualified type alone and the drop displaces the release.
#[test]
fn a_development_drop_has_its_own_tables() {
    let registry = registry();
    let find = |version: &str| {
        registry
            .types()
            .find(|i| i.package == "resqml" && i.version == version && i.name == "ColorMapDictionary")
            .unwrap_or_else(|| panic!("ColorMapDictionary of resqml {version}"))
    };
    let drop = registry
        .relations_of(find("2.2_dev3"))
        .expect("a table for the drop");
    let release = registry
        .relations_of(find("2.2"))
        .expect("a table for the release");
    assert_eq!(drop.contains.len(), 2, "two separate elements in the drop");
    assert_eq!(release.contains.len(), 1, "one abstract element in the release");
    assert!(release.contains[0].abstract_bound);
    assert_eq!(
        registry
            .relations_for("resqml22.ColorMapDictionary")
            .map(|t| t.version),
        Some("2.2"),
        "the release answers by qualified type"
    );
}

/// WITSML's `Log.Wellbore` is mandatory in 2.0 and optional in 2.1 (the
/// schemas' `minOccurs`), and the reference table says so per version.
/// Break it: compute `required` on the leaf alone, or share one table between
/// the two versions.
#[test]
fn log_wellbore_is_required_in_2_0_and_not_in_2_1() {
    let registry = registry();
    let required = |version: &str| {
        let log = registry
            .types()
            .find(|i| i.package == "witsml" && i.version == version && i.name == "Log")
            .unwrap_or_else(|| panic!("witsml {version} Log"));
        registry
            .relations_of(log)
            .expect("a table for Log")
            .refers
            .iter()
            .find(|d| dotted(d.path) == "Wellbore")
            .unwrap_or_else(|| panic!("Log.Wellbore is a reference slot in {version}"))
            .required
    };
    assert!(required("2.0"));
    assert!(!required("2.1"));
}
