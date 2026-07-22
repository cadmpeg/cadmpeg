// SPDX-License-Identifier: Apache-2.0
//! Spatial-sketch semantic write-back integration tests.

use std::{collections::BTreeMap, io::Cursor};

use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, Encoder};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchId,
};

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
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: entity_id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line { start, end },
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#spatial-path".into()),
        ordinal: 0,
        name: Some("Spatial path".into()),
        suppressed: false,
        parent: None,
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
        .encode(
            &source_less_spatial_line(Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)),
            &mut first_encoding,
        )
        .expect("source-less spatial line should encode");
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(first_encoding), &DecodeOptions::default())
        .expect("encoded spatial line should decode")
        .ir;
    let replacement_start = Point3::new(-7.5, 8.25, 9.0);
    let replacement_end = Point3::new(10.0, -11.5, 12.75);
    decoded.model.spatial_sketch_entities[0].geometry = SpatialSketchGeometry::Line {
        start: replacement_start,
        end: replacement_end,
    };

    let mut second_encoding = Vec::new();
    SldprtCodec
        .encode(&decoded, &mut second_encoding)
        .expect("edited retained spatial line should encode");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(second_encoding), &DecodeOptions::default())
        .expect("edited retained spatial line should decode")
        .ir;

    assert!(matches!(
        regenerated.model.spatial_sketch_entities[0].geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == replacement_start && end == replacement_end
    ));
}
