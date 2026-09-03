// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn semantic_writer_rejects_retained_sketch_constraint_edits() {
    use cadmpeg_ir::sketches::{SketchConstraint, SketchConstraintDefinition, SketchConstraintId};

    let source = sldprt_with_nested_sketch_profile(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let sketch = decoded.ir().model.sketches[0].id.clone();
    let entity = decoded.ir().model.sketch_entities[0].id().clone();
    decoded
        .ir_mut()
        .model
        .sketch_constraints
        .push(SketchConstraint {
            id: SketchConstraintId("synthetic:test:constraint#horizontal".into()),
            sketch,
            definition: SketchConstraintDefinition::Horizontal { entity },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    assert_ne!(
        decoded.ir().source.as_ref().unwrap().attributes["document_local_sha256"],
        crate::decode::document_local_sha256(decoded.ir())
    );

    let error = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error
        .to_string()
        .contains("SLDPRT native sketch relation editing is not implemented"));
}

#[test]
fn semantic_writer_round_trips_planar_and_spatial_sketch_space() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Spatial path" Type="3DSketch" id="40"/>
            <Sketch Name="Profile" Type="Sketch" id="41"/>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::SpatialSketch { sketch: None }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));

    decoded.ir_mut().model.features[0].name = Some("Renamed spatial path".into());
    decoded.ir_mut().model.features[1].definition =
        FeatureDefinition::SpatialSketch { sketch: None };

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].kind, "3DSketch");
    assert_eq!(native[0].name, "Renamed spatial path");
    assert_eq!(native[1].kind, "3DSketch");
    assert!(regenerated
        .ir()
        .model
        .features
        .iter()
        .all(|feature| matches!(
            feature.definition,
            FeatureDefinition::SpatialSketch { sketch: None }
        )));
}

#[test]
fn semantic_writer_applies_line_sketch_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_sketch_profile(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let point_ref = decoded.ir().model.sketch_entities[0].endpoint_refs[0].clone();
    for entity in &mut decoded.ir_mut().model.sketch_entities {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            panic!("line sketch entity");
        };
        if entity.endpoint_refs[0] == point_ref {
            start.u += 1.0;
        }
        if entity.endpoint_refs[1] == point_ref {
            end.u += 1.0;
        }
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let edited = regenerated
        .ir()
        .model
        .sketch_entities
        .iter()
        .flat_map(|entity| match &entity.geometry {
            SketchGeometry::Line { start, end } => [start.u, end.u],
            _ => panic!("line sketch entity"),
        })
        .filter(|value| (*value - 1.0).abs() < 1.0e-12)
        .count();
    assert_eq!(edited, 2);
}

#[test]
fn semantic_writer_applies_compressed_line_sketch_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_compressed_nested_sketch_profile(
                &triangle_body(),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let point_ref = decoded.ir().model.sketch_entities[0].endpoint_refs[0].clone();
    for entity in &mut decoded.ir_mut().model.sketch_entities {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            panic!("line sketch entity");
        };
        if entity.endpoint_refs[0] == point_ref {
            start.v += 2.0;
        }
        if entity.endpoint_refs[1] == point_ref {
            end.v += 2.0;
        }
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = container::scan_bytes(&written);
    let lane = scan
        .blocks
        .iter()
        .find(|block| {
            block
                .section
                .as_deref()
                .is_some_and(|section| section.contains("ResolvedFeatures"))
        })
        .unwrap();
    assert!(lane
        .payload
        .windows(2)
        .any(|bytes| { bytes[0] == 0x78 && matches!(bytes[1], 0x01 | 0x9c | 0xda) }));
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let edited = regenerated
        .ir()
        .model
        .sketch_entities
        .iter()
        .flat_map(|entity| match &entity.geometry {
            SketchGeometry::Line { start, end } => [start.v, end.v],
            _ => panic!("line sketch entity"),
        })
        .filter(|value| (*value - 2.0).abs() < 1.0e-12)
        .count();
    assert_eq!(edited, 2);
}

#[test]
fn semantic_writer_rejects_conflicting_shared_sketch_point_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_sketch_profile(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let SketchGeometry::Line { start, .. } = &mut ir_edit.model.sketch_entities[0].geometry
        else {
            panic!("line sketch entity");
        };
        start.u += 1.0;
    }

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("conflicting positions")
    ));
}

#[test]
fn semantic_writer_applies_circle_sketch_edits() {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_circular_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let SketchGeometry::Circle { center, radius } =
            &mut ir_edit.model.sketch_entities[0].geometry
        else {
            panic!("circle sketch entity");
        };
        center.u = 250.0;
        *radius = Length(750.0);
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Circle {
            center: cadmpeg_ir::math::Point2 { u: 250.0, v: 0.0 },
            radius: Length(750.0),
        }
    ));
}

#[test]
fn semantic_writer_applies_ellipse_sketch_edits() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_elliptical_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            ..
        } = &mut ir_edit.model.sketch_entities[0].geometry
        else {
            panic!("ellipse sketch entity");
        };
        center.v = 125.0;
        *major_angle = Angle(0.25);
        *major_radius = Length(1500.0);
        *minor_radius = Length(500.0);
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Ellipse {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 125.0 },
            major_angle: Angle(angle),
            major_radius: Length(1500.0),
            minor_radius: Length(500.0),
            bounds: None,
        } if (angle - 0.25).abs() < 1.0e-12
    ));
}

#[test]
fn semantic_writer_applies_bounded_arc_sketch_edits() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_arc_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let arc = ir_edit
            .model
            .sketch_entities
            .iter_mut()
            .find(|entity| matches!(entity.geometry, SketchGeometry::Arc { .. }))
            .expect("arc sketch entity");
        let SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } = &mut arc.geometry
        else {
            unreachable!();
        };
        center.u = 100.0;
        *radius = Length(800.0);
        *start_angle = Angle(0.25);
        *end_angle = Angle(1.25);
        let endpoint_refs = arc.endpoint_refs.clone();
        let endpoints = [
            cadmpeg_ir::math::Point2::new(100.0 + 800.0 * 0.25f64.cos(), 800.0 * 0.25f64.sin()),
            cadmpeg_ir::math::Point2::new(100.0 + 800.0 * 1.25f64.cos(), 800.0 * 1.25f64.sin()),
        ];
        for entity in &mut ir_edit.model.sketch_entities {
            let SketchGeometry::Line { start, end } = &mut entity.geometry else {
                continue;
            };
            for (reference, target) in endpoint_refs.iter().zip(endpoints) {
                if entity.endpoint_refs[0] == *reference {
                    *start = target;
                }
                if entity.endpoint_refs[1] == *reference {
                    *end = target;
                }
            }
        }
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(regenerated
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2 { u: 100.0, v: 0.0 },
                radius: Length(800.0),
                start_angle: Angle(start),
                end_angle: Angle(end),
            } if (start - 0.25).abs() < 1.0e-12 && (end - 1.25).abs() < 1.0e-12
        )));
}

#[test]
fn semantic_writer_applies_rational_and_non_rational_sketch_nurbs_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_nurbs_sketches(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    for entity in &mut decoded.ir_mut().model.sketch_entities {
        let SketchGeometry::Nurbs {
            control_points,
            weights,
            ..
        } = &mut entity.geometry
        else {
            continue;
        };
        control_points[1].v += 250.0;
        if let Some(weights) = weights {
            weights[1] = 0.75;
        }
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let splines = regenerated
        .ir()
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| match &entity.geometry {
            SketchGeometry::Nurbs {
                control_points,
                weights,
                ..
            } => Some((control_points, weights)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(splines.len(), 2);
    assert!(splines
        .iter()
        .all(|(points, _)| (points[1].v - 1250.0).abs() < 1.0e-12));
    assert!(splines
        .iter()
        .any(|(_, weights)| weights.as_deref() == Some(&[1.0, 0.75, 1.0])));
}

#[test]
fn semantic_writer_preserves_opaque_auxiliary_blocks() {
    let payload = b"vendor-private\x00\x01\x02";
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x77, "Contents/CustomData", payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.points[0].position.z += 1.0;

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .source_fidelity()
        .retained_records
        .iter()
        .any(|record| {
            regenerated
                .source_fidelity()
                .annotations
                .provenance
                .get(record.id())
                .is_some_and(|note| note.stream() == "Contents/CustomData")
                && record.data() == Some(payload.as_slice())
        }));
}

#[test]
fn semantic_writer_round_trips_all_supported_lanes_together() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(face_color_definition());
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut source = sldprt_with_body_and_material(&body, "Steel", [32, 64, 128]);
    source.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &display_list_payload(),
    ));
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords Name="Bracket"><Configuration Name="Default" Material="Steel"/><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12.5mm</Dimension></Extrusion></Keywords>"#));
    source.extend(make_block(0x77, "Contents/CustomData", b"opaque-state"));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.points[0].position.z += 2.0;
    decoded.ir_mut().model.tessellations[0].vertices[0].z = 125.0;
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "20mm".into());
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.name.as_deref() == Some("Steel")));
    assert!(regenerated
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(binding.target, AppearanceTarget::Face(_))));
    assert_eq!(regenerated.ir().model.tessellations[0].vertices[0].z, 125.0);
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "20mm"
    );
    assert!(regenerated
        .source_fidelity()
        .retained_records
        .iter()
        .any(|record| {
            regenerated
                .source_fidelity()
                .annotations
                .provenance
                .get(record.id())
                .is_some_and(|note| note.stream() == "Contents/CustomData")
                && record.data() == Some(b"opaque-state".as_slice())
        }));

    let written = regenerated
        .source_fidelity()
        .retained_record("sldprt:file:source-image#0")
        .and_then(|record| record.data())
        .unwrap();
    let scan = container::scan_bytes(written);
    assert_eq!(scan.directory.len(), scan.blocks.len());
    for block in &scan.blocks {
        let section = block.section.as_deref().unwrap();
        if section == "Contents/CustomData" {
            assert_eq!(block.type_id, 0x77);
        }
        assert!(scan.directory.iter().any(|entry| {
            entry.name == section && entry.size == block.uncomp_sz && entry.type_id == block.type_id
        }));
    }
}

#[test]
fn semantic_writer_preserves_display_list_geometry() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_display_list(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.points[0].position.z += 1.0;
    decoded.ir_mut().model.tessellations[0].vertices[0].z = 250.0;

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir().model.tessellations.len(), 1);
    let mesh = &regenerated.ir().model.tessellations[0];
    assert_eq!(mesh.vertices[0].z, 250.0);
    assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
    assert_eq!(mesh.strip_lengths, vec![3]);
    assert_eq!(mesh.channels.len(), 6);
}

#[test]
fn semantic_writer_rejects_tessellation_f32_overflow() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_display_list(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.tessellations[0].vertices[0].x = f64::MAX;
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("tessellation position exceeds f32 range"));
}

#[test]
fn semantic_writer_expands_indexed_tessellation() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};

    let corner_normals = vec![
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(-1.0, 0.0, 0.0),
        Vector3::new(0.0, -1.0, 0.0),
        Vector3::new(0.0, 0.0, -1.0),
    ];
    let mesh = Tessellation {
        id: "synthetic:test:indexed-tessellation".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: vec![Vector3::new(0.0, 0.0, 1.0); 4],
        corner_normals: corner_normals.clone(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: vec![TessellationChannel {
            domain: cadmpeg_ir::tessellation::TessellationChannelDomain::default(),
            item_size: 1,
            kind: 7,
            flags: 2,
            count: 4,
            data: vec![10, 11, 12, 13],
            indices: Vec::new(),
        }],
    };
    let expanded = crate::writer::sequential_tessellation(&mesh).unwrap();
    assert_eq!(expanded.strip_lengths, vec![3, 3]);
    assert_eq!(expanded.triangles, vec![[0, 1, 2], [3, 4, 5]]);
    assert_eq!(expanded.vertices.len(), 6);
    assert_eq!(expanded.normals, corner_normals);
    assert!(expanded.corner_normals.is_empty());
    assert_eq!(expanded.channels[0].count, 6);
    assert_eq!(expanded.channels[0].data, vec![10, 11, 12, 10, 12, 13]);

    let mut attributed = mesh.clone();
    attributed
        .triangle_groups
        .push(cadmpeg_ir::tessellation::TessellationTriangleGroup {
            source_id: Some("synthetic:test:group#0".into()),
            triangles: vec![0, 1],
        });
    assert!(matches!(
        crate::writer::sequential_tessellation(&attributed),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));

    let mut edged = mesh;
    edged.feature_edges.push([0, 1]);
    assert!(matches!(
        crate::writer::sequential_tessellation(&edged),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
fn semantic_writer_rejects_out_of_range_tessellation_indices() {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::tessellation::Tessellation;

    let mesh = Tessellation {
        id: "synthetic:test:invalid-tessellation".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![Point3::new(0.0, 0.0, 0.0); 3],
        triangles: vec![[0, 1, 3]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    };
    let error = crate::writer::sequential_tessellation(&mesh).unwrap_err();
    assert!(error.to_string().contains("index is out of bounds"));
}
