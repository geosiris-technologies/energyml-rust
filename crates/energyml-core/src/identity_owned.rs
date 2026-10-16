//! References, attributed to the data object that **writes** them.
//!
//! [`identity::collect_refs`] walks a whole tree and gives every reference to
//! the root, whatever sits between. For an object that holds other data objects
//! by value that is wrong: a WITSML `Log` holds `ChannelSet`s which hold
//! `Channel`s, each a data object with a `uuid` of its own (ETP 1.2 section
//! 9.1.3, p. 178), and the reference a `Channel` writes to its `Wellbore`
//! (`Log.xsd` l. 401 in WITSML 2.1) is the `Channel`'s, not the `Log`'s. The
//! source of a relationship is the object that holds the reference (ETP 1.2
//! section 8.1.1.1.1, p. 156); attributing it to the `Log` would state an edge
//! nobody wrote, and its kind (`Channel` to `Wellbore` is the secondary
//! example of section 8.1.1.1.2, p. 157) would be judged on the wrong type.
//!
//! This walk **stops at nested data objects**: when it meets one it opens a new
//! owner, and the references below it are its. A nested data object is what
//! [`identity::nested_objects`] calls one (a type that can be a part and has a
//! non-empty uuid), so the two agree on where the owners are.
//!
//! Paths are **relative to their owner** (`Wellbore`, `DerivedFrom[1]`), which
//! is what a rule keyed on `(owner type, path)` reads. The owner's own path
//! from the root is kept beside them.
//!
//! [`identity::collect_refs`]: crate::identity::collect_refs
//! [`identity::nested_objects`]: crate::identity::nested_objects

use crate::identity::{NestedObject, RefEdge, key_of_reflect, refs_in_source};
use crate::value::{FieldValue, Reflect};

/// The references one data object writes.
#[derive(Clone, Debug)]
pub struct OwnedRefs<'a> {
    /// The data object that writes them: `None` for the root, the nested
    /// object (with its path from the root) otherwise.
    pub owner: Option<NestedObject<'a>>,
    /// What it references, in document order. Each `path` is relative to the
    /// owner.
    pub refs: Vec<RefEdge<'a>>,
}

/// The references the **root** writes itself: those of every nested data
/// object are left out.
///
/// For an object with nothing nested this is [`identity::collect_refs`]
/// exactly.
///
/// [`identity::collect_refs`]: crate::identity::collect_refs
pub fn collect_refs_owned(root: &dyn Reflect) -> Vec<RefEdge<'_>> {
    collect_refs_by_owner(root)
        .into_iter()
        .next()
        .map(|group| group.refs)
        .unwrap_or_default()
}

/// Every reference of an object, grouped by the data object that writes it.
///
/// The first group is the root (`owner` is `None`, present even when it writes
/// nothing); then one group per nested data object, in document order, at any
/// depth. Nothing is lost against [`identity::collect_refs`]: the references of
/// all the groups are exactly its references.
///
/// [`identity::collect_refs`]: crate::identity::collect_refs
pub fn collect_refs_by_owner(root: &dyn Reflect) -> Vec<OwnedRefs<'_>> {
    let mut groups = vec![OwnedRefs {
        owner: None,
        refs: Vec::new(),
    }];
    walk(root, String::new(), String::new(), 0, &mut groups);
    groups
}

/// `a.b`, or `b` when `prefix` is empty.
fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// `full` is the path from the root, `relative` the path from the owner
/// `groups[owner]`.
fn walk<'a>(
    object: &'a dyn Reflect,
    full: String,
    relative: String,
    owner: usize,
    groups: &mut Vec<OwnedRefs<'a>>,
) {
    for field in object.type_info().fields {
        let Some(value) = object.field(field.name) else {
            continue;
        };
        visit(
            value,
            join(&full, field.xml_name),
            join(&relative, field.xml_name),
            field.ty.type_info(),
            owner,
            groups,
        );
    }
}

fn visit<'a>(
    value: FieldValue<'a>,
    full: String,
    relative: String,
    expected: Option<&'static crate::reflect::TypeInfo>,
    owner: usize,
    groups: &mut Vec<OwnedRefs<'a>>,
) {
    match value {
        FieldValue::Object(child) => {
            let info = child.type_info();
            if info.is_object_ref {
                if let Some(target) = key_of_reflect(child) {
                    groups[owner].refs.push(RefEdge {
                        path: relative,
                        target,
                        expected,
                    });
                }
            } else if let Some(any) = child.as_any().downcast_ref::<crate::any_xml::AnyXml>() {
                // The model could not type this subtree, so its references are
                // in the text; they are the owner's, as the subtree is.
                refs_in_source(&any.content, &relative, &mut groups[owner].refs);
            } else if info.is_top_level
                && let Some(key) = key_of_reflect(child)
                && !key.uuid.trim().is_empty()
            {
                // The test `nested_objects` applies: a data object of its own.
                groups.push(OwnedRefs {
                    owner: Some(NestedObject {
                        path: full.clone(),
                        key,
                    }),
                    refs: Vec::new(),
                });
                let nested = groups.len() - 1;
                walk(child, full, String::new(), nested, groups);
            } else {
                walk(child, full, relative, owner, groups);
            }
        }
        FieldValue::List(items) => {
            for (i, item) in items.into_iter().enumerate() {
                visit(
                    item,
                    format!("{full}[{i}]"),
                    format!("{relative}[{i}]"),
                    expected,
                    owner,
                    groups,
                );
            }
        }
        _ => {}
    }
}

#[cfg(all(test, feature = "derive"))]
mod tests {
    use super::*;
    use crate::identity::collect_refs;
    use crate::prelude::*;

    // A slice of WITSML 2.1 shaped like Log.xsd: a `Log` holds `ChannelSet`s
    // (l. 822) which hold `Channel`s (l. 650); each is a data object with a
    // uuid and each writes a `Wellbore` reference (l. 401, 652, 823). `Channel`
    // also writes a list of references (`DerivedFrom`, l. 398).

    /// An eml 2.3 reference: identity in child elements.
    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(
        package = "eml",
        version = "2.3",
        xml_name = "DataObjectReference",
        object_ref
    )]
    struct Dor {
        #[energyml(elem, xml_name = "Uuid")]
        uuid: String,
        #[energyml(elem, xml_name = "QualifiedType")]
        qualified_type: String,
        #[energyml(elem, xml_name = "Title")]
        title: String,
        #[energyml(elem, xml_name = "ObjectVersion")]
        object_version: Option<String>,
    }

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(
        package = "witsml",
        version = "2.1",
        xml_name = "Channel",
        top_level,
        nestable,
        content_type = "application/x-witsml+xml;version=2.1;type=Channel",
        qualified_type = "witsml21.Channel"
    )]
    struct Chan {
        #[energyml(attr, xml_name = "uuid")]
        uuid: String,
        #[energyml(elem, xml_name = "DerivedFrom")]
        derived_from: Vec<Dor>,
        #[energyml(elem, xml_name = "Wellbore")]
        wellbore: Option<Dor>,
    }

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(
        package = "witsml",
        version = "2.1",
        xml_name = "ChannelSet",
        top_level,
        nestable,
        content_type = "application/x-witsml+xml;version=2.1;type=ChannelSet",
        qualified_type = "witsml21.ChannelSet"
    )]
    struct ChanSet {
        #[energyml(attr, xml_name = "uuid")]
        uuid: String,
        #[energyml(elem, xml_name = "Channel")]
        channel: Vec<Chan>,
        #[energyml(elem, xml_name = "Wellbore")]
        wellbore: Option<Dor>,
    }

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(
        package = "witsml",
        version = "2.1",
        xml_name = "Log",
        top_level,
        content_type = "application/x-witsml+xml;version=2.1;type=Log",
        qualified_type = "witsml21.Log"
    )]
    struct LogObj {
        #[energyml(attr, xml_name = "uuid")]
        uuid: String,
        #[energyml(elem, xml_name = "ChannelSet")]
        channel_set: Vec<ChanSet>,
        #[energyml(elem, xml_name = "Wellbore")]
        wellbore: Option<Dor>,
    }

    fn dor(name: &str) -> Dor {
        Dor {
            uuid: name.to_string(),
            qualified_type: "witsml21.Wellbore".to_string(),
            title: String::new(),
            object_version: None,
        }
    }

    /// Log (wellbore `log`) holding set `s0` (wellbore `s0w`) holding channel
    /// `c0` (wellbore `c0w`, derived from `d0`, `d1`), and set `s1` holding a
    /// channel `c1` that writes nothing.
    fn sample() -> LogObj {
        LogObj {
            uuid: "log-uuid".into(),
            wellbore: Some(dor("log")),
            channel_set: vec![
                ChanSet {
                    uuid: "s0-uuid".into(),
                    wellbore: Some(dor("s0w")),
                    channel: vec![Chan {
                        uuid: "c0-uuid".into(),
                        wellbore: Some(dor("c0w")),
                        derived_from: vec![dor("d0"), dor("d1")],
                    }],
                },
                ChanSet {
                    uuid: "s1-uuid".into(),
                    wellbore: None,
                    channel: vec![Chan {
                        uuid: "c1-uuid".into(),
                        wellbore: None,
                        derived_from: vec![],
                    }],
                },
            ],
        }
    }

    fn targets(edges: &[RefEdge<'_>]) -> Vec<String> {
        edges.iter().map(|edge| edge.target.uuid.to_string()).collect()
    }

    /// The walk stops at the nested data objects: the `Log` writes one
    /// reference, its own; those of the `ChannelSet`s and `Channel`s are theirs.
    ///
    /// Break it: recurse into nested objects with the root as owner (the
    /// walk of `collect_refs`), and the `Log` writes five references.
    #[test]
    fn the_root_owns_only_the_references_it_writes() {
        let log = sample();
        let owned = collect_refs_owned(&log);
        assert_eq!(targets(&owned), ["log"]);
        assert_eq!(owned[0].path, "Wellbore");
        assert_eq!(
            collect_refs(&log).len(),
            5,
            "the whole-tree walk gives all of them to the root; that is the defect"
        );
    }

    /// Each nested data object owns its references, with paths relative to
    /// itself and its own path from the root alongside.
    ///
    /// Break it: keep the root-relative path on a nested reference, and
    /// `c0`'s `DerivedFrom[1]` reads `ChannelSet[0].Channel[0].DerivedFrom[1]`.
    #[test]
    fn each_nested_object_owns_its_references_relative_to_itself() {
        let log = sample();
        let groups = collect_refs_by_owner(&log);
        assert_eq!(groups.len(), 5, "the root, two sets, two channels");

        assert!(groups[0].owner.is_none());

        let owner_paths: Vec<String> = groups
            .iter()
            .skip(1)
            .map(|g| g.owner.as_ref().expect("a nested owner").path.clone())
            .collect();
        assert_eq!(
            owner_paths,
            [
                "ChannelSet[0]",
                "ChannelSet[0].Channel[0]",
                "ChannelSet[1]",
                "ChannelSet[1].Channel[0]"
            ]
        );
        let owner_uuids: Vec<&str> = groups
            .iter()
            .skip(1)
            .map(|g| g.owner.as_ref().expect("a nested owner").key.uuid)
            .collect();
        assert_eq!(owner_uuids, ["s0-uuid", "c0-uuid", "s1-uuid", "c1-uuid"]);
        assert_eq!(
            groups[1].owner.as_ref().expect("owner").key.qualified_type(),
            "witsml21.ChannelSet"
        );

        let channel = &groups[2];
        assert_eq!(targets(&channel.refs), ["d0", "d1", "c0w"]);
        let paths: Vec<&str> = channel.refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["DerivedFrom[0]", "DerivedFrom[1]", "Wellbore"]);

        assert_eq!(targets(&groups[1].refs), ["s0w"]);
        assert!(
            groups[3].refs.is_empty(),
            "a set that writes nothing owns nothing"
        );
        assert!(groups[4].refs.is_empty());
    }

    /// Nothing is lost and nothing is made up: the references of all the
    /// groups are those of `collect_refs`, and an owner's path joined to a
    /// relative path is the whole-tree path.
    ///
    /// Break it: drop the references of the last group (a walk that forgets to
    /// return to the owner), or reset the relative path wrongly, and the
    /// joined paths no longer match.
    #[test]
    fn the_groups_together_are_the_whole_tree_walk() {
        let log = sample();
        let mut whole: Vec<(String, String)> = collect_refs(&log)
            .iter()
            .map(|e| (e.path.clone(), e.target.uuid.to_string()))
            .collect();
        let mut grouped: Vec<(String, String)> = collect_refs_by_owner(&log)
            .iter()
            .flat_map(|group| {
                let prefix = group.owner.as_ref().map(|o| o.path.clone()).unwrap_or_default();
                group.refs.iter().map(move |edge| {
                    let path = if prefix.is_empty() {
                        edge.path.clone()
                    } else {
                        format!("{prefix}.{}", edge.path)
                    };
                    (path, edge.target.uuid.to_string())
                })
            })
            .collect();
        whole.sort();
        grouped.sort();
        assert_eq!(grouped, whole);
    }

    /// A nested element with no uuid is no data object: what it writes is its
    /// parent's, as `nested_objects` reads it, while the channel below it, which
    /// has a uuid, still opens an owner of its own.
    ///
    /// Break it: open an owner for any nested top-level type, and the set with
    /// an empty uuid takes its wellbore reference from the `Log`.
    #[test]
    fn a_nested_element_without_a_uuid_is_not_an_owner() {
        let mut log = sample();
        log.channel_set[0].uuid = String::new();
        let groups = collect_refs_by_owner(&log);
        assert_eq!(
            groups.len(),
            4,
            "the root, one channel under the anonymous set, the second set and its channel"
        );
        // Document order: the set comes before the `Log`'s own `Wellbore`
        // (Log.xsd l. 822 then 823).
        assert_eq!(targets(&groups[0].refs), ["s0w", "log"]);
        assert_eq!(groups[0].refs[0].path, "ChannelSet[0].Wellbore");
        let channel = groups[1].owner.as_ref().expect("the channel is an owner");
        assert_eq!(channel.key.uuid, "c0-uuid");
        assert_eq!(channel.path, "ChannelSet[0].Channel[0]");
    }

    /// An object with nothing nested: `collect_refs_owned` is `collect_refs`.
    ///
    /// Break it: filter on anything but ownership, and a plain object loses a
    /// reference.
    #[test]
    fn an_object_with_nothing_nested_is_unchanged() {
        let log = LogObj {
            uuid: "alone".into(),
            wellbore: Some(dor("w")),
            channel_set: vec![],
        };
        let owned = collect_refs_owned(&log);
        let whole = collect_refs(&log);
        assert_eq!(targets(&owned), targets(&whole));
        assert_eq!(owned[0].path, whole[0].path);
        assert_eq!(collect_refs_by_owner(&log).len(), 1);
    }
}
