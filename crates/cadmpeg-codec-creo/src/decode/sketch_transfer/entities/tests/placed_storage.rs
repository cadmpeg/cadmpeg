// SPDX-License-Identifier: Apache-2.0

use super::super::{transfer_section_entities, SectionEntityTransfer};
use crate::feature::definitions::{DefinitionIdentity, FeatureCenteredLineSegment, FeatureCircleSegment, FeatureDefinition, FeatureSegment, FeatureSegmentKind, FeatureSegmentTable};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
enum Family { Ordinary, Circle, CenteredLine }

fn assert_duplicate_curve_storage(family: Family) {
    let segment = match family {
        Family::Ordinary => SegmentRow::Ordinary(FeatureSegment {
            kind: FeatureSegmentKind::Line([1, 2]), directions: [None; 3], center_id: None,
            arc_orientation: None, vertical_horizontal: None, radius_ref: None, radius2_ref: None,
            external_id: 7, body: Vec::new(), offset: 0,
        }),
        Family::Circle => SegmentRow::Circle(FeatureCircleSegment { center_id: 1, radius_ref: 1, external_id: 7, offset: 0 }),
        Family::CenteredLine => SegmentRow::CenteredLine(FeatureCenteredLineSegment { center_id: 1, external_id: 7, offset: 0 }),
    };
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed { schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(), variables: None,
        segments: Some(FeatureSegmentTable { declared_count: 1, has_elided_prototype: false, entity_ref: None, rows: [segment].into_iter().collect(), offset: 0 }),
        trim_entities: None, trim_vertices: None, order_table: None, section_3d: None,
        dimensions: None, relations: None, saved_section: None, offset: 0,
    };
    let table = definition.segments.as_ref().expect("segment table");
    let ordinary: Vec<_> = table.rows.as_slice().iter().filter_map(|row| match row {
        SegmentRow::Ordinary(segment) => Some(segment), _ => None,
    }).collect();
    let geometry = SketchGeometry::try_from(match family {
        Family::Circle => SketchGeometryDefinition::Circle { center: Point2::new(0.0, 0.0), radius: cadmpeg_ir::scalar::Length::new(1.0).expect("radius") },
        _ => SketchGeometryDefinition::Line { start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0) },
    }).expect("section geometry");
    let ordinary_geometries = if matches!(family, Family::Ordinary) { BTreeMap::from([(0, Some(geometry.clone()))]) } else { BTreeMap::new() };
    let circle_geometries = if matches!(family, Family::Circle) { BTreeMap::from([(0, geometry.clone())]) } else { BTreeMap::new() };
    let centered_geometries = if matches!(family, Family::CenteredLine) { BTreeMap::from([(0, geometry.clone())]) } else { BTreeMap::new() };
    let transform = crate::placement::FeatureSectionTransform::new(1, None, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0).expect("section frame");
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let existing = Curve { id: CurveId::mint("creo:featdefs:section_curve#1:7").expect("curve identity"), geometry: crate::decode::sweep::surfaces::placed_section_geometry_curve(&transform, &geometry).expect("placed curve"), source_object: None };
    let scan = crate::test_support::empty_container_scan();
    crate::test_support::assert_refusal_order(ResourceDimension::RetainedBytes, &[], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut ir = CadIr::empty();
        ir.model.curves.push(existing.clone());
        let result = transfer_section_entities(&ctx, SectionEntityTransfer {
            scan: &scan, ir: &mut ir, annotations: &mut cadmpeg_ir::AnnotationBuilder::new(),
            definition: &definition, transform: Some(&transform), sketch_id: &sketch,
            segments: &ordinary, unique_segment_ids: &BTreeSet::from([7]), unique_saved_ids: &BTreeSet::new(), ambiguous_segment_ids: &BTreeSet::new(), complete_segment_table: true,
            solved: &BTreeSet::new(), segment_geometries: &ordinary_geometries, resolved_segment_geometries: &ordinary_geometries,
            circle_geometries: &circle_geometries, point_geometries: &BTreeMap::new(), centered_line_geometries: &centered_geometries, reference_line_geometries: &BTreeMap::new(),
            materialized_saved_section_external_ids: &BTreeSet::new(), profiles: Vec::new(), profile_entities: &BTreeSet::new(), losses: &mut Vec::new(), source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        });
        if let Err(CodecError::ResourceLimit(resource)) = &result { assert_ne!(resource.operation, "creo section curve identity"); }
        result.map(|(entities, profiles)| {
            assert_eq!(ir.model.curves, vec![existing.clone()]);
            assert_eq!(entities.len(), 1);
            assert_eq!(entities[0].geometry_ref.as_deref(), Some(existing.id.as_str()));
            assert!(profiles.is_empty());
            ctx.finish_session().expect("active session");
        })
    });
}

#[test]
fn duplicate_placed_segment_curve_identity_is_not_retained() { assert_duplicate_curve_storage(Family::Ordinary); }
#[test]
fn duplicate_placed_circle_curve_identity_is_not_retained() { assert_duplicate_curve_storage(Family::Circle); }
#[test]
fn duplicate_placed_centered_line_curve_identity_is_not_retained() { assert_duplicate_curve_storage(Family::CenteredLine); }
