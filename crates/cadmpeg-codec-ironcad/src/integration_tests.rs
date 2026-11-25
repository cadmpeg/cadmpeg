// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions, DecodeResult};

use crate::loss::IronCadLossCode;
use crate::test_support::corpus;
use crate::IronCadCodec;

const BOX: &str = "Box_50x40x25.ics";
const CYLINDER: &str = "Cylinder_50x50.ics";
const SCENE: &str = "Ironcad_2025_simple_test_parts.ics";

fn detect(bytes: &[u8]) -> Confidence {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let confidence = IronCadCodec.detect(&ctx, root).unwrap();
    ctx.finish_session().unwrap();
    confidence
}

fn decode(name: &str) -> DecodeResult {
    IronCadCodec
        .decode(&mut Cursor::new(corpus(name)), &DecodeOptions::default())
        .unwrap()
}

fn count(result: &DecodeResult, code: IronCadLossCode) -> usize {
    let code = code.note("").code;
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code)
        .count()
}

fn layer_formats(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .dialects()
        .unwrap()
        .iter()
        .map(|layer| layer.format().to_owned())
        .collect()
}

#[test]
fn scenes_are_detected() {
    for name in [BOX, CYLINDER, SCENE] {
        assert_eq!(detect(&corpus(name)), Confidence::High, "{name}");
    }
}

#[test]
fn compound_file_without_scene_identity_is_refused() {
    let bytes = cadmpeg_test_support::compound::compound_fixture();
    assert_eq!(detect(&bytes), Confidence::No);
    assert!(IronCadCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .is_err());
    assert!(IronCadCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .is_err());
}

#[test]
fn inspect_lists_scene_streams() {
    let summary = IronCadCodec
        .inspect(&mut Cursor::new(corpus(BOX)), &InspectOptions::default())
        .unwrap();
    let names: Vec<_> = summary
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    for stream in ["Brep Data", "Shape Data", "Document Data", "Preview", "HSF"] {
        assert!(names.contains(&stream), "{stream} in {names:?}");
    }
}

#[test]
fn single_part_scenes_hold_two_parasolid_records() {
    for name in [BOX, CYLINDER] {
        let result = decode(name);
        assert_eq!(count(&result, IronCadLossCode::BodiesPending), 2, "{name}");
        assert_eq!(count(&result, IronCadLossCode::KernelPrologueUnreadable), 0);
        assert_eq!(count(&result, IronCadLossCode::RecordHeaderUnrecognized), 0);
        let formats = layer_formats(&result);
        assert_eq!(formats[0], "ironcad");
        assert_eq!(
            formats.iter().filter(|format| *format == "parasolid").count(),
            2,
            "{name}"
        );
    }
}

#[test]
fn mixed_kernel_scene_holds_parasolid_and_acis_records() {
    let result = decode(SCENE);
    assert_eq!(count(&result, IronCadLossCode::BodiesPending), 12);
    assert_eq!(count(&result, IronCadLossCode::KernelPrologueUnreadable), 0);
    assert_eq!(count(&result, IronCadLossCode::KernelUnrecognized), 0);
    let formats = layer_formats(&result);
    assert_eq!(
        formats.iter().filter(|format| *format == "parasolid").count(),
        10
    );
    assert_eq!(formats.iter().filter(|format| *format == "acis").count(), 2);
}

#[test]
fn parasolid_layers_carry_the_schema_token() {
    let result = decode(BOX);
    let layers = result.report().dialects().unwrap();
    for layer in layers.iter().filter(|layer| layer.format() == "parasolid") {
        let schema = layer
            .declared()
            .iter()
            .find(|(key, _)| key.as_str() == cadmpeg_parasolid::DECLARED_SCHEMA)
            .map(|(_, value)| value.as_str());
        assert_eq!(schema, Some("SCH_3401176_34101_13006"));
    }
}

#[test]
fn undecoded_scene_streams_are_named() {
    let result = decode(BOX);
    let pending: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IronCadLossCode::SceneStreamsPending.note("").code)
        .map(|loss| loss.message.as_str())
        .collect();
    assert!(pending.iter().any(|message| message.contains("`Shape Data`")));
    assert!(!pending.iter().any(|message| message.contains("`Brep Data`")));
    assert!(!pending.iter().any(|message| message.contains("`HSF_LIB`")));
}

#[test]
fn container_depth_reads_no_records() {
    let options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    let result = IronCadCodec
        .decode(&mut Cursor::new(corpus(SCENE)), &options)
        .unwrap();
    assert_eq!(count(&result, IronCadLossCode::BodiesPending), 0);
}
