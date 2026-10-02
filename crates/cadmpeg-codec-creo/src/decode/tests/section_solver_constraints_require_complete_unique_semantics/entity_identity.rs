// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap};
use cadmpeg_ir::math::{Point2};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchId, SketchLocus, SketchGeometryDefinition};
use super::super::{declared_solver_rows, section_skamp_constraints, synchronize_skamp_count};
use crate::decode::sketch::coordinates::{resolved_section_points};
use crate::decode::sketch::geometry::{section_centered_line_geometry, section_point_row_geometry, section_reference_line_geometry};
use crate::decode::sketch::skamp::{section_skamp_selected_point_id};
use super::fixtures::{base_definition, resolved_section_reference_line_geometry};
use crate::decode::sketch_transfer::identity::{ambiguous_section_segment_external_ids, section_entity_external_ids, section_segment_identity_suffix, unique_section_segment_external_ids};
use crate::decode::sketch_transfer::loci::{section_skamp_is_line, section_skamp_is_point, section_skamp_locus, section_skamp_midpoint};
use crate::decode::sketch_transfer::profiles::{solver_only_section_entities, solver_only_section_entity_family, SectionEntityIncidenceFamily};
use crate::decode::sketch_transfer::skamp_constraints::{section_skamp_constraints_for_geometry};
use crate::feature::definitions::test_support::{append_points};

#[test]
fn section_solver_entity_identity_and_loci_require_unique_semantics() {
    let definition = base_definition();
    let constraints =
        section_skamp_constraints(&definition, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"));
    let mut solver_only = definition.clone();
    *declared_solver_rows(&mut solver_only.relations.as_mut().expect("relations").skamps) =
        vec![crate::feature::definitions::FeatureSkamp {
            id: 20,
            kind: 0,
            flags: 0,
            status: 35,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 99,
                    sense: 3,
                },
            ],
            offset: 95,
        }];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| solver_only_section_entities(ctx, &solver_only))
            .expect("service solver-only entities"),
        BTreeMap::from([(99, 95)])
    );
    let mut whole_entity_tangent = definition.clone();
    *declared_solver_rows(&mut whole_entity_tangent
        .relations
        .as_mut()
        .expect("relations")
        .skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 20,
        kind: 4,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
        ],
        offset: 95,
    }];
    synchronize_skamp_count(&mut whole_entity_tangent);
    assert_eq!(*(section_skamp_constraints(
            &whole_entity_tangent,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::Tangent {
            first: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string()).expect("valid test fixture"),
            second: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string()).expect("valid test fixture"),
        }
    );
    let mut point_symmetry = definition.clone();
    *declared_solver_rows(&mut point_symmetry.relations.as_mut().expect("relations").skamps) =
        vec![crate::feature::definitions::FeatureSkamp {
            id: 20,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 13,
                    sense: 3,
                },
            ],
            offset: 96,
        }];
    append_points(point_symmetry.variables.as_mut().expect("variables"), vec![crate::feature::definitions::FeatureSectionPoint {
            point_id: 4,
            u: Some(2.0),
            v: Some(3.0),
        }]);
    synchronize_skamp_count(&mut point_symmetry);
    assert_eq!(*(section_skamp_constraints(&point_symmetry, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))[0]
            .0
            .definition).kind(),
        SketchConstraintDefinitionInput::PointSymmetric {
            first: SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:12".to_string()
            ).expect("valid test fixture")),
            second: SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string()
            ).expect("valid test fixture")),
            center: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:14".to_string()
            ).expect("valid test fixture")),
        }
    );
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &point_symmetry)).expect("test section solve")[&3], [4.0, 4.0]);
    point_symmetry.relations.as_mut().expect("relations").skamps.as_mut().expect("skamp table").rows_mut()[0].items[1] =
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        };
    point_symmetry.relations.as_mut().expect("relations").skamps.as_mut().expect("skamp table").rows_mut()[0].items[2] =
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 16,
            sense: 4,
        };
    assert_eq!(*(section_skamp_constraints(&point_symmetry, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))[0]
            .0
            .definition).kind(),
        SketchConstraintDefinitionInput::PointSymmetric {
            first: SketchLocus::Center(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string()
            ).expect("valid test fixture")),
            second: SketchLocus::Center(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:16".to_string()
            ).expect("valid test fixture")),
            center: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:14".to_string()
            ).expect("valid test fixture")),
        }
    );
    point_symmetry.relations.as_mut().expect("relations").skamps.as_mut().expect("skamp table").rows_mut()[0].items[2].sense = 1;
    assert!(matches!(
        section_skamp_constraints(&point_symmetry, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));

    assert!(matches!(
        constraints[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { .. }
    ));
    assert!(matches!(
        constraints[1].0.definition.kind(),
        SketchConstraintDefinitionInput::Vertical { .. }
    ));
    let mut incomplete_segments = definition.clone();
    incomplete_segments
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    let incomplete_constraints = section_skamp_constraints(
        &incomplete_segments,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    assert!(matches!(
        incomplete_constraints[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { .. }
    ));
    assert!(matches!(
        incomplete_constraints[1].0.definition.kind(),
        SketchConstraintDefinitionInput::Vertical { .. }
    ));
    let mut locus_orientation = definition.clone();
    locus_orientation
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items[0]
        .sense = 2;
    assert!(matches!(
        section_skamp_constraints(&locus_orientation, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:skamp:1"
    ));
    let mut duplicate_entity = definition.clone();
    let mut duplicate_line = duplicate_entity.segments.as_ref().expect("segments").rows.ordinary().cloned().collect::<Vec<_>>()[0].clone();
    duplicate_line.offset = 500;
    duplicate_entity
        .segments
        .as_mut()
        .expect("segments")
        .rows.insert(crate::feature::segment_rows::SegmentRow::Ordinary(duplicate_line));
    let unique_ids = crate::decode::with_test_decode_ctx(|ctx| unique_section_segment_external_ids(ctx, &duplicate_entity))
        .expect("service unique identities");
    assert!(!unique_ids.contains(&12));
    assert_eq!(
        section_segment_identity_suffix(
            &unique_ids,
            duplicate_entity
                .segments
                .as_ref()
                .expect("segments")
                .rows.ordinary().cloned().collect::<Vec<_>>()
                .last()
                .expect("duplicate segment")
        ),
        "offset:500"
    );
    assert!(matches!(
        section_skamp_constraints(&duplicate_entity, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:skamp:1"
    ));
    let opaque_segment = crate::feature::definitions::FeatureOpaqueSegment {
        kind: 25,
        directions: [None; 3],
        point_ids: [Some(1), Some(2)],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 99,
        body: Vec::new(),
        offset: 600,
    };
    let mut opaque_entity = definition.clone();
    opaque_entity
        .segments
        .as_mut()
        .expect("segments")
        .rows.insert(crate::feature::segment_rows::SegmentRow::Opaque(opaque_segment.clone()));
    assert!(crate::decode::with_test_decode_ctx(|ctx| unique_section_segment_external_ids(ctx, &opaque_entity))
        .expect("service unique identities").contains(&99));
    assert!(crate::decode::with_test_decode_ctx(|ctx| section_entity_external_ids(ctx, &opaque_entity))
        .expect("service section identities").contains(&99));

    let mut opaque_point = definition.clone();
    opaque_point
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    opaque_point
        .segments
        .as_mut()
        .expect("segments")
        .rows.insert(crate::feature::segment_rows::SegmentRow::Point(crate::feature::definitions::FeaturePointSegment {
            point_id: 1,
            external_id: 99,
            offset: 601,
        }));
    let opaque_point_item = crate::feature::definitions::FeatureSkampItem {
        entity_id: 99,
        sense: 0,
    };
    assert_eq!(
        section_point_row_geometry(
            &crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &opaque_point)).expect("test section solve"),
            &opaque_point.segments.as_ref().expect("segments").rows.points().cloned().collect::<Vec<_>>()[0],
        ),
        Some(SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 2.0),
        }).expect("valid test fixture"))
    );
    assert!(section_skamp_is_point(&opaque_point, &opaque_point_item));
    assert!(matches!(
        crate::decode::sketch_transfer::loci::with_test_locus(|ctx, refusal| section_skamp_locus(
            ctx, refusal,
            &opaque_point,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            &crate::feature::definitions::FeatureSkampItem {
                sense: 4,
                ..opaque_point_item.clone()
            },
        )),
        Some(SketchLocus::Entity(entity)) if entity.as_str().ends_with(":99")
    ));
    assert!(matches!(
        crate::decode::sketch_transfer::loci::with_test_locus(|ctx, refusal| section_skamp_midpoint(
            ctx, refusal,
            &opaque_point,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            &crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            &opaque_point_item,
            None,
        )),
        Some((SketchLocus::Entity(entity), target))
            if entity.as_str().ends_with(":99") && target.as_str().ends_with(":12")
    ));
    assert_eq!(
        crate::decode::sketch_transfer::loci::with_test_locus(|ctx, refusal| section_skamp_midpoint(
            ctx, refusal,
            &opaque_point,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            &opaque_point_item,
            &crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 2,
            },
            Some(&BTreeMap::from([(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string()).expect("valid test fixture"),
                SketchGeometry::try_from(SketchGeometryDefinition::Native {
                    native_kind: cadmpeg_core::text::NonBlankString::new("point").expect("nonempty source identity"),
                }).expect("valid test fixture"),
            )])),
        )),
        None
    );
    let centered_line = crate::feature::definitions::FeatureCenteredLineSegment {
        center_id: 2,
        external_id: 100,
        offset: 602,
    };
    assert_eq!(
        section_centered_line_geometry(
            &BTreeMap::from([(0, [3.0, -1.0]), (1, [3.0, 5.0]), (2, [3.0, 2.0]),]),
            &centered_line,
        ),
        Some(SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(3.0, -1.0),
            end: Point2::new(3.0, 5.0),
        }).expect("valid test fixture"))
    );
    assert_eq!(
        section_centered_line_geometry(
            &BTreeMap::from([(0, [3.0, -1.0]), (1, [3.0, 5.0]), (2, [3.0, 2.0]),]),
            &crate::feature::definitions::FeatureCenteredLineSegment {
                center_id: 0,
                ..centered_line.clone()
            },
        ),
        None
    );
    let reference_line = crate::feature::definitions::FeatureReferenceLineSegment {
        directions: [Some(0), Some(1), Some(0)],
        point_ids: [Some(7), Some(8)],
        vertical_horizontal: Some(1),
        external_id: 101,
        offset: 603,
    };
    assert_eq!(
        section_reference_line_geometry(
            &BTreeMap::from([(7, [-4.0, 2.0]), (8, [6.0, 2.0])]),
            &reference_line,
        ),
        Some(SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(-4.0, 2.0),
            direction: Point2::new(10.0, 0.0),
        }).expect("valid test fixture"))
    );
    assert_eq!(
        section_reference_line_geometry(
            &BTreeMap::from([(7, [-4.0, 2.0]), (8, [-4.0, 2.0])]),
            &reference_line,
        ),
        None
    );
    let axis_reference_line = crate::feature::definitions::FeatureReferenceLineSegment {
        directions: [Some(0), Some(1), Some(0)],
        vertical_horizontal: Some(1),
        ..reference_line.clone()
    };
    let mut reference_definition = definition.clone();
    let reference_segments = reference_definition
        .segments
        .as_mut()
        .expect("segment table");
    reference_segments.declared_count += 1;
    reference_segments
        .rows.insert(crate::feature::segment_rows::SegmentRow::ReferenceLine(axis_reference_line.clone()));
    assert!(section_skamp_is_line(
        &reference_definition,
        &crate::feature::definitions::FeatureSkampItem {
            entity_id: axis_reference_line.external_id,
            sense: 0,
        },
    ));
    assert_eq!(
        resolved_section_reference_line_geometry(
            &reference_definition,
            &BTreeMap::from([(7, [None, Some(2.0)]), (8, [None, Some(2.0)]),]),
            &BTreeMap::new(),
            &axis_reference_line,
        ),
        Some(SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(0.0, 2.0),
            direction: Point2::new(1.0, 0.0),
        }).expect("valid test fixture"))
    );
    assert_eq!(
        resolved_section_reference_line_geometry(
            &reference_definition,
            &BTreeMap::from([(7, [None, Some(2.0)]), (8, [None, Some(3.0)]),]),
            &BTreeMap::new(),
            &axis_reference_line,
        ),
        None
    );
    assert_eq!(
        section_reference_line_geometry(
            &BTreeMap::from([(7, [-4.0, 2.0])]),
            &crate::feature::definitions::FeatureReferenceLineSegment {
                point_ids: [Some(7), None],
                ..reference_line
            },
        ),
        None
    );
    let mut endpoint_families = definition.clone();
    let endpoint_segments = endpoint_families
        .segments
        .as_mut()
        .expect("segment table");
    endpoint_segments.declared_count += 2;
    endpoint_segments
        .rows.insert(crate::feature::segment_rows::SegmentRow::ReferenceLine(crate::feature::definitions::FeatureReferenceLineSegment {
            point_ids: [Some(1), Some(5)],
            external_id: 101,
            ..reference_line.clone()
        }));
    endpoint_segments
        .rows.insert(crate::feature::segment_rows::SegmentRow::BoundedCurve(crate::feature::definitions::FeatureBoundedCurveSegment {
            point_ids: [1, 5],
            external_id: 102,
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            offset: 604,
        }));
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::definitions::FeatureSkampItem {
                entity_id: 101,
                sense: 2,
            },
        ),
        Some(1)
    );
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::definitions::FeatureSkampItem {
                entity_id: 101,
                sense: 3,
            },
        ),
        Some(5)
    );
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::definitions::FeatureSkampItem {
                entity_id: 102,
                sense: 2,
            },
        ),
        Some(1)
    );
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::definitions::FeatureSkampItem {
                entity_id: 102,
                sense: 3,
            },
        ),
        Some(5)
    );
    let mut opaque_line = definition.clone();
    opaque_line
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    opaque_line
        .segments
        .as_mut()
        .expect("segments")
        .rows.insert(crate::feature::segment_rows::SegmentRow::CenteredLine(centered_line));
    let opaque_line_item = crate::feature::definitions::FeatureSkampItem {
        entity_id: 100,
        sense: 0,
    };
    assert!(section_skamp_is_line(&opaque_line, &opaque_line_item));
    assert!(matches!(
        crate::decode::sketch_transfer::loci::with_test_locus(|ctx, refusal| section_skamp_locus(
            ctx, refusal,
            &opaque_line,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            &crate::feature::definitions::FeatureSkampItem {
                sense: 2,
                ..opaque_line_item
            },
        )),
        Some(SketchLocus::Start(entity)) if entity.as_str().ends_with(":100")
    ));
    let mut solver_only_point_midpoint = opaque_line.clone();
    let midpoint_relations = solver_only_point_midpoint
        .relations
        .as_mut()
        .expect("relations");
    *declared_solver_rows(&mut midpoint_relations.skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 102,
        kind: 35,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 101,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
        ],
        offset: 604,
    }];
    midpoint_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_only_point_midpoint, 101),
        Some(SectionEntityIncidenceFamily::Point)
    );
    assert_eq!(*(section_skamp_constraints(
            &solver_only_point_midpoint,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:101".to_string()
            ).expect("valid test fixture")),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string()).expect("valid test fixture"),
        }
    );
    let mut conflicting_midpoint = solver_only_point_midpoint.clone();
    let conflicting_relations = conflicting_midpoint.relations.as_mut().expect("relations");
    declared_solver_rows(&mut conflicting_relations
        .skamps)
        .push(crate::feature::definitions::FeatureSkamp {
            id: 103,
            kind: 0,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 101,
                    sense: 4,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
            ],
            offset: 605,
        });
    conflicting_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 2;
    assert_eq!(
        solver_only_section_entity_family(&conflicting_midpoint, 101),
        None
    );
    let centered_midpoint = crate::feature::definitions::FeatureSkamp {
        id: 35,
        kind: 35,
        flags: 0,
        status: 34,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 101,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 100,
                sense: 4,
            },
        ],
        offset: 83,
    };
    let centered_midpoint_relations = opaque_line.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut centered_midpoint_relations.skamps) = vec![centered_midpoint];
    centered_midpoint_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        solver_only_section_entity_family(&opaque_line, 101),
        Some(SectionEntityIncidenceFamily::Point)
    );
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &opaque_line,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&BTreeMap::from([
                (
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:100".to_string()).expect("valid test fixture"),
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: Point2::new(3.0, -1.0),
                        end: Point2::new(3.0, 5.0),
                    }).expect("valid test fixture"),
                ),
                (
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:101".to_string()).expect("valid test fixture"),
                    SketchGeometry::try_from(SketchGeometryDefinition::Native {
                        native_kind: cadmpeg_core::text::NonBlankString::new("point").expect("nonempty source identity"),
                    }).expect("valid test fixture"),
                ),
            ])),
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:101".to_string()
            ).expect("valid test fixture")),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:100".to_string()).expect("valid test fixture"),
        }
    );
    declared_solver_rows(&mut opaque_line
        .relations
        .as_mut()
        .expect("relations")
        .skamps)
        .insert(
            0,
            crate::feature::definitions::FeatureSkamp {
                id: 2,
                kind: 2,
                flags: 0,
                status: 1,
                items: vec![crate::feature::definitions::FeatureSkampItem {
                    entity_id: 101,
                    sense: 0,
                }],
                offset: 82,
            },
        );
    opaque_line
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 2;
    assert_eq!(
        solver_only_section_entity_family(&opaque_line, 101),
        Some(SectionEntityIncidenceFamily::Line)
    );
    opaque_line.relations.as_mut().expect("relations").skamps.as_mut().expect("skamp table").rows_mut()[0].status = 0;
    assert_eq!(
        solver_only_section_entity_family(&opaque_line, 101),
        Some(SectionEntityIncidenceFamily::Line)
    );
    opaque_line.relations.as_mut().expect("relations").skamps.as_mut().expect("skamp table").rows_mut()[0].status = 1;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &opaque_line,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&BTreeMap::from([(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:101".to_string()).expect("valid test fixture"),
                SketchGeometry::try_from(SketchGeometryDefinition::Native {
                    native_kind: cadmpeg_core::text::NonBlankString::new("line").expect("nonempty source identity"),
                }).expect("valid test fixture"),
            )])),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Vertical { .. }
    ));
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &opaque_line,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&BTreeMap::from([
                (
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:100".to_string()).expect("valid test fixture"),
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: Point2::new(3.0, -1.0),
                        end: Point2::new(3.0, 5.0),
                    }).expect("valid test fixture"),
                ),
                (
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:101".to_string()).expect("valid test fixture"),
                    SketchGeometry::try_from(SketchGeometryDefinition::Native {
                        native_kind: cadmpeg_core::text::NonBlankString::new("line").expect("nonempty source identity"),
                    }).expect("valid test fixture"),
                ),
            ])),
        )).expect("test section solve")[1]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut opaque_family_collision = opaque_point.clone();
    opaque_family_collision
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    let colliding_row = crate::feature::definitions::FeatureOpaqueSegment {
        kind: 10,
        directions: [Some(0); 3],
        point_ids: [None, Some(1)],
        center_id: Some(1),
        arc_orientation: Some(0),
        vertical_horizontal: Some(0),
        radius_ref: Some(0),
        radius2_ref: None,
        external_id: 99,
        body: Vec::new(),
        offset: 602,
    };
    opaque_family_collision
        .segments
        .as_mut()
        .expect("segments")
        .rows.insert(crate::feature::segment_rows::SegmentRow::Opaque(colliding_row));
    assert!(!section_skamp_is_point(
        &opaque_family_collision,
        &opaque_point_item,
    ));

    let mut opaque_collision = definition.clone();
    opaque_collision
        .segments
        .as_mut()
        .expect("segments")
        .rows.insert(crate::feature::segment_rows::SegmentRow::Opaque(crate::feature::definitions::FeatureOpaqueSegment {
            external_id: 12,
            ..opaque_segment
        }));
    assert!(!crate::decode::with_test_decode_ctx(|ctx| unique_section_segment_external_ids(ctx, &opaque_collision))
        .expect("service unique identities").contains(&12));
    assert!(crate::decode::with_test_decode_ctx(|ctx| ambiguous_section_segment_external_ids(ctx, &opaque_collision))
        .expect("service ambiguous identities").contains(&12));
    assert!(!crate::decode::with_test_decode_ctx(|ctx| section_entity_external_ids(ctx, &opaque_collision))
        .expect("service section identities").contains(&12));
}

