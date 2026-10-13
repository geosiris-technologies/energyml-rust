//! `energyml series` — a log, a time series, a flow test or production
//! periods, read as series and printed.
//!
//! Argument parsing and display only: reading, refusing, counting and
//! decimating are `energyml_domain::series`'s, so this command, the front end and
//! Python answer the same thing.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::series::decimate::decimate;
use energyml_domain::series::time_series::Content;
use energyml_domain::series::{self, IndexKind, Quality, Readout, Series};
use energyml_domain::values::Values;
use energyml_storage::epc::core_props::w3cdtf;

use crate::{Modes, open, report_unreadable};

#[derive(Parser)]
pub struct SeriesArgs {
    /// The EPC file.
    file: PathBuf,
    /// The object to read: a WITSML `Log` or `ChannelSet` (nested ones too), a
    /// PRODML `TimeSeriesData`, a flow test, a `WellProductionParameters`.
    uuid: String,
    /// Print the readout as JSON — what the front end receives.
    #[arg(long)]
    json: bool,
    /// Also decimate every numeric channel over its whole index into at most
    /// this many buckets (first, least, greatest, last of each; gaps kept).
    #[arg(long, value_name = "BUCKETS")]
    decimate: Option<usize>,
    /// How many rows of values to print.
    #[arg(long, value_name = "N", default_value_t = 5)]
    rows: usize,
}

pub fn run(args: SeriesArgs, modes: Modes) -> Result<()> {
    let store = open(&args.file, &modes)?;
    // Every object, because a nested channel set is reached through the part
    // that holds it, and a flow test's references resolve among them.
    let mut held: Vec<(String, Box<dyn DynObject>)> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    for summary in store.resources() {
        match store.get_object(&summary.uri) {
            Ok(object) => held.push((summary.uuid.clone(), object)),
            Err(why) => unreadable.push(format!("{}  {}: {why}", summary.uuid, summary.qualified_type)),
        }
    }
    report_unreadable(&unreadable, &modes, 5);
    let objects: Vec<(&str, &dyn Reflect)> = held.iter().map(|(uuid, object)| (uuid.as_str(), object.as_reflect())).collect();

    let readout = match series::read(&args.uuid, &objects, &store) {
        Ok(readout) => readout,
        Err(refused) => bail!("{}: {refused}", args.uuid),
    };

    let mut decimated = Vec::new();
    if let Some(buckets) = args.decimate {
        for one in readout.series() {
            for (at, channel) in one.channels.iter().enumerate() {
                if channel.numbers().is_none() || one.is_empty() {
                    continue;
                }
                let first = one.index.values[0];
                let last = one.index.values[one.len() - 1];
                match decimate(one, at, first, last, buckets) {
                    Ok(done) => decimated.push((one.uuid.clone(), done)),
                    Err(refused) => eprintln!("{} `{}`: {refused}", one.uuid, channel.mnemonic),
                }
            }
        }
    }

    if args.json {
        let mut value = readout.to_json();
        if args.decimate.is_some() {
            value["decimated"] = serde_json::Value::Array(
                decimated.iter().map(|(uuid, done)| serde_json::json!({ "series": uuid, "decimated": done.to_json() })).collect(),
            );
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    match &readout {
        Readout::Sets(sets) => {
            for one in sets {
                print_series("channel set", one, args.rows);
            }
        }
        Readout::TimeSeries(series) => {
            println!("time series {}  {}", series.uuid, series.title);
            for (keyword, value) in &series.keys {
                println!("  key {keyword} = {value}");
            }
            match &series.content {
                Content::Samples(samples) => print_series("samples", samples, args.rows),
                Content::Static { value, quality } => {
                    println!("  static value {}  ({})", first_value(value), quality_word(quality));
                }
                Content::Empty => println!("  no sample"),
            }
        }
        Readout::FlowTest(measured) => {
            for one in measured {
                println!("{} / {}{}", one.group, one.role, one.remark.as_deref().map(|remark| format!("  — {remark}")).unwrap_or_default());
                print_series("channel set", &one.series, args.rows);
            }
        }
        Readout::Periods(periods) => {
            println!("{} period(s)", periods.len());
            for period in periods {
                println!(
                    "  {}  {} {}{}",
                    period.start,
                    period.duration,
                    period.duration_uom,
                    period.status.as_deref().map(|status| format!("  {status}")).unwrap_or_default()
                );
                for product in &period.products {
                    match (product.rate, &product.rate_uom, product.quantity, &product.quantity_uom) {
                        (Some(rate), Some(rate_uom), Some(quantity), Some(quantity_uom)) => println!(
                            "    {}  {rate} {rate_uom} → {quantity} {quantity_uom}",
                            product.product
                        ),
                        _ => println!("    {}  no rate", product.product),
                    }
                }
            }
        }
    }
    for (uuid, done) in &decimated {
        println!(
            "decimated {uuid} `{}`: {} sample(s) → {} point(s) in {} bucket(s), {} gap(s)",
            done.channel,
            done.samples,
            done.points.len(),
            done.buckets,
            done.gaps.len()
        );
        for gap in &done.gaps {
            println!("  gap {} → {}  {} sample(s) {}", gap.from, gap.to, gap.samples, gap.quality);
        }
    }
    Ok(())
}

fn print_series(what: &str, series: &Series, rows: usize) {
    let index = &series.index;
    let span = match (index.values.first(), index.values.last()) {
        (Some(first), Some(last)) => format!(", {} → {}", shown_index(index.kind, *first), shown_index(index.kind, *last)),
        _ => String::new(),
    };
    println!("{what} {}  {}", series.uuid, series.title);
    println!(
        "  index {} ({}, {}, {}) {} sample(s){span}",
        index.mnemonic,
        index.kind.wire(),
        index.uom.as_deref().unwrap_or("no unit"),
        index.direction.wire(),
        index.values.len()
    );
    for secondary in &series.secondary {
        println!("  secondary index {} ({}, {})", secondary.mnemonic, secondary.kind.wire(), secondary.uom.as_deref().unwrap_or("no unit"));
    }
    for channel in &series.channels {
        let tally = channel.tally();
        let flagged: Vec<String> = tally.flagged.iter().map(|(label, count)| format!("{count} {label}")).collect();
        let range = match (tally.min, tally.max) {
            (Some(min), Some(max)) => format!(", {min} → {max}"),
            _ => String::new(),
        };
        println!(
            "  channel {}  {}  {}: {} good, {} absent{}{range}",
            channel.mnemonic,
            channel.uom.as_deref().unwrap_or("no unit"),
            channel.data_kind.as_deref().or(channel.measure_class.as_deref()).unwrap_or("-"),
            tally.good,
            tally.absent,
            if flagged.is_empty() { String::new() } else { format!(", {}", flagged.join(", ")) }
        );
    }
    for at in 0..rows.min(series.len()) {
        let cells: Vec<String> = series.channels.iter().map(|channel| cell(channel, at)).collect();
        println!("    {}  {}", shown_index(index.kind, index.values[at]), cells.join("  "));
    }
    if series.len() > rows {
        println!("    … {} more row(s)", series.len() - rows);
    }
}

fn shown_index(kind: IndexKind, value: f64) -> String {
    if kind == IndexKind::DateTime && value.fract() == 0.0 { w3cdtf(value as i64) } else { value.to_string() }
}

fn cell(channel: &energyml_domain::series::Channel, at: usize) -> String {
    match &channel.quality[at] {
        Quality::Absent => "null".into(),
        quality => {
            let value = match &channel.values {
                Values::Floating(values) => values[at].to_string(),
                Values::Integer(values) => values[at].to_string(),
                Values::Boolean(values) => values[at].to_string(),
                Values::Text(values) => values[at].clone(),
            };
            match quality {
                Quality::Flagged(label) => format!("{value} [{label}]"),
                _ => value,
            }
        }
    }
}

fn first_value(values: &Values) -> String {
    match values {
        Values::Floating(v) => v.first().map(ToString::to_string),
        Values::Integer(v) => v.first().map(ToString::to_string),
        Values::Boolean(v) => v.first().map(ToString::to_string),
        Values::Text(v) => v.first().cloned(),
    }
    .unwrap_or_default()
}

fn quality_word(quality: &Quality) -> &str {
    match quality {
        Quality::Good => "good",
        Quality::Absent => "absent",
        Quality::Flagged(label) => label,
    }
}
