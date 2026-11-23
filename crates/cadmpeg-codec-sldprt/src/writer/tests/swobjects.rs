// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::{edit, EditableDecodeResult};

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::appearance::material_payload;
use crate::test_support::appearance::sldprt_with_body_and_material;
use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::writer::{Held, Unwritten, UnwrittenAttributes};
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
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded
        .ir_mut()
        .source
        .as_mut()
        .unwrap()
        .attributes
        .remove(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE);

    let mut encoded = Vec::new();
    let path = crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    assert!(matches!(
        path,
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    let regenerated = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap(),
    );
    let retained = regenerated
        .source_fidelity()
        .retained_records()
        .values()
        .find(|record| record.stream() == "SWObjects")
        .unwrap();
    assert_eq!(retained.data(), Some(payload.as_slice()));
}

#[test]
fn semantic_writer_keeps_retained_materials_and_drops_their_edit() {
    let source = sldprt_with_body_and_material(&triangle_body(), "Steel", [32, 64, 128]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.appearances[0].base_color =
        Some(cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("valid color"));
    let appearance = decoded.ir().model.appearances[0].id.as_str().to_owned();

    let (written, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert!(
        losses.dropped.iter().any(|message| {
            message.contains("keeps the retained SWObjects materials")
                && message.contains(&appearance)
        }),
        "{losses:?}"
    );
    let members = |bytes: &[u8]| {
        crate::test_support::container::scan(bytes)
            .blocks
            .into_iter()
            .find(|block| block.section.name() == Some("SWObjects"))
            .unwrap()
            .payload
    };
    assert_eq!(members(&written), members(&source));
}

/// A material record the write skips is named when it keeps the retained
/// `SWObjects` materials; a record the retained materials hold is not.
#[test]
fn a_material_skipped_beside_retained_swobjects_is_named() {
    let source = sldprt_with_body_and_material(&triangle_body(), "Steel", [32, 64, 128]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let steel = decoded.ir().model.appearances[0].id.as_str().to_owned();
    let plain = "synthetic:test:appearance#plain";
    decoded
        .ir_mut()
        .model
        .appearances
        .push(cadmpeg_ir::appearance::Appearance {
            id: cadmpeg_ir::ids::AppearanceId::mint(plain).unwrap(),
            name: Some("Plain".into()),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("moVisualProperties_c".into()),
            category: None,
            base_color: None,
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
            native_ref: None,
        });

    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    let omitted = report
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::SldprtLossCode::WriterMaterialsOmitted.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        omitted,
        [format!(
            "1 appearance or body record(s) state no base colour, so the SLDPRT write writes no \
             material for them, and the retained SWObjects materials it keeps hold none: {plain}"
        )
        .as_str()],
        "{:?}",
        report.losses
    );
    let members = |bytes: &[u8]| {
        crate::test_support::container::scan(bytes)
            .blocks
            .into_iter()
            .find(|block| block.section.name() == Some("SWObjects"))
            .unwrap()
            .payload
    };
    assert_eq!(members(&written), members(&source));

    // Clearing the colour of the retained material is an edit the retained
    // records drop, named once as dropped and not as omitted.
    decoded.ir_mut().model.appearances[0].base_color = None;
    let report = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap()
        .write_to(&mut Vec::new())
        .unwrap();
    let names = |loss: &&cadmpeg_ir::report::loss::LossNote| loss.message.contains(&steel);
    assert!(
        report
            .losses
            .iter()
            .filter(names)
            .all(|loss| loss.code.local_code() == "edit.dropped"),
        "{:?}",
        report.losses
    );
    assert!(report.losses.iter().any(|loss| names(&loss)));
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_ir() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].name = None;
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));

    let mut encoded = Vec::new();
    let report = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    // Geometry is authored in the explicit retained envelope.
    assert!(matches!(
        report.write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    let scan = crate::test_support::container::scan(&encoded);
    // The fixture's source image holds one token member, which the write
    // carries as stored.
    assert_eq!(scan.blocks.len(), 3);
    assert_eq!(scan.directory.len(), 3);
    let names: std::collections::BTreeSet<_> = scan
        .blocks
        .iter()
        .map(|block| block.section.name().unwrap())
        .collect();
    assert_eq!(
        names,
        std::collections::BTreeSet::from([
            "Contents/Config-0-Partition",
            "Contents/Keywords-0",
            "FixtureEnvelopeTokenSource",
        ])
    );
    let token = scan
        .blocks
        .iter()
        .find(|block| block.section.name() == Some("FixtureEnvelopeTokenSource"))
        .unwrap();
    assert_eq!(token.payload, b"authored envelope fixture");
    let keywords = scan
        .blocks
        .iter()
        .find(|block| block.section.name() == Some("Contents/Keywords-0"))
        .unwrap();
    // A member with no source member is written after the lead byte and
    // declaration most stored members carry.
    assert_eq!(
        keywords.payload,
        b"\x86<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<Keywords></Keywords>"
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().geometry.faces.len(), 6);
    assert_eq!(decoded.ir().geometry.edges.len(), 12);
    assert_eq!(decoded.ir().geometry.vertices.len(), 8);
}

#[test]
fn semantic_writer_emits_face_records_deterministically() {
    use cadmpeg_ir::topology::Color;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].name = None;
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    for (index, face) in ir.geometry.faces.iter_mut().enumerate() {
        face.color = Some(
            Color::new(
                f64::from(u16::try_from(index).expect("index fits u16")) / 10.0,
                f64::from(u16::try_from(index + 1).expect("index fits u16")) / 10.0,
                f64::from(u16::try_from(index + 2).expect("index fits u16")) / 10.0,
                1.0,
            )
            .expect("valid color"),
        );
    }

    let mut expected = None;
    for _ in 0..4 {
        let mut encoded = Vec::new();
        crate::test_support::plan_fixture_with_native_envelope(
            EncodeInput::new(&ir, None),
            TargetRequest::Inherit,
        )
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
fn encoder_drops_a_source_less_extrusion_with_an_unresolved_profile() {
    use cadmpeg_ir::features::extents::{ExtrudeExtent, ExtrudeSide, LinearTermination};
    use cadmpeg_ir::features::operation_settings::BooleanOp;
    use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:feature#extrude").expect("identity grammar"),
        ordinal: 0,
        name: Some("Extrude".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(
                    "native:missing-owner".into(),
                )),
                direction: cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {},
                start: cadmpeg_ir::features::extents::ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: cadmpeg_ir::scalar::NonZeroLength::new(10.0).unwrap(),
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::Join,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: None,
    });

    let report = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .report()
        .clone();
    assert!(report.losses.iter().any(|loss| {
        loss.code.local_code() == "writer.features-omitted"
            && loss
                .message
                .contains("requires retained extrusion profile data")
            && loss.message.contains("synthetic:test:feature#extrude")
    }));
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_line_sketches() {
    use cadmpeg_ir::features::extents::{
        AngularTermination, ExtrudeExtent, ExtrudeSide, LinearTermination,
    };
    use cadmpeg_ir::features::operation_settings::BooleanOp;
    use cadmpeg_ir::features::profiles::{PathRef, PlanarProfileRef, ProfileRef};
    use cadmpeg_ir::features::revolve::RevolveExtent;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::scalar::Angle;
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
        SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
        SketchId, SketchLocus,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].name = None;
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    let sketch_id = SketchId::mint("synthetic:test:sketch#profile").unwrap();
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(0.0, 10.0),
    ];
    let entity_ids = (0..3)
        .map(|index| {
            SketchEntityId::mint(format!("synthetic:test:sketch-entity#line-{index}")).unwrap()
        })
        .collect::<Vec<_>>();
    for index in 0..3 {
        ir.model.sketch_entities.push(SketchEntity::new(
            entity_ids[index].clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: points[index],
                end: points[(index + 1) % 3],
            })
            .unwrap(),
        ));
    }
    for index in 0..3 {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId::mint(format!("synthetic:test:constraint#coincident-{index}"))
                .unwrap(),
            sketch: sketch_id.clone(),
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::CoincidentLoci {
                    loci: vec![
                        SketchLocus::End(entity_ids[index].clone()),
                        SketchLocus::Start(entity_ids[(index + 1) % 3].clone()),
                    ],
                },
            )
            .unwrap(),
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
            SketchConstraintDefinitionInput::Fixed {
                entity: entity_ids[1].clone(),
            },
        ),
        (
            "horizontal",
            SketchConstraintDefinitionInput::Horizontal {
                entity: entity_ids[0].clone(),
            },
        ),
        (
            "vertical",
            SketchConstraintDefinitionInput::Vertical {
                entity: entity_ids[2].clone(),
            },
        ),
    ] {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId::mint(format!("synthetic:test:constraint#{suffix}")).unwrap(),
            sketch: sketch_id.clone(),
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                .unwrap(),
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
        SketchEntityId::mint("synthetic:test:sketch-entity#point").unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(4.0, 5.0),
        })
        .unwrap(),
    ));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![entity_ids
            .iter()
            .cloned()
            .map(|entity| SketchEntityUse {
                entity,
                reversed: false,
            })
            .collect()])
        .unwrap(),
        native_ref: None,
    });
    let sketch_feature_id =
        FeatureId::mint("synthetic:test:feature#profile").expect("identity grammar");
    ir.model.features.push(Feature {
        product_definition: None,
        id: sketch_feature_id.clone(),
        ordinal: 0,
        name: Some("Profile".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id.clone())),
            }),
        ),
        native_ref: None,
    });
    let profile = cadmpeg_ir::features::profiles::PlanarProfileRef::Sketch(sketch_id.clone());
    let path = PathRef::Sketch(sketch_id.clone());
    let generated = [
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction: cadmpeg_ir::features::revolve::RevolveConstruction::Resolved {
                profile: profile.clone(),
                axis: cadmpeg_ir::features::revolve::RevolutionAxis {
                    origin: cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
                        0.0, 0.0, 0.0,
                    ))
                    .unwrap(),
                    direction: cadmpeg_ir::features::geometry::FeatureDirection3::new(
                        Vector3::new(0.0, 1.0, 0.0),
                    )
                    .unwrap(),
                    reference: None,
                },
                extent: RevolveExtent::OneSided {
                    termination: AngularTermination::Angle {
                        angle: cadmpeg_ir::scalar::PositiveAngle::new(1.2).unwrap(),
                    },
                },
                solid: Some(true),
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: BooleanOp::NewBody,
        }),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::sweeps::SweepShape::sheet_sections(
                cadmpeg_ir::features::sweeps::SweepMode::Solid {
                    op: cadmpeg_ir::features::sweeps::SolidSweepOperation::Join,
                },
                cadmpeg_ir::features::sweeps::SweepSection::Profile(profile.clone()),
                Vec::new(),
            ),
            path: Some(path.clone()),
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: Some(Angle::new(0.3).unwrap()),
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: Some(cadmpeg_ir::scalar::PositiveReal::new(1.5).unwrap()),
            allow_multi_profile_faces: None,
        }),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections: vec![
                cadmpeg_ir::features::profiles::LoftSection::Profile(ProfileRef::Planar(
                    profile.clone(),
                )),
                cadmpeg_ir::features::profiles::LoftSection::Profile(ProfileRef::Planar(
                    profile.clone(),
                )),
            ],
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(vec![path]),
            op: BooleanOp::NewBody,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }),
        FeatureDefinition::Operation(FeatureOperation::Rib {
            construction: cadmpeg_ir::features::revolve::RibConstruction {
                profile: Some(profile),
                direction: Some(
                    cadmpeg_ir::features::geometry::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                ),
                thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.5).unwrap()),
                side: Some(cadmpeg_ir::features::revolve::RibSide::Centered),
                draft: cadmpeg_ir::features::revolve::RibDraft::Angle(
                    cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap(),
                ),
            },
            op: BooleanOp::Join,
        }),
    ];
    for (index, definition) in generated.into_iter().enumerate() {
        ir.model.features.push(Feature {
            product_definition: None,
            id: FeatureId::mint(format!("synthetic:test:feature#profile-op-{index}"))
                .expect("identity grammar"),
            ordinal: cadmpeg_core::decode::u64_from_index(index) + 2,
            name: Some(format!("Profile op {index}")),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
                definition,
            ),
            native_ref: None,
        });
    }
    let extrude_feature_id =
        FeatureId::mint("synthetic:test:feature#extrude").expect("identity grammar");
    ir.model.features.push(Feature {
        product_definition: None,
        id: extrude_feature_id.clone(),
        ordinal: 1,
        name: Some("Boss".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)),
                direction: cadmpeg_ir::features::extents::ExtrudeDirection::Explicit {
                    vector: cadmpeg_ir::features::geometry::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                    source: None,
                },
                start: cadmpeg_ir::features::extents::ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::Join,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: None,
    });
    ir.model
        .set_feature_regeneration_parent(&(extrude_feature_id), &(sketch_feature_id))
        .unwrap();

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let scan = crate::test_support::container::scan(&encoded);
    assert!(scan.blocks.iter().any(|block| {
        block
            .section
            .name()
            .is_some_and(|section| section == "Contents/Config-0-ResolvedFeatures")
    }));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
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
        .filter(|marker| matches!(marker.kind(), crate::records::SketchInputKind::Relation(_)))
        .collect::<Vec<_>>();
    assert_eq!(marker_relations.len(), 3);
    assert!(marker_relations
        .iter()
        .all(|marker| marker.links().len() == 2
            && marker
                .links
                .as_ref()
                .map(crate::records::SketchInputLinks::selector)
                == Some(0)));
    assert!(marker_relations
        .iter()
        .all(|marker| marker.links().iter().all(|link| marker_lane
            .sketch_entities
            .iter()
            .any(|candidate| candidate.id() == link.entity_ref
                && candidate.local_id() == Some(u32::from(link.local_id))))));
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
                constraint.definition.kind(),
                SketchConstraintDefinitionInput::Horizontal { .. }
            )
        }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition.kind(),
                SketchConstraintDefinitionInput::Vertical { .. }
            )
        }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition.kind(),
                SketchConstraintDefinitionInput::Fixed { .. }
            )
        }));
    assert_eq!(
        decoded
            .ir()
            .model
            .sketch_entities
            .iter()
            .filter(|entity| matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            ))
            .count(),
        3
    );
    assert!(decoded.ir().model.sketch_entities.iter().any(
        |entity| matches!(*entity.geometry.definition(),
            SketchGeometryDefinition::Point { position }
                if (position.u - 4.0).abs() < 1.0e-12
                    && (position.v - 5.0).abs() < 1.0e-12
        )
    ));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(_))
        })
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Sketch(_)),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }) if actual_length.get() == 12.0
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve { .. })
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Loft { .. })
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Rib { .. })
    )));
    {
        let mut ir_edit = decoded.ir_mut();
        let point = ir_edit
            .model
            .sketch_entities
            .iter_mut()
            .find(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
            })
            .unwrap();
        edit::replace(&mut point.geometry, |previous| {
            let mut definition = previous.definition().to_raw();
            {
                let definition: &mut cadmpeg_ir::sketches::SketchGeometryDefinition =
                    &mut definition;

                let SketchGeometryDefinition::Point { position } = definition else {
                    panic!("point geometry")
                };
                position.u = 7.0;
                position.v = 8.0;
            };
            definition.try_into()
        })
        .unwrap();
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
    assert!(rewritten.ir().model.sketch_entities.iter().any(
        |entity| matches!(*entity.geometry.definition(),
            SketchGeometryDefinition::Point { position }
                if (position.u - 7.0).abs() < 1.0e-12
                    && (position.v - 8.0).abs() < 1.0e-12
        )
    ));
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_spatial_point_and_line_sketches() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
        SpatialSketchGeometryDefinition, SpatialSketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].name = None;
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    let sketch_id = SpatialSketchId::mint("synthetic:test:spatial-sketch#path").unwrap();
    let entity_id =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#line").unwrap();
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
            SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#a-point").unwrap(),
            sketch_id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: point,
            })
            .unwrap(),
        ));
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            entity_id,
            sketch_id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .unwrap(),
        ));
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#second-line")
                .unwrap(),
            sketch_id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: second_start,
                end: second_end,
            })
            .unwrap(),
        ));
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:feature#spatial-path").expect("identity grammar"),
        ordinal: 0,
        name: Some("Spatial path".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                sketch: Some(sketch_id),
            }),
        ),
        native_ref: None,
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let mut regenerated = EditableDecodeResult::from(regenerated);

    assert_eq!(regenerated.ir().model.spatial_sketches.len(), 1);
    assert_eq!(regenerated.ir().model.spatial_sketch_entities.len(), 3);
    assert!(
        matches!(*regenerated.ir().model.spatial_sketch_entities[0].geometry.definition(),
            SpatialSketchGeometryDefinition::Point { position }
                if (position.x - point.x).abs() < 1.0e-12
                    && (position.y - point.y).abs() < 1.0e-12
                    && (position.z - point.z).abs() < 1.0e-12
        )
    );
    assert!(
        matches!(*regenerated.ir().model.spatial_sketch_entities[1].geometry.definition(),
            SpatialSketchGeometryDefinition::Line {
                start: regenerated_start,
                end: regenerated_end,
            } if regenerated_start == start && regenerated_end == end
        )
    );
    assert!(
        matches!(*regenerated.ir().model.spatial_sketch_entities[2].geometry.definition(),
            SpatialSketchGeometryDefinition::Line {
                start: regenerated_start,
                end: regenerated_end,
            } if regenerated_start == second_start && regenerated_end == second_end
        )
    );
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: Some(_) })
    ));

    let edited_start = Point3::new(13.0, 14.0, 15.0);
    let edited_end = Point3::new(-16.0, 17.0, 18.0);
    let edited_point = Point3::new(19.0, -20.0, 21.5);
    regenerated.ir_mut().model.spatial_sketch_entities[0].geometry =
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
            position: edited_point,
        })
        .unwrap();
    regenerated.ir_mut().model.spatial_sketch_entities[2].geometry =
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
            start: edited_start,
            end: edited_end,
        })
        .unwrap();
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
    assert!(
        matches!(*rewritten.ir().model.spatial_sketch_entities[0].geometry.definition(),
            SpatialSketchGeometryDefinition::Point { position }
                if (position.x - edited_point.x).abs() < 1.0e-12
                    && (position.y - edited_point.y).abs() < 1.0e-12
                    && (position.z - edited_point.z).abs() < 1.0e-12
        )
    );
    assert!(
        matches!(*rewritten.ir().model.spatial_sketch_entities[2].geometry.definition(),
            SpatialSketchGeometryDefinition::Line { start, end }
                if start == edited_start && end == edited_end
        )
    );
}

#[test]
fn encoder_drops_a_source_less_sketch_constraint_it_cannot_write() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
        SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
        SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let sketch_id = SketchId::mint("synthetic:test:sketch#profile").unwrap();
    let entity_id = SketchEntityId::mint("synthetic:test:sketch-entity#line").unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:constraint#horizontal").unwrap(),
        sketch: sketch_id,
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Horizontal { entity: entity_id },
        )
        .unwrap(),
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

    let report = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .report()
        .clone();
    assert!(report.losses.iter().any(|loss| {
        loss.code.local_code() == "writer.sketch-constraints-omitted"
            && loss.message.contains("requires an owning sketch feature")
            && loss
                .message
                .contains("synthetic:test:constraint#horizontal")
    }));
}

/// A finite millimetre length whose product with the length scale is not
/// finite has no source-unit value, so its record cannot hold it.
#[test]
fn the_metadata_projection_cannot_hold_a_length_that_overflows_in_source_units() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let attribute = SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:default_reference_plane#0")
            .unwrap(),
        target: AttributeTarget::Document.into(),
        name: "default_reference_plane".into(),
        values: vec![
            Some(AttributeValue::vector([1.0e10, 2.0, 3.0]).unwrap()),
            Some(AttributeValue::vector([0.0, 0.0, 1.0, 1.0, 0.0, 0.0]).unwrap()),
        ],
        native_ref: None,
    };
    assert!(crate::writer::MetadataRecord::project(&attribute, 0.001, |_| Ok(())).is_ok());
    assert!(matches!(
        crate::writer::MetadataRecord::project(&attribute, 1.0e300, |_| Ok(())),
        Err(Unwritten::Unrepresentable)
    ));
}

/// The retained `SWObjects` section `payload` after the patch writer states
/// the document attribute `name` with `values` in the record at byte zero,
/// and what it did not write.
fn patched_retained_metadata(
    name: &str,
    values: Vec<cadmpeg_ir::attributes::AttributeValue>,
    payload: Vec<u8>,
) -> Result<(Vec<u8>, UnwrittenAttributes), cadmpeg_core::CodecError> {
    use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};

    let id = format!("sldprt:metadata:{name}#0:0");
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint(&id).unwrap(),
        target: AttributeTarget::Document.into(),
        name: name.into(),
        values: values.into_iter().map(Some).collect(),
        native_ref: None,
    });
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    crate::annotations::note(
        &mut annotations,
        id,
        &cadmpeg_ir::stream_name!("SWObjects"),
        0,
        "metadata",
        cadmpeg_ir::Exactness::ByteExact,
    )?;
    let mut sections = vec![("SWObjects".to_owned(), payload)];
    let owners = crate::writer::metadata_owners::MetadataOwners {
        patchable: sections.clone(),
        ..Default::default()
    };
    let mut unwritten = UnwrittenAttributes::default();
    crate::writer::patch_retained_swobjects_metadata(
        &ir,
        &annotations,
        &owners,
        &mut sections,
        1.0,
        &mut unwritten,
    );
    Ok((sections.remove(0).1, unwritten))
}

/// The patch writer leaves `payload` as it was for `values` of `name`, and
/// records `expected`.
fn assert_unwritten(
    name: &str,
    values: Vec<cadmpeg_ir::attributes::AttributeValue>,
    payload: &[u8],
    expected: Unwritten,
) {
    let (patched, unwritten) = patched_retained_metadata(name, values, payload.to_vec()).unwrap();
    assert_eq!(patched, payload);
    assert_eq!(
        unwritten.reason(&format!("sldprt:metadata:{name}#0:0")),
        Some(expected)
    );
}

/// A unit name too long for its length byte, in a record whose token does
/// not match, is kept as stored, judged by its token first.
#[test]
fn a_retained_unit_name_is_judged_by_its_token_before_its_length() {
    assert_unwritten(
        "source_linear_unit_name",
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "u".repeat(200),
        )],
        b"moLengthUserUnitsXc\xff\xfe\xff\x02I\x00",
        Unwritten::RetainedMismatch,
    );
}

/// An empty unit name, in a record whose marker does not match, is kept as
/// stored, judged by its marker first.
#[test]
fn a_retained_unit_name_is_judged_by_its_marker_before_its_emptiness() {
    assert_unwritten(
        "source_linear_unit_name",
        vec![cadmpeg_ir::attributes::AttributeValue::String(String::new())],
        b"moLengthUserUnits_c\x00\x00\x00\x02I\x00",
        Unwritten::RetainedMismatch,
    );
}

/// A unit name too long for its length byte, in a record truncated before
/// that byte, is kept as stored, judged by its truncation first.
#[test]
fn a_retained_unit_name_is_judged_by_its_truncation_before_its_length() {
    assert_unwritten(
        "source_linear_unit_name",
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "u".repeat(200),
        )],
        b"moLengthUserUnits_c\xff\xfe\xff",
        Unwritten::RetainedMismatch,
    );
}

/// A part identity outside `u32`, in a record whose token does not match,
/// is kept as stored, judged by its token first.
#[test]
fn a_retained_part_record_is_judged_by_its_token_before_its_identity() {
    assert_unwritten(
        "part_record",
        vec![
            cadmpeg_ir::attributes::AttributeValue::Integer(-1),
            cadmpeg_ir::attributes::AttributeValue::Integer(1),
        ],
        &[b"moPartXc".as_slice(), &[0; 16]].concat(),
        Unwritten::RetainedMismatch,
    );
}

/// No serializer owns the manager fields: a retained edit keeps the stored
/// record and generation writes none.
#[test]
fn configuration_manager_metadata_is_neither_patched_nor_generated() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let values = vec![
        AttributeValue::Integer(1),
        AttributeValue::Integer(3),
        AttributeValue::Integer(0),
    ];
    assert_unwritten(
        "configuration_manager",
        values.clone(),
        &[b"moConfigurationMgr_c".as_slice(), &[0; 125]].concat(),
        Unwritten::NoSerializer,
    );
    let attribute = SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:configuration_manager#0:0")
            .unwrap(),
        target: AttributeTarget::Document.into(),
        name: "configuration_manager".into(),
        values: values.into_iter().map(Some).collect(),
        native_ref: None,
    };
    assert!(matches!(
        crate::writer::MetadataRecord::project(&attribute, 0.001, |_| {
            panic!("unsupported scalar roles cannot reach a retained-byte patch")
        }),
        Err(Unwritten::NoSerializer)
    ));
}

#[test]
fn retained_part_edits_require_root_ownership_and_preserve_scalar_form() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    for (payload, offset, values) in [
        (
            b"unqlite\xa5class\xa8moPart_c\0\0\0\0\0\0\0\0\0\0\0\0".to_vec(),
            14,
            vec![AttributeValue::Integer(17), AttributeValue::Integer(19)],
        ),
        (
            [
                b"\xff\xff\x01\x00\x08\x00moPart_c".as_slice(),
                &7u32.to_le_bytes(),
                b"\xff\xff\x01\x00\x0a\x00moHeader_c",
            ]
            .concat(),
            6,
            vec![AttributeValue::Integer(17), AttributeValue::Integer(19)],
        ),
    ] {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let id = "sldprt:metadata:part_record#0:6";
        ir.model.attributes.push(SourceAttribute {
            id: cadmpeg_ir::ids::AttributeId::mint(id).unwrap(),
            target: AttributeTarget::Document.into(),
            name: "part_record".into(),
            values: values.into_iter().map(Some).collect(),
            native_ref: None,
        });
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        crate::annotations::note(
            &mut annotations,
            id,
            &cadmpeg_ir::stream_name!("SWObjects"),
            offset,
            "moPart_c",
            cadmpeg_ir::Exactness::ByteExact,
        )
        .unwrap();
        let mut sections = vec![("SWObjects".to_owned(), payload.clone())];
        let owners = crate::writer::metadata_owners::MetadataOwners {
            patchable: sections.clone(),
            ..Default::default()
        };
        let mut unwritten = UnwrittenAttributes::default();
        crate::writer::patch_retained_swobjects_metadata(
            &ir,
            &annotations,
            &owners,
            &mut sections,
            1.0,
            &mut unwritten,
        );
        assert_eq!(unwritten.reason(id), Some(Unwritten::NoSerializer));
        assert_eq!(sections[0].1, payload);
    }
}

/// Without a retained root serializer no Part record is generated; beside a
/// retained section the retained record stands for it.
#[test]
fn generated_part_metadata_needs_a_retained_root_serializer() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let id = "sldprt:metadata:part_record#0:6";
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint(id).unwrap(),
        target: AttributeTarget::Document.into(),
        name: "part_record".into(),
        values: vec![Some(AttributeValue::Integer(17))],
        native_ref: None,
    });
    let mut unwritten = UnwrittenAttributes::default();
    let (objects, units) =
        crate::writer::metadata_payloads(&ir, 1.0, false, &mut unwritten, &mut Vec::new());
    assert!(objects.is_empty());
    assert_eq!(units, None);
    assert_eq!(unwritten.reason(id), Some(Unwritten::NoSerializer));
    let mut retained = UnwrittenAttributes::default();
    assert!(
        crate::writer::metadata_payloads(&ir, 1.0, true, &mut retained, &mut Vec::new())
            .0
            .is_empty()
    );
    assert_eq!(retained.reason(id), None);
}

/// A generated metadata record that states no value at a position is not
/// written, and the others still are.
#[test]
fn generated_metadata_skips_a_record_with_a_null_value() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let attribute = |name: &str, values: Vec<Option<AttributeValue>>| SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint(format!("sldprt:metadata:{name}#0:0")).unwrap(),
        target: AttributeTarget::Document.into(),
        name: name.into(),
        values,
        native_ref: None,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .attributes
        .push(attribute("bounding_envelope", vec![None]));
    ir.model.attributes.push(attribute(
        "source_linear_unit_name",
        vec![Some(AttributeValue::String("mm".into()))],
    ));
    let mut unwritten = UnwrittenAttributes::default();
    let (objects, units) =
        crate::writer::metadata_payloads(&ir, 1.0, false, &mut unwritten, &mut Vec::new());
    assert_eq!(objects, b"moLengthUserUnits_c\xff\xfe\xff\x04m\x00m\x00");
    assert_eq!(units, None);
    assert_eq!(
        unwritten.reason("sldprt:metadata:bounding_envelope#0:0"),
        Some(Unwritten::NoValue)
    );
}

/// The generator writes every record it can hold and records each one it
/// cannot: a wrong-typed value, an unknown name, a record on a part element,
/// and the configuration manager, which it has no serializer for.
#[test]
fn generated_metadata_skips_each_record_it_cannot_hold() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let attribute = |name: &str, offset: u32, values: Vec<AttributeValue>| SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint(format!("sldprt:metadata:{name}#0:{offset}"))
            .unwrap(),
        target: AttributeTarget::Document.into(),
        name: name.into(),
        values: values.into_iter().map(Some).collect(),
        native_ref: None,
    };
    let mut on_body = attribute(
        "default_reference_plane",
        1,
        vec![
            AttributeValue::vector([0.0, 0.0, 0.0]).unwrap(),
            AttributeValue::vector([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap(),
        ],
    );
    on_body.target =
        AttributeTarget::Body(cadmpeg_ir::ids::BodyId::mint("sldprt:parasolid:body#1").unwrap())
            .into();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.attributes.extend([
        attribute(
            "bounding_envelope",
            0,
            vec![AttributeValue::String("wide".into())],
        ),
        on_body,
        attribute("surface_finish", 2, vec![AttributeValue::Integer(3)]),
        attribute(
            "configuration_manager",
            3,
            vec![
                AttributeValue::Integer(1),
                AttributeValue::Integer(3),
                AttributeValue::Integer(0),
            ],
        ),
        attribute(
            "source_linear_unit_name",
            4,
            vec![AttributeValue::String("mm".into())],
        ),
    ]);
    let mut unwritten = UnwrittenAttributes::default();
    let (objects, units) =
        crate::writer::metadata_payloads(&ir, 1.0, false, &mut unwritten, &mut Vec::new());
    assert_eq!(objects, b"moLengthUserUnits_c\xff\xfe\xff\x04m\x00m\x00");
    assert_eq!(units, None);
    for (id, reason) in [
        ("bounding_envelope#0:0", Unwritten::Unrepresentable),
        ("default_reference_plane#0:1", Unwritten::EntityTarget),
        ("surface_finish#0:2", Unwritten::UnknownName),
        ("configuration_manager#0:3", Unwritten::NoSerializer),
    ] {
        assert_eq!(
            unwritten.reason(&format!("sldprt:metadata:{id}")),
            Some(reason)
        );
    }
    assert_eq!(
        unwritten.reason("sldprt:metadata:source_linear_unit_name#0:4"),
        None
    );
}

/// A decoded part whose metadata attributes state a null value writes a file:
/// each record keeps its retained bytes, and the export names the attributes
/// in one `export:edit.dropped` loss.
#[test]
fn a_null_metadata_value_keeps_the_retained_record_and_charges_a_loss() {
    use crate::test_support::container::{native_envelope_fixture, sldprt_with_body_and_envelope};
    let source = native_envelope_fixture(&sldprt_with_body_and_envelope(&triangle_body()));
    let swobjects = |bytes: &[u8]| {
        crate::test_support::container::scan(bytes)
            .blocks
            .into_iter()
            .find(|block| block.section.name() == Some("SWObjects"))
            .unwrap()
            .payload
    };
    let original = swobjects(&source);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, fidelity) = decoded.into_parts();
    for name in ["bounding_envelope", "configuration_manager"] {
        ir.model
            .attributes
            .iter_mut()
            .find(|attribute| attribute.name == name)
            .unwrap()
            .values[0] = None;
    }
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap();
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code.namespace() == "export"
                && loss.code.local_code() == "edit.dropped"
                && loss.message.contains("null value")
        })
        .unwrap();
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Error);
    assert!(
        loss.message.starts_with(
            "the SLDPRT write names sldprt metadata attribute records that state a null value"
        ) && loss.message.contains(" (2): "),
        "{}",
        loss.message
    );
    assert!(loss.message.contains("sldprt:metadata:bounding_envelope#"));
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(swobjects(&written), original);
}

#[test]
fn retained_configuration_manager_writes_only_the_unchanged_raw_tuple() {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut payload = b"moConfigurationMgr_c".to_vec();
    let mut body = [0; 125];
    body[66..70].copy_from_slice(&17u32.to_le_bytes());
    body[107] = 3;
    body[117..125].copy_from_slice(&132_537_600_000_000_000u64.to_le_bytes());
    payload.extend(body);
    let values = vec![
        AttributeValue::Integer(17),
        AttributeValue::Integer(3),
        AttributeValue::Integer(132_537_600_000_000_000),
    ];
    let (patched, unwritten) =
        patched_retained_metadata("configuration_manager", values.clone(), payload.clone())
            .unwrap();
    assert_eq!(patched, payload);
    assert_eq!(
        unwritten.reason("sldprt:metadata:configuration_manager#0:0"),
        None
    );
    for lane in 0..3 {
        let mut edited = values.clone();
        let AttributeValue::Integer(value) = &mut edited[lane] else {
            panic!("integer lane")
        };
        *value += 1;
        assert_unwritten(
            "configuration_manager",
            edited,
            &payload,
            Unwritten::NoSerializer,
        );
    }
}

#[test]
fn configuration_manager_edits_keep_the_retained_tuple_at_the_writer_entry() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    let source = crate::test_support::container::sldprt_with_body_and_envelope(&triangle_body());
    let original = crate::test_support::container::scan(&source)
        .blocks
        .into_iter()
        .find(|block| block.section.name() == Some("SWObjects"))
        .unwrap()
        .payload;
    let manager_at = original
        .windows(b"moConfigurationMgr_c".len())
        .position(|bytes| bytes == b"moConfigurationMgr_c")
        .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (baseline, _, fidelity) = decoded.into_parts();
    for change in 0..6 {
        let mut ir = baseline.clone();
        if change == 1 {
            let bbox = ir
                .model
                .attributes
                .iter_mut()
                .find(|a| a.name == "bounding_envelope")
                .unwrap();
            let Some(AttributeValue::Vector(values)) = &mut bbox.values[0] else {
                panic!("vector")
            };
            values[0] = cadmpeg_ir::scalar::FiniteReal::new(11.0).unwrap();
        } else if (2..5).contains(&change) {
            let manager = ir
                .model
                .attributes
                .iter_mut()
                .find(|a| a.name == "configuration_manager")
                .unwrap();
            let Some(AttributeValue::Integer(value)) = &mut manager.values[change - 2] else {
                panic!("integer")
            };
            *value += 1;
        } else if change == 5 {
            let body = ir.geometry.bodies[0].id.clone();
            ir.model
                .attributes
                .iter_mut()
                .find(|a| a.name == "configuration_manager")
                .unwrap()
                .target = AttributeTarget::Body(body).into();
        }
        let records = crate::source_records(&ir, &fidelity).unwrap();
        let mut written = Vec::new();
        let output = crate::writer::write_semantic_with_records(
            &ir,
            &fidelity.annotations,
            &records,
            &[],
            &mut written,
        )
        .unwrap();
        let manager = ir
            .model
            .attributes
            .iter()
            .find(|a| a.name == "configuration_manager")
            .unwrap();
        let expected = match change {
            2..=4 => Some(Unwritten::NoSerializer),
            5 => Some(Unwritten::EntityTarget),
            _ => None,
        };
        assert_eq!(
            output.unwritten.reason(manager.id.as_str()),
            expected,
            "change {change}"
        );
        let current = crate::test_support::container::scan(&written)
            .blocks
            .into_iter()
            .find(|block| block.section.name() == Some("SWObjects"))
            .unwrap()
            .payload;
        if change == 0 {
            assert_eq!(current, original);
        }
        assert_eq!(
            &current[manager_at..manager_at + 145],
            &original[manager_at..manager_at + 145]
        );
    }
    // With no retained record every metadata record is generated, and the
    // manager tuple has no generator.
    let output = crate::writer::write_semantic_with_records(
        &baseline,
        &fidelity.annotations,
        &[],
        &[],
        &mut Vec::new(),
    )
    .unwrap();
    let manager = baseline
        .model
        .attributes
        .iter()
        .find(|a| a.name == "configuration_manager")
        .unwrap();
    assert_eq!(
        output.unwritten.reason(manager.id.as_str()),
        Some(Unwritten::NoSerializer)
    );
}

/// A decoded part whose `SWObjects` section holds its metadata records, its
/// fidelity, and that section's bytes.
fn retained_metadata_part() -> (cadmpeg_ir::CadIr, cadmpeg_ir::SourceFidelity, Vec<u8>) {
    use crate::test_support::container::{native_envelope_fixture, sldprt_with_body_and_envelope};
    let source = native_envelope_fixture(&sldprt_with_body_and_envelope(&triangle_body()));
    let original = swobjects_payload(&source);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (ir, _, fidelity) = decoded.into_parts();
    (ir, fidelity, original)
}

fn swobjects_payload(bytes: &[u8]) -> Vec<u8> {
    crate::test_support::container::scan(bytes)
        .blocks
        .into_iter()
        .find(|block| block.section.name() == Some("SWObjects"))
        .unwrap()
        .payload
}

/// The `SWObjects` bytes an inherited write of `ir` produces, and its
/// unwritten-attribute losses.
fn written_metadata(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> (Vec<u8>, crate::test_support::UnwrittenLosses) {
    let (bytes, losses) =
        crate::test_support::write_with_unwritten_attributes(ir, fidelity).unwrap();
    (swobjects_payload(&bytes), losses)
}

fn metadata_attribute<'a>(
    ir: &'a mut cadmpeg_ir::CadIr,
    name: &str,
) -> &'a mut cadmpeg_ir::attributes::SourceAttribute {
    ir.model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == name)
        .unwrap()
}

/// A metadata value of a type its record cannot hold keeps the retained
/// record, and `export:edit.dropped` names the attribute.
#[test]
fn a_wrong_typed_metadata_value_keeps_the_retained_record_and_charges_a_loss() {
    let (mut ir, fidelity, original) = retained_metadata_part();
    let bbox = metadata_attribute(&mut ir, "bounding_envelope");
    bbox.values = vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
        "wide".into(),
    ))];
    let id = bbox.id.as_str().to_owned();
    let (written, losses) = written_metadata(&ir, &fidelity);
    assert_eq!(written, original);
    losses.assert_charged(Held::Source, "hold a value whose type, count or range", &id);
}

/// A document metadata attribute moved onto a part element keeps the
/// retained record, and `export:edit.dropped` names the attribute.
#[test]
fn an_entity_metadata_attribute_keeps_the_retained_record_and_charges_a_loss() {
    let (mut ir, fidelity, original) = retained_metadata_part();
    let body = ir.geometry.bodies[0].id.clone();
    let bbox = metadata_attribute(&mut ir, "bounding_envelope");
    bbox.target = cadmpeg_ir::attributes::AttributeTarget::Body(body).into();
    let id = bbox.id.as_str().to_owned();
    let (written, losses) = written_metadata(&ir, &fidelity);
    assert_eq!(written, original);
    losses.assert_charged(Held::Source, "are attached to a part element", &id);
}

/// An added attribute no SLDPRT record has a name for is not written and is
/// omitted; a record the document no longer states keeps its retained bytes,
/// and its removal is a dropped edit.
#[test]
fn an_unknown_or_removed_metadata_attribute_is_written_around_and_named() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let (mut ir, fidelity, original) = retained_metadata_part();
    let added = "sldprt:metadata:surface_finish#0:0";
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint(added).unwrap(),
        target: AttributeTarget::Document.into(),
        name: "surface_finish".into(),
        values: vec![Some(AttributeValue::Integer(3))],
        native_ref: None,
    });
    let bbox = ir
        .model
        .attributes
        .iter()
        .position(|attribute| attribute.name == "bounding_envelope")
        .unwrap();
    let removed = ir.model.attributes.remove(bbox).id.as_str().to_owned();
    let (written, losses) = written_metadata(&ir, &fidelity);
    assert_eq!(written, original);
    losses.assert_charged(Held::Nothing, "name no SLDPRT metadata record", added);
    losses.assert_charged(
        Held::Source,
        "are no longer stated by the document",
        &removed,
    );
}

/// An edited configuration-manager tuple keeps the retained record, which no
/// serializer can rewrite, and `export:edit.dropped` names the attribute.
#[test]
fn an_edited_configuration_manager_keeps_its_retained_tuple_and_charges_a_loss() {
    let (mut ir, fidelity, original) = retained_metadata_part();
    let manager = metadata_attribute(&mut ir, "configuration_manager");
    let Some(cadmpeg_ir::attributes::AttributeValue::Integer(value)) = &mut manager.values[0]
    else {
        panic!("integer manager field")
    };
    *value += 1;
    let id = manager.id.as_str().to_owned();
    let (written, losses) = written_metadata(&ir, &fidelity);
    assert_eq!(written, original);
    losses.assert_charged(Held::Source, "cannot add, regenerate or edit in place", &id);
}

/// A part-record word wider than 32 bits is a value the record cannot hold:
/// `export:value.unwritable` names the attribute (§3.4).
#[test]
fn a_part_record_word_wider_than_32_bits_is_a_value_unwritable() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let id = "sldprt:metadata:part_record#0:6";
    let attribute = SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint(id).unwrap(),
        target: AttributeTarget::Document.into(),
        name: "part_record".into(),
        values: vec![Some(AttributeValue::Integer(1 << 32))],
        native_ref: None,
    };
    assert!(matches!(
        crate::writer::MetadataRecord::project(&attribute, 1.0, |_| Ok(())),
        Err(Unwritten::TooWide)
    ));
    let mut unwritten = UnwrittenAttributes::default();
    unwritten.record(id, Unwritten::TooWide, crate::writer::Held::Source);
    let notes = unwritten.notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].code.local_code(), "value.unwritable");
    assert!(notes[0].message.contains(id), "{}", notes[0].message);
}
