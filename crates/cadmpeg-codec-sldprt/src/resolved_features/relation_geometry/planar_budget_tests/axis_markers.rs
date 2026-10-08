use super::{assert_owned_loci_refusal, planar_fixture, project_relation_bindings};
use crate::records::{
    SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks, SketchRelationKind,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntity, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchLocus, SketchSameCoordinate,
};

fn project_nested_axis_markers_with_policy(policy: &DecodePolicy) -> Result<(), CodecError> {
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances.clear();
    lane.sketch_entities.clear();
    let mut root = SketchInputEntity::new(
        "axis-root",
        "lane",
        0,
        0,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    root.feature_ref = Some("feature".into());
    root.links = SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 1,
            entity_ref: "axis-child".into(),
        }],
    );
    let mut child = SketchInputEntity::new(
        "axis-child",
        "lane",
        1,
        1,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    child.feature_ref = Some("feature".into());
    child.links = SketchInputLinks::new(
        0,
        (0u16..2)
            .map(|index| SketchInputLink {
                local_id: index + 2,
                entity_ref: format!("axis-point-{index}"),
            })
            .collect(),
    );
    lane.sketch_entities.extend([root, child]);
    let entities: Vec<_> = (0u32..2)
        .map(|index| {
            let marker_id = format!("axis-point-{index}");
            let mut marker = SketchInputEntity::new(
                marker_id.clone(),
                "lane",
                index + 2,
                u64::from(index) + 2,
                SketchInputKind::Point,
            );
            marker.feature_ref = Some("feature".into());
            marker.coordinates_m =
                cadmpeg_ir::units::FiniteVector::new([f64::from(index) * 0.002, 0.0]);
            lane.sketch_entities.push(marker);
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#axis-point-{index}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: cadmpeg_ir::math::Point2::new(f64::from(index) * 2.0, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some(marker_id))
        })
        .collect();
    let expected = SketchConstraintDefinitionInput::SameCoordinate {
        relation: SketchSameCoordinate::try_new(
            SketchLocus::Entity(entities[0].id().clone()),
            SketchLocus::Entity(entities[1].id().clone()),
            SketchCoordinateAxis::V,
        )
        .unwrap(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut constraints = Vec::new();
    project_relation_bindings(
        &ctx,
        &mut constraints,
        &[sketch],
        &[feature],
        &entities,
        &[],
        &[lane],
    )?;
    let root = constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some("axis-root"))
        .unwrap();
    assert_eq!(root.definition.kind(), &expected);
    Ok(())
}

#[test]
fn planar_nested_axis_markers_refuse_collection_limit() {
    assert_owned_loci_refusal(
        ResourceDimension::CollectionItems,
        project_nested_axis_markers_with_policy,
    );
}
#[test]
fn planar_nested_axis_markers_refuse_retained_limit() {
    assert_owned_loci_refusal(
        ResourceDimension::RetainedBytes,
        project_nested_axis_markers_with_policy,
    );
}
#[test]
fn planar_nested_axis_markers_refuse_work_limit() {
    assert_owned_loci_refusal(
        ResourceDimension::WorkUnits,
        project_nested_axis_markers_with_policy,
    );
}
#[test]
fn planar_nested_axis_markers_refuse_nesting_limit() {
    project_nested_axis_markers_with_policy(&DecodePolicy::service()).unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RecursionDepth,
        "traverse SLDPRT axis relation point loci",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = cap;
            project_nested_axis_markers_with_policy(&policy)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let error = project_nested_axis_markers_with_policy(&policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.operation == "traverse SLDPRT axis relation point loci"
            && limit.used == 1 && limit.additional == 1));
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RecursionDepth,
        "resolve SLDPRT linked point locus",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = cap;
            project_nested_axis_markers_with_policy(&policy)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.used == 2 && limit.additional == 1));
    // Root and child axis traversal remain live while the point locus is resolved.
    policy.limits.max_recursion_depth = 3;
    project_nested_axis_markers_with_policy(&policy).unwrap();
}
