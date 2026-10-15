use super::{dynamic_relation, length_parameter, typed_relation_definition};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{FeatureInputOperandKind, FeatureInputRelationFamily};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus,
};
use std::collections::HashMap;

fn point_definition_retains_only_published_identities(tag: NativeOperandTag) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 1]);
    for operand in &mut relation.operands {
        operand.kind = FeatureInputOperandKind::Native(tag);
    }
    let parameter = length_parameter(7.0);
    let entities = [
        ("synthetic:test:id#first", 0, 0.0),
        ("synthetic:test:id#second", 1, 7.0),
    ]
    .map(|(id, operand, u)| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            })
            .unwrap(),
        )
        .with_geometry_ref(Some(format!("relation:operand:{operand}")))
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes = u64_from_index(
        parameter.id.as_str().len()
            + entities[0].id().as_str().len()
            + entities[1].id().as_str().len(),
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let definition = typed_relation_definition(
        &ctx,
        &relation,
        Some(&parameter),
        &sketch,
        &entities,
        &HashMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(
        definition,
        Some(SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Entity(entities[0].id().clone()),
            second: SketchLocus::Entity(entities[1].id().clone()),
            parameter: parameter.id.clone(),
        })
    );
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse solver definition scratch",
    )
    .unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn solver_point_definition_retains_only_published_identities() {
    point_definition_retains_only_published_identities(NativeOperandTag::TAG_8100);
}

#[test]
fn dynamic_point_definition_retains_only_published_identities() {
    point_definition_retains_only_published_identities(NativeOperandTag::try_from(0x812a).unwrap());
}

#[test]
fn unresolved_circle_definition_does_not_copy_parameter_identity() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let relation = dynamic_relation(FeatureInputRelationFamily::CircleDiameter, [0, 1]);
    let mut parameter = length_parameter(7.0);
    parameter.display = Some(cadmpeg_ir::features::DimensionDisplay::Diameter);
    let entities = [SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#point").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    )
    .with_geometry_ref(Some(relation.id.clone()))];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let operation = "retain SLDPRT relation parameter identity";
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert!(typed_relation_definition(
        &ctx,
        &relation,
        Some(&parameter),
        &sketch,
        &entities,
        &HashMap::new(),
        &HashMap::new()
    )
    .unwrap()
    .is_none());
    let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
        panic!("unused parameter copy must not run");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);
}

#[test]
fn line_distance_output_slots_are_admitted() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut relation = dynamic_relation(FeatureInputRelationFamily::LineLineDistance, [0, 1]);
    for operand in &mut relation.operands {
        operand.kind = FeatureInputOperandKind::E1;
    }
    let parameter = length_parameter(1.0);
    let entities = [0.0, 1.0].map(|v| {
        super::line_entity(
            if v == 0.0 {
                "synthetic:test:id#first"
            } else {
                "synthetic:test:id#second"
            },
            &sketch,
            Point2::new(0.0, v),
            Point2::new(2.0, v),
        )
    });
    let entities = entities
        .into_iter()
        .enumerate()
        .map(|(i, entity)| entity.with_geometry_ref(Some(format!("feature:solver-line:{i}"))))
        .collect::<Vec<_>>();
    let run = |ctx: &DecodeContext<'_>| {
        typed_relation_definition(
            ctx,
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &HashMap::new(),
            &HashMap::new(),
        )
    };
    assert_eq!(
        run(&cadmpeg_test_support::service_decode_context()).unwrap(),
        Some(SketchConstraintDefinitionInput::Distance {
            entities: entities.iter().map(|entity| entity.id().clone()).collect(),
            parameter: parameter.id.clone(),
        })
    );
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::RetainedBytes,
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            dimension,
            "retain SLDPRT line-distance entity slots",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                    _ => unreachable!(),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                run(&ctx)
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("output slots must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(
            limit.additional,
            if dimension == ResourceDimension::CollectionItems {
                2
            } else {
                2 * u64_from_index(std::mem::size_of::<SketchEntityId>())
            }
        );
    }
}

#[test]
fn dynamic_marker_pair_keeps_only_selected_identities() {
    use super::super::{
        unique_dynamic_marker_point_pair, ProfileEntities, RelationIndex, RelationMarkers,
    };
    use crate::records::{SketchInputEntity, SketchInputKind};
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entities = [
        ("synthetic:test:id#first", 0.0),
        ("synthetic:test:id#wrong", 8.0),
        ("synthetic:test:id#target", 7.0),
    ]
    .map(|(id, u)| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            })
            .unwrap(),
        )
    });
    let mut marker = SketchInputEntity::new("second-marker", "lane", 0, 0, SketchInputKind::Point);
    marker.feature_ref = Some("feature".into());
    let marker_map = HashMap::from([(marker.id(), &marker)]);
    let loci = HashMap::from([(
        marker.id().into(),
        vec![SketchLocus::Entity(entities[2].id().clone())],
    )]);
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 1]);
    relation.operands[1].entity_ref = Some(marker.id().into());
    let known = (
        SketchLocus::Entity(entities[0].id().clone()),
        SketchLocus::Entity(entities[1].id().clone()),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes =
        u64_from_index(entities[0].id().as_str().len() + entities[2].id().as_str().len());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let selected = {
        let (profile, _profile_storage) = ctx
            .with_scoped_storage("test profile index", || {
                ProfileEntities::new(&ctx, &entities)
            })
            .unwrap();
        let (markers, _marker_storage) = ctx
            .with_scoped_storage("test marker index", || {
                RelationMarkers::from_map(&ctx, &marker_map)
            })
            .unwrap();
        let cached = [0, 1].map(|operand| {
            super::super::relation_operand_marker_in(&ctx, &relation, operand, &sketch, &markers)
                .unwrap()
        });
        unique_dynamic_marker_point_pair(
            &ctx,
            &sketch,
            &length_parameter(7.0),
            (Some(&known.0), Some(&known.1)),
            RelationIndex {
                entities: &profile,
                markers: &markers,
                loci_by_marker: &loci,
            },
            &|operand| Ok(cached[operand]),
            |first, second| Some((second.u - first.u).hypot(second.v - first.v)),
        )
        .unwrap()
    };
    assert_eq!(
        selected,
        Some((
            SketchLocus::Entity(entities[0].id().clone()),
            SketchLocus::Entity(entities[2].id().clone())
        ))
    );
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse dynamic marker candidate workspace",
    )
    .unwrap();
    ctx.finish_session().unwrap();
}

fn dynamic_direct_roster_storage(duplicate_identity: bool) {
    use super::super::{
        unique_dynamic_direct_point_roster_pair, ProfileEntities, RelationIndex, RelationMarkers,
    };
    use crate::records::SketchInputKind;
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let inputs = [("first", 0.0), ("wrong", 30.0), ("target", 10.0)];
    let markers = inputs
        .map(|(id, u)| super::marker(id, 0, 0, SketchInputKind::Point, Some([u / 1000.0, 0.0])));
    let mut entities = inputs
        .map(|(id, u)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#{id}")).unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(u, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some(id.into()))
        })
        .to_vec();
    if duplicate_identity {
        entities.push(
            SketchEntity::new(
                entities[0].id().clone(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(10.0, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some("first".into())),
        );
    }
    let marker_map = markers.iter().map(|marker| (marker.id(), marker)).collect();
    let relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 1]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes =
        u64_from_index(entities[0].id().as_str().len() + entities[2].id().as_str().len());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let selected = {
        let (profile, _profile_storage) = ctx
            .with_scoped_storage("test direct profile index", || {
                ProfileEntities::new(&ctx, &entities)
            })
            .unwrap();
        let (markers, _marker_storage) = ctx
            .with_scoped_storage("test marker index", || {
                RelationMarkers::from_map(&ctx, &marker_map)
            })
            .unwrap();
        let cached = [0, 1].map(|operand| {
            super::super::relation_operand_marker_in(&ctx, &relation, operand, &sketch, &markers)
                .unwrap()
        });
        unique_dynamic_direct_point_roster_pair(
            &ctx,
            &relation,
            &sketch,
            &length_parameter(10.0),
            RelationIndex {
                entities: &profile,
                markers: &markers,
                loci_by_marker: &HashMap::new(),
            },
            &|operand| Ok(cached[operand]),
            None,
        )
        .unwrap()
    };
    assert_eq!(
        selected,
        Some((
            SketchLocus::Entity(entities[0].id().clone()),
            SketchLocus::Entity(entities[2].id().clone())
        ))
    );
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse dynamic direct roster workspace",
    )
    .unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn dynamic_direct_roster_keeps_only_selected_identities() {
    dynamic_direct_roster_storage(false);
}

#[test]
fn dynamic_direct_roster_preserves_first_duplicate_identity() {
    dynamic_direct_roster_storage(true);
}

fn dynamic_curve_definition_storage(family: FeatureInputRelationFamily) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let relation = dynamic_relation(family, [0, 1]);
    let mut parameter = length_parameter(1.0);
    if family == FeatureInputRelationFamily::Angle {
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        ));
    }
    let first = if family == FeatureInputRelationFamily::PointLineDistance {
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#first").unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        )
        .with_geometry_ref(Some("relation:operand:0".into()))
    } else {
        super::line_entity(
            "synthetic:test:id#first",
            &sketch,
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
        )
        .with_geometry_ref(Some("feature:solver-line:0".into()))
    };
    let second = super::line_entity(
        "synthetic:test:id#second",
        &sketch,
        if family == FeatureInputRelationFamily::Angle {
            Point2::new(0.0, 0.0)
        } else {
            Point2::new(0.0, 1.0)
        },
        if family == FeatureInputRelationFamily::Angle {
            Point2::new(0.0, 2.0)
        } else {
            Point2::new(2.0, 1.0)
        },
    )
    .with_geometry_ref(Some("feature:solver-line:1".into()));
    let slots = if family == FeatureInputRelationFamily::LineLineDistance {
        2 * std::mem::size_of::<SketchEntityId>()
    } else {
        0
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes = u64_from_index(
        parameter.id.as_str().len()
            + first.id().as_str().len()
            + second.id().as_str().len()
            + slots,
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let expected = match family {
        FeatureInputRelationFamily::PointLineDistance => {
            SketchConstraintDefinitionInput::DistanceLoci {
                first: SketchLocus::Entity(first.id().clone()),
                second: SketchLocus::Entity(second.id().clone()),
                parameter: parameter.id.clone(),
            }
        }
        FeatureInputRelationFamily::LineLineDistance => SketchConstraintDefinitionInput::Distance {
            entities: vec![first.id().clone(), second.id().clone()],
            parameter: parameter.id.clone(),
        },
        FeatureInputRelationFamily::Angle => SketchConstraintDefinitionInput::Angle {
            first: first.id().clone(),
            second: second.id().clone(),
            parameter: parameter.id.clone(),
        },
        _ => unreachable!(),
    };
    assert_eq!(
        typed_relation_definition(
            &ctx,
            &relation,
            Some(&parameter),
            &sketch,
            &[first, second],
            &HashMap::new(),
            &HashMap::new()
        )
        .unwrap(),
        Some(expected)
    );
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse dynamic curve definition workspace",
    )
    .unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn dynamic_point_line_definition_retains_only_published_identities() {
    dynamic_curve_definition_storage(FeatureInputRelationFamily::PointLineDistance);
}

#[test]
fn dynamic_line_distance_definition_retains_only_published_identities() {
    dynamic_curve_definition_storage(FeatureInputRelationFamily::LineLineDistance);
}

#[test]
fn dynamic_angle_definition_retains_only_published_identities() {
    dynamic_curve_definition_storage(FeatureInputRelationFamily::Angle);
}

#[test]
fn compact_operand_marker_candidates_are_scratch() {
    use super::super::{relation_operand_marker_in, RelationMarkers};
    use crate::records::SketchInputKind;
    let sketch = SketchId::mint("synthetic:test:sketch#compact:test").unwrap();
    let markers = [("last", 30), ("first", 10), ("middle", 20)]
        .map(|(id, offset)| super::marker(id, 0, offset, SketchInputKind::Point, Some([0.0, 0.0])));
    let marker_map = markers.iter().map(|marker| (marker.id(), marker)).collect();
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 1]);
    relation.operands[0].kind = FeatureInputOperandKind::D6;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let selected = {
        let (index, _index_storage) = ctx
            .with_scoped_storage("test compact marker index", || {
                RelationMarkers::from_map(&ctx, &marker_map)
            })
            .unwrap();
        relation_operand_marker_in(&ctx, &relation, 0, &sketch, &index).unwrap()
    };
    assert_eq!(selected, Some(markers[1].id()));
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse compact operand marker workspace",
    )
    .unwrap();
    ctx.finish_session().unwrap();
}
