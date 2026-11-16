//! Verbatim provenance: the NWDF tables of the UDAR converter, read as written.
//!
//! The titles of the columns are those of the converter's tables (`process_uid`,
//! `activity`; `source_uid`, `attribute`, `target`, `target_table`; `nwdf_uid`,
//! `role`, `epc_uri`; `path`, `reference_uid`). Every table is built here, as a
//! `ColumnBasedTable` of string columns with **no key column**, as the real
//! mapping table has none; no file is read. TRG-cited rules: common TRG 2.3
//! section 9.1 (a table's columns and their values), EML TRG 2.3 section 3.1
//! p. 40 (an activity parameter's title, the template's equivalent).

use energyml_domain::activity::{
    ActivityParameter, ActivityRecord, ActivityRole, DeclaredReference, ParameterRole, ParameterValue,
    RoleBasis,
};
use energyml_domain::provenance_graph::{EdgeBasis, GraphIssue, NodeKey, ProvEdgeKind};
use energyml_domain::provenance_verbatim::{
    NWDF, NwdfTables, VerbatimId, VerbatimProcess, VerbatimRefusal, VerbatimSource, VerbatimSources,
    VerbatimTarget, reconcile, verbatim_edges,
};
use energyml_domain::source::NoObjects;
use energyml_domain::table::Table;
use energyml_eml_2_3 as eml23;

const ACT_A: &str = "eml:///eml23.Activity(aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa)";
const ACT_B: &str = "eml:///eml23.Activity(bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb)";
const ACT_C: &str = "eml:///eml23.Activity(cccccccc-cccc-4ccc-8ccc-cccccccccccc)";
const SURVEY: &str = "eml:///witsml21.Trajectory(11111111-1111-4111-8111-111111111111)";
const RESAMPLED: &str = "eml:///witsml21.Trajectory(22222222-2222-4222-8222-222222222222)";
const RH: &str = "eml:///resqml22.ContinuousProperty(33333333-3333-4333-8333-333333333333)";
const RV: &str = "eml:///resqml22.ContinuousProperty(44444444-4444-4444-8444-444444444444)";
const RADIUS: &str = "eml:///resqml22.ContinuousProperty(55555555-5555-4555-8555-555555555555)";
const ANGLE: &str = "eml:///resqml22.ContinuousProperty(66666666-6666-4666-8666-666666666666)";
const ACQ_1: &str = "eml:///witsml21.Log(77777777-7777-4777-8777-777777777777)";
const ACQ_2: &str = "eml:///witsml21.Log(88888888-8888-4888-8888-888888888888)";

const LAS: &str = "./data/Banana-1-1.las";
const ABX: &str = "./data/abx.json";
const SURVEY_KEY: &str = "dataset:banana1_survey:001";

// -- building the tables ----------------------------------------------------------

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn column(title: &str, values: &[&str]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
            count_per_value: 1,
            values: values.iter().map(|value| value.to_string()).collect(),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn float_column(title: &str, values: &[f64]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: values.to_vec(),
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

/// A column that has a title and no value at all: nothing could be read from it.
fn titled_only(title: &str) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        ..Default::default()
    }
}

fn table(uuid: &str, columns: Vec<eml23::Column>) -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: uuid.to_string(),
        citation: citation(uuid),
        column: columns,
        ..Default::default()
    }
}

/// The columns of a table from rows of text, one title per cell of a row.
fn text_table(uuid: &str, titles: &[&str], rows: &[Vec<&str>]) -> eml23::ColumnBasedTable {
    let columns = titles
        .iter()
        .enumerate()
        .map(|(at, title)| {
            let values: Vec<&str> = rows.iter().map(|row| row[at]).collect();
            column(title, &values)
        })
        .collect();
    table(uuid, columns)
}

const U_PROCESSES: &str = "00000000-0000-4000-8000-000000000001";
const U_LINKS: &str = "00000000-0000-4000-8000-000000000002";
const U_MAPPING: &str = "00000000-0000-4000-8000-000000000003";
const U_REFERENCES: &str = "00000000-0000-4000-8000-000000000004";

fn processes_rows() -> Vec<Vec<&'static str>> {
    vec![
        vec!["las_survey_read", ACT_A, "LAS survey read", "udar"],
        vec!["traj_resample", ACT_B, "Trajectory resample", "udar"],
        vec!["abx_json_ingest", ACT_C, "ABX json ingest", "udar"],
    ]
}

fn processes_table(rows: &[Vec<&str>]) -> eml23::ColumnBasedTable {
    text_table(
        U_PROCESSES,
        &["process_uid", "activity", "process_name", "software"],
        rows,
    )
}

fn links_rows() -> Vec<Vec<&'static str>> {
    vec![
        vec!["las_survey_read", "processes", "inputs", LAS, "external file"],
        vec!["las_survey_read", "processes", "outputs", SURVEY_KEY, "datasets"],
        vec![
            "las_survey_read",
            "processes",
            "dataset_uids",
            "dataset:acq:001",
            "datasets",
        ],
        vec!["traj_resample", "processes", "inputs", SURVEY_KEY, "datasets"],
        vec![
            "traj_resample",
            "processes",
            "outputs",
            "dataset:banana1_traj:001",
            "datasets",
        ],
        vec!["abx_json_ingest", "processes", "inputs", ABX, "external file"],
        vec![
            "abx_json_ingest",
            "processes",
            "outputs",
            "dataset:abx_rh:001",
            "datasets",
        ],
        vec![
            "abx_json_ingest",
            "processes",
            "outputs",
            "dataset:abx_rv:001",
            "datasets",
        ],
        vec![
            "abx_json_ingest",
            "processes",
            "outputs",
            "dataset:abx_radius:001",
            "datasets",
        ],
        vec![
            "abx_json_ingest",
            "processes",
            "outputs",
            "dataset:abx_angle:001",
            "datasets",
        ],
        vec![
            "abx_json_ingest",
            "processes",
            "dataset_uids",
            "dataset:acq:002",
            "datasets",
        ],
    ]
}

fn links_table(uuid: &str, rows: &[Vec<&str>]) -> eml23::ColumnBasedTable {
    text_table(
        uuid,
        &[
            "source_uid",
            "source_table",
            "attribute",
            "target",
            "target_table",
        ],
        rows,
    )
}

/// Thirteen rows, and no key column.
fn mapping_rows() -> Vec<Vec<&'static str>> {
    vec![
        vec!["las_survey_read", "eml activity", ACT_A],
        vec!["traj_resample", "eml activity", ACT_B],
        vec!["abx_json_ingest", "eml activity", ACT_C],
        vec![SURVEY_KEY, "eml object", SURVEY],
        vec!["dataset:banana1_traj:001", "eml object", RESAMPLED],
        vec!["dataset:abx_rh:001", "eml object", RH],
        vec!["dataset:abx_rv:001", "eml object", RV],
        vec!["dataset:abx_radius:001", "eml object", RADIUS],
        vec!["dataset:abx_angle:001", "eml object", ANGLE],
        vec!["dataset:acq:001", "eml object", ACQ_1],
        vec!["dataset:acq:001", "eml object", ACQ_2],
        vec!["dataset:acq:002", "eml object", ACQ_1],
        vec!["dataset:acq:002", "eml object", ACQ_2],
    ]
}

fn mapping_table(rows: &[Vec<&str>]) -> eml23::ColumnBasedTable {
    text_table(U_MAPPING, &["nwdf_uid", "role", "epc_uri"], rows)
}

fn references_table() -> eml23::ColumnBasedTable {
    text_table(
        U_REFERENCES,
        &["path", "reference_uid"],
        &[
            vec![LAS, "external:las_survey_source"],
            vec![ABX, "external:abx_source"],
        ],
    )
}

fn nwdf_objects() -> Vec<eml23::ColumnBasedTable> {
    vec![
        processes_table(&processes_rows()),
        links_table(U_LINKS, &links_rows()),
        mapping_table(&mapping_rows()),
        references_table(),
    ]
}

fn tables_of(objects: &[eml23::ColumnBasedTable]) -> Vec<Table<'_>> {
    objects
        .iter()
        .map(|object| Table::read(object).expect("a table that reads"))
        .collect()
}

fn read(objects: &[eml23::ColumnBasedTable]) -> Result<Vec<VerbatimProcess>, VerbatimRefusal> {
    NwdfTables.read(&tables_of(objects), &NoObjects)
}

fn processes(objects: &[eml23::ColumnBasedTable]) -> Vec<VerbatimProcess> {
    read(objects).expect("the tables read")
}

// -- the NWDF tables --------------------------------------------------------------

/// The converter's tables state three processes, two files and objects through
/// the mapping; the `dataset_uids` links are acquisition context, not processing
/// provenance.
#[test]
fn nwdf_tables_give_files_processes_and_objects() {
    let objects = nwdf_objects();
    let all = processes(&objects);
    let keys: Vec<&str> = all.iter().map(|process| process.key.as_str()).collect();
    assert_eq!(keys, ["las_survey_read", "traj_resample", "abx_json_ingest"]);

    let first = &all[0];
    assert_eq!(first.activity.as_deref(), Some(ACT_A));
    assert_eq!(first.activity_by_mapping.as_deref(), Some(ACT_A));
    assert_eq!(
        first.inputs,
        vec![VerbatimTarget::File {
            path: LAS.to_string(),
            reference: Some("external:las_survey_source".to_string()),
        }]
    );
    assert_eq!(
        first.outputs,
        vec![VerbatimTarget::Objects {
            key: SURVEY_KEY.to_string(),
            uris: vec![SURVEY.to_string()],
        }]
    );
    assert_eq!(
        first.fields,
        vec![
            ("process_name".to_string(), "LAS survey read".to_string()),
            ("software".to_string(), "udar".to_string()),
        ]
    );

    // The second reads what the first wrote, by its key.
    assert_eq!(
        all[1].inputs,
        vec![VerbatimTarget::Objects {
            key: SURVEY_KEY.to_string(),
            uris: vec![SURVEY.to_string()],
        }]
    );
    // The third: one file, four objects, in the order of the links.
    assert_eq!(all[2].inputs.len(), 1);
    assert!(matches!(&all[2].inputs[0], VerbatimTarget::File { path, .. } if path == ABX));
    let written: Vec<&str> = all[2]
        .outputs
        .iter()
        .filter_map(|target| match target {
            VerbatimTarget::Objects { uris, .. } => Some(uris[0].as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(written, [RH, RV, RADIUS, ANGLE]);

    // `dataset_uids` is nowhere: neither as input nor as output.
    for process in &all {
        for target in process.inputs.iter().chain(&process.outputs) {
            assert!(
                !matches!(target, VerbatimTarget::Objects { key, .. } if key.starts_with("dataset:acq")),
                "{target:?}"
            );
        }
    }
}

/// The mapping table says which object a key is; a title only looks like it
/// (08 section 2.3). A key the mapping does not tie stays unresolved.
#[test]
fn a_dataset_the_mapping_does_not_tie_is_unresolved_never_by_title() {
    let rows: Vec<Vec<&str>> = mapping_rows()
        .into_iter()
        .filter(|row| row[0] != SURVEY_KEY)
        .collect();
    let objects = vec![
        processes_table(&processes_rows()),
        links_table(U_LINKS, &links_rows()),
        mapping_table(&rows),
        references_table(),
    ];
    let all = processes(&objects);
    assert_eq!(
        all[0].outputs,
        vec![VerbatimTarget::Unresolved(SURVEY_KEY.to_string())]
    );
    assert_eq!(
        all[1].inputs,
        vec![VerbatimTarget::Unresolved(SURVEY_KEY.to_string())]
    );
    // Another key whose title would look alike is still tied to its own object.
    assert!(matches!(&all[1].outputs[0], VerbatimTarget::Objects { .. }));
}

/// Two writings of one activity: the processes table's column and the mapping
/// table's `eml activity` row.
#[test]
fn the_activity_column_and_the_mapping_disagree() {
    let mut rows = processes_rows();
    rows[0][1] = ACT_B;
    let objects = vec![
        processes_table(&rows),
        links_table(U_LINKS, &links_rows()),
        mapping_table(&mapping_rows()),
    ];
    let all = processes(&objects);
    let (edges, issues) = verbatim_edges(NWDF, &all[0]);
    assert!(
        issues.iter().any(|issue| matches!(
            issue,
            GraphIssue::ActivityLinkDiffers { source, process, by_column, by_mapping }
                if *source == NWDF
                    && process == "las_survey_read"
                    && by_column.as_deref() == Some(ACT_B)
                    && by_mapping.as_deref() == Some(ACT_A)
        )),
        "{issues:?}"
    );
    // The column wins: the edges are drawn on the activity it names.
    assert!(!edges.is_empty());
    assert!(edges.iter().all(|edge| {
        edge.from == NodeKey::Object(ACT_B.to_string()) || edge.to == NodeKey::Object(ACT_B.to_string())
    }));

    // The same activity written two ways (the hex of a uuid in capitals) is no disagreement.
    let mut same = all[0].clone();
    same.activity_by_mapping = Some(ACT_A.replace(
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
    ));
    same.activity = Some(ACT_A.to_string());
    let (_, issues) = verbatim_edges(NWDF, &same);
    assert!(issues.is_empty(), "{issues:?}");
}

/// What a process reads points in, what it writes points out; a process with
/// no activity draws nothing and says so.
#[test]
fn verbatim_edges_point_in_and_out_and_a_process_with_no_activity_has_none() {
    let objects = nwdf_objects();
    let all = processes(&objects);
    let (edges, issues) = verbatim_edges(NWDF, &all[0]);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(edges.len(), 2);
    assert_eq!(edges[0].from, NodeKey::File(LAS.to_string()));
    assert_eq!(edges[0].to, NodeKey::Object(ACT_A.to_string()));
    assert_eq!(edges[0].kind, ProvEdgeKind::Consumed);
    assert_eq!(edges[1].from, NodeKey::Object(ACT_A.to_string()));
    assert_eq!(edges[1].to, NodeKey::Object(SURVEY.to_string()));
    assert_eq!(edges[1].kind, ProvEdgeKind::Produced);
    assert!(edges.iter().all(|edge| edge.basis == EdgeBasis::Verbatim(NWDF)));

    // A key nothing resolves is a node of its own, keyed by the source.
    let loose = VerbatimProcess {
        key: "p".to_string(),
        activity: Some(ACT_A.to_string()),
        activity_by_mapping: None,
        fields: Vec::new(),
        inputs: vec![VerbatimTarget::Unresolved("dataset:x".to_string())],
        outputs: Vec::new(),
    };
    let (edges, _) = verbatim_edges(NWDF, &loose);
    assert_eq!(
        edges[0].from,
        NodeKey::Unresolved {
            source: NWDF,
            label: "dataset:x".to_string()
        }
    );

    let orphan = VerbatimProcess {
        activity: None,
        activity_by_mapping: None,
        ..loose
    };
    let (edges, issues) = verbatim_edges(NWDF, &orphan);
    assert!(edges.is_empty());
    assert!(
        matches!(issues.as_slice(), [GraphIssue::ProcessWithoutActivity { process, .. }] if process == "p"),
        "{issues:?}"
    );
}

/// An empty text is a value (S16), and a number that is `NaN` is an empty cell:
/// the first is shown, the second is not.
#[test]
fn an_empty_text_is_a_value_and_a_nan_is_not() {
    let mut processes_object = processes_table(&processes_rows());
    processes_object
        .column
        .push(column("note", &["", "kept", "kept too"]));
    processes_object
        .column
        .push(float_column("rank", &[f64::NAN, 2.0, 3.0]));
    let objects = vec![
        processes_object,
        links_table(U_LINKS, &links_rows()),
        mapping_table(&mapping_rows()),
    ];
    let all = processes(&objects);
    assert!(all[0].fields.contains(&("note".to_string(), String::new())));
    assert!(!all[0].fields.iter().any(|(title, _)| title == "rank"));
    assert!(all[1].fields.contains(&("rank".to_string(), "2".to_string())));
}

// -- reconciling an activity with its process -------------------------------------

fn reference(address: &str) -> DeclaredReference {
    let uuid = address
        .split('(')
        .nth(1)
        .map(|rest| rest.trim_end_matches(')'))
        .unwrap_or(address)
        .to_string();
    DeclaredReference {
        address: address.to_string(),
        uuid,
        qualified_type: None,
        title: None,
        version: None,
    }
}

fn parameter(position: usize, title: &str, address: &str) -> ActivityParameter {
    ActivityParameter {
        position,
        title: title.to_string(),
        index: None,
        selection: None,
        is_uncertain: None,
        keys: Vec::new(),
        value: ParameterValue::Object(reference(address)),
        class: "eml23.DataObjectParameter".to_string(),
    }
}

fn activity(parameters: Vec<ActivityParameter>) -> ActivityRecord {
    ActivityRecord {
        uri: ACT_C.to_string(),
        title: Some("abx_json_ingest".to_string()),
        creation: None,
        last_update: None,
        originator: None,
        format: None,
        descriptor: None,
        parent: None,
        parameters,
    }
}

fn roles_of(parameters: &[ActivityParameter], role: ActivityRole) -> Vec<ParameterRole> {
    parameters
        .iter()
        .map(|parameter| ParameterRole {
            position: parameter.position,
            role,
            basis: RoleBasis::Template,
        })
        .collect()
}

/// The same hex of a uuid in capitals: the same object, never the same text.
fn shouted(address: &str) -> String {
    match address.split_once('(') {
        Some((head, rest)) => format!("{head}({}", rest.to_uppercase()),
        None => address.to_string(),
    }
}

/// The activity states its four outputs; the tables state them and the file the
/// activity does not name. Compared by uuid, the tables writing the capitals.
#[test]
fn reconcile_agrees_then_sees_an_output_only_the_activity_states() {
    let objects = nwdf_objects();
    let mut process = processes(&objects).remove(2);
    for target in &mut process.outputs {
        if let VerbatimTarget::Objects { uris, .. } = target {
            *uris = uris.iter().map(|uri| shouted(uri)).collect();
        }
    }
    let parameters = vec![
        parameter(0, "output", RH),
        parameter(1, "output", RV),
        parameter(2, "output", RADIUS),
        parameter(3, "output", ANGLE),
    ];
    let record = activity(parameters.clone());
    let roles = roles_of(&parameters, ActivityRole::Output);

    let reconciled = reconcile(&record, &roles, &process);
    assert_eq!(reconciled.both, [RH, RV, RADIUS, ANGLE]);
    assert_eq!(reconciled.only_tables, [ABX]);
    assert!(reconciled.only_activity.is_empty(), "{reconciled:?}");
    assert!(reconciled.roles_differ.is_empty(), "{reconciled:?}");

    // The tables no longer state Rv: only the activity does.
    process.outputs.retain(
        |target| !matches!(target, VerbatimTarget::Objects { key, .. } if key == "dataset:abx_rv:001"),
    );
    let reconciled = reconcile(&record, &roles, &process);
    assert_eq!(reconciled.only_activity, [RV]);
    assert_eq!(reconciled.both, [RH, RADIUS, ANGLE]);
}

/// Both state an object, one as an input and the other as an output: neither
/// "both" nor "only one".
#[test]
fn an_object_stated_in_opposite_directions_is_a_role_difference() {
    let objects = nwdf_objects();
    let process = processes(&objects).remove(2);
    let parameters = vec![parameter(0, "input", RH)];
    let record = activity(parameters.clone());
    let roles = roles_of(&parameters, ActivityRole::Input);
    let reconciled = reconcile(&record, &roles, &process);
    assert_eq!(reconciled.roles_differ, [RH]);
    assert!(reconciled.both.is_empty());
    assert!(!reconciled.only_activity.contains(&RH.to_string()));
    // The tables' other objects are stated by the tables only.
    assert!(reconciled.only_tables.contains(&RV.to_string()));
    assert!(!reconciled.only_tables.contains(&RH.to_string()));
}

// -- recognising and refusing -----------------------------------------------------

/// Recognition reads the titles of the columns, never an array: the columns here
/// have a title and nothing to read.
#[test]
fn recognition_reads_titles_only() {
    let blank =
        |uuid: &str, titles: &[&str]| table(uuid, titles.iter().map(|title| titled_only(title)).collect());
    let objects = vec![
        blank(U_PROCESSES, &["process_uid", "activity"]),
        blank(U_LINKS, &["source_uid", "attribute", "target", "target_table"]),
        blank(U_MAPPING, &["nwdf_uid", "role", "epc_uri"]),
    ];
    let tables = tables_of(&objects);
    assert!(NwdfTables.recognises(&tables));
    // Reading is another matter: the columns carry nothing, and it says so.
    assert!(matches!(
        NwdfTables.read(&tables, &NoObjects),
        Err(VerbatimRefusal::Unreadable { .. })
    ));
    // Without the mapping, no recognition.
    assert!(!NwdfTables.recognises(&tables[..2]));
}

#[test]
fn two_link_tables_are_refused_not_merged() {
    let mut objects = nwdf_objects();
    objects.push(links_table("00000000-0000-4000-8000-0000000000ff", &links_rows()));
    let refusal = read(&objects).expect_err("two links tables");
    assert_eq!(
        refusal,
        VerbatimRefusal::SeveralTables {
            role: "links",
            tables: vec![
                U_LINKS.to_string(),
                "00000000-0000-4000-8000-0000000000ff".to_string()
            ],
        }
    );
}

/// A processes table that has no `activity` column is not the processes table
/// of the convention: not recognised, and read directly it names the column.
#[test]
fn a_missing_column_is_named() {
    let without_activity = text_table(
        U_PROCESSES,
        &["process_uid", "process_name"],
        &[vec!["las_survey_read", "LAS survey read"]],
    );
    let objects = vec![
        without_activity,
        links_table(U_LINKS, &links_rows()),
        mapping_table(&mapping_rows()),
    ];
    assert!(!NwdfTables.recognises(&tables_of(&objects)));
    assert_eq!(
        read(&objects).expect_err("no activity column"),
        VerbatimRefusal::MissingColumn {
            table: U_PROCESSES.to_string(),
            column: "activity".to_string(),
        }
    );
}

#[test]
fn a_column_titled_twice_is_ambiguous() {
    let mut processes_object = processes_table(&processes_rows());
    processes_object.column.push(column("activity", &["x", "y", "z"]));
    let objects = vec![
        processes_object,
        links_table(U_LINKS, &links_rows()),
        mapping_table(&mapping_rows()),
    ];
    assert_eq!(
        read(&objects).expect_err("two activity columns"),
        VerbatimRefusal::AmbiguousColumn {
            table: U_PROCESSES.to_string(),
            column: "activity".to_string(),
        }
    );
}

// -- the registry -----------------------------------------------------------------

/// A source of a caller's, which recognises every set of tables.
struct Everything;

impl VerbatimSource for Everything {
    fn id(&self) -> VerbatimId {
        VerbatimId("test")
    }
    fn recognises(&self, _tables: &[Table<'_>]) -> bool {
        true
    }
    fn read(
        &self,
        _tables: &[Table<'_>],
        _arrays: &dyn energyml_domain::source::ArraySource,
    ) -> Result<Vec<VerbatimProcess>, VerbatimRefusal> {
        Ok(Vec::new())
    }
}

/// ADR-0006: a caller's source is tried before the built-ins.
#[test]
fn a_callers_source_is_tried_before_the_built_in() {
    let objects = nwdf_objects();
    let tables = tables_of(&objects);
    let mut sources = VerbatimSources::new();
    assert_eq!(sources.ids(), [NWDF]);
    sources.add(Box::new(Everything));
    assert_eq!(sources.ids(), [VerbatimId("test"), NWDF]);
    let found: Vec<VerbatimId> = sources
        .recognised(&tables)
        .iter()
        .map(|source| source.id())
        .collect();
    assert_eq!(found, [VerbatimId("test"), NWDF]);

    assert!(VerbatimSources::empty().recognised(&tables).is_empty());
    // Tables that follow no convention are recognised by no built-in source.
    let other = vec![text_table(U_REFERENCES, &["a", "b"], &[vec!["1", "2"]])];
    assert!(VerbatimSources::new().recognised(&tables_of(&other)).is_empty());
}
