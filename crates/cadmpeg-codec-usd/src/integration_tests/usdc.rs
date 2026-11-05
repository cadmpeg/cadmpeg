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
fn binary_export_obeys_the_caller_entity_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeError, EncodeInput, Encoder};

    let (_, source) = inputs("usda")
        .into_iter()
        .find(|(name, _)| name == "assembly_instances")
        .expect("assembly fixture");
    let decoded = UsdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode assembly");
    let mut input = EncodeInput::new(decoded.ir(), None);
    input.limits.max_entities = 0;
    let result = UsdCodec.plan(input, TargetRequest::Explicit("usdc"));
    assert!(matches!(
        result,
        Err(EncodeError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::Entities
    ));
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

/// A value representation that sets an unused bit is refused.
#[test]
fn a_representation_with_an_unused_bit_is_refused() {
    let text = "#usda 1.0\ndef Xform \"A\"\n{\n    double value.timeSamples = {\n        0: 1.5,\n    }\n}\n";
    let mut bytes = crate::test_support::with_context(text.as_bytes(), |ctx| {
        let layer = crate::text::grammar::parse_layer(ctx, text.as_bytes()).expect("parse");
        crate::usdc::write::write(&layer).expect("write")
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

#[test]
fn output_entity_limit_counts_written_prims_and_properties() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeError, EncodeInput, Encoder};

    let (_, source) = inputs("usda")
        .into_iter()
        .find(|(name, _)| name == "assembly_instances")
        .expect("assembly fixture");
    let decoded = UsdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode assembly");
    let plan = UsdCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("usda-1.0"),
        )
        .expect("plan text output");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write text output");
    let count = written_entities(&bytes);
    assert!(count > 0);
    for target in ["usda-1.0", "usdc", "usdz"] {
        let mut limited = EncodeInput::new(decoded.ir(), None);
        limited.limits.max_entities = count - 1;
        assert!(
            matches!(UsdCodec.plan(limited, TargetRequest::Explicit(target)),
            Err(EncodeError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::Entities)
        );
        let mut exact = EncodeInput::new(decoded.ir(), None);
        exact.limits.max_entities = count;
        UsdCodec
            .plan(exact, TargetRequest::Explicit(target))
            .expect("exact output entity budget");
    }
}

fn written_entities(bytes: &[u8]) -> u64 {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    fn count(body: &crate::text::syntax::PrimBody) -> u64 {
        assert!(body.variant_sets.is_empty());
        u64::try_from(body.properties.len()).unwrap()
            + body
                .children
                .iter()
                .map(|prim| 1 + count(&prim.body))
                .sum::<u64>()
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let layer = crate::text::grammar::parse_layer(&ctx, bytes).expect("parse written layer");
    layer.prims.iter().map(|prim| 1 + count(&prim.body)).sum()
}

#[test]
fn omitted_solver_target_does_not_consume_output_entity_budget() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeError, EncodeInput, Encoder};
    use cadmpeg_ir::physics::solver::{SolverTarget, SolverValue};

    let (_, source) = inputs("usda")
        .into_iter()
        .find(|(name, _)| name == "physx_arm")
        .expect("PhysX fixture");
    let decoded = UsdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode PhysX fixture");
    let mut ir = decoded.ir().clone();
    let settings = ir
        .model
        .solver_settings
        .iter_mut()
        .find(|settings| settings.schema == "PhysxMimicJointAPI:transX")
        .expect("mimic settings");
    let value = settings
        .values
        .get_mut("physxMimicJoint:transX:referenceJoint")
        .expect("mimic target");
    let SolverValue::Targets { targets } = value else {
        panic!("target list")
    };
    *targets = vec![SolverTarget::Joint {
        joint: cadmpeg_ir::products::JointId::mint("test:usd:joint#missing").unwrap(),
    }];
    let plan = UsdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("usda-1.0"),
        )
        .expect("plan with omitted relationship");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("solver properties")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).contains("rel physxMimicJoint:transX:referenceJoint =")
    );
    let count = written_entities(&bytes);
    assert!(count > 0);
    let mut below = EncodeInput::new(&ir, None);
    below.limits.max_entities = count - 1;
    assert!(
        matches!(UsdCodec.plan(below, TargetRequest::Explicit("usda-1.0")),
        Err(EncodeError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::Entities)
    );
    let mut exact = EncodeInput::new(&ir, None);
    exact.limits.max_entities = count;
    UsdCodec
        .plan(exact, TargetRequest::Explicit("usda-1.0"))
        .expect("omitted relationship does not consume a record");
}

#[test]
fn every_text_fixture_uses_the_written_record_budget() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeError, EncodeInput, Encoder};

    let fixtures = inputs("usda");
    assert!(!fixtures.is_empty());
    for (name, source) in fixtures {
        let decoded = UsdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let target = TargetRequest::Explicit("usda-1.0");
        let plan = UsdCodec
            .plan(EncodeInput::new(decoded.ir(), None), target)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let count = written_entities(&bytes);
        if let Some(below_count) = count.checked_sub(1) {
            let mut below = EncodeInput::new(decoded.ir(), None);
            below.limits.max_entities = below_count;
            assert!(
                matches!(UsdCodec.plan(below, target),
                Err(EncodeError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::Entities),
                "{name}: {count} written records"
            );
        }
        let mut exact = EncodeInput::new(decoded.ir(), None);
        exact.limits.max_entities = count;
        UsdCodec
            .plan(exact, target)
            .unwrap_or_else(|error| panic!("{name}: {count} records: {error}"));
    }
}
