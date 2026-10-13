//! **A Galaxy tool, written from an operation's declaration** - the tool XML a
//! Galaxy server runs to call one operation through the `energyml` image.
//!
//! The tool XML is generated rather than written by hand, and it reuses the
//! `energyml` image already published: no extra container is needed, only the
//! runner script that the tool XML calls inside that image.
//!
//! **The form is rendered, never written**, the rule the front end's creation door
//! follows: what the operation takes ([`AnyOperation::takes`]) becomes Galaxy's
//! `data` inputs, what it needs ([`AnyOperation::needs`] on an empty bag)
//! becomes its parameters, and the `<command>` hands both to the runner
//! (`scripts/galaxy/energyml-galaxy.py`, in the image) that calls
//! `energyml create`. An operation added to the registry has its tool without a
//! line written for it.
//!
//! **Outputs by role**:
//! every tool has a `manifest` output (JSON, what the run wrote), and an
//! operation that declares [`AnyOperation::gives`] gets one `uris_<role>` file
//! per role (`-` becomes `_`), written by the runner from the manifest. An
//! operation that declares nothing has the manifest only - its roles are not
//! known in advance - and its help says so. `uris_out` and `uris_type` stay.
//!
//! What a Galaxy tool cannot say is **left out and said** in the tool's help: a
//! ramp, a field or a lattice are pieces a front end builds, not files a workflow
//! passes. A slot that takes objects of the package becomes two optional
//! inputs - URIs typed in, or a file of URIs written by an upstream step - and
//! one that takes every object when left empty says so.

use std::fmt::Write as _;

use crate::operation::{AnyOperation, Need, Otherwise, Slot};
use crate::pieces::Pieces;

/// What the tool is bound to: the image it runs in and where the runner is.
#[derive(Debug, Clone)]
pub struct ToolOptions {
    /// The container - the published `energyml` image.
    pub image: String,
    /// The runner's path inside the image.
    pub runner: String,
    /// The tool's version, Galaxy's `version` attribute.
    pub version: String,
}

impl Default for ToolOptions {
    fn default() -> Self {
        Self {
            image: "ghcr.io/geosiris-technologies/energyml:latest".to_string(),
            runner: "/opt/energyml/scripts/galaxy/energyml-galaxy.py".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// The setting the runner fixes itself - the HDF5 it writes and copies out.
const RUNNER_OWNS: &str = "dataFile";

/// The file kind a piece of this sort is staged as - what the runner gives the
/// file as its extension, since Galaxy names every dataset `.dat`. `None`: not a
/// file a workflow passes.
fn file_kind(kind: &str) -> Option<(&'static str, &'static str)> {
    // (Galaxy's datatype, the extension the command reads it by)
    match kind {
        "table" => Some(("csv", "csv")),
        "features" => Some(("json", "geojson")),
        "document" => Some(("data", "dat")),
        _ => None,
    }
}

/// A Galaxy parameter name: letters, digits and `_` only.
fn ident(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// XML text, escaped.
fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A scalar default for a parameter, from the slot's suggestion on an empty bag.
fn default_of(slot: &Slot) -> Option<String> {
    match slot.suggested.as_ref()? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The tool XML for `operation`.
pub fn tool_xml(operation: &dyn AnyOperation, options: &ToolOptions) -> String {
    let name = operation.name();
    let id = format!("energyml_{}", ident(name));
    let mut inputs = String::new();
    let mut command = format!("python3 '{}' run '{}'\n", options.runner, name);
    let mut left_out: Vec<String> = Vec::new();
    let mut takes_tables = false;
    // The input package is required only when a slot **must** be given objects
    // of it: optional slots, and those that take every object when left empty,
    // leave it optional - and without it the operation writes a new package.
    let mut needs_package = false;

    // What it takes: files, or objects of the input package.
    for slot in operation.takes() {
        let Need::Input(takes) = &slot.need else { continue };
        let param = ident(slot.name);
        let optional = slot.least == 0 || matches!(slot.otherwise, Otherwise::Every);
        if let Some((format, extension)) = file_kind(takes.kind) {
            takes_tables |= takes.kind == "table";
            let _ = writeln!(
                inputs,
                r#"        <param name="{param}" type="data" format="{format}" multiple="{}" optional="{optional}" label="{}" help="{}" />"#,
                slot.takes_many(),
                esc(slot.name),
                esc(slot.about),
            );
            let _ = write!(
                command,
                "#if ${param}\n#for $f in ${param}\n --input '{}:{extension}=${{f}}::${{f.element_identifier}}'\n#end for\n#end if\n",
                slot.name
            );
        } else if matches!(takes.kind, "shape" | "object") {
            needs_package |= !optional;
            let every = if matches!(slot.otherwise, Otherwise::Every) {
                " Left empty: every object of the input package that fits."
            } else {
                ""
            };
            let types = if takes.types.is_empty() {
                String::new()
            } else {
                format!(" ({})", takes.types.join(", "))
            };
            let _ = writeln!(
                inputs,
                r#"        <param name="{param}" type="text" value="" optional="true" label="{} - URIs" help="{}{}: URIs of the input package, comma-separated.{every}" />"#,
                esc(slot.name),
                esc(slot.about),
                esc(&types),
            );
            let _ = writeln!(
                inputs,
                r#"        <param name="{param}_uris" type="data" format="txt" optional="true" label="{} - URI file" help="A file of URIs, one per line - what an upstream step's URI output holds." />"#,
                esc(slot.name),
            );
            let _ = write!(
                command,
                "#if str(${param}).strip() not in ('', 'None')\n --object '{n}=${param}'\n#end if\n#if ${param}_uris\n --object '{n}=@${param}_uris'\n#end if\n",
                n = slot.name
            );
        } else {
            left_out.push(format!(
                "`{}` ({}): a piece a front end builds, not a file a workflow passes",
                slot.name, takes.kind
            ));
        }
    }
    if takes_tables {
        let _ = writeln!(
            inputs,
            r#"        <param name="template" type="data" format="json" optional="false" label="Columns template" help="What the tables' columns mean: an OSDU ColumnBasedTableTemplate, a File.TabularData, or the bare list of column definitions." />"#
        );
        command.push_str(" --template '$template'\n");
    }

    // What it needs: its parameters.
    for slot in operation.needs(&Pieces::empty()) {
        // `dataFile` is the runner's: it names the HDF5 it copies to Galaxy's output.
        if matches!(slot.need, Need::Input(_)) || slot.name == RUNNER_OWNS {
            continue;
        }
        let param = ident(slot.name);
        let label = esc(slot.name);
        let help = esc(slot.about);
        let value = default_of(&slot);
        let optional = slot.least == 0;
        let line = match &slot.need {
            Need::Number { least, most, uom } => {
                let unit = uom.map(|u| format!(" ({u})")).unwrap_or_default();
                format!(
                    r#"<param name="{param}" type="float" value="{}" optional="{optional}"{}{} label="{label}{}" help="{help}" />"#,
                    esc(value.as_deref().unwrap_or("")),
                    least.map(|v| format!(r#" min="{v}""#)).unwrap_or_default(),
                    most.map(|v| format!(r#" max="{v}""#)).unwrap_or_default(),
                    esc(&unit),
                )
            }
            Need::Integer { least, most } => format!(
                r#"<param name="{param}" type="integer" value="{}" optional="{optional}"{}{} label="{label}" help="{help}" />"#,
                esc(value.as_deref().unwrap_or("")),
                least.map(|v| format!(r#" min="{v}""#)).unwrap_or_default(),
                most.map(|v| format!(r#" max="{v}""#)).unwrap_or_default(),
            ),
            Need::Epsg => format!(
                r#"<param name="{param}" type="integer" value="{}" optional="{optional}" min="1" label="{label} (EPSG)" help="{help}" />"#,
                esc(value.as_deref().unwrap_or("")),
            ),
            Need::Flag => format!(
                r#"<param name="{param}" type="boolean" checked="{}" truevalue="true" falsevalue="false" label="{label}" help="{help}" />"#,
                value.as_deref() == Some("true"),
            ),
            Need::Choice(choices) => {
                let mut select = format!(
                    r#"<param name="{param}" type="select" optional="{optional}" label="{label}" help="{help}">"#
                );
                for choice in *choices {
                    let selected = if value.as_deref() == Some(*choice) {
                        r#" selected="true""#
                    } else {
                        ""
                    };
                    let _ = write!(
                        select,
                        r#"<option value="{c}"{selected}>{c}</option>"#,
                        c = esc(choice)
                    );
                }
                select.push_str("</param>");
                select
            }
            // Free text, a title, a term of a vocabulary, and an object of the
            // package named by its URI.
            _ => format!(
                r#"<param name="{param}" type="text" value="{}" optional="{optional}" label="{label}" help="{help}" />"#,
                esc(value.as_deref().unwrap_or("")),
            ),
        };
        let _ = writeln!(inputs, "        {line}");
        if matches!(slot.need, Need::Flag) {
            let _ = writeln!(command, " --set '{}=${param}'", slot.name);
        } else {
            // **`None` is not a value.** An optional parameter left unset - a
            // workflow saved before the parameter existed, a run through the API
            // that does not give it - reaches Cheetah as `None`, which `str()`
            // spells "None": a guard on emptiness alone passed `--set
            // language=None`. Unset, the
            // operation's own default applies.
            let _ = write!(
                command,
                "#if str(${param}).strip() not in ('', 'None')\n --set '{}=${param}'\n#end if\n",
                slot.name
            );
        }
    }

    // The package it adds to, and where the result goes.
    let package_optional = !needs_package;
    let _ = writeln!(
        inputs,
        r#"        <param name="epc_in" type="data" format="epc" optional="{package_optional}" label="Input EPC" help="The package to add to - its objects are the inputs named by URI." />"#
    );
    let _ = writeln!(
        inputs,
        r#"        <param name="h5_in" type="data" format="h5" optional="true" label="Input HDF5" help="The arrays of the input EPC." />"#
    );
    command.push_str("#if $epc_in\n --epc-in '$epc_in'\n#end if\n#if $h5_in\n --h5-in '$h5_in'\n#end if\n");
    // Which of the objects made the URI output lists: what a downstream step
    // reads (the point sets of `contacts-from-frames`, one surface each).
    let mut kinds = String::from(r#"<option value="" selected="true">every object made</option>"#);
    for made in operation.makes() {
        let class = made.rsplit('.').next().unwrap_or(made);
        let _ = write!(kinds, r#"<option value="{c}">{c}</option>"#, c = esc(class));
    }
    let _ = writeln!(
        inputs,
        r#"        <param name="uris_type" type="select" label="URIs listed" help="Which objects the URI output lists, one per line.">{kinds}</param>"#
    );
    command.push_str("#if str($uris_type).strip() not in ('', 'None')\n --uris-type '$uris_type'\n#end if\n");
    command.push_str(" --epc-out '$epc_out' --h5-out '$h5_out' --uris-out '$uris_out' --log '$log'\n");

    // What it gives, by role: the manifest is
    // always there; one URI file per declared role, so a workflow wires the
    // right output to the next step instead of filtering `uris_out` by type.
    let gives = operation.gives();
    command.push_str(" --manifest-out '$manifest'\n");
    let mut outputs = String::new();
    let mut role_help = String::new();
    for output in &gives {
        let role = ident(output.role);
        let _ = writeln!(
            command,
            " --role-out '{}=$uris_{role}'",
            output.role.replace('\'', "")
        );
        let _ = writeln!(
            outputs,
            r#"        <data name="uris_{role}" format="txt" label="[Output] URIs of {} - {}" />"#,
            esc(output.role),
            esc(output.describe),
        );
        let _ = writeln!(
            role_help,
            "- `{}`: {} ({})",
            output.role,
            output.describe,
            output.types.join(", ")
        );
    }
    let roles = if gives.is_empty() {
        "\n\n**Outputs**: the `manifest` (JSON) lists every object the run wrote, by role. This operation does not declare its roles in advance, so it has no per-role URI output; read them from the manifest.".to_string()
    } else {
        format!(
            "\n\n**Outputs**: the `manifest` (JSON) lists every object the run wrote, by role; each role below also has its own URI file, one URI per line (empty when the run wrote nothing under it):\n\n{role_help}"
        )
    };

    let left = if left_out.is_empty() {
        String::new()
    } else {
        format!("\n\n**Not offered here**: {}.", left_out.join("; "))
    };
    format!(
        r#"<tool id="{id}" name="energyml - {name}" version="{version}" profile="21.05">
    <description>{describe}</description>
    <!-- Written by `energyml galaxy-tool {name}` from the operation's declaration; regenerate rather than edit. -->
    <requirements>
        <container type="docker">{image}</container>
    </requirements>
    <command detect_errors="exit_code"><![CDATA[
{command}    ]]></command>
    <inputs>
{inputs}    </inputs>
    <outputs>
        <data name="epc_out" format="epc" label="[Output] EPC of ${{tool.name}}" />
        <data name="h5_out" format="h5" label="[Output] HDF5 of ${{tool.name}}" />
        <data name="uris_out" format="txt" label="[Output] URIs of the objects made" />
        <data name="manifest" format="json" label="[Output] Manifest of ${{tool.name}}" />
{outputs}        <data name="log" format="txt" label="[Output] Log of ${{tool.name}}" />
    </outputs>
    <help><![CDATA[
**{name}** - {describe_raw}

Makes: {makes}.{left}{roles}
    ]]></help>
</tool>
"#,
        version = esc(&options.version),
        describe = esc(operation.describe()),
        describe_raw = operation.describe(),
        image = esc(&options.image),
        makes = operation.makes().join(", "),
    )
}
