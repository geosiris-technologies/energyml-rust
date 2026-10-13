//! External subcommands: `energyml foo …` runs the program `energyml-foo`.
//!
//! The way `git` and `cargo` do it. Every other tool of the family is a
//! separate, self-contained binary that can be installed and published on its
//! own, and the user still has one command to remember. Nothing in this
//! crate names those tools' crates, so a public build never depends on them.
//!
//! Two words are aliases, because the program they stand for does not follow
//! the `energyml-<word>` pattern: `etp` runs `retp`, `translate` runs
//! `energyml-translate`.
//!
//! The lookup is pure - [`find_in`] and [`list_in`] take the `PATH` as an
//! argument - so it is tested without spawning anything.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Result, bail};

/// The prefix of an external program's name: the word `foo` is `energyml-foo`.
const PREFIX: &str = "energyml-";

/// The words whose program is not `energyml-<word>`, with the program they run.
pub const ALIASES: &[(&str, &str)] = &[("etp", "retp"), ("translate", "energyml-translate")];

/// What to install when the program is missing, by program name.
const INSTALL: &[(&str, &str)] = &[
    ("retp", "cargo install retp-cli"),
    ("energyml-translate", "cargo install energyml-translate-cli"),
];

/// The program a subcommand word stands for: the alias if there is one,
/// `energyml-<word>` otherwise.
pub fn executable_name(word: &str) -> String {
    match ALIASES.iter().find(|(alias, _)| *alias == word) {
        Some((_, program)) => (*program).to_string(),
        None => format!("{PREFIX}{word}"),
    }
}

/// The word a program is run under: the reverse of [`executable_name`].
/// `None` for a program that is not named `energyml-<word>` and is not an
/// alias's target.
pub fn word_of(program: &str) -> Option<String> {
    if let Some((alias, _)) = ALIASES.iter().find(|(_, target)| *target == program) {
        return Some((*alias).to_string());
    }
    program
        .strip_prefix(PREFIX)
        .filter(|word| !word.is_empty())
        .map(str::to_string)
}

/// The sentence that says how to get a program that is not installed.
pub fn install_hint(program: &str) -> String {
    match INSTALL.iter().find(|(name, _)| *name == program) {
        Some((_, command)) => format!("install it with `{command}`"),
        None => format!("install a program named `{program}` and put it on the PATH"),
    }
}

/// Whether `path` is a file this process may run.
fn is_runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The first file called `program` (plus the platform's executable suffix) in
/// the directories of `path`, a `PATH`-style list.
pub fn find_in(program: &str, path: &OsStr) -> Option<PathBuf> {
    let file = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path)
        .map(|dir| dir.join(&file))
        .find(|candidate| is_runnable(candidate))
}

/// [`find_in`] over this process's `PATH`.
pub fn find(program: &str) -> Option<PathBuf> {
    find_in(program, &std::env::var_os("PATH")?)
}

/// Every external subcommand on `path`: the word, and the program found for it,
/// sorted by word, each word once (the first directory of the `PATH` wins).
pub fn list_in(path: &OsStr) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for dir in std::env::split_paths(path) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            let program = name.strip_suffix(std::env::consts::EXE_SUFFIX).unwrap_or(name);
            let Some(word) = word_of(program) else {
                continue;
            };
            // The name must be exactly what `executable_name` gives the word:
            // a stale `energyml-etp` is not the program of `etp`, `retp` is.
            if executable_name(&word) != program || !is_runnable(&entry.path()) {
                continue;
            }
            if !found.iter().any(|(known, _)| *known == word) {
                found.push((word, entry.path()));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// The block that closes `energyml --help`: the external subcommands found on
/// the `PATH`, or `None` when there are none.
pub fn help_section() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    section_of(&list_in(&path))
}

/// The text of the block for a list of subcommands. Split from
/// [`help_section`] so that it is tested without a `PATH`.
pub fn section_of(found: &[(String, PathBuf)]) -> Option<String> {
    if found.is_empty() {
        return None;
    }
    let mut out = String::from("External commands found on the PATH (`energyml <command> --help`):\n");
    for (word, path) in found {
        let program = executable_name(word);
        out.push_str(&format!("  {word:<16}runs {program} ({})\n", path.display()));
    }
    Some(out.trim_end().to_string())
}

/// Run the external subcommand `words[0]` with the rest of the line, with this
/// process's standard streams, and return its exit code.
///
/// A program that is not installed is an error that names it and says how to
/// get it; a program killed by a signal exits 1.
pub fn run(words: &[OsString]) -> Result<ExitCode> {
    let Some((word, rest)) = words.split_first() else {
        bail!("no command given");
    };
    let word = word.to_string_lossy();
    let program = executable_name(&word);
    let Some(path) = find(&program) else {
        bail!(
            "`{word}` is not a command of `energyml`, and no program `{program}` was found on the PATH: {}",
            install_hint(&program)
        );
    };
    let status = Command::new(&path)
        .args(rest)
        .status()
        .map_err(|why| anyhow::anyhow!("cannot run {}: {why}", path.display()))?;
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::from(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory with the given (empty) files in it. Files are made
    /// executable on Unix, where the lookup checks the bit.
    fn dir_with(tag: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("energyml-external-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for file in files {
            let path = dir.join(format!("{file}{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&path, b"").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        dir
    }

    fn path_of(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[test]
    fn a_word_is_the_program_with_the_prefix() {
        assert_eq!(executable_name("foo"), "energyml-foo");
        assert_eq!(executable_name("lutetia"), "energyml-lutetia");
    }

    #[test]
    fn the_two_aliases_name_their_program() {
        assert_eq!(executable_name("etp"), "retp");
        assert_eq!(executable_name("translate"), "energyml-translate");
    }

    #[test]
    fn a_program_goes_back_to_its_word() {
        assert_eq!(word_of("retp").as_deref(), Some("etp"));
        assert_eq!(word_of("energyml-translate").as_deref(), Some("translate"));
        assert_eq!(word_of("energyml-foo").as_deref(), Some("foo"));
        assert_eq!(word_of("energyml-"), None);
        assert_eq!(word_of("cargo"), None);
    }

    #[test]
    fn the_hint_names_what_to_install() {
        assert!(install_hint("retp").contains("cargo install retp-cli"));
        assert!(install_hint("energyml-translate").contains("energyml-translate-cli"));
        assert!(install_hint("energyml-foo").contains("energyml-foo"));
    }

    #[test]
    fn the_first_directory_of_the_path_wins() {
        let first = dir_with("first", &["energyml-foo"]);
        let second = dir_with("second", &["energyml-foo", "energyml-bar"]);
        let path = path_of(&[&first, &second]);
        assert_eq!(
            find_in("energyml-foo", &path),
            Some(first.join(format!("energyml-foo{}", std::env::consts::EXE_SUFFIX)))
        );
        assert!(find_in("energyml-bar", &path).is_some());
        assert_eq!(find_in("energyml-baz", &path), None);
    }

    #[test]
    fn a_directory_is_not_a_program() {
        let dir = dir_with("directory", &[]);
        std::fs::create_dir_all(dir.join(format!("energyml-foo{}", std::env::consts::EXE_SUFFIX))).unwrap();
        assert_eq!(find_in("energyml-foo", &path_of(&[&dir])), None);
    }

    #[test]
    fn the_listing_gives_words_sorted_once_with_the_aliases() {
        let first = dir_with("list-a", &["energyml-zeta", "retp", "cargo", "energyml-alpha"]);
        let second = dir_with(
            "list-b",
            &["energyml-alpha", "energyml-translate", "energyml-etp"],
        );
        let words: Vec<String> = list_in(&path_of(&[&first, &second]))
            .into_iter()
            .map(|(word, _)| word)
            .collect();
        // `energyml-etp` is not the program of `etp` (that is `retp`), so it is
        // not listed under it; `cargo` is not ours.
        assert_eq!(words, ["alpha", "etp", "translate", "zeta"]);
    }

    #[test]
    fn nothing_found_prints_no_section() {
        let empty = dir_with("empty", &[]);
        assert_eq!(section_of(&list_in(&path_of(&[&empty]))), None);
    }

    #[test]
    fn the_section_names_each_command_and_its_program() {
        let dir = dir_with("section", &["retp"]);
        let section = section_of(&list_in(&path_of(&[&dir]))).expect("one command");
        assert!(section.starts_with("External commands"));
        assert!(section.contains("etp"));
        assert!(section.contains("runs retp"));
    }

    #[test]
    fn a_missing_program_is_an_error_that_names_it() {
        let words = [OsString::from("surely-not-installed-xyz")];
        let message = run(&words).unwrap_err().to_string();
        assert!(message.contains("energyml-surely-not-installed-xyz"), "{message}");
    }
}
