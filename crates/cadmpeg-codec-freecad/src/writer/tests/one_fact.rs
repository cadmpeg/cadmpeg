// SPDX-License-Identifier: Apache-2.0
//! Bound operands, source-only evidence, and native recompute state.

use crate::native::{ObjectRecord, PropertyRecord};
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::features::{Dimension, ParameterValue};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const HISTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
));

#[test]
fn editing_bound_pad_dimension_writes_one_value_and_native_touched_state() {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(HISTORY), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let pad = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .unwrap();
    let owner = pad.id.clone();
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&owner) && parameter.name == "Length")
        .unwrap()
        .id
        .clone();
    let definition = serde_json::to_value(pad.definition()).unwrap();
    let path = pad
        .definition()
        .dimension_paths()
        .into_iter()
        .find(|path| {
            definition.pointer(path).is_some_and(|value| {
                value["kind"] == "parameter" && value["value"] == parameter.as_str()
            })
        })
        .expect("Pad Length is a binding, never a separate literal copy");
    let length = cadmpeg_ir::scalar::Length::new(37.0).unwrap();
    ir.set_dimension(&owner, &path, length).unwrap();
    assert_eq!(
        ir.dimension_value(&Dimension::<cadmpeg_ir::scalar::Length>::Parameter(
            parameter
        )),
        Some(length)
    );
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let pad = reread
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .unwrap();
    assert!(pad.needs_recompute);
    let parameter = reread
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&pad.id) && parameter.name == "Length")
        .unwrap();
    assert_eq!(parameter.value, Some(ParameterValue::Length(length)));
    assert_eq!(
        parameter.expression.literal_text().as_deref(),
        Some("37 mm")
    );
    let native = reread.ir().native.namespace("fcstd").unwrap();
    let objects = native.arena_as::<ObjectRecord>("objects").unwrap();
    let object = objects
        .iter()
        .find(|object| Some(object.id().as_str()) == pad.native_ref.as_deref())
        .unwrap();
    assert_eq!(
        object.attributes.get("Touched").map(String::as_str),
        Some("1")
    );
    let properties = native.arena_as::<PropertyRecord>("properties").unwrap();
    let length = properties
        .iter()
        .find(|property| property.owner == object.id().as_str() && property.name == "Length")
        .unwrap();
    assert_eq!(
        length.values()[0]
            .attributes
            .get("value")
            .map(String::as_str),
        Some("37")
    );
    assert!(properties
        .iter()
        .all(|property| { property.owner != object.id().as_str() || property.name != "Touched" }));
}

#[test]
fn cadir_native_records_hold_evidence_and_typed_values_without_source_images() {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(HISTORY), &DecodeOptions::default())
        .unwrap();
    let wire = serde_json::to_value(decoded.ir()).unwrap();
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    for (arena, forbidden) in [
        ("entries", &["data"][..]),
        ("properties", &["raw_xml", "byte_start", "byte_end"][..]),
        ("objects", &["data_xml", "data_start", "data_end"][..]),
        ("extensions", &["raw_xml"][..]),
    ] {
        if let Some(records) = native.arenas().get(arena) {
            for record in records {
                for field in forbidden {
                    assert!(record.field(field).is_none(), "{arena} serialized {field}");
                }
            }
        }
    }
    assert!(decoded
        .source_fidelity()
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .and_then(|record| record.data())
        .is_some());
    let plain: cadmpeg_ir::CadIr = serde_json::from_value(wire).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&plain, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
}

#[test]
fn a_renamed_bound_parameter_drives_the_native_control_through_a_live_expression() {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(HISTORY), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let owner = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .unwrap()
        .id
        .clone();
    let parameter = ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.owner.as_ref() == Some(&owner) && parameter.name == "Length")
        .unwrap();
    parameter.name = "Depth".into();
    let parameter_id = parameter.id.clone();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let feature = reread
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .unwrap();
    let parameter = reread
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Depth"
        })
        .unwrap();
    assert_eq!(
        parameter.value,
        ir.model
            .parameters
            .iter()
            .find(|parameter| parameter.id == parameter_id)
            .unwrap()
            .value
    );
    assert!(!reread
        .ir()
        .model
        .parameters
        .iter()
        .any(
            |parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Length"
        ));
    let definition = serde_json::to_value(feature.definition()).unwrap();
    assert!(feature
        .definition()
        .dimension_paths()
        .iter()
        .any(|path| definition
            .pointer(path)
            .is_some_and(|dimension| dimension["kind"] == "parameter"
                && dimension["value"] == parameter.id.as_str())));
    let properties = reread
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<PropertyRecord>("properties")
        .unwrap();
    assert!(properties
        .iter()
        .filter(|property| property.name == "ExpressionEngine")
        .flat_map(PropertyRecord::values)
        .any(|value| value.tag == "Expression"
            && value
                .attributes
                .get("path")
                .is_some_and(|value| value == "Length")
            && value
                .attributes
                .get("expression")
                .is_some_and(|value| value == "Depth")));
}

#[test]
fn arbitrary_parameter_names_use_native_selectors_without_changing_the_binding() {
    for name in ["Depth in mm", "Label", "9 depth"] {
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(HISTORY), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        let owner = ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Pad"))
            .unwrap()
            .id
            .clone();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| {
                parameter.owner.as_ref() == Some(&owner) && parameter.name == "Length"
            })
            .unwrap();
        parameter.name = name.into();
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let reread = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let feature = reread
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Pad"))
            .unwrap();
        let parameters: Vec<_> = reread
            .ir()
            .model
            .parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
            .collect();
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .expect("the semantic parameter name survives a native selector alias");
        assert!(!parameters
            .iter()
            .any(|parameter| parameter.name == "Length"));
        let definition = serde_json::to_value(feature.definition()).unwrap();
        assert!(feature
            .definition()
            .dimension_paths()
            .iter()
            .any(
                |path| definition
                    .pointer(path)
                    .is_some_and(|value| value["kind"] == "parameter"
                        && value["value"] == parameter.id.as_str())
            ));
    }
}

#[test]
fn unheld_neutral_model_does_not_erase_typed_native_geometry_during_synthesis() {
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.ir().model.bodies.is_empty());
    let expected = decoded.ir().model.bodies.len();
    // This editor deliberately removes the neutral projection. The native
    // shape payloads remain typed content; no fidelity accompanies the save.
    let mut wire = serde_json::to_value(decoded.ir()).unwrap();
    for arena in cadmpeg_ir::document::ArenaName::ALL {
        wire["model"][arena.as_str()] = serde_json::json!([]);
    }
    wire["scope"] = serde_json::to_value(cadmpeg_ir::ContentScope::container()).unwrap();
    let ir = serde_json::from_value(wire).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(reread.ir().model.bodies.len(), expected);
    assert!(crate::test_support::validate_native(reread.ir()).is_empty());
}
