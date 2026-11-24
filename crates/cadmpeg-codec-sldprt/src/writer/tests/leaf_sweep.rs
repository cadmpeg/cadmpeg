// SPDX-License-Identifier: Apache-2.0
//! Every numeric and boolean leaf of each feature kind's definition on the
//! two public NIST parts, edited alone and written: the full decoder reads the
//! edit back, or a loss names it and the feature reads back as stored, and no
//! other feature or parameter changes.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::features::Feature;
use cadmpeg_test_support::EditableDecodeResult;
use serde_json::Value;

use crate::loss::SldprtLossCode;
use crate::test_support::nist::{ftc_06, ftc_08};
use crate::test_support::readback::{self, Entry};
use crate::SldprtCodec;

/// One edit: the feature, by its unique name, and the JSON pointer of the
/// leaf of its definition's wire form.
struct Job {
    feature: String,
    pointer: String,
}

/// The kind of a definition in wire form, under its `operation` and
/// `post_process` wrappers.
fn kind(wire: &Value) -> Option<&str> {
    let mut value = wire;
    while matches!(
        value.get("kind").and_then(Value::as_str),
        Some("operation" | "post_process")
    ) {
        value = value.get("value")?;
    }
    value.get("kind")?.as_str()
}

fn leaves(value: &Value, pointer: &str, out: &mut Vec<(String, &'static str)>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let key = key.replace('~', "~0").replace('/', "~1");
                leaves(value, &format!("{pointer}/{key}"), out);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                leaves(value, &format!("{pointer}/{index}"), out);
            }
        }
        Value::Bool(_) => out.push((pointer.to_owned(), "bool")),
        Value::Number(number) if number.is_f64() => out.push((pointer.to_owned(), "real")),
        Value::Number(_) => out.push((pointer.to_owned(), "integer")),
        _ => {}
    }
}

/// The pointer with each array index replaced by `*`.
fn generic(pointer: &str) -> String {
    pointer
        .split('/')
        .map(|segment| {
            if !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit()) {
                "*"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// For each feature kind, leaf path and leaf type, the first feature in
/// history order with a unique name that has that leaf.
fn jobs(part: &DecodeResult) -> Vec<Job> {
    let mut features = part.ir().model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    let named = |name: &str| {
        features
            .iter()
            .filter(|feature| feature.name.as_deref() == Some(name))
            .count()
    };
    let mut seen = BTreeSet::new();
    let mut jobs = Vec::new();
    for feature in &features {
        let Some(name) = feature.name.as_deref().filter(|name| named(name) == 1) else {
            continue;
        };
        let wire = serde_json::to_value(feature.evaluation.definition()).unwrap();
        let mut found = Vec::new();
        leaves(&wire, "", &mut found);
        for (pointer, leaf_type) in found {
            let group = (
                kind(&wire).unwrap_or_default().to_owned(),
                generic(&pointer),
                leaf_type,
            );
            if seen.insert(group) {
                jobs.push(Job {
                    feature: name.to_owned(),
                    pointer,
                });
            }
        }
    }
    jobs
}

/// `value` changed: a boolean flipped, a number plus one.
fn changed(value: &Value) -> Value {
    match value {
        Value::Bool(flag) => Value::Bool(!flag),
        Value::Number(number) => match (number.as_u64(), number.as_i64()) {
            (Some(integer), _) if !number.is_f64() => serde_json::json!(integer + 1),
            (_, Some(integer)) if !number.is_f64() => serde_json::json!(integer + 1),
            _ => serde_json::json!(number.as_f64().unwrap() + 1.0),
        },
        other => other.clone(),
    }
}

fn named<'a>(features: &'a [Feature], name: &str) -> Vec<&'a Feature> {
    features
        .iter()
        .filter(|feature| feature.name.as_deref() == Some(name))
        .collect()
}

enum Outcome {
    /// The edited document is not a valid definition or fails validation.
    Invalid,
    Carried,
    Named,
}

/// Edits one leaf, writes, and reads the output back with the full decoder.
/// `baseline` is [`readback::baseline`] of `part`. `Err` describes a save
/// that is refused, carries a false note, or changes what it did not edit.
fn run(part: &DecodeResult, baseline: &[Entry], job: &Job) -> Result<Outcome, String> {
    let label = format!("{} {}", job.feature, job.pointer);
    let mut decoded = EditableDecodeResult::from(part.clone());
    let (source_wire, edited_wire, feature_id) = {
        let mut ir = decoded.ir_mut();
        let feature = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some(job.feature.as_str()))
            .unwrap();
        let source_wire = serde_json::to_value(feature.evaluation.definition()).unwrap();
        let mut edited_wire = source_wire.clone();
        let leaf = edited_wire.pointer_mut(&job.pointer).unwrap();
        *leaf = changed(leaf);
        let Ok(definition) = serde_json::from_value(edited_wire.clone()) else {
            eprintln!("{label}: not a definition");
            return Ok(Outcome::Invalid);
        };
        feature.evaluation.set_definition(definition);
        (source_wire, edited_wire, feature.id.clone())
    };
    if !cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
        .is_ok_and(|report| report.is_ok())
    {
        eprintln!("{label}: fails validation");
        return Ok(Outcome::Invalid);
    }
    let mut output = Vec::new();
    let report = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut output))
        .map_err(|error| format!("{label}: refused: {error}"))?;
    let unwritten = report
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::WriterFeatureEditUnwritten.kind())
        .filter(|loss| loss.message.contains(feature_id.as_str()))
        .count();
    let stale = report
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::WriterSolidStaleAfterEdit.kind())
        .filter(|loss| loss.message.contains(&format!("\"{}\"", job.feature)))
        .count();
    let written = SldprtCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .map_err(|error| format!("{label}: output does not decode: {error}"))?;
    let read = named(&written.ir().model.features, &job.feature);
    let [read] = read.as_slice() else {
        return Err(format!("{label}: the feature does not read back once"));
    };
    let read_leaf = serde_json::to_value(read.evaluation.definition())
        .unwrap()
        .pointer(&job.pointer)
        .cloned()
        .unwrap_or(Value::Null);
    // A stale-solid note names the edited feature as written, so it names
    // no difference of the read-back.
    let messages = report
        .losses
        .iter()
        .filter(|loss| loss.code != SldprtLossCode::WriterSolidStaleAfterEdit.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let (differences, outcome) = if unwritten == 0 {
        let entries = readback::unnamed(
            readback::beyond(readback::diff(decoded.ir(), written.ir()), baseline),
            &messages,
        );
        let mut differences = readback::unaccounted(
            decoded.ir(),
            &[feature_id.as_str()],
            &["source_properties", "source_content"],
            entries,
        );
        let stated = &named(&decoded.ir().model.features, &job.feature)[0].source_properties;
        differences.extend(
            stated
                .keys()
                .filter(|key| !read.source_properties.contains_key(*key))
                .map(|key| format!("source property {key} is missing")),
        );
        (differences, Outcome::Carried)
    } else {
        if stale != 0 {
            return Err(format!(
                "{label}: a loss names the edit, and a stale note calls it written"
            ));
        }
        let differences = readback::unnamed(
            readback::beyond(readback::diff(part.ir(), written.ir()), baseline),
            &messages,
        )
        .into_iter()
        .map(|entry| format!("{entry:?}"))
        .collect::<Vec<_>>();
        (differences, Outcome::Named)
    };
    if !differences.is_empty() {
        return Err(format!(
            "{label}: {} loss names the edit; the leaf reads back as {read_leaf} (source {}, edit {}); stale notes {stale}; {differences:?}",
            if unwritten == 0 { "no" } else { "a" },
            source_wire.pointer(&job.pointer).unwrap(),
            edited_wire.pointer(&job.pointer).unwrap(),
        ));
    }
    eprintln!(
        "{label}: {}",
        if unwritten == 0 { "carried" } else { "named" }
    );
    Ok(outcome)
}

/// Runs every job of `part` on a few threads and returns the wrong saves and
/// the count of each outcome.
fn sweep(part: &'static DecodeResult) -> (Vec<String>, [usize; 3]) {
    let jobs = jobs(part);
    let baseline = readback::baseline(part);
    let next = AtomicUsize::new(0);
    let results = Mutex::new((Vec::new(), [0_usize; 3]));
    let workers = std::thread::available_parallelism().map_or(4, |count| count.get().min(6));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(index) else {
                    break;
                };
                let result = run(part, &baseline, job);
                let mut results = results.lock().unwrap();
                match result {
                    Ok(Outcome::Invalid) => results.1[0] += 1,
                    Ok(Outcome::Carried) => results.1[1] += 1,
                    Ok(Outcome::Named) => results.1[2] += 1,
                    Err(wrong) => results.0.push(wrong),
                }
            });
        }
    });
    let (mut wrong, counts) = results.into_inner().unwrap();
    wrong.sort();
    (wrong, counts)
}

fn assert_every_leaf_edit_is_carried_or_named(part: &'static DecodeResult) {
    let (wrong, [invalid, carried, named]) = sweep(part);
    eprintln!(
        "leaf sweep: {invalid} invalid, {carried} carried, {named} named, {} wrong",
        wrong.len()
    );
    assert!(
        wrong.is_empty(),
        "{} wrong saves ({invalid} invalid edits, {carried} carried, {named} named):\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert!(carried > 0 && named > 0, "{invalid} {carried} {named}");
}

#[test]
fn every_leaf_edit_of_ftc_06_is_carried_or_named() {
    assert_every_leaf_edit_is_carried_or_named(ftc_06());
}

#[test]
fn every_leaf_edit_of_ftc_08_is_carried_or_named() {
    assert_every_leaf_edit_is_carried_or_named(ftc_08());
}

/// The job that edits the first leaf of feature `feature` whose pointer
/// `selected` accepts.
fn job(part: &DecodeResult, feature: &str, selected: impl Fn(&str) -> bool) -> Job {
    let [found] = named(&part.ir().model.features, feature)[..] else {
        panic!("{feature} is not named once");
    };
    let wire = serde_json::to_value(found.evaluation.definition()).unwrap();
    let mut found = Vec::new();
    leaves(&wire, "", &mut found);
    let (pointer, _) = found
        .into_iter()
        .find(|(pointer, leaf_type)| *leaf_type == "real" && selected(pointer))
        .unwrap();
    Job {
        feature: feature.to_owned(),
        pointer,
    }
}

/// The record states the first depth as `D1`: the edit is written there, and
/// no `Depth` parameter is added beside it.
#[test]
fn an_extrusion_depth_edit_is_written_to_the_stored_dimension() {
    let job = job(ftc_06(), "Boss-Extrude1", |pointer| {
        pointer.ends_with("/length") && !pointer.contains("second")
    });
    let outcome = run(ftc_06(), &readback::baseline(ftc_06()), &job);
    assert!(
        matches!(outcome, Ok(Outcome::Carried)),
        "{:?}",
        outcome.err()
    );
}

/// A linear pattern reads both directions from its record once one is
/// written: an edit of the first direction alone keeps the second. The `y`
/// component is zero, so plus one leaves a direction that is not parallel to
/// the source.
#[test]
fn a_linear_pattern_first_direction_edit_is_carried() {
    let job = job(ftc_08(), "LPattern1", |pointer| {
        pointer.ends_with("/pattern/direction/y")
    });
    let outcome = run(ftc_08(), &readback::baseline(ftc_08()), &job);
    assert!(
        matches!(outcome, Ok(Outcome::Carried)),
        "{:?}",
        outcome.err()
    );
}
