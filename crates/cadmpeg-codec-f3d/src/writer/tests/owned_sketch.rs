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
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
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
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.sketch_entities.is_empty());
    assert!(written.ir().model.features.is_empty());
}
