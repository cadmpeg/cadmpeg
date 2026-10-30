// SPDX-License-Identifier: Apache-2.0
//! A USDC layer and a USDZ package decode to the model of the USDA layer
//! they were written from.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::Harness;

use crate::UsdCodec;

fn inputs(extension: &'static str) -> Vec<(String, Vec<u8>)> {
    Harness::new(env!("CARGO_MANIFEST_DIR"), extension, "").fixture_inputs()
}

fn model(bytes: &[u8], name: &str) -> serde_json::Value {
    let result = UsdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    serde_json::to_value(&result.ir().model).expect("serialize the model")
}

/// The text fixture a binary fixture `<stem>_<major>_<minor>_<patch>` was
/// written from.
fn text_stem(name: &str) -> &str {
    let mut stem = name;
    for _ in 0..3 {
        stem = stem.rsplit_once('_').map_or(stem, |(head, _)| head);
    }
    stem
}

#[test]
fn every_binary_fixture_decodes_to_the_model_of_its_text_layer() {
    let texts = inputs("usda");
    let binaries = inputs("usdc");
    assert!(binaries.len() >= 15);
    for (name, bytes) in binaries {
        let stem = text_stem(&name);
        let (_, text) = texts
            .iter()
            .find(|(candidate, _)| candidate == stem)
            .unwrap_or_else(|| panic!("no text fixture for {name}"));
        assert_eq!(model(&bytes, &name), model(text, stem), "{name}");
    }
}

#[test]
fn every_crate_version_is_admitted() {
    for (name, bytes) in inputs("usdc") {
        let result = UsdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let primary = result
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialects()
            .unwrap()
            .primary()
            .dialect()
            .as_str()
            .to_owned();
        let version = name.rsplitn(4, '_').take(3).collect::<Vec<_>>();
        let expected = format!("usd:usdc-{}.{}.{}", version[2], version[1], version[0]);
        assert_eq!(primary, expected, "{name}");
    }
}

#[test]
fn a_package_decodes_its_root_layer() {
    let packages = inputs("usdz");
    let texts = inputs("usda");
    let (_, package) = packages
        .iter()
        .find(|(name, _)| name == "assembly_package")
        .expect("package fixture");
    let (_, text) = texts
        .iter()
        .find(|(name, _)| name == "assembly_instances")
        .expect("text fixture");
    let result = UsdCodec
        .decode(&mut Cursor::new(package), &DecodeOptions::default())
        .unwrap();
    let dialects: Vec<_> = result
        .ir()
        .source
        .as_ref()
        .unwrap()
        .dialects()
        .unwrap()
        .iter()
        .map(|layer| layer.dialect().as_str().to_owned())
        .collect();
    assert_eq!(dialects, ["usd:usdz", "usd:usdc-0.8.0"]);
    assert_eq!(model(package, "package"), model(text, "text"));
}

#[test]
fn a_written_package_reads_back() {
    let (_, text) = inputs("usda")
        .into_iter()
        .find(|(name, _)| name == "mesh_cube")
        .expect("fixture");
    let decoded = UsdCodec
        .decode(&mut Cursor::new(&text), &DecodeOptions::default())
        .unwrap();
    let plan = cadmpeg_ir::codec::write::Encoder::plan(
        &UsdCodec,
        cadmpeg_ir::codec::write::EncodeInput::new(decoded.ir(), None),
        cadmpeg_ir::codec::write::target::TargetRequest::Explicit("usdz"),
    )
    .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let (written, _) = crate::test_support::encode(decoded.ir());
    assert_eq!(model(&bytes, "package"), model(written.as_bytes(), "layer"));
}

/// A model written as a USDC layer reads back as the same model as when it
/// is written as a USDA layer.
#[test]
fn a_written_crate_layer_reads_as_its_text_layer() {
    use cadmpeg_ir::codec::write::target::TargetRequest;
    for (name, bytes) in crate::integration_tests::fixtures() {
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let source = crate::test_support::decode(text);
        let as_text =
            crate::test_support::encode_bytes(source.ir(), TargetRequest::Explicit("usda-1.0"));
        let as_crate =
            crate::test_support::encode_bytes(source.ir(), TargetRequest::Explicit("usdc"));
        assert!(as_crate.starts_with(b"PXR-USDC\x00\x08\x00"), "{name}");
        let from_text = crate::test_support::decode_bytes(&as_text);
        let from_crate = crate::test_support::decode_bytes(&as_crate);
        assert_eq!(from_text.ir().model, from_crate.ir().model, "{name}");
    }
}
