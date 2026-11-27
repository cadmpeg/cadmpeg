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
    cadmpeg_core::json!({ "geometry": &result.ir().geometry, "model": &result.ir().model })
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
        cadmpeg_ir::codec::write::EncodeInput::new(decoded.ir(), None).with_target(
            cadmpeg_ir::codec::write::target::TargetRequest::Explicit("usdz"),
        ),
    )
    .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let (written, _) = crate::test_support::encode(decoded.ir());
    assert_eq!(model(&bytes, "package"), model(written.as_bytes(), "layer"));
}

/// A legacy `sdf` layer is outside the synthesis catalog, so `Inherit` writes
/// it as the catalog default `usda-1.0` (§3.4, Refusals). The report names
/// that row, `usd/target.source-dialect-displaced` names both dialects, and
/// the written layer reads back as `usda-1.0`.
#[test]
fn inherit_writes_an_off_catalog_source_as_the_default() {
    use cadmpeg_ir::codec::write::target::{target_loss_kind, SOURCE_DIALECT_DISPLACED};
    let (_, text) = inputs("usda")
        .into_iter()
        .find(|(name, _)| name == "sdf_legacy")
        .expect("fixture");
    let dialect = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.dialect())
            .map(|dialect| dialect.dialect().to_string())
    };
    let decoded = crate::test_support::decode_bytes(&text);
    let source = dialect(&decoded).expect("the source records a dialect");
    let plan = cadmpeg_ir::codec::write::Encoder::plan(
        &UsdCodec,
        cadmpeg_ir::codec::write::EncodeInput::new(decoded.ir(), None),
    )
    .expect("the source is written as the catalog default");
    assert_eq!(
        cadmpeg_test_support::wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("usd:usda-1.0".to_owned())
    );
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code
                == target_loss_kind(cadmpeg_ir::loss_namespace!("usd"), SOURCE_DIALECT_DISPLACED)
        })
        .expect("the source dialect displacement is charged");
    assert!(
        loss.message.contains(&source) && loss.message.contains("usd:usda-1.0"),
        "{}",
        loss.message
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert_eq!(
        dialect(&crate::test_support::decode_bytes(&bytes)).as_deref(),
        Some("usd:usda-1.0")
    );
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
        assert_eq!(from_text.ir().geometry, from_crate.ir().geometry, "{name}");
        assert_eq!(from_text.ir().model, from_crate.ir().model, "{name}");
    }
}

/// A field whose value has no USDC form is left out of its spec and named,
/// and the layer reads back with the rest of the spec.
#[test]
fn a_field_with_no_crate_form_is_left_out_and_named() {
    use crate::text::syntax::{AttributeField, PropertySpec};
    let text = "#usda 1.0\ndef Xform \"A\"\n{\n    float a = AnimationBlock\n    double b.spline = {\n        1: 0,\n    }\n    float c = 2\n}\n";
    let (bytes, left_out) = crate::test_support::with_context(text.as_bytes(), |ctx| {
        let layer = crate::text::grammar::parse_layer(ctx, text.as_bytes()).expect("parse");
        crate::usdc::write::write(&layer).expect("write")
    });
    assert_eq!(left_out, ["/A.a default", "/A.b spline"]);
    let (read, unread) = crate::test_support::with_root(&bytes, |ctx, root| {
        crate::usdc::read(ctx, root).expect("read")
    });
    assert!(unread.is_empty(), "{unread:?}");
    let fields: Vec<(&str, bool)> = read.prims[0]
        .body
        .properties
        .iter()
        .map(|property| match property {
            PropertySpec::Attribute(attribute) => (
                attribute.name.as_str(),
                matches!(attribute.field, AttributeField::Default(_)),
            ),
            other @ PropertySpec::Relationship(_) => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(fields, [("a", false), ("b", false), ("c", true)]);
}

/// A value representation that sets an unused bit is refused.
#[test]
fn a_representation_with_an_unused_bit_is_refused() {
    let text = "#usda 1.0\ndef Xform \"A\"\n{\n    double value.timeSamples = {\n        0: 1.5,\n    }\n}\n";
    let mut bytes = crate::test_support::with_context(text.as_bytes(), |ctx| {
        let layer = crate::text::grammar::parse_layer(ctx, text.as_bytes()).expect("parse");
        crate::usdc::write::write(&layer).expect("write").0
    });
    // The sample is a `double` representation in the value bytes, which
    // precede the compressed structural sections; its payload is the offset
    // of the value.
    let value = bytes
        .windows(8)
        .position(|value| value == 1.5_f64.to_le_bytes())
        .expect("the sample value");
    let payload = u64::try_from(value).unwrap().to_le_bytes();
    let sample = bytes
        .windows(8)
        .position(|rep| rep[..6] == payload[..6] && rep[7] == 0)
        .expect("the sample representation");
    bytes[sample + 7] |= 1;
    let refused = UsdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default());
    let error = refused.expect_err("refused").to_string();
    assert!(error.contains("bits 56 to 59"), "{error}");
}
