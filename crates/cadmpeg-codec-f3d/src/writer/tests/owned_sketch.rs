// SPDX-License-Identifier: Apache-2.0
//! Canonical Sketch parameters drive source-less output and retain their owner.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{Expression, ParameterValue};
use cadmpeg_ir::scalar::Length;

use crate::F3dCodec;

#[test]
fn owned_sketch_parameter_synthesis_uses_the_canonical_cache_without_source_bytes() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir: cadmpeg_ir::document::CadIr =
        serde_json::from_slice(&serde_json::to_vec(decoded.ir()).unwrap()).unwrap();
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d1")
        .unwrap();
    let parameter_id = parameter.id.clone();
    let owner_id = parameter.owner.clone().unwrap();
    let owner_name = ir
        .model
        .features
        .iter()
        .find(|feature| feature.id == owner_id)
        .unwrap()
        .name
        .clone();
    let value = ParameterValue::Length(Length::new(15.0).unwrap());
    ir.set_parameter_expression(&parameter_id, Expression::literal(&value), Some(value))
        .unwrap();
    let native = crate::test_support::native_test::f3d_native(&ir);
    assert_eq!(native.design_parameters.len(), 2);
    assert!(native
        .design_parameters
        .iter()
        .all(|parameter| parameter.source_ordinal == 9));
    assert_eq!(
        native
            .design_parameters
            .iter()
            .find(|parameter| parameter.name() == "d1")
            .unwrap()
            .evaluated_value()
            .get(),
        1.0
    );
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.design-records-omitted"));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "evaluation.stale"));
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, written.ir()).unwrap()
    });
    assert!(findings.is_empty(), "native findings: {findings:?}");
    let validation = cadmpeg_ir::validate::validate_neutral(written.ir(), Vec::new()).unwrap();
    assert!(
        validation.is_ok(),
        "neutral findings: {:?}",
        validation.findings
    );
    assert_eq!(written.ir().model.features.len(), 1);
    assert_eq!(written.ir().model.sketches.len(), 1);
    assert_eq!(
        written.ir().model.sketch_entities.len(),
        ir.model.sketch_entities.len()
    );
    assert_eq!(written.ir().model.parameters.len(), 2);
    assert_eq!(written.ir().model.sketch_constraints.len(), 4);
    for name in ["d1", "d2"] {
        let parameter = written
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap();
        assert_eq!(
            written
                .ir()
                .model
                .sketch_constraints
                .iter()
                .filter(|constraint| {
                    let definition = serde_json::to_value(&constraint.definition).unwrap();
                    definition["parameter"] == serde_json::json!(parameter.id)
                        || definition["parameter"]
                            == serde_json::json!({"kind":"parameter", "value":parameter.id})
                })
                .count(),
            1,
            "{name} retains precisely one canonical constraint binding"
        );
    }
    let owner = written
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name == owner_name)
        .expect("the Sketch owner survives synthesis");
    let parameter = written
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d1" && parameter.owner.as_ref() == Some(&owner.id))
        .expect("the canonical parameter retains its owner");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(15.0).unwrap()))
    );
    let native = crate::test_support::native_test::f3d_native(written.ir());
    assert_eq!(native.design_parameters.len(), 2);
    assert!(native
        .design_parameters
        .iter()
        .all(|parameter| parameter.source_ordinal == 9));
    let record = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.name() == "d1")
        .unwrap();
    assert_eq!(record.evaluated_value().get(), 1.5);
    let owner = native
        .design_parameter_owners
        .iter()
        .find(|owner| Some(owner.record_index()) == record.owner_record_index())
        .unwrap();
    assert_eq!(owner.evaluated_value().get(), 1.5);
    assert!(written
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            serde_json::to_value(&constraint.definition).unwrap()["parameter"]
                == serde_json::json!(parameter.id)
        }));
}

#[test]
fn a_changed_neutral_sketch_entity_is_not_replaced_by_the_native_graph() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.sketch_entities[0].construction = true;
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let cadmpeg_ir::codec::write::ArenaCoverage::Declared(coverage) = plan.report().coverage else {
        panic!("omitted Sketch content must have explicit arena dispositions")
    };
    assert_eq!(
        coverage.features,
        cadmpeg_ir::codec::write::ArenaDisposition::Dropped
    );
    assert_eq!(
        coverage.sketch_entities,
        cadmpeg_ir::codec::write::ArenaDisposition::Dropped
    );
    for arena in ["model.features", "model.sketch_entities"] {
        assert!(
            plan.report().losses.iter().any(|loss| {
                loss.code.local_code() == "writer.design-records-omitted"
                    && loss.message.contains(arena)
            }),
            "the {arena} omission is reported by name"
        );
    }
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.sketch_entities.is_empty());
    assert!(written.ir().model.features.is_empty());
}

#[test]
fn deleted_neutral_sketch_design_is_not_recreated_from_stale_native_records() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.features.clear();
    ir.model.parameters.clear();
    ir.model.sketches.clear();
    ir.model.sketch_entities.clear();
    ir.model.sketch_constraints.clear();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for arena in [
        "design_parameter_scopes",
        "design_parameter_owners",
        "design_parameters",
        "design_sketch_placements",
    ] {
        assert_eq!(
            plan.report().native_dispositions.disposition("f3d", arena),
            cadmpeg_ir::codec::write::NativeDisposition::Dropped
        );
    }
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.features.is_empty());
    assert!(written.ir().model.parameters.is_empty());
    assert!(written.ir().model.sketches.is_empty());
    assert!(written.ir().model.sketch_entities.is_empty());
    assert!(written.ir().model.sketch_constraints.is_empty());
    let native = crate::test_support::native_test::f3d_native(written.ir());
    assert!(native.design_parameter_scopes.is_empty());
    assert!(native.design_parameter_owners.is_empty());
    assert!(native.design_parameters.is_empty());
    assert!(native.design_sketch_placements.is_empty());
}

#[test]
fn canonical_owned_parameter_order_updates_its_writable_owner_lane() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    for parameter in &mut ir.model.parameters {
        parameter.ordinal = match parameter.name.as_str() {
            "d1" => 1,
            "d2" => 0,
            _ => panic!("the fixture has precisely its two stated dimension parameters"),
        };
    }
    let mut output = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output))
        .unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native_test::f3d_native(written.ir());
    for (name, expected) in [("d1", 1), ("d2", 0)] {
        assert_eq!(
            written
                .ir()
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .unwrap()
                .ordinal,
            expected
        );
        let record = native
            .design_parameters
            .iter()
            .find(|parameter| parameter.name() == name)
            .unwrap();
        assert_eq!(record.source_ordinal, 9);
        let owner = native
            .design_parameter_owners
            .iter()
            .find(|owner| Some(owner.record_index()) == record.owner_record_index())
            .unwrap();
        assert_eq!(owner.local_ordinal(), Some(expected));
        assert_eq!(owner.owned_ordinal(), 9);
    }
}
