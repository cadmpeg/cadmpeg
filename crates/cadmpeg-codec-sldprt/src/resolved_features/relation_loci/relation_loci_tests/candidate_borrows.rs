use super::super::{
    dynamic_marker_has_arc, dynamic_marker_line_candidates, dynamic_marker_point_candidates,
    RelationIndex, RelationMarkers,
};
use super::*;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn dynamic_candidates_borrow_duplicate_and_rejected_identities() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let other = SketchId::mint("synthetic:test:id#other").unwrap();
    let long_id = |name: &str| {
        SketchEntityId::mint(format!("synthetic:test:id#{name}{}", "x".repeat(65536))).unwrap()
    };
    let entities = [
        SketchEntity::new(
            long_id("line"),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        )
        .with_native_ref(Some("root".into()))
        .with_geometry_ref(Some("root".into())),
        SketchEntity::new(
            long_id("point"),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        )
        .with_native_ref(Some("point".into()))
        .with_geometry_ref(Some("point".into())),
        SketchEntity::new(
            long_id("rejected"),
            other,
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        )
        .with_native_ref(Some("point".into())),
    ];
    let loci = HashMap::from([
        (
            "root".into(),
            vec![SketchLocus::Entity(entities[0].id().clone())],
        ),
        (
            "point".into(),
            vec![SketchLocus::Entity(entities[1].id().clone())],
        ),
    ]);
    let profile = profile(&entities);
    let marker_map = HashMap::new();
    let markers =
        RelationMarkers::from_map(&cadmpeg_test_support::service_decode_context(), &marker_map)
            .unwrap();
    let index = RelationIndex {
        entities: &profile,
        markers: &markers,
        loci_by_marker: &loci,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (lines, line_storage) = ctx
        .with_scoped_storage("test line candidates", || {
            dynamic_marker_line_candidates(&ctx, "root", index)
        })
        .unwrap();
    let (points, point_storage) = ctx
        .with_scoped_storage("test point candidates", || {
            dynamic_marker_point_candidates(&ctx, "point", &sketch, index)
        })
        .unwrap();
    assert_eq!(lines, [entities[0].id()]);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].entity, entities[1].id());
    assert_eq!(points[0].role.ordinal(), 0);
    drop(points);
    drop(point_storage);
    drop(lines);
    drop(line_storage);
    ctx.finish_session().unwrap();
}

#[test]
fn dynamic_arc_presence_needs_no_center_geometry_or_identity_copy() {
    let mut root = marker("root", 0, 0, SketchInputKind::LineOrCircle, None);
    root.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 0,
            entity_ref: "arc".into(),
        }],
    );
    let arc = marker("arc", 1, 40, SketchInputKind::Arc, None);
    let marker_map = HashMap::from([(root.id(), &root), (arc.id(), &arc)]);
    let markers =
        RelationMarkers::from_map(&cadmpeg_test_support::service_decode_context(), &marker_map)
            .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let operation = "resolve SLDPRT linked point locus";
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert!(dynamic_marker_has_arc(&ctx, "root", &markers).unwrap());
    let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
        panic!("presence must not resolve center geometry");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn dynamic_angle_skips_center_presence() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    for operand in &mut relation.operands {
        operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap());
    }
    let entities = [
        line_entity(
            "synthetic:test:id#first",
            &sketch,
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .with_geometry_ref(Some("feature:solver-line:0".into())),
        line_entity(
            "synthetic:test:id#second",
            &sketch,
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
        )
        .with_geometry_ref(Some("feature:solver-line:1".into())),
    ];
    let mut root =
        marker("root", 0, 0, SketchInputKind::LineOrCircle, None).with_test_identity(Some(0), None);
    root.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 0,
            entity_ref: "arc".into(),
        }],
    );
    let arc = marker("arc", 1, 40, SketchInputKind::Arc, None);
    let marker_map = HashMap::from([(root.id(), &root), (arc.id(), &arc)]);
    let mut parameter = length_parameter(7.0);
    parameter.value = Some(ParameterValue::Angle(
        Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
    ));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let operation = "find SLDPRT dynamic arc marker";
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert_eq!(
        typed_relation_definition(
            &ctx,
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &marker_map,
            &HashMap::new()
        )
        .unwrap(),
        Some(SketchConstraintDefinitionInput::Angle {
            first: entities[0].id().clone(),
            second: entities[1].id().clone(),
            parameter: parameter.id.clone()
        })
    );
    let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
        panic!("angle must not inspect center presence");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn dynamic_point_fallback_consumes_cached_marker_results() {
    use super::super::{
        relation_operand_marker_in, unique_dynamic_direct_point_roster_pair,
        unique_dynamic_marker_point_pair,
    };
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 1]);
    for operand in &mut relation.operands {
        operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap());
    }
    let markers = [
        marker("first", 0, 0, SketchInputKind::Point, Some([0.0, 0.0]))
            .with_test_identity(Some(0), None),
        marker("second", 1, 40, SketchInputKind::Point, Some([0.008, 0.0]))
            .with_test_identity(Some(1), None),
    ];
    let entities = [
        ("first", "first", 0.0),
        ("wrong", "second", 8.0),
        ("target", "second", 7.0),
    ]
    .map(|(name, reference, u)| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            })
            .unwrap(),
        )
        .with_native_ref(Some(reference.into()))
    });
    let marker_map = markers.iter().map(|marker| (marker.id(), marker)).collect();
    let profile = profile(&entities);
    let markers =
        RelationMarkers::from_map(&cadmpeg_test_support::service_decode_context(), &marker_map)
            .unwrap();
    let loci = HashMap::new();
    let index = RelationIndex {
        entities: &profile,
        markers: &markers,
        loci_by_marker: &loci,
    };
    for direct in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let cached = [0, 1].map(|operand| {
            relation_operand_marker_in(&ctx, &relation, operand, &sketch, &markers).unwrap()
        });
        let operation = "collect SLDPRT relation operand markers";
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let known = [
            SketchLocus::Entity(entities[0].id().clone()),
            SketchLocus::Entity(entities[1].id().clone()),
        ];
        let selected = if direct {
            unique_dynamic_direct_point_roster_pair(
                &ctx,
                &relation,
                &sketch,
                &length_parameter(7.0),
                index,
                &|operand| Ok(cached[operand]),
                None,
            )
        } else {
            unique_dynamic_marker_point_pair(
                &ctx,
                &sketch,
                &length_parameter(7.0),
                (Some(&known[0]), Some(&known[1])),
                index,
                &|operand| Ok(cached[operand]),
                |first, second| Some((second.u - first.u).hypot(second.v - first.v)),
            )
        }
        .unwrap();
        assert_eq!(
            selected,
            Some((
                SketchLocus::Entity(entities[0].id().clone()),
                SketchLocus::Entity(entities[2].id().clone())
            ))
        );
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
            panic!("fallback must consume cached markers");
        };
        assert_eq!(limit.additional, 1);
    }
}

#[test]
fn dynamic_arc_presence_stops_before_unused_link_tail() {
    let tail = "unreachable".repeat(4096);
    let mut root = marker("root", 0, 0, SketchInputKind::LineOrCircle, None);
    root.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 0,
                entity_ref: "arc".into(),
            },
            SketchInputLink {
                local_id: 0,
                entity_ref: tail.clone(),
            },
        ],
    );
    let arc = marker("arc", 1, 40, SketchInputKind::Arc, None);
    let marker_map = HashMap::from([(root.id(), &root), (arc.id(), &arc)]);
    let markers =
        RelationMarkers::from_map(&cadmpeg_test_support::service_decode_context(), &marker_map)
            .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let operation = "find SLDPRT dynamic arc marker";
    let size = cadmpeg_core::decode::u64_from_index(tail.len());
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(size));
    assert!(dynamic_marker_has_arc(&ctx, "root", &markers).unwrap());
    let CodecError::ResourceLimit(limit) = ctx.charge_work(size, operation).unwrap_err() else {
        panic!("presence must stop at the first reachable Arc");
    };
    assert_eq!(limit.additional, size);
}

#[test]
fn solver_candidates_validate_the_first_record_with_the_selected_identity() {
    use super::super::dynamic_line_operand_candidates;
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let other = SketchId::mint("synthetic:test:id#other").unwrap();
    let entities = [
        line_entity(
            "synthetic:test:id#line",
            &other,
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
        line_entity(
            "synthetic:test:id#line",
            &sketch,
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
    ];
    let profile = profile(&entities);
    let marker_map = HashMap::new();
    let markers =
        RelationMarkers::from_map(&cadmpeg_test_support::service_decode_context(), &marker_map)
            .unwrap();
    let loci = HashMap::new();
    let relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let selected = dynamic_line_operand_candidates(
        &ctx,
        &relation,
        0,
        &sketch,
        RelationIndex {
            entities: &profile,
            markers: &markers,
            loci_by_marker: &loci,
        },
        &|_| panic!("a solver candidate needs no marker lookup"),
        &|_| Ok(Some(entities[1].id())),
    )
    .unwrap();
    assert!(selected.is_empty());
}

#[test]
fn dynamic_line_selection_skips_the_second_roster_when_the_first_is_empty() {
    use super::super::{select_dynamic_line_pair, LinePairOrdering};
    let profile = profile(&[]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let selected = select_dynamic_line_pair(
        &ctx,
        |operand| {
            assert_eq!(operand, 0, "an empty first roster decides the pair");
            Ok(Vec::new())
        },
        &profile,
        LinePairOrdering::Operand,
        |_, _| panic!("no pair has geometry to measure"),
    )
    .unwrap();
    assert!(selected.is_none());
}
