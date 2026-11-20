// SPDX-License-Identifier: Apache-2.0
//! Thin sketch projection failures preserve bundle history.
use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions, Decoded};
use serde_json::json;

use super::projection::stored_thin;
use crate::reader::id;
use crate::test_support::review::{validate, wire};

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
            serde_json::from_value(json!({
                "id":id("sketch",name),"placement":placement,"profiles":[]
            }))
            .unwrap(),
        );
        decoded.ir.model.features.push(
            serde_json::from_value(json!({
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
            serde_json::from_value(json!({
                "id":id("entity",name),"sketch":id("sketch",name),
                "endpoint_refs":[],"construction":false,"geometry":geometry
            }))
            .unwrap(),
        );
    }
    Ok(decoded)
}

fn refused_projection(decoder: crate::StepDecoder, reason: &str) {
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
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(result.ir().design, cadmpeg_ir::document::Design::default());
    assert!(result.report().losses.iter().any(|note| {
        crate::loss::OnshapeLossCode::ThinNotSupported.matches(note)
            && note.message.contains(reason)
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|note| { crate::loss::OnshapeLossCode::ThinModelNotSupported.matches(note) }));
    validate(result);
}

#[test]
fn unresolved_sketch_placement_restores_thin_design() {
    refused_projection(
        |ctx, view| imported_sketches(ctx, view, true),
        "thin sketch frame is unresolved",
    );
}

#[test]
fn uncovered_sketch_curve_restores_thin_design() {
    refused_projection(
        |ctx, view| imported_sketches(ctx, view, false),
        "thin sketch curve is unresolved",
    );
}
