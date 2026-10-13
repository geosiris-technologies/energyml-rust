//! Everything that can be said against a package, in one report.
//!
//! # Why this is here and not in the command line
//!
//! It was in the command line: three hundred lines that read, judged and
//! printed in one pass, so `energyml check` was the only thing in the workspace
//! that could answer *is this package sound?* - not the front end, not the Python
//! bridge, not a service. The rule (ADR-0012): the CLI must be like the Python
//! bridge, every computation in the library, and the CLI and wasm only bridges
//! to the library's features.
//!
//! So the judging is here and the printing stayed there. What moved with it is
//! the part that is easy to miss: **the accounting**. Which finding counts as
//! an error is a decision, not a display choice, and two faces that counted
//! differently would disagree about whether a file is valid - which is the
//! exact bug this repository found between its own `check` and a partner's
//! validator on 07/09.
//!
//! # Why in this crate
//!
//! A report joins what the storage layer noticed (diagnostics, references,
//! `.rels`), what the schema says (`energyml-core`), and what the technical
//! guides say (property kinds, business rules - this crate). `energyml-domain`
//! is the lowest crate that can see all three; storage cannot, because the
//! rules live above it.
//!
//! Nothing here takes a store: an `EpcStore` and an ETP dataspace hand over the
//! same four things, and the day the second one exists this file will not have
//! changed.

use energyml_core::registry::{DynObject, Registry};
use energyml_core::validate::{self, Severity as IssueSeverity};
use energyml_storage::Relationship;
use energyml_storage::error::{Diagnostic, Severity};
use energyml_storage::graph::{self, RefIssue, RelIssue, Related};
use energyml_storage::resource::{ObjectSummary, type_matches};

use crate::source::ObjectSource;

/// What to check, and with what.
#[derive(Debug, Default)]
pub struct Options {
    /// Check each object against its schema: facets, cardinality, patterns.
    ///
    /// The slowest section on a large package, and the one a caller who has
    /// just validated elsewhere can do without.
    pub schema: bool,
    /// Check the business rules the technical guides state.
    pub rules: bool,
    /// Rules of the caller's own, **added** to the shipped set rather than
    /// replacing it: a caller who brings one rule still wants the other forty.
    pub extra_rules: Vec<crate::rules::Rule>,
}

impl Options {
    /// Everything this build can check.
    pub fn everything() -> Self {
        Options {
            schema: true,
            rules: true,
            extra_rules: Vec::new(),
        }
    }
}

/// What a package hands over to be judged.
///
/// Four facts, and any store can produce them. Deliberately not a store: see
/// the module doc.
pub struct Package<'a> {
    /// Every object that reads, with its summary and its recorded `.rels`.
    pub objects: &'a [Part<'a>],
    /// Every object the package holds, **readable or not**.
    ///
    /// What a reference is resolved against, and it is not the same list as
    /// `objects`: a reference to a part that failed to deserialize points at
    /// something the package *has*, and reporting it as dangling would blame
    /// the wrong file.
    pub summaries: &'a [ObjectSummary],
    /// What could not be read: its uri, and why.
    ///
    /// **A list, not a silence.** An unreadable part is the one finding no
    /// other section can make, because every other section starts from an
    /// object.
    pub unreadable: &'a [(String, String)],
    /// What the reader had to say while indexing and deserializing.
    pub diagnostics: &'a [Diagnostic],
    /// Where a uuid is looked up - for the property kinds and the rules.
    pub source: &'a dyn ObjectSource,
    /// The types this build can read, for the rules that ask.
    pub known: Option<&'a Registry>,
}

/// One part of a package, read.
///
/// The object arrives as a `DynObject` and not as a `&dyn Reflect` because two
/// sections need two different views of it - the schema section validates, the
/// others reflect - and handing over one of them would make the caller pass the
/// same object twice.
pub struct Part<'a> {
    /// Its place in the inventory.
    pub summary: &'a ObjectSummary,
    /// The object itself, already deserialized.
    pub object: &'a dyn DynObject,
    /// The relationships recorded for its part, as the `.rels` state them.
    pub relationships: &'a [Relationship],
}

/// One object failing its schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIssue {
    /// Uuid of the object.
    pub uuid: String,
    /// How much it matters.
    pub severity: IssueSeverity,
    /// Dotted path to the offending value.
    pub path: String,
    /// Stable machine-readable code.
    pub code: String,
    /// What is wrong, in one sentence.
    pub message: String,
}

/// One property saying something the guides forbid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyIssue {
    /// Uuid of the property.
    pub uuid: String,
    /// What is wrong, in the words the check gives.
    pub says: String,
    /// Whether it is a defect of the file or a remark.
    ///
    /// The difference the caller needs to count: a deprecated kind is worth
    /// saying, a unit outside its quantity class is worth refusing.
    pub is_a_defect: bool,
    /// **An abstract kind used directly** - an error, not a warning. The
    /// specification only *advises* against it - common TRG 2.3 §11.9,
    /// `PropertyKind.IsAbstract`: *"If the Is Abstract flag is set, then this
    /// entry should be used only as the parent of a real property"*. The weight
    /// is a house decision, not a MUST of the specification.
    pub is_error: bool,
}

/// One property whose values do not add up with what it hangs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountIssue {
    /// Uuid of the property.
    pub uuid: String,
    /// What is wrong, in the words `Discrepancy` gives.
    pub says: String,
    /// Whether it condemns the file. **A whole multiple of what is declared is
    /// not**: a time series written as one dataset holds `steps ×` the values of
    /// one, which the guides *neither describe nor forbid* (a property that
    /// contradicts itself is reported) - said, not condemned. Any other disagreement cannot be laid on the elements the
    /// property names, and is an error. The remark is **listed and not counted**
    /// in [`Report::warnings`], like a catalogue reference: Lutetia's own
    /// time series are such, and a clean model must not warn about its design.
    pub is_error: bool,
}

/// One business rule, judged against one object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFinding {
    /// Uuid of the object judged.
    pub uuid: String,
    /// The rule's id.
    pub rule: String,
    /// What the rule means.
    pub says: String,
    /// What was read, when the rule was broken.
    pub read: Option<String>,
    /// Why it could not be judged, when it could not.
    ///
    /// **Never a pass.** A rule that could not be evaluated says something is
    /// missing in order to judge - a target outside the package - not that the
    /// file is right.
    pub why: Option<String>,
    /// How much a violation weighs - declared by the rule.
    pub gravity: crate::rules::Gravity,
}

/// Everything that can be said against a package.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// What the reader noticed while indexing and deserializing.
    pub diagnostics: Vec<Diagnostic>,
    /// How many objects read - what the other sections judged. Beside
    /// `unreadable`, so that *nothing to say* about three objects and about
    /// three hundred never read the same.
    pub read: usize,
    /// Parts that could not be read: uri, and why.
    pub unreadable: Vec<(String, String)>,
    /// Objects against their schema. Empty and `schema_checked` false when the
    /// caller did not ask - which is not the same as clean.
    pub schema: Vec<SchemaIssue>,
    /// Whether the schema section was run at all.
    pub schema_checked: bool,
    /// Properties against the guides.
    pub properties: Vec<PropertyIssue>,
    /// Properties whose kind could not be resolved, so were not checked.
    ///
    /// Counted rather than reported one by one, and **not** an error: an
    /// unresolvable kind is usually one the package legitimately does not
    /// carry.
    pub properties_unresolved: usize,
    /// Properties whose number of values is not `element count × values per
    /// element` - `energyml properties --check` used to be the only thing that
    /// asked. **An error** when the values cannot be laid on the elements they
    /// name, which is what the TRGs' *"array length must equal …"* sentences
    /// say (RESQML TRG 2.2 §7.1; Properties.xsd, `ValueCountPerIndexableElement`);
    /// a remark, listed and not counted, for a whole multiple (see [`CountIssue::is_error`]).
    pub counts: Vec<CountIssue>,
    /// Properties whose count could not be judged: the pair (representation,
    /// indexable element) is not one this build can count, the representation
    /// is not in the package, or the values could not be counted. Counted and
    /// never an error - it says *not checked*, not *fine*.
    pub counts_unchecked: usize,
    /// Business rules.
    pub rules: Vec<RuleFinding>,
    /// Whether the rules section was run at all.
    pub rules_checked: bool,
    /// How many rules were evaluated, the caller's own included.
    pub rule_count: usize,
    /// References that point somewhere wrong, or nowhere.
    pub references: Vec<RefIssue>,
    /// Where the `.rels` index and the objects disagree.
    pub relationships: Vec<RelIssue>,
}

impl Report {
    /// How many findings are faults of the file.
    ///
    /// **The accounting, and it is a decision rather than a presentation.**
    ///
    /// A package diagnostic of severity `Warning` is an *error*: it is
    /// documented, at its definition, as *the package departs from the
    /// specification; the reader coped* - and a departure from the
    /// specification is what every validator calls an error. This counted it as
    /// a warning and exited 0, so on the BRGM package of 07/09 `energyml check`
    /// said *0 error(s)* about four `BoundaryFeature`s whose `Citation` omits
    /// `Originator`, which a partner's validator refused outright. Two tools
    /// looking at one file and disagreeing about whether it is valid.
    ///
    /// Structural validation cannot cover for this and never will: by the time
    /// it sees the object, the absent element has been read as an empty string,
    /// which is present. **Absence is the reader's news and only the
    /// reader's**, so the reader's own count is where it has to be honoured.
    ///
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|one| one.severity == Severity::Warning)
            .count()
            + self.unreadable.len()
            + self
                .schema
                .iter()
                .filter(|one| one.severity == IssueSeverity::Error)
                .count()
            + self.references.iter().filter(|one| one.is_error()).count()
            + self.properties.iter().filter(|one| one.is_error).count()
            + self.counts.iter().filter(|one| one.is_error).count()
            + self
                .rules
                .iter()
                .filter(|one| one.read.is_some() && one.gravity == crate::rules::Gravity::Error)
                .count()
    }

    /// How many findings are worth knowing without condemning the file.
    ///
    /// The relationship section stays here and never in [`Report::errors`]: a
    /// `.rels` is an index and the DORs are the truth
    /// (ADR-0002), so a disagreement costs a viewer
    /// built on the index and changes nothing about what the file means. On one
    /// BRGM package that is 495 findings, and drowning eight real ones in them
    /// is how the eight went unnoticed.
    pub fn warnings(&self) -> usize {
        self.schema
            .iter()
            .filter(|one| one.severity == IssueSeverity::Warning)
            .count()
            + self
                .properties
                .iter()
                .filter(|one| one.is_a_defect && !one.is_error)
                .count()
            + self
                .rules
                .iter()
                .filter(|one| one.read.is_some() && one.gravity == crate::rules::Gravity::Warning)
                .count()
            + self
                .references
                .iter()
                .filter(|one| !one.is_error() && !one.is_info())
                .count()
            + self.relationships.len()
    }

    /// Whether the file is sound: no errors.
    ///
    /// What an exit code is built on, in one place rather than in each face.
    pub fn is_sound(&self) -> bool {
        self.errors() == 0
    }

    /// References to a concept the standard publishes in a catalogue.
    ///
    /// Absent from the package **by design**, so neither an error nor a
    /// warning - and listed apart, because a thousand of them mixed in bury the
    /// ones that mean something.
    pub fn catalogue_references(&self) -> Vec<&RefIssue> {
        self.references.iter().filter(|one| one.is_info()).collect()
    }

    /// The reference findings that are about this package's own objects.
    pub fn reportable_references(&self) -> Vec<&RefIssue> {
        self.references.iter().filter(|one| !one.is_info()).collect()
    }
}

/// What the schema's own documentation asks and its facets cannot.
///
/// Run with the schema section and reported in it, at warning weight: the
/// file passes the XSD, and the XSD's text says it should not.
pub fn documented_rules() -> Vec<Box<dyn validate::ValidationRule>> {
    vec![Box::new(CitationFormat)]
}

/// `Citation.Format` names the software that wrote the object.
///
/// EML 2.3 `Abstract.xsd` l. 393-399: *"Software or service that was used to
/// originate the object [...] Must be human and machine readable and
/// unambiguously identify the software by including the organization name,
/// software application name and software version"*. The element is
/// mandatory, but `String2000` has no `minLength`, so an empty one is valid for
/// the schema. That is why nothing reported the 924 empty ones Lutetia wrote
/// until 06/10/2026.
///
/// Common 2.0 is left out: its `Format` carries `minLength="1"`, so the schema
/// section already reports an empty one as an error, and a second finding
/// would count the same fault twice.
struct CitationFormat;

impl validate::ValidationRule for CitationFormat {
    fn name(&self) -> &str {
        "citation-format"
    }

    fn applies_to(&self, info: &'static energyml_core::reflect::TypeInfo) -> bool {
        info.package == "eml" && info.name == "Citation" && info.version != "2.0"
    }

    fn check(&self, object: &dyn energyml_core::value::Reflect, ctx: &mut validate::ValidationCtx<'_>) {
        let format = energyml_core::path::get_string(object, "Format")
            .ok()
            .flatten()
            .unwrap_or_default();
        if format.trim().is_empty() {
            ctx.enter("Format");
            ctx.warn(
                "citation-format-empty",
                "Citation.Format is empty: EML Abstract.xsd l. 393-399 asks for the software that wrote the object,                  as `organizationName:applicationName:applicationVersion/format`",
            );
            ctx.leave();
        }
    }
}

/// Read everything an EPC holds and [`check`] it - the reading the command
/// line and the Python bridge each wrote out, line for line, before 24/09.
///
/// **What it reads, and why all of it**: every `.rels` (not on the open path,
/// an open proportional to the number of objects being what level 1 promises
/// not to be - but a truncated or duplicated relationship index is exactly what
/// a check is for), every external file (the store records what is wrong with
/// each on the way past), and every object **before** the diagnostics are
/// taken: a lenient read records what it had to invent while deserializing,
/// and taking the diagnostics first would miss exactly those.
pub fn of_store(store: &energyml_storage::EpcStore, options: &Options) -> Report {
    let _ = store.external_files();
    let summaries = store.resources();
    let (held, unreadable) = store.read_objects(None);
    let unreadable: Vec<(String, String)> = unreadable
        .iter()
        .map(|line| match line.split_once(": ") {
            Some((uri, why)) => (uri.to_string(), why.to_string()),
            None => (line.clone(), String::new()),
        })
        .collect();
    let relationships: std::collections::HashMap<String, Vec<energyml_storage::Relationship>> = summaries
        .iter()
        .filter_map(|summary| {
            store
                .relationships(&summary.locator)
                .ok()
                .map(|rels| (summary.locator.clone(), rels))
        })
        .collect();
    let parts: Vec<Part<'_>> = held
        .iter()
        .map(|(summary, object)| Part {
            summary,
            object: object.as_ref(),
            relationships: relationships
                .get(&summary.locator)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        })
        .collect();
    let diagnostics = store.diagnostics();
    let package = Package {
        objects: &parts,
        summaries: &summaries,
        unreadable: &unreadable,
        diagnostics: &diagnostics,
        source: store,
        known: Some(store.registry()),
    };
    check_with_arrays(&package, options, Some(store))
}

/// Judge a package.
///
/// Reads nothing: everything it needs has been handed over, which is what lets
/// a caller decide when to pay for reading and lets this be the same code for a
/// file and for a dataspace.
///
/// With no array reader, the one rule that judges values
/// (`house/discrete-value-in-lookup`) comes back *uncheckable* for each
/// property it would have judged - [`check_with_arrays`] is the same with one.
pub fn check(package: &Package<'_>, options: &Options) -> Report {
    check_with_arrays(package, options, None)
}

/// [`check`], with something to read arrays with.
///
/// A separate entry rather than a field of [`Package`] or [`Options`]: both
/// are built field by field by their callers, and a new field would break
/// every one of them for a rule most of them never meet. [`of_store`] passes
/// the store, which reads both.
pub fn check_with_arrays(
    package: &Package<'_>,
    options: &Options,
    arrays: Option<&dyn crate::source::ArraySource>,
) -> Report {
    let mut report = Report {
        diagnostics: package.diagnostics.to_vec(),
        read: package.objects.len(),
        unreadable: package.unreadable.to_vec(),
        schema_checked: options.schema,
        rules_checked: options.rules,
        ..Default::default()
    };

    // 1 - the objects against their schema: facets, cardinality, patterns.
    if options.schema {
        let documented = documented_rules();
        for one in package.objects {
            let found = validate::validate_with(one.object.as_validate(), &documented);
            for issue in found.issues {
                report.schema.push(SchemaIssue {
                    uuid: one.summary.uuid.clone(),
                    severity: issue.severity,
                    path: issue.path,
                    code: issue.code,
                    message: issue.message,
                });
            }
        }
    }

    // 2 - the properties, against what the technical reference guides state and
    // no XSD can: an abstract property kind used directly, a deprecated one, a
    // unit outside its kind's quantity class. All three are perfectly readable
    // files saying something they should not.
    {
        use crate::property;
        use crate::property_kind::{self, Resolver};

        let resolver = Resolver::new(package.source);
        for one in package.objects {
            if !type_matches(&one.summary.qualified_type, "Property") {
                continue;
            }
            let Ok(read) = property::read(one.object.as_reflect()) else {
                continue;
            };
            let Some(reference) = &read.kind else { continue };
            let Ok(resolution) = resolver.resolve_at(reference, read.kind_uri.as_deref()) else {
                report.properties_unresolved += 1;
                continue;
            };
            for issue in property_kind::check(&resolution, read.uom.as_deref()) {
                report.properties.push(PropertyIssue {
                    uuid: one.summary.uuid.clone(),
                    is_a_defect: issue.is_a_defect(),
                    is_error: matches!(issue, property_kind::KindIssue::AbstractKindUsed { .. }),
                    says: format!("{issue}"),
                });
            }
        }
    }

    // 2b - the counts: what a property holds against what it hangs on. Only when
    // there is something to count with (`arrays`): without it the values cannot
    // be counted, and `Property::check` has nothing to compare - said by
    // `counts_unchecked`, never by silence.
    if options.rules {
        use crate::property;
        use crate::values;

        for one in package.objects {
            if !type_matches(&one.summary.qualified_type, "Property") {
                continue;
            }
            let Ok(read) = property::read(one.object.as_reflect()) else {
                continue;
            };
            let (Some(arrays), Some(support)) = (arrays, read.supporting_representation.as_deref()) else {
                report.counts_unchecked += 1;
                continue;
            };
            // By the uri the property names, so the version it names is the one
            // its values are counted against.
            let held = match read.supporting_representation_uri.as_deref() {
                Some(uri) => package.source.object_at(uri),
                None => package.source.object(support),
            };
            let Some(representation) = held else {
                report.counts_unchecked += 1;
                continue;
            };
            let mut actual = 0u64;
            let mut counted = true;
            for patch in &read.patches {
                match values::count(patch.array, arrays, &format!("patch {}", patch.position)) {
                    Ok(n) => actual += n as u64,
                    Err(_) => counted = false,
                }
            }
            if !counted || read.patches.is_empty() {
                report.counts_unchecked += 1;
                continue;
            }
            for found in read.check(representation.as_reflect(), Some(actual)) {
                match found {
                    property::Discrepancy::CountUnknown { .. } => report.counts_unchecked += 1,
                    other => {
                        let multiple = matches!(
                            &other,
                            property::Discrepancy::ValueCount { expected, actual, .. }
                                if *expected > 0 && actual > expected && actual % expected == 0
                        );
                        let says = if multiple {
                            format!(
                                "{other} - a whole multiple: a time series or realizations in one dataset, which the guides neither describe nor forbid"
                            )
                        } else {
                            format!("{other}")
                        };
                        report.counts.push(CountIssue {
                            uuid: one.summary.uuid.clone(),
                            says,
                            is_error: !multiple,
                        });
                    }
                }
            }
        }
    }

    // 3 - the business constraints: what the technical guides associate and no
    // XSD can enforce. Three outcomes and not two: the inapplicable is silent
    // by construction, a violation is a fault of the file, and a rule that
    // could not be evaluated is *uncheckable* - never a pass.
    //
    // Two halves, one list: the rules the closed vocabulary can say (data,
    // `rules::trg`), then the ones that need several objects or an array
    // (code, `integrity`). Their findings have the same shape and are counted
    // the same way - a reader of the report has no reason to know which half
    // spoke.
    if options.rules {
        use crate::rules::{self, Outcome};

        let mut set = rules::trg();
        set.extend(options.extra_rules.clone());
        report.rule_count = set.len() + crate::integrity::checks().len();

        let pairs: Vec<(&str, &dyn energyml_core::value::Reflect)> = package
            .objects
            .iter()
            .map(|one| (one.summary.uuid.as_str(), one.object.as_reflect()))
            .collect();
        let found = rules::run(&set, pairs.iter().copied(), package.source, package.known)
            .into_iter()
            .chain(crate::integrity::run(&pairs, package.source, arrays));
        for finding in found {
            let (read, why) = match finding.outcome {
                Outcome::Violation { read } => (Some(read), None),
                Outcome::Unverifiable { why } => (None, Some(why)),
            };
            report.rules.push(RuleFinding {
                uuid: finding.uuid,
                rule: finding.rule,
                says: finding.says,
                read,
                why,
                gravity: finding.gravity,
            });
        }
    }

    // 4 - the references. Nothing else finds these: each DOR is well-formed on
    // its own, and no XSD validator knows what the package contains.
    let pairs = package
        .objects
        .iter()
        .map(|one| (one.summary, one.object.as_reflect()));
    // **Every** entry of a uuid, not the first: a package may hold one object in
    // several versions, and a reference is resolved by uuid, type and version.
    // Indexed once - a lookup per reference would scan the inventory each time.
    let mut by_uuid: std::collections::HashMap<String, Vec<&ObjectSummary>> =
        std::collections::HashMap::new();
    for one in package.summaries.iter() {
        by_uuid
            .entry(one.uuid.to_ascii_lowercase())
            .or_default()
            .push(one);
    }
    let known = |uuid: &str| {
        by_uuid
            .get(&uuid.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    };
    report.references = graph::check_all_references_with(pairs, &known);
    // …and the type each one names, when this build can say what types exist.
    if let Some(registry) = package.known {
        let knows = |qualified: &str| registry.find_type(qualified).is_some();
        let pairs = package
            .objects
            .iter()
            .map(|one| (one.summary, one.object.as_reflect()));
        report
            .references
            .extend(graph::check_declared_types(pairs, &knows));
    }

    // 5 - the index against the objects. The `.rels` are a cache of the graph
    // the DORs describe; §3.4.1 makes both directions mandatory, and nothing
    // keeps the cache honest.
    let related: Vec<Related<'_>> = package
        .objects
        .iter()
        .map(|one| Related {
            summary: one.summary,
            object: one.object.as_reflect(),
            relationships: one.relationships,
        })
        .collect();
    report.relationships = graph::check_relationships(&related);

    report
}
