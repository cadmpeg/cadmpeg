// SPDX-License-Identifier: Apache-2.0
//! Replay partition and Deltas stream pairs and print one JSON line per pair.
//!
//! Usage: `delta_replay PARTITION DELTAS [PARTITION DELTAS ...]`.

use std::collections::BTreeMap;

use cadmpeg_core::json;
use cadmpeg_parasolid::deltas::replay::{MainLine, Replay};
use cadmpeg_parasolid::records::{DeltasRecords, PrimaryRecords};
use serde_json::Value;

/// The variant name of an error's `Debug` form.
fn kind(error: &impl std::fmt::Debug) -> String {
    let debug = format!("{error:?}");
    debug
        .split([' ', '{', '('])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// Write the state as a partition and record the outcome.
fn transmit(replay: &Replay<'_, '_>, outcomes: &mut BTreeMap<String, usize>) -> Option<Vec<u8>> {
    match replay.transmit() {
        Ok(bytes) => {
            *outcomes.entry("ok".to_owned()).or_default() += 1;
            Some(bytes)
        }
        Err(error) => {
            *outcomes.entry(kind(&error)).or_default() += 1;
            None
        }
    }
}

fn replay(partition: &[u8], deltas: &[u8]) -> Value {
    let records = match DeltasRecords::read(deltas) {
        Ok(records) => records,
        Err(error) => {
            return json!({ "error": format!("deltas: {error}"), "error_kind": "DeltasRead" })
        }
    };
    let end = cadmpeg_core::serde_erase::json::to_value(records.end()).unwrap_or(Value::Null);
    let line = match MainLine::read(&records) {
        Ok(line) => line,
        Err(error) => {
            return json!({
                "deltas_end": end,
                "error": format!("main line: {error}"),
                "error_kind": kind(&error),
            })
        }
    };
    let marks: Vec<u32> = line.marks().iter().map(|mark| mark.identifier()).collect();
    let nonempty = line
        .deltas()
        .iter()
        .filter(|delta| !delta.is_empty())
        .count();
    let groups = line
        .deltas()
        .iter()
        .filter(|delta| delta.created_groups().next().is_some())
        .count();
    let mut out = json!({
        "deltas_end": end,
        "marks": marks,
        "off_line": line.off_line(),
        "nonempty": nonempty,
        "creating_groups": groups,
    });
    let primary = match PrimaryRecords::read(partition) {
        Ok(Some(primary)) => primary,
        Ok(None) => {
            out["error"] = json!("partition: no partition header");
            out["error_kind"] = json!("PartitionHeader");
            return out;
        }
        Err(error) => {
            out["error"] = json!(format!("partition: {error}"));
            out["error_kind"] = json!("PartitionRead");
            return out;
        }
    };
    let mut walk = match line.replay(&primary) {
        Ok(walk) => walk,
        Err(error) => {
            out["error"] = json!(format!("replay: {error}"));
            out["error_kind"] = json!(kind(&error));
            return out;
        }
    };
    let mut transmits = BTreeMap::new();
    let current = transmit(&walk, &mut transmits);
    out["current_identical"] = json!(current.as_deref() == Some(partition));
    let mut dangling = vec![walk.state().dangling().count()];
    let mut sizes = vec![walk.state().len()];
    let mut steps = 0;
    loop {
        match walk.step() {
            Ok(Some(_)) => {
                steps += 1;
                dangling.push(walk.state().dangling().count());
                sizes.push(walk.state().len());
                transmit(&walk, &mut transmits);
            }
            Ok(None) => break,
            Err(error) => {
                out["error"] = json!(format!("replay: {error}"));
                out["error_kind"] = json!(kind(&error));
                break;
            }
        }
    }
    out["steps_ok"] = json!(steps);
    out["states"] = json!(dangling.len());
    out["dangling"] = json!(dangling);
    out["nodes"] = json!(sizes);
    out["transmit"] = json!(transmits);
    out["reached"] = json!(walk.mark().identifier());
    out["initial"] = json!(walk.mark().preceding().is_none());
    out["initial_empty"] = json!(walk.mark().preceding().is_none() && walk.state().is_empty_part());
    out
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    for pair in paths.chunks(2) {
        let [partition, deltas] = pair else {
            eprintln!("delta_replay: give PARTITION DELTAS pairs");
            std::process::exit(2);
        };
        let line = match (std::fs::read(partition), std::fs::read(deltas)) {
            (Ok(partition), Ok(deltas)) => replay(&partition, &deltas),
            (Err(error), _) | (_, Err(error)) => json!({ "error": format!("read: {error}") }),
        };
        println!("{line}");
    }
}
