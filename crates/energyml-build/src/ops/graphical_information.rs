//! `graphical-information` — the colours a package chooses: one
//! `eml23.GraphicalInformationSet` whose entries give objects a constant
//! colour (`DefaultGraphicalInformation`) and draw properties through a colour
//! map (`ColorInformation`).
//!
//! The graphical-information step of the synthetic reference model's build plan,
//! and its cascade: the same colour on the feature,
//! the interpretation and the representation of one horizon. Until this module,
//! the one operation writing a graphical information set was the BRGM chart
//! import, which writes the chart's own colours in a set of its own
//! (`ops/stratigraphy.rs`).
//!
//! # One writer of an entry, and it is the domain's
//!
//! A `GraphicalInformationSet` is an EML 2.3 object whose entries are RESQML
//! 2.2 classes — *« Currently, only RESQML v2.2 specifies detailed graphical
//! information »* (common v2.3 `GraphicalInformation.xsd` l. 13) — so the
//! generated field is a subtree (`AnyXml`), and writing an entry means writing
//! that subtree. `energyml_domain::color::constant_colour_entry` and
//! `colour_map_entry` do exactly that, against the reader the rest of the
//! workspace uses (`GraphicalSet::read`): this module calls them and writes no
//! XML of its own. A second writer would be a second place for the same
//! errors — an operation that writes the same object as another delegates to it (ADR-0017).
//!
//! What that writer fixes is therefore fixed here too, and said in the plan:
//! `ViewerKind` is `3d` and `IsVisible` true on every constant colour.
//!
//! # What is written
//!
//! - **a constant colour per object of `targets`**, one
//!   `DefaultGraphicalInformation` each, in their order: *« Either for
//!   Feature, Interp or representation, marker »* (RESQML TRG 2.2 §4.10
//!   p. 61). The cascade is three of them with the same `HsvColor`
//!   — the reference model's ×3 — and **one entry per object** is what
//!   keeps each one recolourable alone: `energyml_domain::color::set_constant_colour`
//!   refuses to recolour one object of a shared entry, rightly, since it would
//!   take the colour from the others;
//! - **a collection is a target too** (the reference model's fallback case): the
//!   standard's list is *« Feature, Interp or representation, marker »* (TRG 2.2
//!   §4.10 p. 61) but `TargetObject` is a plain reference
//!   (`GraphicalInformation.xsd` l. 19), and the house's inheritance order reads
//!   a colour **on a collection** for every object it holds
//!   (the inheritance-resolution rules, ranks 2 and 4).
//!   The writer is the same; what the standard leaves unsaid is what the
//!   walk of `energyml_domain::color_inherit` decides;
//! - **`shared`** writes one entry naming every object instead —
//!   `TargetObject` is `1..unbounded` in eml 2.3 (`GraphicalInformation.xsd`
//!   l. 19) — and then takes one colour;
//! - **a colour map on the objects of `mapped`**, one `ColorInformation` each
//!   (or one for all under `shared`), with its `MinMax`, its two required flags
//!   and its optional `ValueVectorIndex` (`GraphicalInformationObject.xsd`
//!   l. 257–288). *« Used for properties and property kinds and for geometry.
//!   In the latter case, we need to point to the representation »* (TRG 2.2
//!   §4.5 p. 56). The map is an object that **exists** — the writer's own rule
//!   (a rule of the colours design); `colormap` makes one.
//!
//! The constant colours come first, then the colour maps, each in the order
//! given: the order of a set is part of its meaning (`Active*InformationIndex`
//! counts positions), so it is the caller's and never sorted here.
//!
//! # The form a colour takes: `#rrggbb` or `#rrggbbaa`, and nothing else
//!
//! The standard stores HSV and has no RGB type; the domain writer takes an
//! [`Rgba`] and converts it with `Hsv::from_rgba`, *the one conversion*. So
//! one form is accepted, the one every colour picker gives and the one
//! `Rgba::to_hex` prints. An HSV text is **not** accepted: through that writer
//! it would go to RGB and back, and the four numbers written would not be the
//! four typed — a neighbour colour, written without a word. The day the domain
//! writes an `HsvColor` as given, a second form can join this slot.
//!
//! A colour that does not read is **refused by name**, every one of them at
//! once — never replaced by a default colour, which would be a colour nobody
//! chose that looks exactly like one somebody did.
//!
//! # What is refused
//!
//! - nothing to colour; a colour map with nothing to draw, or objects to draw
//!   and no map (a `ColorInformation` without a map names no colour);
//! - an object of the wrong kind **on its slot**: a constant colour on
//!   something that is no feature, interpretation, representation, marker or
//!   collection (TRG 2.2 §4.10 p. 61, and the house's collection) — a property takes its colours through a map; a
//!   colour map on something that is no property, property kind or
//!   representation (§4.5 p. 56); a `colourMap` that is no
//!   `ContinuousColorMap` or `DiscreteColorMap`, the schema's only two (TRG 2.2
//!   §4.1 p. 51, `ColorInformation.ColorMap` → `AbstractColorMap`);
//! - as many colours as neither one nor the objects; one object given twice
//!   in two colours — two statements about one object in one set is a question
//!   the set would leave to its reader (`set_constant_colour`'s own reason);
//! - an element scope (`nodes`, `edges`, `faces`, `volumes`) on what is no
//!   representation: indexable elements are a representation's — *« Each
//!   property is "attached" to the indexable elements of a representation »*
//!   (TRG 2.2 §8 p. 232);
//! - a setting of a map without a map; `minimum` without `maximum` or the
//!   reverse (`MinMax` carries both, `GraphicalInformationObject.xsd`
//!   l. 579–592); a range whose minimum is not below its maximum — an interval
//!   of nothing, and `reverse` is how the colours go the other way; a
//!   logarithmic mapping over a range that reaches zero or below — *« the log
//!   of the property values »* (l. 269–273) has no value there; a
//!   `valueVectorIndex` past the last component of a property that states its
//!   count (`KnownProperty::value_count`) — *an index into the data*, never
//!   into the map (`energyml_domain::color`, module documentation).
//!
//! # What is not written, and why
//!
//! - `AlphaInformation`, `SizeInformation`, `AnnotationInformation`,
//!   `ContourLineSetInformation`: no writer in the
//!   domain, and its reader keeps them as `Information::Other`. Writing them
//!   here would be the second writer this module refuses to be;
//! - `ActiveColorInformationIndex`: a position in the set, and nothing here
//!   says which constant colour a map should override;
//! - **the chart's colours** (already written by the GeolTs import):
//!   `stratigraphy-from-geolts` writes them in a set
//!   whose uuid comes from the selection, a statement of the register and not
//!   a choice of the package. Copying them here would be two sources for one
//!   colour. The model therefore has two
//!   sets, not one.
//!
//! # Identity
//!
//! The same content is the same uuid: the title and every entry — each
//! object's uuid and its colour as `#rrggbbaa`, the scope, `shared`; the map,
//! its objects, its range, its flags and its component. `#FF0000` and
//! `#ff0000ff` are one colour, so one set. Another colour is another set, and
//! `apply` says a second one titled alike was written beside the first.

use energyml_core::any_xml::AnyXml;
use energyml_domain::color::{self, Decorated, Rgba, Scaling, Scope};
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use serde_json::json;

use crate::declare::{Choice, Inputs, ObjectIn, Params, Suggestions, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Need, NewObject, Note, Operation, Plan, Refusal, Slot, Takes,
};
use crate::pieces::Pieces;

/// The operation.
pub struct GraphicalInformation;

/// What of an object a constant colour covers — the concrete class of the
/// `IndexableElementInfo` (`GraphicalInformationObject.xsd` l. 401), one of the
/// five `AbstractGraphicalInformationForIndexableElement` the schema has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Covers {
    /// The whole object — `GraphicalInformationForWholeObject`.
    WholeObject,
    /// Its nodes — `GraphicalInformationForNodes`.
    Nodes,
    /// Its edges — `GraphicalInformationForEdges`.
    Edges,
    /// Its faces — `GraphicalInformationForFaces`.
    Faces,
    /// Its volumes — `GraphicalInformationForVolumes`.
    Volumes,
}

impl Covers {
    /// The domain's word for it — the one `constant_colour_entry` writes from.
    fn scope(self) -> Scope {
        match self {
            Covers::WholeObject => Scope::WholeObject,
            Covers::Nodes => Scope::Nodes,
            Covers::Edges => Scope::Edges,
            Covers::Faces => Scope::Faces,
            Covers::Volumes => Scope::Volumes,
        }
    }
}

/// What `graphical-information` is told.
#[derive(Debug, Params)]
pub struct GraphicalInformationParams {
    /// the set's title
    pub title: Title,
    /// the colours of `targets`, #rrggbb or #rrggbbaa — one for all of them, or one per object in their order
    ///
    /// RGB because that is what the domain writer takes; see the module.
    #[param(ordered)]
    pub colours: Vec<String>,
    /// what of each object the colour covers: the whole object, or the nodes, edges, faces or volumes of a representation
    #[param(default = Covers::WholeObject, optional)]
    pub scope: Covers,
    /// one entry naming every object, rather than one entry per object — one colour then
    #[param(default = false, optional)]
    pub shared: bool,
    /// with a colour map: the property value drawn with the map's lowest colour — lower values too
    pub minimum: Option<f64>,
    /// with a colour map: the property value drawn with the map's highest colour — higher values too
    pub maximum: Option<f64>,
    /// with a colour map: place the logarithm of the values on it
    #[param(default = false, optional)]
    pub logarithmic: bool,
    /// with a colour map: the lowest value takes the map's highest colour
    #[param(default = false, optional)]
    pub reverse: bool,
    /// with a colour map: the component of a vector property that is drawn, from 0
    #[param(min = 0.0)]
    pub value_vector_index: Option<i64>,
}

/// What `graphical-information` takes.
#[derive(Debug, Inputs)]
pub struct GraphicalInformationInputs {
    /// the objects a constant colour goes on — features, interpretations, representations, markers, collections — in the order of `colours`
    ///
    /// No type is proposed first: every feature, interpretation and
    /// representation of the package is a candidate, and the refusal of
    /// anything else is the operation's, by name.
    #[input(ordered)]
    pub targets: Vec<ObjectIn>,
    /// the colour map the objects of `mapped` are drawn through
    #[input(types = "resqml22.ContinuousColorMap, resqml22.DiscreteColorMap")]
    pub colour_map: Option<ObjectIn>,
    /// the properties, property kinds or representations drawn through `colourMap`
    #[input(types = "resqml22.ContinuousProperty, resqml22.DiscreteProperty, eml23.PropertyKind")]
    pub mapped: Vec<ObjectIn>,
}

/// The two colour maps the schema has — the same two the `colourMap` slot
/// proposes first (TRG 2.2 §4.1 p. 51: `ColorInformation.ColorMap` →
/// `AbstractColorMap`, whose concrete classes are these).
const COLOUR_MAPS: &[&str] = &["resqml22.ContinuousColorMap", "resqml22.DiscreteColorMap"];

impl Operation for GraphicalInformation {
    type Params = GraphicalInformationParams;
    type Inputs = GraphicalInformationInputs;

    fn about(&self) -> About {
        About {
            name: "graphical-information",
            describe: "a graphical information set — a constant colour on features, interpretations and representations, and properties drawn through a colour map",
            category: Category::Properties,
            makes: &["eml23.GraphicalInformationSet"],
            recipe: "graphical-information",
            represents: None,
        }
    }

    /// The title, from the first object coloured: a suggestion reads the bag
    /// and nothing else (ADR-0017).
    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let pieces = crate::declare::paired(bag);
        let first = ["targets", "mapped"]
            .iter()
            .find_map(|slot| pieces.iter().find(|piece| piece.name == *slot))
            .and_then(|piece| piece.known)
            .and_then(|known| known.reference.as_ref());
        if let Some(reference) = first {
            slots.set("title", json!(format!("{} — colours", reference.title)));
        }
    }

    /// What the inputs alone say: something to colour, a map and what it
    /// draws together, and each object of a kind its slot takes.
    ///
    /// The messages name the class and never a release: this is an aptitude,
    /// and `no_unfitness_names_a_release` holds every `Unfit` to it.
    fn check(&self, inputs: &GraphicalInformationInputs) -> Result<(), Refusal> {
        if inputs.targets.is_empty() && inputs.mapped.is_empty() && inputs.colour_map.is_none() {
            return Err(Refusal::on(
                "targets",
                "nothing to colour: give the objects a constant colour goes on to `targets`, or a colour map to `colourMap` and what it draws to `mapped`",
            ));
        }
        match (&inputs.colour_map, inputs.mapped.is_empty()) {
            (None, false) => {
                return Err(Refusal::on(
                    "colourMap",
                    format!(
                        "{} object(s) to draw through a colour map, and no colour map: a colour information without one names no colour",
                        inputs.mapped.len()
                    ),
                ));
            }
            (Some(map), true) => {
                // The map's own title: `name` is the slot that received it.
                let title = map.known.reference.as_ref().map_or(map.name.as_str(), |reference| reference.title.as_str());
                return Err(Refusal::on(
                    "mapped",
                    format!(
                        "the colour map `{title}` is given and nothing is drawn through it: an entry colours one object at least"
                    ),
                ));
            }
            _ => {}
        }
        for target in &inputs.targets {
            let reference = decorated("targets", target)?;
            if !decorable(&reference.qualified_type) {
                return Err(Refusal::on(
                    "targets",
                    format!(
                        "`{}` is a {}: a constant colour goes on a feature, an interpretation, a representation, a marker or a collection{}",
                        reference.title,
                        class_of(&reference.qualified_type),
                        if mappable(&reference.qualified_type) {
                            " — a property takes its colours through a colour map, in `mapped`"
                        } else {
                            ""
                        }
                    ),
                ));
            }
        }
        for one in &inputs.mapped {
            let reference = decorated("mapped", one)?;
            if !mappable(&reference.qualified_type) {
                return Err(Refusal::on(
                    "mapped",
                    format!(
                        "`{}` is a {}: a colour map draws a property, a property kind or a representation{}",
                        reference.title,
                        class_of(&reference.qualified_type),
                        if decorable(&reference.qualified_type) { " — a constant colour on it goes in `targets`" } else { "" }
                    ),
                ));
            }
        }
        if let Some(map) = &inputs.colour_map {
            let reference = decorated("colourMap", map)?;
            if !Takes::object(COLOUR_MAPS).accepts(&reference.qualified_type) {
                return Err(Refusal::on(
                    "colourMap",
                    format!(
                        "`{}` is a {}, and a colour map is a ContinuousColorMap or a DiscreteColorMap",
                        reference.title,
                        class_of(&reference.qualified_type)
                    ),
                ));
            }
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: GraphicalInformationInputs,
        params: GraphicalInformationParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = params.title.0.trim().to_string();
        if title.is_empty() {
            return Err(Refusal::on("title", "a set needs a title: it names what the package chose to draw"));
        }

        // **Every colour, and all the bad ones at once**: stopping at the first
        // would make correcting a list cost as many runs as it has faults.
        let mut colours: Vec<Rgba> = Vec::with_capacity(params.colours.len());
        let mut unread: Vec<String> = Vec::new();
        for (at, text) in params.colours.iter().enumerate() {
            match parse_colour(text) {
                Ok(colour) => colours.push(colour),
                Err(why) => unread.push(format!("colour {}: {why}", at + 1)),
            }
        }
        if !unread.is_empty() {
            return Err(Refusal::on(
                "colours",
                format!("{} — a colour that does not read is refused, never replaced by another", unread.join("; ")),
            ));
        }
        if params.shared && colours.len() > 1 {
            return Err(Refusal::on(
                "shared",
                format!(
                    "{} colours are given, and one shared entry holds one colour: give one, or leave `shared` off for one entry per object",
                    colours.len()
                ),
            ));
        }

        // The objects and their colours: one for all, or one each in order.
        let targets: Vec<Decorated> =
            inputs.targets.iter().map(|one| decorated("targets", one)).collect::<Result<_, _>>()?;
        let paired: Vec<(Decorated, Rgba)> = match (targets.len(), colours.len()) {
            (0, 0) => Vec::new(),
            (0, given) => {
                return Err(Refusal::on(
                    "colours",
                    format!("{given} colour(s) and no object in `targets` to put them on"),
                ));
            }
            (count, 0) => {
                return Err(Refusal::on(
                    "colours",
                    format!(
                        "{count} object(s) in `targets` and no colour: give one for all of them, or one per object in their order"
                    ),
                ));
            }
            (_, 1) => targets.into_iter().map(|target| (target, colours[0])).collect(),
            (count, given) if count == given => targets.into_iter().zip(colours.iter().copied()).collect(),
            (count, given) => {
                return Err(Refusal::on(
                    "colours",
                    format!(
                        "{given} colours for {count} objects in `targets`: one colour for all of them, or one per object in their order"
                    ),
                ));
            }
        };

        // An object given twice: once if the colour is the same, refused if not.
        let mut coloured: Vec<(Decorated, Rgba)> = Vec::with_capacity(paired.len());
        let mut twice = 0usize;
        for (target, colour) in paired {
            let first: Option<Rgba> =
                coloured.iter().find(|(one, _)| same_uuid(&one.uuid, &target.uuid)).map(|(_, first)| *first);
            match first {
                Some(first) if first.to_hex() == colour.to_hex() => twice += 1,
                Some(first) => {
                    return Err(Refusal::on(
                        "colours",
                        format!(
                            "`{}` is given twice, in {} and in {}: two colours for one object in one set is a question the set would leave to its reader",
                            target.title,
                            first.to_hex(),
                            colour.to_hex()
                        ),
                    ));
                }
                None => coloured.push((target, colour)),
            }
        }

        // An element scope is a representation's (TRG 2.2 §8 p. 232).
        let scope = params.scope.scope();
        if scope != Scope::WholeObject
            && let Some((target, _)) = coloured.iter().find(|(one, _)| !is_representation(&one.qualified_type))
        {
            return Err(Refusal::on(
                "scope",
                format!(
                    "`{}` is a {}, which has no {scope} of its own: indexable elements are a representation's — `whole-object` colours it",
                    target.title,
                    class_of(&target.qualified_type)
                ),
            ));
        }

        let mut says: Vec<Note> = Vec::new();
        let drawn = drawn_through(&inputs, &params, &mut says)?;

        // The entries: the constant colours, then the colour maps, each in the
        // order given — the order of a set is part of its meaning.
        let mut entries: Vec<AnyXml> = Vec::new();
        if params.shared {
            if let Some((_, colour)) = coloured.first() {
                let all: Vec<Decorated> = coloured.iter().map(|(target, _)| target.clone()).collect();
                entries.push(color::constant_colour_entry(&all, scope, *colour, None));
            }
        } else {
            for (target, colour) in &coloured {
                entries.push(color::constant_colour_entry(std::slice::from_ref(target), scope, *colour, None));
            }
        }
        let constant_entries = entries.len();
        if let Some(drawn) = &drawn {
            if params.shared {
                entries.push(color::colour_map_entry(&drawn.objects, &drawn.map, &drawn.scaling, drawn.component));
            } else {
                for one in &drawn.objects {
                    entries.push(color::colour_map_entry(
                        std::slice::from_ref(one),
                        &drawn.map,
                        &drawn.scaling,
                        drawn.component,
                    ));
                }
            }
        }
        let entry_count = entries.len();

        // **What the set is is what it says**: the same content, the same uuid.
        let mut content: Vec<String> =
            vec![format!("scope={}", params.scope.word()), format!("shared={}", params.shared)];
        for (target, colour) in &coloured {
            content.push(format!("{}={}", target.uuid.to_ascii_lowercase(), colour.to_hex()));
        }
        if let Some(drawn) = &drawn {
            content.push(format!(
                "map={}:{}:{:?}:{}:{}:{:?}",
                drawn.map.uuid.to_ascii_lowercase(),
                drawn.objects.iter().map(|one| one.uuid.to_ascii_lowercase()).collect::<Vec<_>>().join(","),
                drawn.scaling.min_max,
                drawn.scaling.logarithmic,
                drawn.scaling.reverse,
                drawn.component
            ));
        }
        let seed = format!("graphical-information:{title}:{}", content.join(";"));
        let uuid = cx.identity.uuid(&seed, 0)?;

        let set = eml::GraphicalInformationSet {
            uuid: uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: eml::Citation {
                title: title.clone(),
                originator: cx.when.originator.clone(),
                creation: cx.when.creation.clone(),
                format: energyml_core::CITATION_FORMAT.to_string(),
                ..Default::default()
            },
            graphical_information: entries,
            ..Default::default()
        };

        // What was written, said first; `ViewerKind` and `IsVisible` are the
        // domain writer's, and a reader of the plan should not have to know it.
        let mut parts: Vec<String> = Vec::new();
        if !coloured.is_empty() {
            parts.push(format!(
                "{constant_entries} constant colour entry(ies) on {} object(s), covering {scope}, for the `3d` viewer and visible",
                coloured.len()
            ));
        }
        if let Some(drawn) = &drawn {
            parts.push(format!(
                "{} colour map entry(ies) through `{}` on {} object(s)",
                entry_count - constant_entries,
                drawn.map.title,
                drawn.objects.len()
            ));
        }
        says.insert(0, Note { key: "entries", detail: format!("{entry_count} entry(ies): {}", parts.join("; ")) });
        if twice > 0 {
            says.push(Note {
                key: "twice",
                detail: format!("{twice} object(s) given twice in the same colour, coloured once"),
            });
        }

        // What was said, colour by colour: a list is not recorded by the
        // registry, and an activity that does not record its inputs is a
        // receipt (ADR-0017).
        let mut parameters =
            vec![Parameter::Double { title: "entryCount".to_string(), value: entry_count as f64, uom: None }];
        for text in &params.colours {
            parameters.push(Parameter::Text { title: "colours".to_string(), value: text.trim().to_string() });
        }

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![NewObject {
                uuid,
                qualified_type: "eml23.GraphicalInformationSet".to_string(),
                role: "colours".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&set),
            }],
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "graphical-information",
                title: format!("graphical-information → {title}"),
                seed,
                parameters,
            },
            says,
            dropped: Vec::new(),
            // No geometry: a set of colours has no shape.
            shapes: Vec::new(),
        })
    }

    fn reports(&self) -> Vec<Slot> {
        vec![Slot::optional(
            "entryCount",
            Need::Number { uom: None, least: None, most: None },
            "the entries written in the set",
        )]
    }
}

/// What `ColorInformation` says: the map, what it draws, how.
struct Drawn {
    map: Decorated,
    objects: Vec<Decorated>,
    scaling: Scaling,
    component: Option<u64>,
}

/// The colour map part, checked against the schema — `None` when no map is
/// given, and then no setting of a map may be either.
fn drawn_through(
    inputs: &GraphicalInformationInputs,
    params: &GraphicalInformationParams,
    says: &mut Vec<Note>,
) -> Result<Option<Drawn>, Refusal> {
    let Some(map) = &inputs.colour_map else {
        // A setting of a map with no map: named, not dropped — a range that
        // reads as applied and applies to nothing is the failure to avoid.
        let given = [
            ("minimum", params.minimum.is_some()),
            ("maximum", params.maximum.is_some()),
            ("logarithmic", params.logarithmic),
            ("reverse", params.reverse),
            ("valueVectorIndex", params.value_vector_index.is_some()),
        ];
        if let Some((slot, _)) = given.iter().find(|(_, set)| *set) {
            return Err(Refusal::on(
                slot,
                "a setting of a colour map, and no colour map is given in `colourMap`",
            ));
        }
        return Ok(None);
    };
    let map = decorated("colourMap", map)?;

    // `MinMax` carries both bounds, and an interval is a minimum below a
    // maximum (`GraphicalInformationObject.xsd` l. 579–592).
    let min_max = match (params.minimum, params.maximum) {
        (None, None) => None,
        (Some(_), None) => {
            return Err(Refusal::on("maximum", "`minimum` is given without `maximum`: a range carries both"));
        }
        (None, Some(_)) => {
            return Err(Refusal::on("minimum", "`maximum` is given without `minimum`: a range carries both"));
        }
        (Some(low), Some(high)) => {
            if !low.is_finite() || !high.is_finite() {
                return Err(Refusal::on("minimum", format!("[{low}, {high}] is not a range of numbers")));
            }
            if low >= high {
                return Err(Refusal::on(
                    "maximum",
                    format!(
                        "the range [{low}, {high}] holds nothing: a range's minimum is below its maximum — `reverse` is how the colours go the other way"
                    ),
                ));
            }
            Some((low, high))
        }
    };
    // *« the log of the property values are taken into account »*
    // (l. 269–273): a range that reaches zero holds values with no logarithm.
    if params.logarithmic
        && let Some((low, _)) = min_max
        && low <= 0.0
    {
        return Err(Refusal::on(
            "logarithmic",
            format!("the range starts at {low}, and a value of zero or less has no logarithm to place on the map"),
        ));
    }
    let component = params
        .value_vector_index
        .map(|index| {
            u64::try_from(index).map_err(|_| {
                Refusal::on("valueVectorIndex", format!("{index} is not an index: the first component is 0"))
            })
        })
        .transpose()?;

    // The objects drawn, each once.
    let mut objects: Vec<Decorated> = Vec::with_capacity(inputs.mapped.len());
    let mut twice = 0usize;
    let mut uncounted: Vec<String> = Vec::new();
    for one in &inputs.mapped {
        let reference = decorated("mapped", one)?;
        if objects.iter().any(|kept| same_uuid(&kept.uuid, &reference.uuid)) {
            twice += 1;
            continue;
        }
        // **An index into the data, never into the map.** A property that
        // states how many values each element holds says how many components
        // there are; an index past the last names none of them.
        if let Some(index) = component {
            match one.known.property.as_ref().filter(|property| !property.value_count.is_empty()) {
                Some(property) => {
                    let components: u64 = property.value_count.iter().product();
                    if index >= components {
                        return Err(Refusal::on(
                            "valueVectorIndex",
                            format!(
                                "`{}` holds {components} value(s) per element, components 0 to {}, and {index} is none of them",
                                reference.title,
                                components.saturating_sub(1)
                            ),
                        ));
                    }
                }
                None => uncounted.push(reference.title.clone()),
            }
        }
        objects.push(reference);
    }
    if twice > 0 {
        says.push(Note { key: "twice-mapped", detail: format!("{twice} object(s) given twice in `mapped`, drawn once") });
    }
    if !uncounted.is_empty() {
        says.push(Note {
            key: "components-unchecked",
            detail: format!(
                "`valueVectorIndex` could not be checked against {}: nothing given says how many components each holds",
                uncounted.join(", ")
            ),
        });
    }
    Ok(Some(Drawn {
        map,
        objects,
        scaling: Scaling { min_max, logarithmic: params.logarithmic, reverse: params.reverse },
        component,
    }))
}

/// The reference an input stands for, as the writer names an object: uuid,
/// qualified type, title — or a refusal naming its slot.
fn decorated(slot: &str, input: &ObjectIn) -> Result<Decorated, Refusal> {
    let Some(known) = input.known.reference.as_ref() else {
        return Err(Refusal::on(slot, format!("`{}` was given without a reference to write", input.name)));
    };
    Ok(Decorated { uuid: known.uuid.clone(), qualified_type: known.qualified_type.clone(), title: known.title.clone() })
}

/// Two uuids, compared as uuids: case is no part of one.
fn same_uuid(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The class a qualified type names, as written — `ContinuousProperty`.
fn class_of(qualified_type: &str) -> &str {
    qualified_type.rsplit('.').next().unwrap_or(qualified_type)
}

/// The bare class, compared the way `Takes::accepts` compares: case, the
/// package prefix and RESQML 2.0.1's `obj_` ignored.
fn bare(qualified_type: &str) -> String {
    let class = class_of(qualified_type).to_ascii_lowercase();
    class.strip_prefix("obj_").map(str::to_string).unwrap_or(class)
}

/// Whether a representation — what has indexable elements.
fn is_representation(qualified_type: &str) -> bool {
    bare(qualified_type).ends_with("representation")
}

/// Whether a `DefaultGraphicalInformation` may name it: *« Either for Feature,
/// Interp or representation, marker »* (RESQML TRG 2.2 §4.10 p. 61). A
/// `Model` is RESQML 2.2's one feature whose class name does not say so, and a
/// `DataobjectCollection` is the house's extension: the colour of a collection
/// is the colour of what it holds (the inheritance-resolution rules).
fn decorable(qualified_type: &str) -> bool {
    let class = bare(qualified_type);
    class == "model"
        || class == "dataobjectcollection"
        || ["feature", "interpretation", "representation", "marker"].iter().any(|kind| class.ends_with(kind))
}

/// Whether a `ColorInformation` may name it: *« Used for properties and
/// property kinds and for geometry. In the latter case, we need to point to the
/// representation »* (RESQML TRG 2.2 §4.5 p. 56).
fn mappable(qualified_type: &str) -> bool {
    let class = bare(qualified_type);
    class.ends_with("property") || class == "propertykind" || class.ends_with("representation")
}

/// A colour's text into an [`Rgba`] — `energyml_domain::color::Rgba::parse`,
/// the one reader, kept under this name for the callers of this module.
pub fn parse_colour(text: &str) -> Result<Rgba, String> {
    Rgba::parse(text)
}
