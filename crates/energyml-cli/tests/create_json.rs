//! `energyml create --list --json` — what a script reads to learn what each
//! operation takes, asks and gives (07/10/2026, for the workflow server).
//!
//! The JSON is assembled by `energyml_build::describe`; this test is on the
//! binary because the contract is the stdout: **only** the JSON goes there.

use std::process::Command;

use serde_json::Value;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_energyml"))
        .arg("create")
        .args(args)
        .output()
        .expect("runs")
}

#[test]
fn list_json_prints_one_parsable_array_with_the_gives() {
    let out = run(&["--list", "--json"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // Stdout is the document and nothing else: a script parses it whole.
    let all: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let all = all.as_array().expect("an array");
    let contacts = all
        .iter()
        .find(|one| one["name"] == "contacts-from-frames")
        .expect("contacts-from-frames is listed");
    let gives = contacts["gives"].as_array().expect("gives");
    assert!(
        gives
            .iter()
            .any(|o| o["role"] == "representation" && o["types"][0] == "resqml22.PointSetRepresentation")
    );
    assert!(contacts["takes"].as_array().is_some_and(|t| !t.is_empty()));
    assert!(contacts["needs"].is_array());
    // An operation that declares nothing yet is still listed, with an empty list.
    assert!(
        all.iter()
            .any(|one| one["gives"].as_array().is_some_and(|g| g.is_empty()))
    );
}

#[test]
fn naming_an_operation_prints_that_one_object() {
    let out = run(&["--list", "--json", "contacts-from-frames"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let one: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_eq!(one["name"], "contacts-from-frames");
    assert!(one["gives"].as_array().is_some_and(|g| !g.is_empty()));
}

#[test]
fn an_unknown_operation_is_an_error_and_stdout_stays_empty() {
    let out = run(&["--list", "--json", "no-such-operation"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no-such-operation"));
}

#[test]
fn json_without_list_is_refused() {
    assert!(!run(&["--json"]).status.success());
}
