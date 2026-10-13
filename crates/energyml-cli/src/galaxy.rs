//! `energyml galaxy-tool` — the Galaxy tool XML of an operation, written from
//! its declaration (`energyml_build::galaxy`). This file reads arguments and
//! writes files; what a tool holds is the library's.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args;

use energyml_build::galaxy::{ToolOptions, tool_xml};
use energyml_build::operation::Registry;

/// Write the Galaxy tool XML of an operation — or of every one (`--all`).
#[derive(Args, Debug)]
pub struct GalaxyToolArgs {
    /// The operation, as `energyml create --list` names it.
    pub operation: Option<String>,
    /// Every operation of the registry, one file each, into `--dir`.
    #[arg(long, requires = "dir")]
    pub all: bool,
    /// The folder `--all` writes into, `energyml_<operation>.xml` each.
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// The file to write; the standard output when absent.
    #[arg(long, short, value_name = "FILE.xml")]
    pub out: Option<PathBuf>,
    /// The container image the tool runs in.
    #[arg(long)]
    pub image: Option<String>,
    /// The runner's path inside the image.
    #[arg(long)]
    pub runner: Option<String>,
}

pub fn run(args: GalaxyToolArgs) -> Result<ExitCode> {
    let mut options = ToolOptions::default();
    if let Some(image) = args.image {
        options.image = image;
    }
    if let Some(runner) = args.runner {
        options.runner = runner;
    }
    let registry = Registry::new();
    if args.all {
        let dir = args.dir.expect("clap requires --dir with --all");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let mut written = 0;
        for operation in registry.iter() {
            let path = dir.join(format!("energyml_{}.xml", operation.name().replace('-', "_")));
            std::fs::write(&path, tool_xml(operation, &options)).with_context(|| format!("writing {}", path.display()))?;
            written += 1;
        }
        println!("{written} tool(s) written to {}", dir.display());
        return Ok(ExitCode::SUCCESS);
    }
    let Some(name) = args.operation else { bail!("name an operation, or give --all --dir DIR") };
    let Some(operation) = registry.get(&name) else {
        bail!("no operation is called `{name}` — `energyml create --list` names them");
    };
    let xml = tool_xml(operation, &options);
    match args.out {
        Some(path) => std::fs::write(&path, xml).with_context(|| format!("writing {}", path.display()))?,
        None => print!("{xml}"),
    }
    Ok(ExitCode::SUCCESS)
}
