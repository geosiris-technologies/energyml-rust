//! The Galaxy tool of an operation, written from its declaration
//! (`energyml_build::galaxy`). The need: could a script or Rust code
//! generate a Galaxy tool xml
//! for a given operation?
//!
//! What would have to break: a variable the `<command>` uses that no input or
//! output declares — Galaxy renders it as the empty string, and the runner gets
//! an argument with nothing after it; or `dataFile` offered as a parameter,
//! which the runner fixes itself (the HDF5 it copies to Galaxy's output).

use energyml_build::galaxy::{ToolOptions, tool_xml};
use energyml_build::operation::{
    AnyOperation, Choices, Error, Fitness, Identity, Output, Plan, Registry, Slot, When,
};
use energyml_build::pieces::Pieces;

/// An operation declared **here**, not shipped: a stand-in in the registry
/// would be a tool someone installs. It writes nothing; only its declaration
/// of what it gives is looked at.
struct Declares(Vec<Output>);

impl AnyOperation for Declares {
    fn name(&self) -> &'static str {
        "declares-roles"
    }
    fn describe(&self) -> &'static str {
        "declares what it gives"
    }
    fn makes(&self) -> &'static [&'static str] {
        &[]
    }
    fn recipe(&self) -> &'static str {
        "declares-roles"
    }
    fn fitness(&self, _pieces: &Pieces) -> Fitness {
        Fitness::Fit
    }
    fn needs(&self, _pieces: &Pieces) -> Vec<Slot> {
        Vec::new()
    }
    fn gives(&self) -> Vec<Output> {
        self.0.clone()
    }
    fn plan(&self, _: &Pieces, _: &Choices, _: &Identity, _: &When) -> Result<Plan, Error> {
        Err(Error::new("a test operation is never planned"))
    }
}

fn two_roles() -> Declares {
    Declares(vec![
        Output {
            role: "representation",
            types: &["resqml22.PointSetRepresentation"],
            least: 0,
            most: None,
            describe: "the point sets",
        },
        Output {
            role: "category-lookup",
            types: &["eml23.ColumnBasedTable"],
            least: 0,
            most: Some(1),
            describe: "the codes",
        },
    ])
}

/// Every `$name` / `${name…}` the command uses, loop variables and Galaxy's own left out.
fn variables(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = command.as_bytes();
    let mut at = 0;
    while let Some(found) = command[at..].find('$') {
        let mut i = at + found + 1;
        if bytes.get(i) == Some(&b'{') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let name = &command[start..i];
        if !name.is_empty() && !matches!(name, "f" | "tool") {
            out.push(name.to_string());
        }
        at = i.max(at + found + 1);
    }
    out
}

fn section<'a>(xml: &'a str, open: &str, close: &str) -> &'a str {
    let from = xml.find(open).expect("the section is there") + open.len();
    let to = xml[from..].find(close).expect("it closes") + from;
    &xml[from..to]
}

#[test]
fn every_variable_of_every_tools_command_is_declared() {
    let registry = Registry::new();
    let options = ToolOptions::default();
    let mut tools = 0;
    for operation in registry.iter() {
        let xml = tool_xml(operation, &options);
        let command = section(
            &xml,
            "<command detect_errors=\"exit_code\"><![CDATA[",
            "]]></command>",
        );
        let declared =
            section(&xml, "<inputs>", "</inputs>").to_string() + section(&xml, "<outputs>", "</outputs>");
        for name in variables(command) {
            assert!(
                declared.contains(&format!("name=\"{name}\"")),
                "`{}`: the command uses `${name}`, which no input or output declares",
                operation.name()
            );
        }
        assert!(
            !declared.contains("name=\"dataFile\""),
            "`{}`: dataFile is the runner's",
            operation.name()
        );
        tools += 1;
    }
    assert!(tools > 50, "the registry has its operations: {tools}");
}

/// A reported error: *“error: `language`: expected one of fr, en, got
/// "None"; `textColumns`: … got "None"”*. An optional parameter left unset
/// reaches Cheetah as `None`, spelt "None" by `str()`; every guard that passes
/// a value on must refuse it, or the operation is told a choice nobody made.
///
/// To make it fail: drop `not in ('', 'None')` from the `--set` guard.
#[test]
fn no_guard_lets_an_unset_parameter_through_as_none() {
    let registry = Registry::new();
    let options = ToolOptions::default();
    let mut guards = 0;
    for operation in registry.iter() {
        let xml = tool_xml(operation, &options);
        let command = section(
            &xml,
            "<command detect_errors=\"exit_code\"><![CDATA[",
            "]]></command>",
        );
        for line in command.lines().filter(|line| line.starts_with("#if str(")) {
            assert!(
                line.ends_with(".strip() not in ('', 'None')"),
                "`{}`: `{line}` lets \"None\" through",
                operation.name()
            );
            guards += 1;
        }
    }
    assert!(guards > 50, "the guards were seen: {guards}");
}

#[test]
fn a_table_input_takes_every_file_given_and_the_template_beside_it() {
    let registry = Registry::new();
    let operation = registry
        .get("wells-and-properties-from-table")
        .expect("the import");
    let xml = tool_xml(operation, &ToolOptions::default());
    assert!(
        xml.contains(r#"<param name="table" type="data" format="csv" multiple="true""#),
        "{xml}"
    );
    assert!(
        xml.contains("--input 'table:csv=${f}::${f.element_identifier}'"),
        "each file, under the name Galaxy shows"
    );
    assert!(xml.contains(r#"<param name="template" type="data" format="json""#));
    assert!(xml.contains("--template '$template'"));
}

#[test]
fn an_object_slot_left_empty_says_it_takes_every_object_and_the_uris_can_be_filtered() {
    let registry = Registry::new();
    let operation = registry.get("contacts-from-frames").expect("the contacts");
    let xml = tool_xml(operation, &ToolOptions::default());
    assert!(xml.contains(r#"<param name="frames" type="text""#));
    assert!(xml.contains("Left empty: every object of the input package that fits."));
    assert!(
        xml.contains(r#"<param name="frames_uris" type="data" format="txt""#),
        "an upstream step's URI file"
    );
    assert!(
        xml.contains(r#"<option value="PointSetRepresentation">"#),
        "the URI output can list the point sets alone"
    );
    assert!(
        xml.contains(r#"<param name="wellOrigin" type="boolean" checked="true""#),
        "on, as the operation declares"
    );
}

/// The need: run a translation of a csv, optionally passing
/// a source epc+h5 to complete, and get
/// an epc + h5 either new or completed — the input package is optional when
/// every object slot is (here `wells` and `lookups`, which take every object
/// when left empty), required only when a slot must be given objects of it.
#[test]
fn the_input_package_is_optional_unless_a_slot_must_be_given_its_objects() {
    let registry = Registry::new();
    let import = tool_xml(
        registry
            .get("wells-and-properties-from-table")
            .expect("the import"),
        &ToolOptions::default(),
    );
    assert!(
        import.contains(r#"<param name="epc_in" type="data" format="epc" optional="true""#),
        "a new package or a completed one"
    );
    let mut required = 0;
    for operation in registry.iter() {
        let xml = tool_xml(operation, &ToolOptions::default());
        required +=
            usize::from(xml.contains(r#"<param name="epc_in" type="data" format="epc" optional="false""#));
    }
    assert!(
        required > 0,
        "an operation that must be given objects of a package requires one"
    );
}

/// The manifest is always an output; each
/// declared role is an output of its own, `-` written `_`, and the runner is
/// told where to write it. Every variable of that command is declared too.
#[test]
fn a_declared_role_is_an_output_and_the_runner_is_told_where_to_write_it() {
    let xml = tool_xml(&two_roles(), &ToolOptions::default());
    let outputs = section(&xml, "<outputs>", "</outputs>");
    assert!(
        outputs.contains(r#"<data name="manifest" format="json""#),
        "{outputs}"
    );
    assert!(outputs.contains(r#"<data name="uris_representation" format="txt" label="[Output] URIs of representation — the point sets""#), "{outputs}");
    assert!(outputs.contains(r#"<data name="uris_category_lookup" format="txt" label="[Output] URIs of category-lookup — the codes""#), "{outputs}");
    assert!(
        outputs.contains(r#"<data name="uris_out" "#),
        "the existing output stays"
    );
    let command = section(
        &xml,
        "<command detect_errors=\"exit_code\"><![CDATA[",
        "]]></command>",
    );
    assert!(command.contains("--manifest-out '$manifest'"), "{command}");
    assert!(
        command.contains("--role-out 'representation=$uris_representation'"),
        "{command}"
    );
    assert!(
        command.contains("--role-out 'category-lookup=$uris_category_lookup'"),
        "the role keeps its dash for the runner: {command}"
    );
    let declared = section(&xml, "<inputs>", "</inputs>").to_string() + outputs;
    for name in variables(command) {
        assert!(
            declared.contains(&format!("name=\"{name}\"")),
            "the command uses `${name}`, which nothing declares"
        );
    }
    assert!(
        xml.contains("each role below also has its own URI file"),
        "the help lists the roles"
    );
}

/// An operation that declares nothing has its roles unknown in advance: the
/// manifest only, and the help says why there is no URI file per role.
#[test]
fn an_operation_without_gives_has_the_manifest_only() {
    let xml = tool_xml(&Declares(Vec::new()), &ToolOptions::default());
    let outputs = section(&xml, "<outputs>", "</outputs>");
    assert!(outputs.contains(r#"<data name="manifest" format="json""#));
    assert_eq!(
        outputs.matches("<data name=\"uris_").count(),
        1,
        "`uris_out` alone: {outputs}"
    );
    let command = section(
        &xml,
        "<command detect_errors=\"exit_code\"><![CDATA[",
        "]]></command>",
    );
    assert!(command.contains("--manifest-out '$manifest'"));
    assert!(!command.contains("--role-out"), "{command}");
    assert!(
        xml.contains("does not declare its roles in advance"),
        "the help says it"
    );
}

/// The built-in operations that declare `gives()` get a file per role; none
/// of them may clash with the older `uris_out` (a role named `out`).
#[test]
fn no_role_output_takes_the_name_of_another_output() {
    let registry = Registry::new();
    for operation in registry.iter() {
        let xml = tool_xml(operation, &ToolOptions::default());
        let outputs = section(&xml, "<outputs>", "</outputs>");
        let mut names: Vec<&str> = outputs
            .split("<data name=\"")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap())
            .collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            total,
            "`{}`: two outputs share a name: {outputs}",
            operation.name()
        );
        assert!(
            names.contains(&"manifest"),
            "`{}`: no manifest output",
            operation.name()
        );
    }
}
