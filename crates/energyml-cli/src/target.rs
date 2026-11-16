//! The two steps every writing command shares: reading a `--set name=value`
//! and opening the package to write into.
//!
//! `create`, `import` and `terrain` each carried their own copy of both, and
//! the copies had drifted (a list value was accepted by one and became a
//! literal string in another; one of them forgot that a bare `--out` name is in
//! the current folder). One copy, here.

use std::path::Path;

use anyhow::{Context, Result, bail};
use energyml_storage::EpcStore;

/// Read one `--set name=value`.
///
/// The value is JSON when it is a number, a boolean or a list (a slot that
/// takes several values is answered `--set 'units=["Cretaceous","Jurassic"]'`),
/// and the text as typed otherwise.
pub fn parse_choice(one: &str) -> Result<(String, serde_json::Value)> {
    let Some((name, value)) = one.split_once('=') else {
        bail!("`--set {one}`: expected `name=value`");
    };
    let value = match serde_json::from_str(value) {
        Ok(
            json @ (serde_json::Value::Number(_) | serde_json::Value::Bool(_) | serde_json::Value::Array(_)),
        ) => json,
        _ => serde_json::Value::String(value.to_string()),
    };
    Ok((name.to_string(), value))
}

/// Open the package a command writes into: `--into` when given, an empty
/// package otherwise, with its external files rooted beside `out`.
///
/// **A bare name is in the current folder**, and the store has to be told:
/// without a root, `--out package.epc` wrote the package and refused each of
/// its arrays (`data.h5`: file not found), two thousand lines of refusals for a
/// file that looks as if it came out fine.
pub fn open(into: Option<&Path>, out: &Path) -> Result<EpcStore> {
    open_named(into, &package_name(out), out)
}

/// The name a package written to `out` takes: its file name, else the path as written.
pub fn package_name(out: &Path) -> String {
    out.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| out.display().to_string())
}

/// [`open`], for a command that names the package itself.
pub fn open_named(into: Option<&Path>, name: &str, out: &Path) -> Result<EpcStore> {
    let mut store = energyml_build::epc::open(into, name, std::sync::Arc::new(super::registry::all()))
        .with_context(|| match into {
            Some(path) => format!("opening {}", path.display()),
            None => "making an empty package".to_string(),
        })?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    store.external_files_mut().set_root(parent);
    Ok(store)
}

/// Save the package to `out`.
pub fn save(store: &EpcStore, out: &Path) -> Result<()> {
    store
        .save_as(out, &energyml_storage::SaveOptions::default())
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_value_is_a_list_in_every_command() {
        let (name, value) = parse_choice(r#"units=["Cretaceous","Jurassic"]"#).expect("parses");
        assert_eq!(name, "units");
        assert!(value.is_array());
        assert_eq!(parse_choice("n=3").expect("parses").1, serde_json::json!(3));
        assert_eq!(parse_choice("t=abc").expect("parses").1, serde_json::json!("abc"));
        assert!(parse_choice("no-equals").is_err());
        // Break it: drop the `Array(_)` arm from `parse_choice`.
    }
}
