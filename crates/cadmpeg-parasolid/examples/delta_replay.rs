// SPDX-License-Identifier: Apache-2.0
//! Replay partition and Deltas stream pairs and print one JSON line per pair.
//!
//! Usage: `delta_replay PARTITION DELTAS [PARTITION DELTAS ...]`.

use cadmpeg_parasolid::deltas::replay::MainLine;
use cadmpeg_parasolid::records::{DeltasRecords, PrimaryRecords};
use serde_json::{json, Value};

fn replay(partition: &[u8], deltas: &[u8]) -> Value {
    let records = match DeltasRecords::read(deltas) {
        Ok(records) => records,
        Err(error) => return json!({ "error": format!("deltas: {error}") }),
    };
    let end = serde_json::to_value(records.end()).unwrap_or(Value::Null);
    let line = match MainLine::read(&records) {
        Ok(line) => line,
        Err(error) => return json!({ "deltas_end": end, "error": format!("main line: {error}") }),
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
    let forward = line
        .deltas()
        .iter()
        .filter(|delta| delta.mark().delta_is_forward())
        .count();
    let mut out = json!({
        "deltas_end": end,
        "marks": marks,
        "off_line": line.off_line(),
        "nonempty": nonempty,
        "creating_groups": groups,
        "forward": forward,
    });
    let primary = match PrimaryRecords::read(partition) {
        Ok(Some(primary)) => primary,
        Ok(None) => {
            out["error"] = json!("partition: no partition header");
            return out;
        }
        Err(error) => {
            out["error"] = json!(format!("partition: {error}"));
            return out;
        }
    };
    let mut walk = match line.replay(&primary) {
        Ok(walk) => walk,
        Err(error) => {
            out["error"] = json!(format!("replay: {error}"));
            return out;
        }
    };
    let mut dangling = walk.state().dangling().count();
    let mut states = 1;
    loop {
        match walk.step() {
            Ok(Some(_)) => {
                states += 1;
                dangling += walk.state().dangling().count();
            }
            Ok(None) => break,
            Err(error) => {
                out["error"] = json!(format!("replay: {error}"));
                break;
            }
        }
    }
    out["states"] = json!(states);
    out["dangling"] = json!(dangling);
    out["reached"] = json!(walk.mark().identifier());
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
