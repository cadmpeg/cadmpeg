// SPDX-License-Identifier: Apache-2.0
//! Interop materialization (§5.4 item 3, #243).
//!
//! When `CADMPEG_INTEROP_OUT` names a directory, each `*_interop_*` test
//! writes every §5.1 (same-format) and §5.2 (cross-format) synthesis output of
//! one writer to `<dir>/<writer>/`, with a `manifest.json` that records the
//! cadmpeg re-decode counts the native-application jobs compare against:
//! solid and face counts for `fcstd`, `iges` and `step`, object and layer
//! counts for `rhino`. A refusal or an output the writer's own decoder
//! rejects is recorded in the manifest, never failed here: removals,
//! refusals and re-decode agreement belong to the writer closure tests, while
//! the interop jobs check what the native application accepts. Without the
//! variable the tests pass after decoding nothing, so `cargo test-fast`
//! stays clean.
//!
//! In a debug build each test covers the §9.4 fixed sample (the first fixture
//! of every format in path order). A release build covers every fixture,
//! including `damaged/`.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_registry::build_encoder;
use cadmpeg_test_support::cross_closure::cross_request;
use cadmpeg_test_support::writer_closure::{same_request, write};
use serde_json::{json, Value};

use super::support::sampled_fixtures;

/// Every fixture-backed format that can feed a writer (§5.2 inputs).
const INPUTS: &[&str] = &[
    "step", "iges", "rhino", "fcstd", "f3d", "sldprt", "catia", "creo",
];

/// The file extension of one writer's outputs.
fn extension(writer: &str) -> &'static str {
    match writer {
        "step" => "step",
        "iges" => "igs",
        "fcstd" => "FCStd",
        "rhino" => "3dm",
        _ => unreachable!("{writer} has no interop job"),
    }
}

/// The relative output name of one input: `<source>__<stem>.<ext>`, under
/// `damaged/` for a damaged input. Stems come from committed fixture names.
fn output_name(source: &str, input: &Path, damaged: bool, writer: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .expect("fixture stem")
        .to_string_lossy()
        .replace(['/', '\\'], "__");
    assert!(
        !stem.is_empty() && stem != "." && stem != "..",
        "unsafe fixture stem"
    );
    let ext = extension(writer);
    let name = PathBuf::from(format!("{source}__{stem}.{ext}"));
    if damaged {
        PathBuf::from("damaged").join(name)
    } else {
        name
    }
}

fn decode(codec: &dyn Codec, encoder: &dyn Encoder, bytes: &[u8]) -> Result<DecodeResult, String> {
    let options = DecodeOptions {
        depth: encoder.request(),
        ..DecodeOptions::default()
    };
    codec
        .decode(&mut Cursor::new(bytes), &options)
        .map_err(|error| error.to_string())
}

fn solid_face_counts(ir: &Value) -> (u64, u64) {
    let mut solids = 0u64;
    if let Some(bodies) = ir.pointer("/model/bodies").and_then(Value::as_array) {
        for body in bodies {
            let kind = body.get("kind").and_then(Value::as_str).unwrap_or("solid");
            if kind == "solid" {
                solids += 1;
            }
        }
    }
    let faces = u64::try_from(
        ir.pointer("/model/faces")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    )
    .expect("the face count fits u64");
    (solids, faces)
}

/// Every distinct `rhino:object:record#` reference in the document: one per
/// written object, whatever neutral shape it took.
fn rhino_object_refs(ir: &Value) -> BTreeSet<String> {
    fn walk(value: &Value, found: &mut BTreeSet<String>) {
        match value {
            Value::String(text) if text.starts_with("rhino:object:record#") => {
                found.insert(text.clone());
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, found)),
            Value::Object(fields) => fields.values().for_each(|field| walk(field, found)),
            _ => {}
        }
    }

    let mut found = BTreeSet::new();
    walk(ir, &mut found);
    found
}

fn rhino_counts(ir: &Value) -> (u64, u64) {
    let objects = u64::try_from(rhino_object_refs(ir).len()).expect("the object count fits u64");
    let layers = u64::try_from(
        ir.pointer("/native/rhino/layers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    )
    .expect("the layer count fits u64");
    (objects, layers)
}

/// Write every §5.1 and §5.2 synthesis output of one writer, with its manifest.
fn materialize(writer: &str, format: cadmpeg_registry::Format) {
    let Some(root) = std::env::var_os("CADMPEG_INTEROP_OUT") else {
        eprintln!("{writer}: CADMPEG_INTEROP_OUT is unset; materializing nothing");
        return;
    };
    let directory = Path::new(&root).join(writer);
    std::fs::create_dir_all(&directory).expect("interop output directory");

    let catalog = Box::leak(Box::new(cadmpeg_registry::InputCatalog::with_builtins()));
    let lookup = |name: &str| {
        catalog
            .descriptors()
            .find(|descriptor| descriptor.format_id().as_str() == name)
            .and_then(cadmpeg_registry::InputDescriptor::codec)
    };
    let encoder = build_encoder(format);
    let Some(writer_codec) = lookup(writer) else {
        panic!("{writer} has no codec in this build");
    };

    let mut files = serde_json::Map::new();
    let mut skipped = Vec::new();
    for source in INPUTS {
        let Some(source_codec) = lookup(source) else {
            skipped.push(json!({"input": source, "reason": "codec missing from this build"}));
            continue;
        };
        for fixture in sampled_fixtures(source) {
            let input = fixture
                .path
                .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                .unwrap_or(&fixture.path)
                .to_string_lossy()
                .into_owned();
            let Ok(first) = decode(source_codec, encoder.as_ref(), &fixture.bytes) else {
                skipped.push(json!({"input": input, "reason": "input does not decode"}));
                continue;
            };
            let request = if *source == writer {
                same_request(encoder.as_ref(), &first)
            } else {
                cross_request(encoder.as_ref())
            };
            let written = match write(encoder.as_ref(), &first, false, request) {
                Ok(written) => written,
                Err(refusal) => {
                    skipped.push(json!({"input": input, "reason": refusal}));
                    continue;
                }
            };
            let name = output_name(source, &fixture.path, fixture.damaged, writer);
            let target = directory.join(&name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).expect("interop output subdirectory");
            }
            std::fs::write(&target, &written.bytes).expect("interop output bytes");
            let key = name.to_string_lossy().into_owned();
            match decode(writer_codec, encoder.as_ref(), &written.bytes) {
                Ok(after) => {
                    let after = serde_json::to_value(after.ir()).expect("re-decode serializes");
                    if writer == "rhino" {
                        let (objects, layers) = rhino_counts(&after);
                        files.insert(key, json!({"objects": objects, "layers": layers}));
                    } else {
                        let (solids, faces) = solid_face_counts(&after);
                        files.insert(key, json!({"solids": solids, "faces": faces}));
                    }
                }
                Err(error) => {
                    files.insert(key, json!({"error": error}));
                }
            }
        }
    }
    let manifest = json!({
        "version": 1,
        "writer": writer,
        "files": files,
        "skipped": skipped,
    });
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).expect("manifest serializes") + "\n",
    )
    .expect("manifest bytes");
    let file_count = files.len();
    let skip_count = skipped.len();
    eprintln!("{writer}: {file_count} file(s), {skip_count} skipped");
    assert!(
        !files.is_empty(),
        "{writer}: no interop output materialized; the native-application job would check nothing"
    );
}

#[cfg(feature = "fcstd")]
#[test]
fn fcstd_interop_materializes_every_output() {
    materialize("fcstd", cadmpeg_registry::Format::Fcstd);
}

#[cfg(feature = "iges")]
#[test]
fn iges_interop_materializes_every_output() {
    materialize("iges", cadmpeg_registry::Format::Iges);
}

#[cfg(feature = "step")]
#[test]
fn step_interop_materializes_every_output() {
    materialize("step", cadmpeg_registry::Format::Step);
}

#[cfg(feature = "rhino")]
#[test]
fn rhino_interop_materializes_every_output() {
    materialize("rhino", cadmpeg_registry::Format::Rhino);
}
