//! Resource admission on the hole-axis projection route.

use super::{cylinder, lane_with_position_reference, model_hole, native_history};
use crate::records::{FeatureSource, SketchInputEntity, SketchInputKind};
use crate::resolved_features::holes::{project_hole_axes, HoleTopology};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::sketches::SketchId;

fn topology(surfaces: &[Surface]) -> HoleTopology<'_> {
    HoleTopology {
        surfaces, faces: &[], loops: &[], coedges: &[], edges: &[], vertices: &[], points: &[],
    }
}

#[test]
fn hole_position_axes_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project_hole_axes(&ctx, &mut [], &[], &topology(&[]), &[native_history()], &[]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT hole position features"));
}

#[test]
fn hole_position_axes_refuse_work_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project_hole_axes(&ctx, &mut [], &[], &topology(&[]), &[native_history()], &[]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT hole position features"));
}

#[test]
fn hole_position_axes_refuse_retained_limit() {
    let mut feature = model_hole();
    feature.evaluation.set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
        sketch: SketchFeatureBinding::Planar(Some(SketchId::mint("synthetic:test:id#position").unwrap())),
    }));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(feature.native_ref.as_ref().unwrap().len()).unwrap() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project_hole_axes(&ctx, &mut [feature], &[], &topology(&[]), &[], &[]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "index SLDPRT hole position features"));
}

#[test]
fn hole_position_axes_refuse_nesting_limit() {
    let mut history = native_history();
    let mut position = history.features[0].clone();
    position.id = "native-position".into();
    position.source_id = FeatureSource::from_value(12);
    position.ordinal = 1;
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    history.features.push(position);
    let mut lane = lane_with_position_reference(12);
    for (ordinal, x) in [(0, -0.005), (1, 0.005)] {
        let mut marker = SketchInputEntity::new(
            format!("position-{ordinal}"), "lane", ordinal, u64::from(ordinal), SketchInputKind::Arc,
        ).with_test_identity(Some(ordinal + 1), None);
        marker.feature_ref = Some("native-position".into());
        marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([x, 0.0]);
        lane.sketch_entities.push(marker);
    }
    let surfaces = [cylinder(0, -5.0), cylinder(1, 5.0)];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &DecodePolicy::service()).unwrap();
    let mut features = [model_hole()];
    project_hole_axes(&ctx, &mut features, &[], &topology(&surfaces), std::slice::from_ref(&history), std::slice::from_ref(&lane)).unwrap();
    assert!(matches!(features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Hole { placements: Some(placements), .. }) if placements.len() == 2));
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 2;
    let (limited, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    let error = project_hole_axes(&limited, &mut [model_hole()], &[], &topology(&surfaces), std::slice::from_ref(&history), std::slice::from_ref(&lane)).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.operation == "search SLDPRT congruent bore subsets"));
}
