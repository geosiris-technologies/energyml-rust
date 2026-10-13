//! Where a test finds Lutetia, the reference model.
//!
//! Lookup order, one place after the other:
//!
//! 1. `$ENERGYML_LUTETIA_DIR`, laid out like the public `energyml-lutetia`
//!    repository (the contract is written in that repository's README and in
//!    `scripts/fetch-lutetia.py` of this one):
//!    - `package/Lutetia.epc` (+ `Lutetia.h5`) — the **clean** model;
//!    - `package-with-errors/Lutetia-with-errors.epc` or `Lutetia.epc`
//!      (+ the `.h5` the EPC names) — the variant with deliberate errors;
//!    - `recipe/recette.toml` and `recipe/donnees/…`;
//!    - `recipe/energyml-version` (read by nobody here; it pins the CLI that replays);
//!    - `written-on`, optional: the platform stamp the replay writes, see [`Stamp`];
//! 2. the in-tree copies of the same four items, when a checkout carries them
//!    (**the clean package and the one with errors are filed under the other
//!    names there**).
//!
//! Nothing found: a panic when `$ENERGYML_FIXTURES_REQUIRED` is set — a missing
//! reference package is a failure in CI, never a skip — and otherwise a printed
//! note and `None`, the test returning.
//!
//! The lookup is the same as `energyml-cache/tests/lutetia.rs`; keep them alike.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The two packages the recipe is played into.
#[derive(Clone, Copy)]
pub enum Variant {
    /// Without the `erreur` group: `check` finds nothing.
    Clean,
    /// The whole recipe, `[ERROR]` objects included.
    WithErrors,
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn env_dir() -> Option<PathBuf> {
    std::env::var_os("ENERGYML_LUTETIA_DIR").map(PathBuf::from)
}

fn missing(what: &str) -> Option<PathBuf> {
    if std::env::var_os("ENERGYML_FIXTURES_REQUIRED").is_some() {
        panic!(
            "Lutetia's {what} not found (looked in $ENERGYML_LUTETIA_DIR and the monorepo): \
             a missing reference package is a failure in CI, never a skip"
        );
    }
    eprintln!(
        "skipped: Lutetia's {what} is not available (set ENERGYML_LUTETIA_DIR to a checkout of energyml-lutetia, \
         or run `python scripts/fetch-lutetia.py --dest target/lutetia`)"
    );
    None
}

/// The folder of the recipe: `recette.toml` and `donnees/` are inside.
pub fn recipe_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = env_dir() {
        candidates.push(dir.join("recipe"));
    }
    candidates.push(workspace().join("models/lutetia"));
    candidates
        .into_iter()
        .find(|dir| dir.join("recette.toml").is_file())
        .or_else(|| missing("recipe"))
}

/// The EPC of one variant (its `.h5` sits beside it).
pub fn package(variant: Variant) -> Option<PathBuf> {
    let (public, names, monorepo, what) = match variant {
        Variant::Clean => (
            "package",
            &["Lutetia.epc"][..],
            "web/public/examples/lutetia-sans-erreurs",
            "clean package",
        ),
        Variant::WithErrors => (
            "package-with-errors",
            &["Lutetia-with-errors.epc", "Lutetia.epc"][..],
            "web/public/examples/lutetia",
            "package with errors",
        ),
    };
    let mut candidates = Vec::new();
    if let Some(dir) = env_dir() {
        candidates.extend(names.iter().map(|name| dir.join(public).join(name)));
    }
    candidates.push(workspace().join(monorepo).join("Lutetia.epc"));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| missing(what))
}

/// Who wrote the packages: what decides between a byte-for-byte comparison and
/// a comparison up to float noise (the platforms' math libraries differ by an
/// ulp, and a derived uuid follows the last bit).
///
/// The file `written-on` is text, one fact per line:
///
/// ```text
/// linux
/// arch: x64
/// ```
///
/// - line 1, the platform, spelled as node's `process.platform` (`win32`,
///   `linux`, `darwin`) — the only line the monorepo's file has;
/// - an optional `arch: …` line, spelled as node's `process.arch` (`x64`,
///   `arm64`).
///
/// The comparison is exact when this machine matches the platform **and**, when
/// the file names one, the architecture. No file: exact.
pub struct Stamp {
    platform: String,
    arch: Option<String>,
}

impl Stamp {
    fn parse(text: &str) -> Option<Stamp> {
        let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
        let platform = lines.next()?.to_string();
        let arch = lines
            .find_map(|line| line.strip_prefix("arch:"))
            .map(|arch| arch.trim().to_string());
        Some(Stamp { platform, arch })
    }

    /// Whether this machine is the one that wrote the packages, as far as the
    /// stamp says.
    pub fn is_here(&self) -> bool {
        self.platform == node_platform() && self.arch.as_deref().is_none_or(|arch| arch == node_arch())
    }
}

impl std::fmt::Display for Stamp {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.arch {
            Some(arch) => write!(out, "{}/{arch}", self.platform),
            None => write!(out, "{}", self.platform),
        }
    }
}

/// This platform, spelled as node's `process.platform`.
pub fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    }
}

/// This architecture, spelled as node's `process.arch`.
pub fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// This machine, spelled the way a stamp spells it.
pub fn here() -> String {
    format!("{}/{}", node_platform(), node_arch())
}

/// The stamp shipped with the packages — `None` when there is none, and the
/// comparison is then exact.
pub fn written_on() -> Option<Stamp> {
    let mut candidates = Vec::new();
    if let Some(dir) = env_dir() {
        candidates.push(dir.join("written-on"));
    }
    candidates.push(workspace().join("web/public/examples/written-on"));
    candidates
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| Stamp::parse(&text))
}

/// What to run when a versioned Lutetia file (read from `path`) is stale: the
/// replay script of `energyml-lutetia` when it was read from
/// `$ENERGYML_LUTETIA_DIR`, the monorepo's own command otherwise.
pub fn refresh(path: &Path) -> String {
    match env_dir() {
        Some(dir) if path.starts_with(&dir) => {
            "replay the recipe with `python scripts/replay.py` in the energyml-lutetia checkout (it rewrites package/, package-with-errors/ and written-on), commit there, then advance tests/lutetia.lock".to_string()
        }
        _ => "replay the recipe into the in-tree example folders (see the recipe folder README), then commit them".to_string(),
    }
}
