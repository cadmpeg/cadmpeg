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
        surfaces,
        faces: &[],
        loops: &[],
        coedges: &[],
        edges: &[],
        vertices: &[],
        points: &[],
    }
}

#[test]
fn hole_position_axes_refuse_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "index SLDPRT hole position features",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            project_hole_axes(
                &ctx,
                &mut [model_hole()],
                &[],
                &topology(&[]),
                &[native_history()],
                &[],
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT hole position features"));
}

#[test]
fn hole_position_axes_refuse_work_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "index SLDPRT hole position features",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            project_hole_axes(
                &ctx,
                &mut [model_hole()],
                &[],
                &topology(&[]),
                &[native_history()],
                &[],
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT hole position features"));
}

#[test]
fn hole_position_axes_refuse_scoped_materialized_limit() {
    let mut feature = model_hole();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(
                SketchId::mint("synthetic:test:id#position").unwrap(),
            )),
        }));
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "resolve SLDPRT holes keys",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            project_hole_axes(
                &ctx,
                &mut [feature.clone(), model_hole()],
                &[],
                &topology(&[]),
                &[native_history()],
                &[],
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "resolve SLDPRT holes keys"));
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
            format!("position-{ordinal}"),
            "lane",
            ordinal,
            u64::from(ordinal),
            SketchInputKind::Arc,
        )
        .with_test_identity(Some(ordinal + 1), None);
        marker.feature_ref = Some("native-position".into());
        marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([x, 0.0]);
        lane.sketch_entities.push(marker);
    }
    let surfaces = [cylinder(0, -5.0), cylinder(1, 5.0)];
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&lane.native_payload, &arena, &DecodePolicy::service())
            .unwrap();
    let mut features = [model_hole()];
    project_hole_axes(
        &ctx,
        &mut features,
        &[],
        &topology(&surfaces),
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    )
    .unwrap();
    assert!(matches!(features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Hole { placements: Some(placements), .. }) if placements.len() == 2));
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 2;
    let (limited, _) =
        DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    let error = project_hole_axes(
        &limited,
        &mut [model_hole()],
        &[],
        &topology(&surfaces),
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    )
    .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.operation == "search SLDPRT congruent bore subsets"));
}

#[test]
fn bore_backed_position_projection_refuses_retained_limit() {
    use crate::resolved_features::holes::project_bore_backed_position_sketches;
    use cadmpeg_ir::features::holes::HolePlacement;
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut history = native_history();
    let mut position = history.features[0].clone();
    position.id = "native-position".into();
    position.source_id = FeatureSource::from_value(12);
    position.ordinal = 1;
    position.name = "Position".into();
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    history.features.push(position);
    let lane = lane_with_position_reference(12);
    let mut hole = model_hole();
    hole.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) = definition
        else {
            panic!("hole fixture");
        };
        *placements = Some(vec![HolePlacement::Axis {
            origin: FinitePoint3::new(Point3::new(2.0, 3.0, 0.0)).unwrap(),
            axis: FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        }]);
    });
    let mut position = model_hole();
    position.id = cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#position").unwrap();
    position.native_ref = Some("native-position".into());
    position.name = Some("Position".into());
    position
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(None),
        }));
    let surfaces = [Surface {
        id: SurfaceId::mint("synthetic:test:id#plane").unwrap(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    }];
    let input = [hole, position];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut features = input.clone();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    project_bore_backed_position_sketches(
        &ctx,
        &mut features,
        &mut sketches,
        &mut entities,
        &surfaces,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    )
    .unwrap();
    let expected = SketchId::mint("sldprt:model:sketch#bore:lane:1").unwrap();
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].id, expected);
    assert_eq!(sketches[0].name.as_deref(), Some("Position"));
    assert_eq!(sketches[0].native_ref.as_deref(), Some("lane"));
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].sketch, expected);
    assert!(
        matches!(entities[0].geometry.definition(), SketchGeometryDefinition::Point { position } if position.get() == Point2::new(2.0, 3.0))
    );
    assert!(
        matches!(features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(id)) }) if id == &expected)
    );
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            dimension,
            "project SLDPRT bore backed position sketches",
            |cap| {
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    _ => unreachable!(),
                }
                let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                project_bore_backed_position_sketches(
                    &limited,
                    &mut input.clone(),
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &surfaces,
                    std::slice::from_ref(&history),
                    std::slice::from_ref(&lane),
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == dimension
                && limit.operation == "project SLDPRT bore backed position sketches"));
    }
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.as_str().len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project_bore_backed_position_sketches(
        &limited,
        &mut input.clone(),
        &mut Vec::new(),
        &mut Vec::new(),
        &surfaces,
        &[history],
        &[lane],
    )
    .unwrap_err();
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT bore backed position sketches"
            && limit.limit == u64::try_from(expected.as_str().len()).unwrap() - 1
            && limit.used + limit.additional == u64::try_from(expected.as_str().len()).unwrap()),
        "{error:?}"
    );
}

#[test]
fn helix_projection_skips_lane_indexes_without_target() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let lane = lane_with_position_reference(12);
    super::super::project_helix_axes(&ctx, &mut [model_hole()], &[native_history()], &[lane])
        .unwrap();
}

#[test]
fn hole_ownership_skips_indexes_without_eligible_holes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut history = native_history();
    history.features[0].properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableChildren"),
        "12".into(),
    );
    let lane = lane_with_position_reference(12);
    super::super::enrich_history_hole_constructions(&ctx, &mut [history], &[lane]).unwrap();
    super::super::enrich_history_hole_constructions(
        &ctx,
        &mut [],
        &[lane_with_position_reference(12)],
    )
    .unwrap();
}

#[test]
fn hole_carrier_skips_lane_indexes_without_native_hole() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let hole = model_hole();
    let lane = lane_with_position_reference(12);
    assert!(!super::super::hole_position_carrier_present(
        &ctx,
        &hole,
        &[],
        std::slice::from_ref(&lane)
    )
    .unwrap());
    let mut history = native_history();
    history.features[0].input_class = Some("moProfileFeature_c".into());
    history.features[0].xml_tag = "Sketch".into();
    assert!(
        !super::super::hole_position_carrier_present(&ctx, &hole, &[history], &[lane]).unwrap()
    );
}

#[test]
fn planar_hole_projection_skips_indexes_without_target() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let lane = lane_with_position_reference(12);
    super::super::project_hole_position_sketches(
        &ctx,
        &mut [],
        &[],
        &[],
        &[],
        std::slice::from_ref(&lane),
    )
    .unwrap();
    let mut hole = model_hole();
    hole.suppressed = Some(true);
    super::super::project_hole_position_sketches(
        &ctx,
        &mut [hole],
        &[],
        &[],
        &[native_history()],
        &[lane],
    )
    .unwrap();
}

#[test]
fn spatial_hole_projection_skips_indexes_without_target() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let lane = lane_with_position_reference(12);
    super::super::project_spatial_hole_position_sketches(
        &ctx,
        &mut [],
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&lane),
    )
    .unwrap();
    let mut hole = model_hole();
    hole.suppressed = Some(true);
    super::super::project_spatial_hole_position_sketches(
        &ctx,
        &mut [hole],
        &[],
        &[],
        &[],
        &[native_history()],
        &[lane],
    )
    .unwrap();
}

#[test]
fn hole_axis_projection_skips_indexes_without_target() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let lane = lane_with_position_reference(12);
    project_hole_axes(
        &ctx,
        &mut [],
        &[],
        &topology(&[]),
        &[],
        std::slice::from_ref(&lane),
    )
    .unwrap();
    project_hole_axes(&ctx, &mut [model_hole()], &[], &topology(&[]), &[], &[lane]).unwrap();
}

#[test]
fn bore_projection_skips_indexes_without_bore_target() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let lane = lane_with_position_reference(12);
    super::super::project_bore_backed_position_sketches(
        &ctx,
        &mut [],
        &mut Vec::new(),
        &mut Vec::new(),
        &[],
        &[],
        std::slice::from_ref(&lane),
    )
    .unwrap();
    super::super::project_bore_backed_position_sketches(
        &ctx,
        &mut [model_hole()],
        &mut Vec::new(),
        &mut Vec::new(),
        &[],
        &[native_history()],
        &[lane],
    )
    .unwrap();
}
