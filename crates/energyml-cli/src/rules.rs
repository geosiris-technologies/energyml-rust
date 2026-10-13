//! `energyml rules` — the rules this library holds.
//!
//! **No decision here.** The registry, the filters and the JSON are in
//! `energyml-rules`; this module parses arguments and prints.

use anyhow::{Result, bail};
use clap::Parser;
use energyml_rules::select::{Filter, parse_anchor, parse_status, select, to_json};

/// `energyml rules` — list the rules.
#[derive(Parser)]
pub struct RulesArgs {
    /// The house rules: what this library decides where the specification is
    /// silent, ambiguous or only interpreted. (The only family there is yet.)
    #[arg(long)]
    pub house: bool,
    /// One theme (`energyml rules --house` without it lists them all).
    #[arg(long)]
    pub theme: Option<String>,
    /// One rule, in full: text, reason, references.
    #[arg(long)]
    pub id: Option<String>,
    /// The rules that show at a place, as `kind:key` — `type:Attachment`,
    /// `type:Attachment.Md`, `view:object.colour-stack`, `finding:<id>`,
    /// `operation:attach`.
    #[arg(long)]
    pub anchor: Option<String>,
    /// One status: active, designed, superseded.
    #[arg(long)]
    pub status: Option<String>,
    /// The registry as JSON (every language that exists, the version).
    #[arg(long)]
    pub json: bool,
    /// The language of the texts. A rule not translated answers in English and
    /// says so.
    #[arg(long, default_value = "en")]
    pub lang: String,
}

/// Runs the command.
pub fn run(args: RulesArgs) -> Result<()> {
    if !args.house {
        bail!("say which rules: `--house` is the only family this build holds (the standard's `trg/` and `brt` rules are listed by `energyml check`)");
    }
    let filter = Filter {
        theme: args.theme.as_deref(),
        id: args.id.as_deref(),
        anchor: args.anchor.as_deref().map(parse_anchor).transpose().map_err(anyhow::Error::msg)?,
        status: args.status.as_deref().map(parse_status).transpose().map_err(anyhow::Error::msg)?,
    };
    let rules = select(&filter);
    if rules.is_empty() {
        bail!("no house rule matches (the registry holds {})", energyml_rules::all().len());
    }
    if args.json {
        println!("{}", to_json(&rules));
        return Ok(());
    }
    if args.id.is_some() {
        for rule in rules {
            full(rule, &args.lang);
        }
        return Ok(());
    }
    list(&rules, &args.lang);
    Ok(())
}

fn list(rules: &[&energyml_rules::Rule], lang: &str) {
    let mut fell_back = 0;
    for theme in energyml_rules::themes() {
        let mine: Vec<_> = rules.iter().filter(|r| r.theme == theme.id).collect();
        if mine.is_empty() {
            continue;
        }
        println!("{} ({})", energyml_rules::theme_title(theme, lang).value, theme.id);
        for rule in mine {
            let said = energyml_rules::text_in(rule, lang);
            fell_back += usize::from(said.title.lang == "en" && lang != "en");
            println!("  {:<58} {:<10} {}", rule.id, rule.status.as_str(), said.title.value);
        }
    }
    println!();
    println!("{} rules, registry version {}", rules.len(), energyml_rules::RULESET_VERSION);
    if lang != "en" && fell_back > 0 {
        println!("{fell_back} of them have no `{lang}` title and are shown in English.");
    }
}

fn full(rule: &energyml_rules::Rule, lang: &str) {
    let said = energyml_rules::text_in(rule, lang);
    println!("{}  [{}; spec {}; level {}; since {}]", rule.id, rule.status.as_str(), rule.stance.as_str(), rule.level, rule.since);
    println!("{}\n", said.title.value);
    println!("{}\n", said.text.value);
    println!("Why: {}", said.why.value);
    if said.fallback {
        println!("(some of this is not translated into `{lang}` and is shown in English)");
    }
    for g in rule.rulings {
        println!("Decided by {} on {}{}", g.by, g.on, g.quote.map(|q| format!(": {q}")).unwrap_or_default());
    }
    for f in rule.refs {
        let place = [f.doc, f.path, f.at].into_iter().flatten().collect::<Vec<_>>().join(" ");
        println!("See ({}): {place}{}", f.kind.as_str(), f.note.map(|n| format!(" - {n}")).unwrap_or_default());
    }
    for a in rule.at {
        println!("Shows at {}:{}", a.kind.as_str(), a.key);
    }
    for s in rule.supersedes {
        println!("Supersedes {s}");
    }
    for r in energyml_rules::superseded_by(rule.id) {
        println!("Superseded by {}", r.id);
    }
    println!();
}
