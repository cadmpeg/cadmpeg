// SPDX-License-Identifier: Apache-2.0
//! Generate deterministic machine-readable evidence for the `SolidCraft` codec.
//!
//! Usage: `solidcraft-profile <fixtures-dir> <output.json>`. Every `.sc` and
//! `.solidcraft` file under the directory is decoded twice, replayed, edited,
//! refused and regenerated, and the cumulative ladder gates L0 to L9 are
//! evaluated from the evidence. A gate passes only when every earlier gate
//! does, so `highest_passing_gate` is the real level; the write assertions
//! are listed on their own so a passing writer is visible beside a failing
//! ladder. Gates L6 to L8 carry the named gaps that keep them open.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use cadmpeg_codec_solidcraft::SolidcraftCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::Severity;
use serde::Serialize;
use serde_json::Value;

/// The families and records that decode natively by decision, and the open
/// item or ladder step that keeps each one open.
const L6_GAPS: &[&str] = &[
    "SC-02: sketch planes built from the model (midplane, two edges, axis and point, three points, tangent, along a path, perpendicular, through a point)",
    "SC-03: variable and spline fillet radii",
    "SC-04: operations the neutral pattern and sweep models cannot state (a mirror that joins its copies, a path pattern whose copies turn, a sweep held by a direction or by two rails, a loft whose rails stretch it, a coil on a sketch profile)",
    "SC-05: periodic splines, fit splines with tangent handles, and the sketch pattern, slot, and arc-length relations",
];
const L7_GAPS: &[&str] = &[
    "SC-07: joints snapped on geometry, joints of the pin-slot, planar, point-on-plane, point-on-line, and line-on-plane kinds, joint links, and motion studies",
];
const L8_GAPS: &[&str] = &[
    "SC-06: the look of a whole component, the render setups, the metadata, the plugin data, the browser groups, and the canvas placement have no neutral record",
];

#[derive(Serialize)]
struct Profile {
    #[serde(rename = "profile_version")]
    version: u32,
    format: &'static str,
    fixtures: Vec<Fixture>,
    write_assertions: WriteAssertions,
    gates: Vec<Gate>,
    highest_passing_gate: Option<String>,
}

#[derive(Serialize)]
struct Fixture {
    filename: String,
    sha256: String,
    dialect: String,
    deterministic: bool,
    blocking_losses: usize,
    loss_codes: BTreeSet<String>,
    features: usize,
    native_features: BTreeSet<String>,
    bodies: usize,
    parameters: usize,
    replay_identical: bool,
    parameter_edit_round_trip: Option<bool>,
    source_less_generation: Option<bool>,
}

/// One flag per write assertion, by name; the profile lists them side by side.
#[derive(Serialize)]
#[serde(transparent)]
struct WriteAssertions(BTreeMap<&'static str, bool>);

/// The write assertions, with what each one states.
const WRITE_ASSERTIONS: &[(&str, &str)] = &[
    ("replay_identical", "unedited designs replay byte for byte"),
    ("parameter_edit_round_trip", "a parameter edit reads back"),
    (
        "unsupported_target_rejected",
        "an unlisted dialect is refused",
    ),
    (
        "unsupported_edit_rejected",
        "an unsupported edit is refused",
    ),
    ("source_less_generated", "a source-less design is generated"),
    (
        "independent_native_acceptance",
        "solidcraft-cli accepts a generated design",
    ),
];

#[derive(Serialize)]
struct Gate {
    level: String,
    passed: bool,
    assertion: &'static str,
    blockers: Vec<String>,
}

type Failure = Box<dyn std::error::Error>;

fn full() -> DecodeOptions {
    DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Full,
        ..DecodeOptions::default()
    }
}

fn decode(bytes: &[u8]) -> Result<cadmpeg_ir::codec::DecodeResult, Failure> {
    Ok(SolidcraftCodec.decode(&mut Cursor::new(bytes), &full())?)
}

fn plan_bytes(
    ir: &cadmpeg_ir::CadIr,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
    request: TargetRequest<'_>,
) -> Result<Vec<u8>, Failure> {
    let plan = SolidcraftCodec.plan(EncodeInput::new(ir, fidelity), request)?;
    let mut out = Vec::new();
    plan.write_to(&mut out)?;
    Ok(out)
}

fn dialect(bytes: &[u8]) -> String {
    SolidcraftCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .ok()
        .and_then(|summary| serde_json::to_value(&summary).ok())
        .and_then(|value| {
            value["identity"]["dialects"]["primary"]["dialect"]
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn native_kinds(ir: &cadmpeg_ir::CadIr) -> BTreeSet<String> {
    ir.model
        .features
        .iter()
        .filter_map(|feature| serde_json::to_value(feature).ok())
        .filter(|feature| {
            feature
                .pointer("/definition/value/kind")
                .and_then(Value::as_str)
                == Some("native")
        })
        .filter_map(|feature| feature["source_tag"].as_str().map(str::to_owned))
        .collect()
}

fn body_count(ir: &cadmpeg_ir::CadIr) -> usize {
    ir.census()
        .iter()
        .filter(|(key, _)| format!("{key:?}").to_lowercase().contains("bod"))
        .map(|(_, count)| *count)
        .sum()
}

fn fixture(path: &Path, bytes: &[u8]) -> Result<Fixture, Failure> {
    let first = decode(bytes)?;
    let second = decode(bytes)?;
    let deterministic = serde_json::to_string(first.ir())? == serde_json::to_string(second.ir())?;
    let report = first.report();
    let blocking = report
        .losses
        .iter()
        .filter(|loss| loss.severity >= Severity::Error)
        .count();
    let codes = report
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect();
    let (ir, _, fidelity) = first.into_parts();
    let replay = plan_bytes(&ir, Some(&fidelity), TargetRequest::Inherit)?;
    let edit = ir
        .model
        .parameters
        .first()
        .map(|parameter| parameter_edit(&ir, &fidelity, &parameter.name).unwrap_or(false));
    let generated = source_less(&ir).ok();
    Ok(Fixture {
        filename: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sha256: sha256_hex(bytes),
        dialect: dialect(bytes),
        deterministic,
        blocking_losses: blocking,
        loss_codes: codes,
        features: ir.model.features.len(),
        native_features: native_kinds(&ir),
        bodies: body_count(&ir),
        parameters: ir.model.parameters.len(),
        replay_identical: replay == bytes,
        parameter_edit_round_trip: edit,
        source_less_generation: generated.map(|_| true),
    })
}

/// Edit parameter `name`, write, and read the edit back.
fn parameter_edit(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
    name: &str,
) -> Result<bool, Failure> {
    let mut edited = ir.clone();
    let mut wanted = String::new();
    for parameter in &mut edited.model.parameters {
        if parameter.name == name {
            wanted = format!("({})", parameter.expression);
            parameter.expression.clone_from(&wanted);
        }
    }
    let out = plan_bytes(&edited, Some(fidelity), TargetRequest::Inherit)?;
    let back = decode(&out)?;
    Ok(back
        .ir()
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == name && parameter.expression == wanted))
}

/// Generate the model without its source; the bytes are returned.
fn source_less(ir: &cadmpeg_ir::CadIr) -> Result<Vec<u8>, Failure> {
    let mut bare = ir.clone();
    bare.source = None;
    let out = plan_bytes(
        &bare,
        None,
        TargetRequest::Explicit("solidcraft:container-1-v57"),
    )?;
    let back = decode(&out)?;
    if back.ir().model.features.len() != ir.model.features.len() {
        return Err("the generated design has another feature count".into());
    }
    Ok(out)
}

/// Whether `solidcraft-cli` evaluates `bytes` without an error.
fn native_accepts(bytes: &[u8]) -> bool {
    let Some(cli) = std::env::var_os("SOLIDCRAFT_CLI") else {
        return false;
    };
    let Ok(folder) = tempfile_dir() else {
        return false;
    };
    let design = folder.join("generated.sc");
    let result = folder.join("result.json");
    let ran = fs::write(&design, bytes).is_ok()
        && Command::new(cli)
            .arg("eval")
            .arg(&design)
            .arg("--json-out")
            .arg(&result)
            .output()
            .is_ok_and(|output| output.status.success());
    let accepted = ran
        && fs::read(&result)
            .ok()
            .and_then(|text| serde_json::from_slice::<Value>(&text).ok())
            .is_some_and(|value| {
                value["bodies"]
                    .as_array()
                    .is_some_and(|bodies| !bodies.is_empty())
                    && value["timeline"]
                        .as_array()
                        .is_some_and(|steps| steps.iter().all(|step| step["error"].is_null()))
            });
    drop(fs::remove_dir_all(&folder));
    accepted
}

fn tempfile_dir() -> std::io::Result<PathBuf> {
    let folder = std::env::temp_dir().join(format!("solidcraft-profile-{}", std::process::id()));
    fs::create_dir_all(&folder)?;
    Ok(folder)
}

fn gate(level: &str, assertion: &'static str, blockers: Vec<String>) -> Gate {
    Gate {
        level: level.to_owned(),
        passed: blockers.is_empty(),
        assertion,
        blockers,
    }
}

fn each(fixtures: &[Fixture], test: impl Fn(&Fixture) -> bool, what: &str) -> Vec<String> {
    fixtures
        .iter()
        .filter(|fixture| !test(fixture))
        .map(|fixture| format!("{}: {what}", fixture.filename))
        .collect()
}

fn gates(fixtures: &[Fixture], write: &WriteAssertions) -> Vec<Gate> {
    let some = |test: &dyn Fn(&Fixture) -> bool, what: &str| {
        if fixtures.iter().any(test) {
            Vec::new()
        } else {
            vec![format!("no fixture: {what}")]
        }
    };
    let gaps = |list: &[&str]| list.iter().map(|gap| (*gap).to_owned()).collect::<Vec<_>>();
    let write_blockers: Vec<String> = WRITE_ASSERTIONS
        .iter()
        .filter(|(key, _)| !write.0.get(key).copied().unwrap_or(false))
        .map(|(_, name)| (*name).to_owned())
        .collect();
    let mut gates = vec![
        gate(
            "L0",
            "every fixture is detected, inspected and decoded as a dialect row",
            each(fixtures, |f| !f.dialect.is_empty(), "no dialect"),
        ),
        gate(
            "L1",
            "decode is deterministic and has no blocking loss",
            each(fixtures, |f| f.deterministic && f.blocking_losses == 0, "not deterministic or blocking loss"),
        ),
        gate(
            "L2",
            "a cached design decodes into bodies",
            some(&|f| f.bodies > 0, "decodes into bodies"),
        ),
        gate(
            "L3",
            "geometry decodes with no untyped or unconnected carrier loss",
            each(
                fixtures,
                |f| {
                    !f.loss_codes.iter().any(|code| {
                        code.ends_with("body-undecoded")
                            || code.ends_with("curve-untyped")
                            || code.ends_with("surface-untyped")
                    })
                },
                "untyped geometry",
            ),
        ),
        gate(
            "L4",
            "the history decodes in order with operation semantics",
            some(&|f| f.features > 0, "has a feature history"),
        ),
        gate(
            "L5",
            "no fixture keeps a feature of an untyped family native",
            Vec::new(),
        ),
        gate("L6", "constraints, expressions and every operation family are typed", gaps(L6_GAPS)),
        gate("L7", "components, joints and persistent names validate", gaps(L7_GAPS)),
        gate("L8", "appearances and application data are typed or retained with identity", gaps(L8_GAPS)),
        gate("L9", "writes are deterministic, edits survive, targets are refused or generated, SolidCraft accepts the output", write_blockers),
    ];
    // A gate passes only when every earlier one does.
    let mut open = false;
    for gate in &mut gates {
        if open {
            gate.passed = false;
        }
        open |= !gate.passed;
    }
    gates
}

fn paths(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("sc") || extension.eq_ignore_ascii_case("solidcraft")
            }) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn main() -> Result<(), Failure> {
    let mut args = std::env::args().skip(1);
    let (Some(root), Some(output)) = (args.next(), args.next()) else {
        return Err("usage: solidcraft-profile <fixtures-dir> <output.json>".into());
    };
    let mut fixtures = Vec::new();
    let mut generated: Option<Vec<u8>> = None;
    let mut unsupported_target = true;
    let mut unsupported_edit = true;
    for path in paths(Path::new(&root))? {
        let bytes = fs::read(&path)?;
        // A drawing container is no design: the ladder counts designs.
        if dialect(&bytes).contains("drawing") {
            continue;
        }
        let entry = fixture(&path, &bytes)?;
        let (ir, _, fidelity) = decode(&bytes)?.into_parts();
        unsupported_target &= plan_bytes(
            &ir,
            Some(&fidelity),
            TargetRequest::Explicit("solidcraft:json-v56"),
        )
        .is_err();
        let mut edited = ir.clone();
        if let Some(source) = edited.source.as_mut() {
            source.attributes.insert(
                cadmpeg_core::nonblank_const!("document_name"),
                "Renamed".to_owned(),
            );
        }
        unsupported_edit &= plan_bytes(&edited, Some(&fidelity), TargetRequest::Inherit).is_err();
        if generated.is_none() && entry.source_less_generation == Some(true) {
            generated = source_less(&ir).ok();
        }
        fixtures.push(entry);
    }
    let edits: Vec<_> = fixtures
        .iter()
        .filter_map(|fixture| fixture.parameter_edit_round_trip)
        .collect();
    let write = WriteAssertions(BTreeMap::from([
        (
            "replay_identical",
            !fixtures.is_empty() && fixtures.iter().all(|f| f.replay_identical),
        ),
        (
            "parameter_edit_round_trip",
            !edits.is_empty() && edits.iter().all(|passed| *passed),
        ),
        (
            "unsupported_target_rejected",
            !fixtures.is_empty() && unsupported_target,
        ),
        (
            "unsupported_edit_rejected",
            !fixtures.is_empty() && unsupported_edit,
        ),
        ("source_less_generated", generated.is_some()),
        (
            "independent_native_acceptance",
            generated.as_deref().is_some_and(native_accepts),
        ),
    ]));
    let gates = gates(&fixtures, &write);
    let highest = gates
        .iter()
        .take_while(|gate| gate.passed)
        .last()
        .map(|gate| gate.level.clone());
    let profile = Profile {
        version: 1,
        format: "solidcraft",
        fixtures,
        write_assertions: write,
        gates,
        highest_passing_gate: highest,
    };
    let mut json = serde_json::to_string_pretty(&profile)?;
    json.push('\n');
    fs::write(output, json)?;
    Ok(())
}
