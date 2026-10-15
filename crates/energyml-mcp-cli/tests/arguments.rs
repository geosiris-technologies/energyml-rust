//! The command line: what is refused before serving, and nothing on stdout.

use crate::support::{TempDir, run_to_end, write_package};

#[test]
fn a_missing_package_ends_the_program_with_a_message_on_stderr() {
    let dir = TempDir::new("missing");
    let absent = dir.path().join("absent.epc");
    let (status, stdout, stderr) = run_to_end(&[absent.to_str().unwrap()], &[]);
    assert!(!status.success());
    assert!(
        stdout.is_empty(),
        "a failed start writes nothing on stdout: {stdout:?}"
    );
    assert!(stderr.contains("opening"), "{stderr}");
}

#[test]
fn a_limit_outside_the_accepted_range_is_refused() {
    let dir = TempDir::new("range");
    let file = write_package(dir.path());
    let file = file.to_str().unwrap();
    for args in [
        vec![file, "--timeout-seconds", "0"],
        vec![file, "--max-list", "0"],
        vec![file, "--max-window-elements", "0"],
        vec![file, "--max-response-bytes", "10"],
    ] {
        let (status, stdout, stderr) = run_to_end(&args, &[]);
        assert_eq!(status.code(), Some(2), "{args:?}: {stderr}");
        assert!(stdout.is_empty(), "{args:?}: {stdout:?}");
        assert!(
            stderr.contains("--"),
            "{args:?}: the message names the option: {stderr}"
        );
    }
}

#[test]
fn a_limit_from_the_environment_is_checked_like_one_from_the_command_line() {
    let dir = TempDir::new("env-range");
    let file = write_package(dir.path());
    let (status, _, stderr) = run_to_end(&[file.to_str().unwrap()], &[("ENERGYML_MCP_MAX_LIST", "0")]);
    assert_eq!(status.code(), Some(2), "{stderr}");
}

#[test]
fn help_lists_the_limits_and_the_h5_switch() {
    let (status, stdout, _) = run_to_end(&["--help"], &[]);
    assert!(status.success());
    for option in [
        "--h5",
        "--max-response-bytes",
        "--max-list",
        "--max-header-scan",
        "--max-window-elements",
        "--max-object-bytes",
        "--timeout-seconds",
    ] {
        assert!(stdout.contains(option), "{option} in:\n{stdout}");
    }
}
