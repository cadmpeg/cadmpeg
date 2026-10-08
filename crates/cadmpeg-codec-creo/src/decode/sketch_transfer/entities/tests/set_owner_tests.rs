// SPDX-License-Identifier: Apache-2.0

use super::super::{transfer_section_entities, SectionEntityTransfer};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn emitted_profile_entity_refuses_work_and_preserves_construction_state() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::definitions::FeatureSavedSection {
                entities: Vec::new(),
                offset: 0,
            }),
            offset: 0,
        });
    let definition = &scan.features.definitions[0];
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch ID");
    let segment = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 7,
        body: Vec::new(),
        offset: 0,
    };
    let segments = [&segment];
    let entity_id =
        SketchEntityId::mint("creo:featdefs:sketch_entity#1:7").expect("sketch entity ID");
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .expect("line geometry");
    let segment_geometries = BTreeMap::from([(0, Some(geometry.clone()))]);
    let unique_segment_ids = BTreeSet::from([7]);
    let unique_saved_ids = BTreeSet::new();
    let ambiguous_segment_ids = BTreeSet::new();
    let solved = BTreeSet::new();
    let no_resolved_geometries = BTreeMap::<usize, Option<SketchGeometry>>::new();
    let no_curve_geometries = BTreeMap::<usize, SketchGeometry>::new();
    let profile_entities = BTreeSet::from([entity_id.clone()]);
    let materialized_saved_section_external_ids = BTreeSet::new();

    let (entities, profiles) = crate::test_support::assert_work_boundaries(
        &["creo emitted profile entity membership"],
        |ctx| {
            let mut ir = CadIr::empty();
            let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
            let mut losses = Vec::new();
            let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
            transfer_section_entities(
                ctx,
                SectionEntityTransfer {
                    scan: &scan,
                    ir: &mut ir,
                    annotations: &mut annotations,
                    definition,
                    transform: None,
                    sketch_id: &sketch,
                    segments: &segments,
                    unique_segment_ids: &unique_segment_ids,
                    unique_saved_ids: &unique_saved_ids,
                    ambiguous_segment_ids: &ambiguous_segment_ids,
                    complete_segment_table: false,
                    solved: &solved,
                    segment_geometries: &segment_geometries,
                    resolved_segment_geometries: &no_resolved_geometries,
                    circle_geometries: &no_curve_geometries,
                    point_geometries: &no_curve_geometries,
                    centered_line_geometries: &no_curve_geometries,
                    reference_line_geometries: &no_curve_geometries,
                    materialized_saved_section_external_ids:
                        &materialized_saved_section_external_ids,
                    profiles: Vec::new(),
                    profile_entities: &profile_entities,
                    losses: &mut losses,
                    source_carriers: &mut source_carriers,
                },
            )
        },
    );
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].id(), &entity_id);
    assert_eq!(entities[0].sketch, sketch);
    assert!(!entities[0].construction);
    assert_eq!(entities[0].geometry, geometry);
    assert_eq!(
        entities[0].endpoint_refs,
        vec![
            "creo:featdefs:sketch#1:point#1".to_owned(),
            "creo:featdefs:sketch#1:point#2".to_owned(),
        ]
    );
    assert!(profiles.is_empty());
}

#[test]
fn mixed_section_families_keep_family_and_source_order() {
    use crate::feature::definitions::{FeatureCircleSegment, FeaturePointSegment, FeatureSegmentTable};
    use crate::feature::segment_rows::SegmentRow;
    let scan = crate::test_support::empty_container_scan();
    let mut definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None,
            },
            body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(),
            variables: None, segments: None, trim_entities: None, trim_vertices: None,
            order_table: None, section_3d: None, dimensions: None, relations: None,
            saved_section: None, offset: 0,
        };
    definition.segments = Some(FeatureSegmentTable {
        declared_count: 4,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [
            SegmentRow::Point(FeaturePointSegment { point_id: 1, external_id: 7, offset: 0 }),
            SegmentRow::Circle(FeatureCircleSegment { center_id: 2, radius_ref: 1, external_id: 10, offset: 1 }),
            SegmentRow::Point(FeaturePointSegment { point_id: 3, external_id: 3, offset: 2 }),
            SegmentRow::Circle(FeatureCircleSegment { center_id: 4, radius_ref: 2, external_id: 2, offset: 3 }),
        ].into_iter().collect(),
        offset: 0,
    });
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch ID");
    let unique_ids = BTreeSet::from([2, 3, 7, 10]);
    let empty_ids = BTreeSet::new();
    let empty_entities = BTreeSet::new();
    let ordinary_geometries = BTreeMap::new();
    let special_geometries = BTreeMap::new();
    let (entities, profiles) = crate::test_support::assert_work_boundaries(
        &["creo section family source rows", "creo entities circles segment rows", "creo entities points segment rows"],
        |ctx| {
            let mut ir = CadIr::empty();
            let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
            let mut losses = Vec::new();
            let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
            transfer_section_entities(ctx, SectionEntityTransfer {
                scan: &scan,
                ir: &mut ir,
                annotations: &mut annotations,
                definition: &definition,
                transform: None,
                sketch_id: &sketch,
                segments: &[],
                unique_segment_ids: &unique_ids,
                unique_saved_ids: &empty_ids,
                ambiguous_segment_ids: &empty_ids,
                complete_segment_table: true,
                solved: &empty_ids,
                segment_geometries: &ordinary_geometries,
                resolved_segment_geometries: &ordinary_geometries,
                circle_geometries: &special_geometries,
                point_geometries: &special_geometries,
                centered_line_geometries: &special_geometries,
                reference_line_geometries: &special_geometries,
                materialized_saved_section_external_ids: &empty_ids,
                profiles: Vec::new(),
                profile_entities: &empty_entities,
                losses: &mut losses,
                source_carriers: &mut source_carriers,
            })
        });
    assert_eq!(entities.iter().map(|entity| entity.id().as_str()).collect::<Vec<_>>(), [
        "creo:featdefs:sketch_entity#1:10", "creo:featdefs:sketch_entity#1:2",
        "creo:featdefs:sketch_entity#1:7", "creo:featdefs:sketch_entity#1:3",
    ]);
    assert!(profiles.is_empty());
}
