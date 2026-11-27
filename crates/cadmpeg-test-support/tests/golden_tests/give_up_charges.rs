// SPDX-License-Identifier: Apache-2.0
//! Every give-up loss (`record.unreadable` or `record.undecoded`) in a
//! committed golden names what the document lost (`docs/cadir-v11.md` §3.3).
//!
//! A record give-up names a record id the document does not hold. A part
//! give-up names the `unknowns` stub that stands in for the lost part. No id
//! is named by two give-ups. No id in a `dropped=[..]` list is a record the
//! document holds, except a STEP omitted statement, which the document keeps
//! as its untyped retained text when the give-up drops what the statement
//! states.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{collect_documents, golden_files};

const GIVE_UP_CODES: [&str; 2] = ["record.unreadable", "record.undecoded"];

/// The id prefix of the STEP omitted statements a dropped list may name.
const STEP_OMITTED: &str = "step:file:omitted#";

#[test]
fn every_give_up_names_a_record_the_document_does_not_hold() {
    let mut findings = Vec::new();
    let mut charges = 0_usize;
    for path in golden_files() {
        let text = std::fs::read_to_string(&path).expect("read golden");
        let golden: Value = serde_json::from_str(&text).expect("parse golden");
        let mut roots = Vec::new();
        collect_documents(&golden, &mut String::new(), &mut roots, &mut Vec::new());
        for (pointer, ir) in roots {
            let report = pointer
                .strip_suffix("/ir")
                .and_then(|parent| golden.pointer(&format!("{parent}/report/losses")))
                .and_then(Value::as_array);
            let Some(losses) = report else { continue };
            let messages = losses
                .iter()
                .filter(|loss| {
                    loss.pointer("/code/value/code")
                        .and_then(Value::as_str)
                        .is_some_and(|code| GIVE_UP_CODES.contains(&code))
                })
                .filter_map(|loss| loss.get("message").and_then(Value::as_str))
                .collect::<Vec<_>>();
            charges += messages.len();
            findings.extend(
                audit(ir, &messages)
                    .into_iter()
                    .map(|finding| format!("{} at {pointer}: {finding}", path.display())),
            );
        }
    }
    assert!(charges > 0, "the goldens carry no give-up loss to audit");
    assert!(
        findings.is_empty(),
        "{} give-up charges name the wrong record:\n{}",
        findings.len(),
        findings.join("\n")
    );
}

/// What one give-up loss message names.
#[derive(Debug, PartialEq, Eq)]
struct Charge<'a> {
    /// The record id of a record give-up, or the stub of a part give-up.
    named: &'a str,
    /// Whether `named` is a part's stub, which the document holds.
    part: bool,
    dropped: Vec<&'a str>,
}

fn parse(message: &str) -> Option<Charge<'_>> {
    let (named, part) = if let Some(rest) = message.strip_prefix("record ") {
        (rest.split(' ').next()?, false)
    } else if message.starts_with("part ") {
        let start = message.find(" record=")? + " record=".len();
        let rest = &message[start..];
        (&rest[..rest.find(": ")?], true)
    } else {
        return None;
    };
    let dropped = message
        .rfind("; dropped=[")
        .and_then(|start| {
            let list = &message[start + "; dropped=[".len()..];
            list.find(']').map(|end| &list[..end])
        })
        .map(|list| list.split(", ").filter(|id| !id.is_empty()).collect())
        .unwrap_or_default();
    Some(Charge {
        named,
        part,
        dropped,
    })
}

/// Each way the give-up loss messages of one document misname a
/// record.
fn audit(ir: &Value, messages: &[&str]) -> Vec<String> {
    let mut held = BTreeSet::new();
    held_ids(ir, &mut held);
    let stubs = ir
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|codecs| codecs.values())
        .filter_map(|codec| codec.get("unknowns").and_then(Value::as_array))
        .flatten()
        .filter_map(|stub| stub.get("id").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let mut findings = Vec::new();
    let mut named = BTreeMap::<&str, usize>::new();
    for message in messages {
        let Some(charge) = parse(message) else {
            findings.push(format!("unshaped give-up: {message}"));
            continue;
        };
        if charge.part {
            if !stubs.contains(charge.named) {
                findings.push(format!("part names no held stub {}", charge.named));
            }
        } else if held.contains(charge.named) {
            findings.push(format!("names held record {}", charge.named));
        }
        *named.entry(charge.named).or_default() += 1;
        for id in charge.dropped {
            if held.contains(id) && !id.starts_with(STEP_OMITTED) {
                findings.push(format!("{} drops held record {id}", charge.named));
            }
            *named.entry(id).or_default() += 1;
        }
    }
    findings.extend(
        named
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(id, count)| format!("{count} give-ups name {id}")),
    );
    findings
}

/// Every string `id` member in the document.
fn held_ids<'a>(value: &'a Value, held: &mut BTreeSet<&'a str>) {
    match value {
        Value::Object(fields) => {
            if let Some(id) = fields.get("id").and_then(Value::as_str) {
                held.insert(id);
            }
            fields.values().for_each(|field| held_ids(field, held));
        }
        Value::Array(items) => items.iter().for_each(|item| held_ids(item, held)),
        _ => {}
    }
}

#[test]
fn the_audit_finds_each_misnamed_give_up() {
    let ir = cadmpeg_core::json!({
        "model": { "bodies": [{ "id": "t:body#1" }, { "id": "t:body#2" }] },
        "native": { "t": {
            "unknowns": [{ "id": "t:source:part#a" }],
            "entities": [{ "id": "t:entity:directory#5" }]
        }, "step": {
            "unknowns": [{ "id": "step:file:omitted#7" }]
        }, "iges": {
            "entities": [{ "id": "iges:entity:directory#7" }]
        } }
    });
    let messages = [
        "record t:body#1 given up: bad; dropped=[iges:entity:directory#7, step:file:omitted#7, t:body#2, t:entity:directory#5, t:face#9]",
        "record t:face#9 given up: bad; dropped=[]",
        "part blob \"b\" stream=\"s\" offset=0 length=1 record=t:source:part#a: bad; dropped=[]",
        "part blob \"b\" stream=\"s\" offset=1 length=1 record=t:source:part#b: bad; dropped=[]",
        "t:body#3 was not read",
    ];
    assert_eq!(
        audit(&ir, &messages),
        [
            "names held record t:body#1",
            "t:body#1 drops held record iges:entity:directory#7",
            "t:body#1 drops held record t:body#2",
            "t:body#1 drops held record t:entity:directory#5",
            "part names no held stub t:source:part#b",
            "unshaped give-up: t:body#3 was not read",
            "2 give-ups name t:face#9",
        ]
    );
}
