// SPDX-License-Identifier: Apache-2.0
//! A sketch the design cannot state falls back to its family alone.
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions, Decoded};

use super::projection::stored_thin;
use crate::reader::id;
use crate::test_support::review::{design_operation, unresolved, validate, wire};

fn imported_sketches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    view: cadmpeg_core::decode::View<'_>,
    unresolved: bool,
) -> Result<Decoded, cadmpeg_core::CodecError> {
    let mut decoded = cadmpeg_codec_step::StepCodec::default().decode_impl(ctx, view)?;
    decoded.ir.design = cadmpeg_ir::document::Design::default();
    for (ordinal, name) in ["imported-good", "imported-failure"]
        .into_iter()
        .enumerate()
    {
        let bad = ordinal == 1;
        let placement = if bad && unresolved {
            json!({"kind":"unresolved"})
        } else {
            json!({"kind":"resolved","frame":{
                "origin":{"x":0.0,"y":0.0,"z":0.0},
                "normal":{"x":0.0,"y":0.0,"z":1.0},
                "u_axis":{"x":1.0,"y":0.0,"z":0.0}}})
        };
        decoded.ir.model.sketches.push(
            json::from_value(json!({
                "id":id("sketch",name),"placement":placement,"profiles":[]
            }))
            .unwrap(),
        );
        decoded.ir.model.features.push(
            json::from_value(json!({
                "id":id("feature",name),"ordinal":ordinal + 10,"dependencies":[],
                "source_properties":{},"source_content":[],"outputs":[],
                "definition":{"kind":"operation","value":{"kind":"sketch","value":{
                    "sketch":{"kind":"planar","sketch":id("sketch",name)}}}}
            }))
            .unwrap(),
        );
        let geometry = if bad {
            json!({"kind":"parabola","vertex":{"u":0.0,"v":0.0},
                "axis_angle":0.0,"focal_length":1.0})
        } else {
            json!({"kind":"point","position":{"u":0.0,"v":0.0}})
        };
        decoded.ir.model.sketch_entities.push(
            json::from_value(json!({
                "id":id("entity",name),"sketch":id("sketch",name),
                "endpoint_refs":[],"construction":false,"geometry":geometry
            }))
            .unwrap(),
        );
    }
    Ok(decoded)
}

fn one_sketch_falls_back(decoder: crate::StepDecoder, reason: &str) {
    let bytes = stored_thin(true, false, false);
    let result = crate::OnshapeCodec::new(decoder)
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let wire = wire(&result);
    let native = wire["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["id"] == id("feature", "Plate"))
        .unwrap();
    assert_eq!(native["definition"]["value"]["kind"], "native");
    assert_eq!(result.ir().model.features.len(), 4);
    assert_eq!(result.ir().model.sketches.len(), 3);
    assert_eq!(
        design_operation(result.ir(), "imported-failure"),
        unresolved("sketch")
    );
    assert_eq!(
        design_operation(result.ir(), "imported-good")["kind"],
        "sketch"
    );
    assert_eq!(design_operation(result.ir(), "Plate")["kind"], "extrude");
    assert!(!result
        .ir()
        .design
        .sketches
        .iter()
        .any(|sketch| sketch.id.as_str() == id("sketch", "imported-failure")));
    assert!(result.report().losses.iter().any(|note| {
        crate::loss::OnshapeLossCode::DesignOperationUnresolved.matches(note)
            && note.message.contains(reason)
            && note
                .message
                .ends_with("subject: onshape:model:feature#imported-failure")
    }));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|note| crate::loss::OnshapeLossCode::ThinModelNotSupported.matches(note)));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|note| crate::loss::OnshapeLossCode::ThinNotSupported.matches(note)));
    validate(result);
}

#[test]
fn unresolved_sketch_placement_falls_back_alone() {
    one_sketch_falls_back(
        |ctx, view| imported_sketches(ctx, view, true),
        "the sketch frame is unresolved",
    );
}

#[test]
fn uncovered_sketch_curve_falls_back_alone() {
    one_sketch_falls_back(
        |ctx, view| imported_sketches(ctx, view, false),
        "the sketch curve kind has no design form",
    );
}
