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
fn semantic_writer_replays_unchanged_swobjects_payload() {
    let mut payload = material_payload("Steel", [32, 64, 128]);
    payload.extend([0xde, 0xad, 0xbe, 0xef]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x40, "SWObjects", &payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded
        .ir_mut()
        .source
        .as_mut()
        .unwrap()
        .attributes
        .remove(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE);

    let mut encoded = Vec::new();
    let path = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    assert_eq!(path, cadmpeg_ir::WritePath::Patched);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let retained = regenerated
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.stream() == "SWObjects")
        .unwrap();
    assert_eq!(retained.data(), Some(payload.as_slice()));
}

#[test]
fn semantic_writer_rejects_edits_to_retained_swobjects_semantics() {
    let source = sldprt_with_body_and_material(&triangle_body(), "Steel", [32, 64, 128]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    });

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot edit retained SWObjects semantics"),
        "{error}"
    );
}

#[test]
fn encoder_writes_source_less_ir() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);

    let mut encoded = Vec::new();
    let report = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    // No retained source content reached the writer, so it authored every byte.
    assert_eq!(report.write_path, cadmpeg_ir::WritePath::Synthesized);
    let scan = container::scan_bytes(&encoded);
    assert_eq!(scan.blocks.len(), 1);
    assert_eq!(scan.directory.len(), 1);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 6);
    assert_eq!(decoded.ir().model.edges.len(), 12);
    assert_eq!(decoded.ir().model.vertices.len(), 8);
}

#[test]
fn semantic_writer_emits_face_records_deterministically() {
    use cadmpeg_ir::topology::Color;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    for (index, face) in ir.model.faces.iter_mut().enumerate() {
        face.color = Some(Color {
            r: index as f32 / 10.0,
            g: (index + 1) as f32 / 10.0,
            b: (index + 2) as f32 / 10.0,
            a: 1.0,
        });
    }

    let mut expected = None;
    for _ in 0..4 {
        let mut encoded = Vec::new();
        SldprtCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap();
        if let Some(expected) = &expected {
            assert_eq!(expected, &encoded);
        } else {
            expected = Some(encoded);
        }
    }
}

#[test]
fn encoder_rejects_source_less_unresolved_extrusion_profile() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        LinearTermination, ProfileRef,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#extrude".into()),
        ordinal: 0,
        name: Some("Extrude".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved("native:missing-owner".into()),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(10.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::Join,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });

    let error = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("requires retained extrusion profile data"));
}

#[test]
fn encoder_writes_source_less_line_sketches() {
    use cadmpeg_ir::features::{
        Angle, AngularTermination, BooleanOp, ExtrudeExtent, ExtrudeSide, Feature,
        FeatureDefinition, FeatureId, Length, LinearTermination, PathRef, ProfileRef,
        RevolveExtent,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let sketch_id = SketchId("synthetic:test:sketch#profile".into());
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(0.0, 10.0),
    ];
    let entity_ids = (0..3)
        .map(|index| SketchEntityId(format!("synthetic:test:sketch-entity#line-{index}")))
        .collect::<Vec<_>>();
    for index in 0..3 {
        ir.model.sketch_entities.push(SketchEntity::new(
            entity_ids[index].clone(),
            sketch_id.clone(),
            SketchGeometry::Line {
                start: points[index],
                end: points[(index + 1) % 3],
            },
        ));
    }
    for index in 0..3 {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId(format!("synthetic:test:constraint#coincident-{index}")),
            sketch: sketch_id.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entity_ids[index].clone()),
                    SketchLocus::Start(entity_ids[(index + 1) % 3].clone()),
                ],
            },
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
    }
    for (suffix, definition) in [
        (
            "fixed",
            SketchConstraintDefinition::Fixed {
                entity: entity_ids[1].clone(),
            },
        ),
        (
            "horizontal",
            SketchConstraintDefinition::Horizontal {
                entity: entity_ids[0].clone(),
            },
        ),
        (
            "vertical",
            SketchConstraintDefinition::Vertical {
                entity: entity_ids[2].clone(),
            },
        ),
    ] {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId(format!("synthetic:test:constraint#{suffix}")),
            sketch: sketch_id.clone(),
            definition,
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
    }
    ir.model.sketch_entities.push(SketchEntity::new(
        SketchEntityId("synthetic:test:sketch-entity#point".into()),
        sketch_id.clone(),
        SketchGeometry::Point {
            position: Point2::new(4.0, 5.0),
        },
    ));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![entity_ids
            .iter()
            .cloned()
            .map(|entity| SketchEntityUse {
                entity,
                reversed: false,
            })
            .collect()],
        native_ref: None,
    });
    let sketch_feature_id = FeatureId("synthetic:test:feature#profile".into());
    ir.model.features.push(Feature {
        id: sketch_feature_id.clone(),
        ordinal: 0,
        name: Some("Profile".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: None,
    });
    let profile = ProfileRef::Sketch(sketch_id.clone());
    let path = PathRef::Sketch(sketch_id.clone());
    let generated = [
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: Some(profile.clone()),
                axis: Some(cadmpeg_ir::features::RevolutionAxis {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    direction: Vector3::new(0.0, 1.0, 0.0),
                }),
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: Angle(1.2) },
                }),
                axis_reference: None,
                solid: Some(true),
                face_maker_class: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(profile.clone()),
            sections: Vec::new(),
            path: Some(path.clone()),
            mode: cadmpeg_ir::features::SweepMode::Solid {
                op: cadmpeg_ir::features::BooleanKind::Join,
            },
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: Some(Angle(0.3)),
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: Some(1.5),
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Loft {
            sections: vec![
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
            ],
            guides: vec![path],
            centerline: None,
            op: BooleanOp::NewBody,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(profile),
                direction: Some(Vector3::new(0.0, 0.0, 1.0)),
                thickness: Some(Length(2.5)),
                side: Some(cadmpeg_ir::features::RibSide::Centered),
                draft: cadmpeg_ir::features::RibDraft::Angle(Angle(0.1)),
            },
            op: BooleanOp::Join,
        },
    ];
    for (index, definition) in generated.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#profile-op-{index}")),
            ordinal: index as u64 + 2,
            name: Some(format!("Profile op {index}")),
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    let extrude_feature_id = FeatureId("synthetic:test:feature#extrude".into());
    ir.model.features.push(Feature {
        id: extrude_feature_id.clone(),
        ordinal: 1,
        name: Some("Boss".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch_id),
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3::new(
                0.0, 0.0, 1.0,
            )),
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(12.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::Join,
            direction_source: None,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });
    ir.model
        .set_feature_regeneration_parent(extrude_feature_id, sketch_feature_id)
        .unwrap();

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan.blocks.iter().any(|block| {
        block
            .section
            .as_deref()
            .is_some_and(|section| section == "Contents/Config-0-ResolvedFeatures")
    }));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let marker_lane = &sldprt_native(decoded.ir()).feature_input_lanes[0];
    assert_eq!(
        marker_lane
            .sketch_entities
            .iter()
            .filter(|marker| marker.coordinates_m.is_some())
            .count(),
        7
    );
    let marker_relations = marker_lane
        .sketch_entities
        .iter()
        .filter(|marker| matches!(marker.kind, crate::records::SketchInputKind::Relation(_)))
        .collect::<Vec<_>>();
    assert_eq!(marker_relations.len(), 3);
    assert!(marker_relations
        .iter()
        .all(|marker| marker.links.len() == 2 && marker.link_selector == Some(0)));
    assert!(marker_relations
        .iter()
        .all(|marker| marker.links.iter().all(|link| marker_lane
            .sketch_entities
            .iter()
            .any(|candidate| candidate.id == link.entity_ref
                && candidate.local_id == Some(u32::from(link.local_id))))));
    assert_eq!(decoded.ir().model.sketches.len(), 1);
    assert_eq!(decoded.ir().model.sketches[0].profiles.len(), 1);
    assert_eq!(decoded.ir().model.sketches[0].profiles[0].len(), 3);
    assert_eq!(decoded.ir().model.sketch_entities.len(), 4);
    assert_eq!(
        decoded.ir().model.sketch_constraints.len(),
        6,
        "{:?}",
        decoded.ir().model.sketch_constraints
    );
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Horizontal { .. }
            )
        }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Vertical { .. }
            )
        }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Fixed { .. }
            )
        }));
    assert_eq!(
        decoded
            .ir()
            .model
            .sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
            .count(),
        3
    );
    assert!(decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Point { position }
                if (position.u - 4.0).abs() < 1.0e-12
                    && (position.v - 5.0).abs() < 1.0e-12
        )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.definition,
        FeatureDefinition::Sketch { sketch: Some(_) }
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(12.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    )));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Revolve { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Sweep { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Loft { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Rib { .. })));
    {
        let mut ir_edit = decoded.ir_mut();
        let point = ir_edit
            .model
            .sketch_entities
            .iter_mut()
            .find_map(|entity| match &mut entity.geometry {
                SketchGeometry::Point { position } => Some(position),
                _ => None,
            })
            .unwrap();
        point.u = 7.0;
        point.v = 8.0;
    }
    let mut rewritten = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut rewritten,
    )
    .unwrap();
    let rewritten = SldprtCodec
        .decode(&mut Cursor::new(rewritten), &DecodeOptions::default())
        .unwrap();
    assert!(rewritten
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Point { position }
                if (position.u - 7.0).abs() < 1.0e-12
                    && (position.v - 8.0).abs() < 1.0e-12
        )));
}

#[test]
fn encoder_writes_source_less_spatial_point_and_line_sketches() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
        SpatialSketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let sketch_id = SpatialSketchId("synthetic:test:spatial-sketch#path".into());
    let entity_id = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#line".into());
    let start = Point3::new(1.25, -2.5, 3.75);
    let end = Point3::new(4.5, 5.25, -6.0);
    let second_start = Point3::new(-7.0, 8.5, 9.25);
    let second_end = Point3::new(10.0, -11.5, 12.75);
    let point = Point3::new(-13.0, 14.25, 15.5);
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
            SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#a-point".into()),
            sketch_id.clone(),
            SpatialSketchGeometry::Point { position: point },
        ));
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            entity_id,
            sketch_id.clone(),
            SpatialSketchGeometry::Line { start, end },
        ));
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#second-line".into()),
            sketch_id.clone(),
            SpatialSketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ));
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#spatial-path".into()),
        ordinal: 0,
        name: Some("Spatial path".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id),
        },
        native_ref: None,
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let mut regenerated = cadmpeg_test_support::EditableDecodeResult::from(regenerated);

    assert_eq!(regenerated.ir().model.spatial_sketches.len(), 1);
    assert_eq!(regenerated.ir().model.spatial_sketch_entities.len(), 3);
    assert!(matches!(
        regenerated.ir().model.spatial_sketch_entities[0].geometry,
        SpatialSketchGeometry::Point { position }
            if (position.x - point.x).abs() < 1.0e-12
                && (position.y - point.y).abs() < 1.0e-12
                && (position.z - point.z).abs() < 1.0e-12
    ));
    assert!(matches!(
        regenerated.ir().model.spatial_sketch_entities[1].geometry,
        SpatialSketchGeometry::Line {
            start: regenerated_start,
            end: regenerated_end,
        } if regenerated_start == start && regenerated_end == end
    ));
    assert!(matches!(
        regenerated.ir().model.spatial_sketch_entities[2].geometry,
        SpatialSketchGeometry::Line {
            start: regenerated_start,
            end: regenerated_end,
        } if regenerated_start == second_start && regenerated_end == second_end
    ));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::SpatialSketch { sketch: Some(_) }
    ));

    let edited_start = Point3::new(13.0, 14.0, 15.0);
    let edited_end = Point3::new(-16.0, 17.0, 18.0);
    let edited_point = Point3::new(19.0, -20.0, 21.5);
    regenerated.ir_mut().model.spatial_sketch_entities[0].geometry = SpatialSketchGeometry::Point {
        position: edited_point,
    };
    regenerated.ir_mut().model.spatial_sketch_entities[2].geometry = SpatialSketchGeometry::Line {
        start: edited_start,
        end: edited_end,
    };
    let mut rewritten = Vec::new();
    crate::test_support::plan_inherited_write(
        regenerated.ir(),
        regenerated.source_fidelity(),
        &mut rewritten,
    )
    .unwrap();
    let rewritten = SldprtCodec
        .decode(&mut Cursor::new(rewritten), &DecodeOptions::default())
        .unwrap();
    assert_eq!(rewritten.ir().model.spatial_sketch_entities.len(), 3);
    assert!(matches!(
        rewritten.ir().model.spatial_sketch_entities[0].geometry,
        SpatialSketchGeometry::Point { position }
            if (position.x - edited_point.x).abs() < 1.0e-12
                && (position.y - edited_point.y).abs() < 1.0e-12
                && (position.z - edited_point.z).abs() < 1.0e-12
    ));
    assert!(matches!(
        rewritten.ir().model.spatial_sketch_entities[2].geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == edited_start && end == edited_end
    ));
}

#[test]
fn encoder_rejects_unrepresentable_source_less_sketch_constraints() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#profile".into());
    let entity_id = SketchEntityId("synthetic:test:sketch-entity#line".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:constraint#horizontal".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Horizontal { entity: entity_id },
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

    let error = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error
        .to_string()
        .contains("requires an owning sketch feature"));
}
