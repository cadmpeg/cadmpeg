// SPDX-License-Identifier: Apache-2.0
//! Inventor sketch graph projection tests.

use super::*;

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).expect("finite test scalar")
}

#[test]
fn profile_builder_refuses_collection_limit_before_source_index() {
    let entity = circle_profile_entity();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
    assert!(matches!(
        super::super::build_profiles(&ctx, &[&entity]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "index Inventor profile source positions"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service profile context");
    assert_eq!(
        super::super::build_profiles(&ctx, &[&entity])
            .expect("circle profile")
            .len(),
        1
    );
}

fn circle_profile_entity() -> cadmpeg_ir::sketches::SketchEntity {
    cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1").expect("entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Circle {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: cadmpeg_ir::scalar::Length::new(1.0).expect("positive radius"),
            },
        )
        .expect("circle geometry"),
    )
}

#[test]
fn profile_builder_refuses_collection_limits_before_circular_profile_allocations() {
    let entity = circle_profile_entity();
    let arena = DecodeArena::new();
    // The count includes the source-position index, circular use, and outer profile slot.
    for (cap, operation) in [
        (1, "project Inventor circular profile use"),
        (2, "collect Inventor sketch items"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
        assert!(matches!(
            super::super::build_profiles(&ctx, &[&entity]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service profile context");
    assert_eq!(
        super::super::build_profiles(&ctx, &[&entity])
            .expect("circle profile")
            .len(),
        1
    );
}

#[test]
fn short_closed_line_components_skip_profile_entity_id_copies() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id");
    let line = |id| {
        cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint(id).expect("entity id"),
            sketch.clone(),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                    start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                },
            )
            .expect("line geometry"),
        )
        .with_endpoint_refs(vec!["point-a".into(), "point-b".into()])
    };
    let first = line("inventor:test:entity#1");
    let second = line("inventor:test:entity#2");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "retain Inventor profile use id",
        None,
    );
    let profiles = super::super::build_profiles(&ctx, &[&first, &second])
        .expect("two-edge closed component is not a profile");
    assert!(profiles.is_empty());
    ctx.finish_session()
        .expect("short component needs no profile ID copies");
}

#[test]
fn open_line_component_admits_only_the_next_member() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id");
    let lines = [
        ("inventor:test:entity#1", "point-a", "point-b"),
        ("inventor:test:entity#2", "point-b", "point-c"),
    ]
    .map(|(id, start, end)| {
        cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint(id).expect("entity id"),
            sketch.clone(),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                    start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                },
            )
            .expect("line geometry"),
        )
        .with_endpoint_refs(vec![start.into(), end.into()])
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    // The component has two members. The first wrong-degree endpoint needs
    // one member visit; the second member is not prepaid by this check.
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "visit Inventor line component",
        Some(1),
    );
    let error = super::super::build_profiles(&ctx, &lines.each_ref())
        .expect_err("the first component member is admitted separately");
    drop(probe);
    assert!(matches!(&error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "visit Inventor line component"
            && limit.additional == 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
        if matches!(&error, CodecError::ResourceLimit(original) if original == &limit)));

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(super::super::build_profiles(&ctx, &lines.each_ref())
        .expect("open component has no profile")
        .is_empty());
}

#[test]
fn closed_line_profile_preserves_promotion_and_original_refusal() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id");
    let lines = [
        ("inventor:test:entity#1", "point-a", "point-b"),
        ("inventor:test:entity#2", "point-b", "point-c"),
        ("inventor:test:entity#3", "point-c", "point-a"),
    ]
    .map(|(id, start, end)| {
        cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint(id).expect("entity id"),
            sketch.clone(),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                    start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                },
            )
            .expect("line geometry"),
        )
        .with_endpoint_refs(vec![start.into(), end.into()])
    });
    let references = lines.each_ref();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("profile context");
    let profiles = super::super::build_profiles(&ctx, &references).expect("closed profile");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 3);
    for (index, entity_use) in profiles[0].iter().enumerate() {
        assert_eq!(entity_use.entity, *lines[index].id());
        assert!(!entity_use.reversed);
    }
    ctx.finish_session().expect("successful profile session");

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "collect Inventor profile use",
        None,
    );
    let CodecError::ResourceLimit(original) =
        super::super::build_profiles(&ctx, &references).expect_err("promotion refusal")
    else {
        panic!("original resource refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(original.operation, "collect Inventor profile use");
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(found)) if found == original));
}

#[test]
fn line_component_expands_shared_endpoint_neighbours_once() {
    let entity = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#2").expect("entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
        )
        .expect("line geometry"),
    )
    .with_endpoint_refs(vec!["point-a".into(), "point-b".into()]);
    for count in [1_usize, 256] {
        let lines = vec![&entity; count];
        let neighbours = (0..count).collect::<Vec<_>>();
        let adjacency = std::collections::HashMap::from([
            ("point-a", neighbours.clone()),
            ("point-b", neighbours),
        ]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One seed, n component members, two endpoint keys and 2*n queued neighbours.
        let slots = cadmpeg_core::decode::u64_from_index(3 * count + 3);
        policy.limits.max_collection_items = slots;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (component, storage) = super::super::line_component(&ctx, 0, &lines, &adjacency)
            .expect("linear neighbour expansion");
        assert_eq!(
            component.iter().copied().collect::<Vec<_>>(),
            (0..count).collect::<Vec<_>>()
        );
        drop((component, storage));
        assert!(matches!(ctx.charge_collection_items(1, "probe"),
            Err(CodecError::ResourceLimit(limit)) if limit.used == slots));
        // Resource refusals are sticky; use a fresh context for the storage probe.
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (component, storage) = super::super::line_component(&ctx, 0, &lines, &adjacency)
            .expect("linear neighbour expansion");
        drop((component, storage));
        assert!(
            matches!(ctx.reserve_scoped(u64::MAX, "released component storage"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == 0)
        );
    }
}

fn empty_projectable_inventory() -> SketchInventory {
    let mut transform = parse(
        &{
            let mut bytes = content(0);
            bytes.extend_from_slice(&0x8421_u16.to_le_bytes());
            bytes.extend_from_slice(&0x7bde_u16.to_le_bytes());
            bytes
        },
        |_ctx, source| parse_transform(source, 22).expect("transform"),
    );
    transform.header.source_index = 0;
    let direction = parse(
        &{
            let mut bytes = content(1);
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&0.0_f64.to_le_bytes());
            bytes.extend_from_slice(&0.0_f64.to_le_bytes());
            bytes.extend_from_slice(&0.0_f64.to_le_bytes());
            bytes.extend_from_slice(&1.0_f64.to_le_bytes());
            bytes
        },
        |_ctx, source| parse_direction(source, 22).expect("direction"),
    );
    let mut sketch_bytes = content(2);
    sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
    sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
    sketch_bytes.extend(list(8, &[]));
    sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&2_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&[0; 8]);
    sketch_bytes.extend(list(2, &[]));
    let sketch = parse(&sketch_bytes, |ctx, source| {
        parse_sketch(ctx, source, 22).expect("sketch")
    });
    SketchInventory {
        sketches: vec![Located::new(
            sketch,
            fixture_record_type_id(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            2,
        )],
        entities: Vec::new(),
        transforms: vec![Located::new(
            transform,
            fixture_record_type_id(TRANSFORM_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        )],
        directions: vec![Located::new(
            direction,
            fixture_record_type_id(DIRECTION_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        constraints: Vec::new(),
        issues: Vec::new(),
    }
}

fn horizontal_constraint_fixture(index: u32, reference: u32) -> super::super::PmDcSketchConstraint {
    let mut bytes = constraint_header(index, 0);
    bytes.extend_from_slice(&reference.to_le_bytes());
    bytes.push(0);
    let payload = parse(&bytes, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22)
            .expect("horizontal constraint")
    });
    Located::new(
        payload,
        fixture_record_type_id(HORIZONTAL_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        index,
    )
}

#[test]
fn unprojectable_sketches_skip_constraint_native_ids() {
    let mut inventory = empty_projectable_inventory();
    inventory.directions.clear();
    let mut constraint = horizontal_constraint_fixture(3, 0);
    constraint.identity.segment_token =
        cadmpeg_ir::ids::IdentityKey::try_new("x".repeat(4097)).expect("key");
    inventory.constraints.push(constraint);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Three borrowed raw-record tables fit; the unused native key alone exceeds this limit.
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projection =
        project(&ctx, &inventory, &[]).expect("rejected sketches need no native index");
    assert_eq!(projection.unresolved_sketches, 1);
    assert_eq!(projection.unresolved_constraints, 1);
    assert!(projection.sketches.is_empty());
    assert!(projection.entities.is_empty());
    assert!(projection.constraints.is_empty());
    assert!(matches!(ctx.reserve_scoped(u64::MAX, "probe"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == 0));
}

#[test]
fn orphan_entities_skip_projected_native_lookup_strings() {
    let mut inventory = empty_projectable_inventory();
    let point = parse(&point_bytes(3, 0, [0.0, 0.0]), |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
    });
    inventory.entities.push(Located::new(
        point,
        fixture_record_type_id(POINT_TYPE),
        cadmpeg_ir::ids::IdentityKey::try_new("x".repeat(4097)).expect("key"),
        3,
    ));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // A borrowed key needs no string storage; its unused formatted copy would exceed the limit.
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projection = project(&ctx, &inventory, &[]).expect("no projected entity lookup keys");
    assert_eq!(projection.sketches.len(), 1);
    assert_eq!(projection.unresolved_entities, 1);
    assert!(projection.entities.is_empty());
    assert!(projection.constraints.is_empty());
}

#[test]
fn sketch_projection_without_constraints_skips_native_constraint_indexes() {
    for has_raw_constraint in [false, true] {
        let mut inventory = empty_projectable_inventory();
        if has_raw_constraint {
            inventory
                .constraints
                .push(horizontal_constraint_fixture(3, 0));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Three raw-record entries, one output sketch, one projected-ID entry,
        // one raw native-sketch entry and one closed-ID entry: seven slots.
        // A raw constraint adds one; no projected constraints need the final native tables.
        let slots = 7 + u64::from(has_raw_constraint);
        policy.limits.max_collection_items = slots;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let projection = project(&ctx, &inventory, &[]).expect("only read indexes are stored");
        assert_eq!(projection.sketches.len(), 1);
        assert_eq!(
            projection.unresolved_constraints,
            usize::from(has_raw_constraint)
        );
        assert!(projection.entities.is_empty());
        assert!(projection.constraints.is_empty());
        assert!(matches!(ctx.charge_collection_items(1, "probe"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems && limit.used == slots));
    }
}

#[test]
fn unprojectable_sketches_skip_parameter_index() {
    let parameters = [cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("inventor:design:parameter#segment-0")
            .expect("parameter id"),
        owner: None,
        ordinal: 0,
        name: "p".into(),
        expression: String::new(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: Some(format!("inventor:pmdc:parameter#{}-0", "x".repeat(4097))),
    }];
    let mut inventory = empty_projectable_inventory();
    inventory.directions.clear();
    inventory
        .constraints
        .push(horizontal_constraint_fixture(3, 0));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The unused parameter key exceeds the whole work budget; rejected sketches do not hash it.
    policy.limits.max_work_units = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projection = project(&ctx, &inventory, &parameters).expect("no unused parameter index");
    assert_eq!(projection.unresolved_sketches, 1);
    assert_eq!(projection.unresolved_constraints, 1);
    assert!(projection.sketches.is_empty());
    assert!(projection.entities.is_empty());
    assert!(projection.constraints.is_empty());
}

#[test]
fn rejected_closed_sketches_skip_constraint_native_indexes() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    for closed in [true, false] {
        let mut inventory = empty_projectable_inventory();
        let point = parse(&point_bytes(3, 3, [0.0, 0.0]), |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
        });
        inventory.entities.push(Located::new(
            point,
            fixture_record_type_id(POINT_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            3,
        ));
        inventory
            .constraints
            .push(horizontal_constraint_fixture(5, 4));
        let references = if closed { &[4, 6][..] } else { &[4, 6, 0][..] };
        let mut bytes = content(2);
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(references.len())
                .expect("count")
                .to_le_bytes(),
        );
        bytes.extend(list(8, references));
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend(list(2, &[]));
        let sketch = parse(&bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch")
        });
        inventory.sketches = vec![Located::new(
            sketch,
            fixture_record_type_id(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        )];
        for operation in [
            "index Inventor raw sketch projected ids",
            "index Inventor raw constraint native refs",
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = u64::MAX;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            // Refuse any final native-index entry only in the rejected-sketch case.
            let _probe = (!closed)
                .then(|| RefusalProbe::arm(ResourceDimension::CollectionItems, operation, None));
            let projection = project(&ctx, &inventory, &[]).expect("native indexes need readers");
            let count = usize::from(closed);
            assert_eq!(projection.sketches.len(), count);
            assert_eq!(projection.entities.len(), count);
            assert_eq!(projection.constraints.len(), count);
            assert_eq!(projection.unresolved_sketches, 1 - count);
            assert_eq!(projection.unresolved_entities, 1 - count);
            assert_eq!(projection.unresolved_constraints, 1 - count);
        }
    }
}

#[test]
fn sketch_parameter_index_borrows_unselected_values() {
    let parameters = [cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("inventor:design:parameter#segment-0")
            .expect("parameter id"),
        owner: None,
        ordinal: 0,
        name: "p".into(),
        expression: String::new(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: Some("inventor:pmdc:parameter#segment-0".into()),
    }];
    let mut inventory = empty_projectable_inventory();
    let mut bytes = constraint_header(3, 0);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(0);
    let constraint = parse(&bytes, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22).expect("constraint")
    });
    inventory.constraints.push(Located::new(
        constraint,
        fixture_record_type_id(HORIZONTAL_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        3,
    ));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    // The index borrows text and IDs. The first retained copy is the output sketch ID.
    assert!(matches!(project(&ctx, &inventory, &parameters),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.operation == "retain projected Inventor sketch_id identity"));
    // Two output identities fill this budget before the returned vector is allocated.
    let text_bytes = cadmpeg_core::decode::u64_from_index(
        "inventor:design:sketch#segment-2".len() + "inventor:pmdc:sketch#segment-2".len(),
    );
    policy.limits.max_retained_bytes = text_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let sketch_bytes = std::mem::size_of::<cadmpeg_ir::sketches::Sketch>();
    assert!((2..=1024).contains(&sketch_bytes));
    // Core amortized growth reserves four slots for an initial element of this size.
    let vector_bytes = cadmpeg_core::decode::u64_from_index(4 * sketch_bytes);
    assert!(matches!(project(&ctx, &inventory, &parameters),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "collect Inventor sketch items"
                && limit.used == text_bytes && limit.additional == vector_bytes));
    // Only output text and returned vector storage are retained. The live
    // parameter index borrows its key and ID after those output copies.
    let retained = text_bytes + vector_bytes;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projection = project(&ctx, &inventory, &parameters).expect("borrowed parameter index");
    assert_eq!(projection.sketches.len(), 1);
    assert_eq!(projection.unresolved_constraints, 1);
    assert!(projection.entities.is_empty());
    assert!(projection.constraints.is_empty());
    assert!(matches!(ctx.charge_retained(1, "probe"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes && limit.used == retained));
}

#[test]
fn sketch_projection_without_constraints_skips_parameter_index() {
    let parameters = [cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("inventor:design:parameter#segment-0")
            .expect("parameter id"),
        owner: None,
        ordinal: 0,
        name: "p".into(),
        expression: String::new(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: Some("inventor:pmdc:parameter#segment-0".into()),
    }];
    let inventory = empty_projectable_inventory();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let mut slots = Vec::new();
    for parameters in [&[][..], &parameters[..]] {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let projection = project(&ctx, &inventory, parameters).expect("empty sketch");
        assert_eq!(projection.sketches.len(), 1);
        assert!(projection.entities.is_empty());
        assert!(projection.constraints.is_empty());
        // No constraints read the parameter index; unused parameters add zero entries.
        match ctx.charge_collection_items(policy.limits.max_collection_items + 1, "probe") {
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems =>
            {
                slots.push(limit.used);
            }
            other => panic!("expected collection probe refusal, got {other:?}"),
        }
    }
    assert_eq!(slots[0], slots[1]);
}

#[test]
fn sketch_projection_without_sketches_skips_parameter_index() {
    let parameters = [cadmpeg_ir::features::DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("inventor:design:parameter#segment-0")
            .expect("parameter id"),
        owner: None,
        ordinal: 0,
        name: "p".into(),
        expression: String::new(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: Some("inventor:pmdc:parameter#segment-0".into()),
    }];
    let inventory = SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projection = project(&ctx, &inventory, &parameters).expect("index holds borrowed values");
    assert!(projection.sketches.is_empty());
    assert!(projection.entities.is_empty());
    assert!(projection.constraints.is_empty());
}

#[test]
fn line_component_refuses_collection_limit_before_queue_creation() {
    let entity = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#2").expect("entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
        )
        .expect("line geometry"),
    )
    .with_endpoint_refs(vec!["point-a".into(), "point-b".into()]);
    let adjacency =
        std::collections::HashMap::from([("point-a", vec![0_usize]), ("point-b", vec![0_usize])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
    assert!(matches!(
        super::super::line_component(&ctx, 0, &[&entity], &adjacency),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "queue Inventor profile line"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service profile context");
    let (component, _storage) =
        super::super::line_component(&ctx, 0, &[&entity], &adjacency).expect("line component");
    assert_eq!(component.len(), 1);
}

#[test]
fn sketch_projection_without_sketches_needs_no_entity_index() {
    let point = parse(&point_bytes(0, 1, [0.0, 0.0]), |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
    });
    let inventory = SketchInventory {
        sketches: Vec::new(),
        entities: vec![Located::new(
            point,
            fixture_record_type_id(POINT_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        )],
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    let projection = project(&ctx, &inventory, &[]).expect("orphan entities need no index");
    assert_eq!(projection.unresolved_entities, 1);
    assert!(projection.sketches.is_empty());
    assert!(projection.entities.is_empty());
    assert!(projection.constraints.is_empty());
    // One orphan entity yields zero output and index entries.
    assert!(matches!(ctx.charge_collection_items(1, "probe"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems && limit.used == 0));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert_eq!(
        project(&ctx, &inventory, &[])
            .expect("sketch projection")
            .unresolved_entities,
        1
    );
}

#[test]
fn sketch_projection_refuses_entity_limit_before_entity_creation() {
    let mut sketch_bytes = content(0);
    sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
    sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
    sketch_bytes.extend(list(8, &[2]));
    sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&[0; 8]);
    sketch_bytes.extend(list(2, &[]));
    let sketch = parse(&sketch_bytes, |ctx, source| {
        parse_sketch(ctx, source, 22).expect("sketch record")
    });
    let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
    });
    let inventory = SketchInventory {
        sketches: vec![Located::new(
            sketch,
            fixture_record_type_id(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        )],
        entities: vec![Located::new(
            point,
            fixture_record_type_id(POINT_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        project(&ctx, &inventory, &[]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "project Inventor sketch entity"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(project(&ctx, &inventory, &[]).is_ok());
}

#[test]
fn sketch_reference_cannot_name_the_maximum_record_ordinal() {
    let mut sketch_bytes = content(0);
    sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
    sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
    sketch_bytes.extend(list(8, &[u32::MAX]));
    sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&[0; 8]);
    sketch_bytes.extend(list(2, &[]));
    let sketch = parse(&sketch_bytes, |ctx, source| {
        parse_sketch(ctx, source, 22).expect("sketch record")
    });
    let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
    });
    let inventory = SketchInventory {
        sketches: vec![Located::new(
            sketch,
            fixture_record_type_id(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        )],
        entities: vec![Located::new(
            point,
            fixture_record_type_id(POINT_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            u32::MAX,
        )],
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let projection = project(&ctx, &inventory, &[]).expect("sketch projection");
    assert_eq!(projection.unresolved_entities, 1);
}

#[test]
fn sketch_projection_refuses_entity_limit_before_sketch_creation() {
    let mut transform = parse(
        &{
            let mut bytes = content(0);
            bytes.extend_from_slice(&0x8421_u16.to_le_bytes());
            bytes.extend_from_slice(&0x7bde_u16.to_le_bytes());
            bytes
        },
        |_ctx, source| parse_transform(source, 22).expect("transform"),
    );
    transform.header.source_index = 0;
    let direction = parse(
        &{
            let mut bytes = content(1);
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&0.0_f64.to_le_bytes());
            bytes.extend_from_slice(&0.0_f64.to_le_bytes());
            bytes.extend_from_slice(&0.0_f64.to_le_bytes());
            bytes.extend_from_slice(&1.0_f64.to_le_bytes());
            bytes
        },
        |_ctx, source| parse_direction(source, 22).expect("direction"),
    );
    let mut sketch_bytes = content(2);
    sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
    sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
    sketch_bytes.extend(list(8, &[]));
    sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&2_u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&[0; 8]);
    sketch_bytes.extend(list(2, &[]));
    let sketch = parse(&sketch_bytes, |ctx, source| {
        parse_sketch(ctx, source, 22).expect("sketch")
    });
    let inventory = SketchInventory {
        sketches: vec![Located::new(
            sketch,
            fixture_record_type_id(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            2,
        )],
        entities: Vec::new(),
        transforms: vec![Located::new(
            transform,
            fixture_record_type_id(TRANSFORM_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        )],
        directions: vec![Located::new(
            direction,
            fixture_record_type_id(DIRECTION_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        project(&ctx, &inventory, &[]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "project Inventor sketch"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert_eq!(
        project(&ctx, &inventory, &[])
            .expect("service sketch projection")
            .sketches
            .len(),
        1
    );
}

#[test]
fn sketch_constraint_projection_refuses_retained_limit_before_entity_copy() {
    let mut bytes = constraint_header(0, 0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    let payload = parse(&bytes, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22)
            .expect("horizontal constraint")
    });
    let constraint = Located::new(
        payload,
        fixture_record_type_id(HORIZONTAL_TYPE),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        0,
    );
    let entity = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1").expect("entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            },
        )
        .expect("point geometry"),
    );
    let entities = std::collections::BTreeMap::from([(("segment", 0), &entity)]);
    let parameters = std::collections::HashMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(entity.id().as_str().len()) - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::super::project_constraint(&ctx, &constraint, &entities, &parameters),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor sketch constraint entity id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(
        super::super::project_constraint(&ctx, &constraint, &entities, &parameters)
            .expect("horizontal constraint projects")
            .is_ok()
    );
}

#[test]
fn cross_sketch_constraint_skips_retained_member_id_copies() {
    let mut bytes = constraint_header(0, 0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    let payload = parse(&bytes, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Coincident, source, 22)
            .expect("coincident constraint")
    });
    let constraint = Located::new(
        payload,
        fixture_record_type_id(COINCIDENT_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        0,
    );
    let point = || {
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            },
        )
        .expect("point geometry")
    };
    let first = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1")
            .expect("first entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("first sketch id"),
        point(),
    );
    let second = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#2")
            .expect("second entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#2").expect("second sketch id"),
        point(),
    );
    let entities =
        std::collections::BTreeMap::from([(("segment", 0), &first), (("segment", 1), &second)]);
    let parameters = std::collections::HashMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(super::super::project_constraint(&ctx, &constraint, &entities, &parameters).is_none());
    ctx.finish_session()
        .expect("cross-sketch rejection copies no member IDs");
}

#[test]
fn dimensioned_constraint_copies_parameter_before_member_ids() {
    use super::super::SketchConstraintTag::{
        Diameter, HorizontalDistance, Radius, VerticalDistance,
    };

    let first = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#first")
            .expect("first entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#same").expect("first sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            },
        )
        .expect("point geometry"),
    );
    let second = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#second")
            .expect("second entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#same")
            .expect("second sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            },
        )
        .expect("point geometry"),
    );
    let entities =
        std::collections::BTreeMap::from([(("segment", 0), &first), (("segment", 1), &second)]);
    let parameter =
        cadmpeg_ir::features::ParameterId::mint("inventor:test:parameter#selected-parameter-value")
            .expect("parameter id");
    let parameters =
        std::collections::HashMap::from([("inventor:pmdc:parameter#segment-0", &parameter)]);

    let mut horizontal_distance = constraint_header(0, 0);
    horizontal_distance.extend_from_slice(&1_u32.to_le_bytes());
    horizontal_distance.extend_from_slice(&2_u32.to_le_bytes());
    horizontal_distance.extend_from_slice(&1_u32.to_le_bytes());
    horizontal_distance.extend_from_slice(&[0; 16]);

    let mut vertical_distance = constraint_header(0, 0);
    vertical_distance.extend_from_slice(&1_u32.to_le_bytes());
    vertical_distance.extend_from_slice(&2_u32.to_le_bytes());
    vertical_distance.extend_from_slice(&1_u32.to_le_bytes());
    vertical_distance.extend_from_slice(&[0; 16]);

    let mut radius = constraint_header(0, 1);
    radius.extend_from_slice(&0_u32.to_le_bytes());
    radius.extend_from_slice(&1_u32.to_le_bytes());
    radius.extend_from_slice(&[0; 16]);

    let mut diameter = constraint_header(0, 1);
    diameter.extend_from_slice(&0_u32.to_le_bytes());
    diameter.extend_from_slice(&1_u32.to_le_bytes());
    diameter.extend_from_slice(&[0; 16]);

    let cases = [
        (
            HorizontalDistance,
            HORIZONTAL_DISTANCE_TYPE,
            horizontal_distance,
        ),
        (VerticalDistance, VERTICAL_DISTANCE_TYPE, vertical_distance),
        (Radius, RADIUS_TYPE, radius),
        (Diameter, DIAMETER_TYPE, diameter),
    ];
    let parameter_bytes = cadmpeg_core::decode::u64_from_index(parameter.as_str().len());
    let first_member_bytes = cadmpeg_core::decode::u64_from_index(first.id().as_str().len());

    for (tag, type_id, bytes) in cases {
        let payload = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, tag, source, 22).expect("dimensioned constraint")
        });
        let constraint = Located::new(
            payload,
            fixture_record_type_id(type_id),
            cadmpeg_ir::identity_key!("segment"),
            0,
        );

        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = parameter_bytes - 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(matches!(
            super::super::project_constraint(&ctx, &constraint, &entities, &parameters),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor sketch constraint parameter id"
                    && limit.used == 0
                    && limit.additional == parameter_bytes
        ));
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "retain Inventor sketch constraint parameter id"
                    && limit.used == 0
                    && limit.additional == parameter_bytes
        ));

        policy.limits.max_retained_bytes = parameter_bytes + first_member_bytes - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(matches!(
            super::super::project_constraint(&ctx, &constraint, &entities, &parameters),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor sketch constraint entity id"
                    && limit.used == parameter_bytes
                    && limit.additional == first_member_bytes
        ));
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "retain Inventor sketch constraint entity id"
                    && limit.used == parameter_bytes
                    && limit.additional == first_member_bytes
        ));

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        assert!(matches!(
            super::super::project_constraint(&ctx, &constraint, &entities, &parameters),
            Some(Ok(_))
        ));
        ctx.finish_session()
            .expect("accepted dimensioned constraint keeps its output IDs");
    }
}

#[test]
fn sketch_constraint_projection_refuses_entity_limit_before_creation() {
    let mut bytes = constraint_header(0, 0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    let payload = parse(&bytes, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22)
            .expect("horizontal constraint")
    });
    let constraint = Located::new(
        payload,
        fixture_record_type_id(HORIZONTAL_TYPE),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        0,
    );
    let entity = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1").expect("entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            },
        )
        .expect("point geometry"),
    );
    let entities = std::collections::BTreeMap::from([(("segment", 0), &entity)]);
    let parameters = std::collections::HashMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::super::project_constraint(&ctx, &constraint, &entities, &parameters),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "project Inventor sketch constraint"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(matches!(
        super::super::project_constraint(&ctx, &constraint, &entities, &parameters),
        Some(Ok(_))
    ));
}

#[test]
fn projects_generated_closed_square_and_resolved_plane() {
    let mut transform = parse(
        &{
            let mut bytes = content(0);
            bytes.extend_from_slice(&0x8421u16.to_le_bytes());
            bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
            bytes
        },
        |_ctx, source| parse_transform(source, 22).expect("transform"),
    );
    let direction = parse(
        &{
            let mut bytes = content(1);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&0.0f64.to_le_bytes());
            bytes.extend_from_slice(&0.0f64.to_le_bytes());
            bytes.extend_from_slice(&0.0f64.to_le_bytes());
            bytes.extend_from_slice(&1.0f64.to_le_bytes());
            bytes
        },
        |_ctx, source| parse_direction(source, 22).expect("direction"),
    );
    let mut sketch_bytes = content(2);
    sketch_bytes.extend_from_slice(&0i32.to_le_bytes());
    sketch_bytes.extend_from_slice(&8u32.to_le_bytes());
    sketch_bytes.extend(list(8, &[4, 5, 6, 7, 8, 9, 10, 11]));
    sketch_bytes.extend_from_slice(&1u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&2u32.to_le_bytes());
    sketch_bytes.extend_from_slice(&[0; 8]);
    sketch_bytes.extend(list(2, &[]));
    let sketch = parse(&sketch_bytes, |ctx, source| {
        parse_sketch(ctx, source, 22).expect("sketch")
    });
    let points = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
    let mut entities = points
        .into_iter()
        .enumerate()
        .map(|(index, position)| {
            parse(
                &point_bytes(
                    u32::try_from(index).expect("fixture value fits u32") + 3,
                    3,
                    position,
                ),
                |ctx, source| parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point"),
            )
        })
        .collect::<Vec<_>>();
    for (index, endpoints) in [[4, 5], [5, 6], [6, 7], [7, 4]].into_iter().enumerate() {
        let mut line = parse(
            &line_bytes(
                u32::try_from(index).expect("fixture value fits u32") + 7,
                3,
                endpoints,
            ),
            |ctx, source| parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line"),
        );
        let start = points[cadmpeg_core::decode::index_from_u32(endpoints[0]) - 4];
        let end = points[cadmpeg_core::decode::index_from_u32(endpoints[1]) - 4];
        let PmDcSketchEntityKind::Line {
            origin, direction, ..
        } = &mut line.kind
        else {
            unreachable!("generated line")
        };
        *origin = start.map(real);
        *direction = [real(end[0] - start[0]), real(end[1] - start[1])];
        entities.push(line);
    }
    transform.header.source_index = 0;
    let transform = Located::new(
        transform,
        fixture_record_type_id(TRANSFORM_TYPE),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        0,
    );
    let direction = Located::new(
        direction,
        fixture_record_type_id(DIRECTION_TYPE),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        1,
    );
    let located_sketch = Located::new(
        sketch.clone(),
        fixture_record_type_id(SKETCH_TYPE),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        2,
    );
    let entities = entities
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let type_id = if index < 4 { POINT_TYPE } else { LINE_TYPE };
            Located::new(
                value,
                fixture_record_type_id(type_id),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                u32::try_from(index).expect("fixture value fits u32") + 3,
            )
        })
        .collect();
    let mut inventory = SketchInventory {
        sketches: vec![located_sketch],
        entities,
        transforms: vec![transform],
        directions: vec![direction],
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
    assert_eq!(projected.unresolved_sketches, 0);
    assert_eq!(projected.unresolved_entities, 0);
    assert_eq!(projected.sketches[0].profiles[0].len(), 4);
    assert!(matches!(
        projected.sketches[0].placement,
        SketchPlacement::Resolved { .. }
    ));

    let mut mapped_constraint = content(11);
    mapped_constraint.extend_from_slice(&(-1i32).to_le_bytes());
    mapped_constraint.extend_from_slice(&0u32.to_le_bytes());
    mapped_constraint.extend_from_slice(&6u16.to_le_bytes());
    mapped_constraint.extend_from_slice(&0x3000u16.to_le_bytes());
    mapped_constraint.extend_from_slice(&1u32.to_le_bytes());
    mapped_constraint.extend_from_slice(&[0; 8]);
    mapped_constraint.extend_from_slice(&4u32.to_le_bytes());
    mapped_constraint.extend_from_slice(&0.5f64.to_le_bytes());
    mapped_constraint.extend(list(6, &[]));
    mapped_constraint.extend_from_slice(&0u32.to_le_bytes());
    mapped_constraint.extend_from_slice(&4u32.to_le_bytes());
    mapped_constraint.extend_from_slice(&5u32.to_le_bytes());
    let mapped_constraint = parse(&mapped_constraint, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Coincident, source, 22)
            .expect("mapped coincident")
    });
    inventory.constraints.push(Located::new(
        mapped_constraint,
        fixture_record_type_id(COINCIDENT_TYPE),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        11,
    ));
    let mut sketch = sketch;
    let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
    references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
    sketch.entities =
        PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
    inventory.sketches[0] = Located::new(
        sketch,
        fixture_record_type_id(SKETCH_TYPE),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        2,
    );
    let incomplete = project(&ctx, &inventory, &[]).expect("incomplete sketch projection");
    assert_eq!(incomplete.unresolved_sketches, 1);
    assert_eq!(incomplete.unresolved_constraints, 1);
    assert!(incomplete.sketches.is_empty());
    assert!(incomplete.entities.is_empty());
}

#[test]
fn large_collinear_sketch_carrier_matches() {
    assert!(super::super::line_carrier_matches(
        [0., 0.],
        [1e200, 1e200],
        [1e200, 1e200],
        [2e200, 2e200]
    ));
    assert!(!super::super::line_carrier_matches(
        [0., 0.],
        [1e200, 0.],
        [0., 0.],
        [0., 1e200]
    ));
}

#[test]
fn sketch_carrier_agreement_is_independent_of_scale() {
    for scale in [f64::from_bits(1), 5e-11, 1.0, 1e200] {
        assert!(super::super::line_carrier_matches(
            [0.0, 0.0],
            [scale, 0.0],
            [scale, 0.0],
            [2.0 * scale, 0.0],
        ));
        assert!(super::super::line_carrier_matches(
            [0.0, 0.0],
            [scale, 0.0],
            [0.0, 0.0],
            [scale, 0.0],
        ));
        assert!(!super::super::line_carrier_matches(
            [0.0, 0.0],
            [scale, 0.0],
            [0.0, scale],
            [scale, scale],
        ));
    }
    for invalid in [0.0, f64::NAN, f64::INFINITY] {
        assert!(!super::super::line_carrier_matches(
            [0.0, 0.0],
            [invalid, 0.0],
            [0.0, 0.0],
            [1.0, 0.0],
        ));
    }
}

#[test]
fn audit_regression_distant_parallel_segment_is_not_on_carrier() {
    for origin in [[0., 0.], [1e12, 0.]] {
        assert!(!super::super::line_carrier_matches(
            origin,
            [1., 0.],
            [1e12, 1.],
            [1e12 + 1., 1.]
        ));
        assert!(super::super::line_carrier_matches(
            origin,
            [1., 0.],
            [1e12, 0.],
            [1e12 + 1., 0.]
        ));
    }
}

#[test]
fn audit_regression_distant_oblique_point_keeps_stored_direction_ratio() {
    assert!(super::super::line_carrier_matches(
        [0., 0.],
        [1., 12.],
        [1e12, 12e12],
        [1e12 + 1., 12e12 + 12.]
    ));
    assert!(!super::super::line_carrier_matches(
        [0., 0.],
        [1., 12.],
        [1e12, 12e12 + 1.],
        [1e12 + 1., 12e12 + 13.]
    ));
}
