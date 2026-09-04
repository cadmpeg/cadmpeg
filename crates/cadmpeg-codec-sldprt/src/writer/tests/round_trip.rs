// SPDX-License-Identifier: Apache-2.0
//! Spatial-sketch write-back and semantic-write round-trip pins.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::{collections::BTreeMap, io::Cursor};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchId,
};
use cadmpeg_ir::transform::Transform;

use crate::test_support::{sldprt_with_body, triangle_body};
use crate::SldprtCodec;

fn source_less_spatial_line(start: Point3, end: Point3) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let sketch_id = SpatialSketchId("synthetic:test:spatial-sketch#path".into());
    let entity_id = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#line".into());
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: Some("Spatial path".into()),
        configuration: Some("0".into()),
        visible: None,
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            entity_id,
            sketch_id.clone(),
            SpatialSketchGeometry::Line { start, end },
        ));
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#spatial-path".into()),
        ordinal: 0,
        name: Some("Spatial path".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id),
        },
        native_ref: None,
    });
    ir
}

#[test]
fn retained_spatial_line_endpoint_edits_round_trip() {
    let mut first_encoding = Vec::new();
    SldprtCodec
        .plan(
            EncodeInput::new(
                &source_less_spatial_line(Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)),
                None,
            ),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut first_encoding))
        .expect("source-less spatial line should encode");
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(first_encoding), &DecodeOptions::default())
        .expect("encoded spatial line should decode")
        .into_parts()
        .0;
    let replacement_start = Point3::new(-7.5, 8.25, 9.0);
    let replacement_end = Point3::new(10.0, -11.5, 12.75);
    decoded.model.spatial_sketch_entities[0].geometry = SpatialSketchGeometry::Line {
        start: replacement_start,
        end: replacement_end,
    };

    let mut second_encoding = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&decoded, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut second_encoding))
        .expect("edited retained spatial line should encode");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(second_encoding), &DecodeOptions::default())
        .expect("edited retained spatial line should decode")
        .into_parts()
        .0;

    assert!(matches!(
        regenerated.model.spatial_sketch_entities[0].geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == replacement_start && end == replacement_end
    ));
}

#[test]
fn mutated_semantic_write_round_trips() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .expect("triangle fixture should decode");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.points[0].position.z += 1.0;
    let expected_z = decoded.ir().model.points[0].position.z;
    let expected_bodies = decoded.ir().model.bodies.len();
    let expected_faces = decoded.ir().model.faces.len();

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .expect("mutated triangle should write");
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("written triangle should decode");
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert_eq!(round_trip.ir().model.bodies.len(), expected_bodies);
    assert_eq!(round_trip.ir().model.faces.len(), expected_faces);
    assert!(
        floats_agree(round_trip.ir().model.points[0].position.z, expected_z),
        "mutated z drifted: got {} expected {}",
        round_trip.ir().model.points[0].position.z,
        expected_z
    );
}

#[test]
fn bake_transform_is_applied_and_output_stays_valid() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .expect("triangle fixture should decode");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let original_x = decoded.ir().model.points[0].position.x;
    decoded.ir_mut().model.bodies[0].transform = Some(Transform {
        rows: [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .expect("translated triangle should write");
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("written translated triangle should decode");
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert!(
        floats_agree(
            round_trip.ir().model.points[0].position.x,
            original_x + 10.0
        ),
        "baked translation drifted: got {} expected {}",
        round_trip.ir().model.points[0].position.x,
        original_x + 10.0
    );
}
