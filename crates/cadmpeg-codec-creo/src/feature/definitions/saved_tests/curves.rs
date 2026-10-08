// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn saved_generated_arc_body_refuses_before_retained_copy() {
    assert!(
        matches!(generated_arc_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo saved generated arc body"), |cap| generated_arc_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved generated arc body")
    );
    assert_eq!(
        generated_arc_with_limits(
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| generated_arc_with_limits(u64::MAX, cap)
            )
        )
        .expect("generated arc admitted")
        .len(),
        1
    );
}

#[test]
fn saved_generated_line_body_refuses_before_retained_copy() {
    assert!(
        matches!(generated_line_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo saved generated line body"), |cap| generated_line_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved generated line body")
    );
    let entities = generated_line_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| generated_line_with_limits(u64::MAX, cap),
        ),
    )
    .expect("generated line admitted");
    let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
        panic!("generated line");
    };
    assert_eq!(line.body.len(), 8);
}

#[test]
fn saved_arc_body_refuses_before_retained_copy() {
    assert!(
        matches!(saved_circular_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo saved arc body"), |cap| saved_circular_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved arc body")
    );
    assert_eq!(
        saved_circular_with_limits(
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| saved_circular_with_limits(u64::MAX, cap)
            )
        )
        .expect("arc and circle admitted")
        .len(),
        2
    );
}

#[test]
fn saved_circle_body_refuses_before_retained_copy() {
    assert!(
        matches!(saved_circular_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo saved circle body"), |cap| saved_circular_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved circle body")
    );
    assert_eq!(
        saved_circular_with_limits(
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| saved_circular_with_limits(u64::MAX, cap)
            )
        )
        .expect("arc and circle admitted")
        .len(),
        2
    );
}

#[test]
fn saved_circular_entities_refuse_before_each_append() {
    crate::test_support::assert_refusal_order(
        ResourceDimension::CollectionItems,
        &[
            "creo saved circular entities",
            "creo saved circular entities",
        ],
        |cap| saved_circular_with_limits(cap, u64::MAX),
    );
    assert_eq!(
        saved_circular_with_limits(u64::MAX, u64::MAX)
            .expect("arc and circle admitted")
            .len(),
        2
    );
}

#[test]
fn saved_conic_body_refuses_before_retained_copy() {
    let run = |collection, retained| {
        with_saved_leaf_limits(SAVED_CONIC_LIMIT_INPUT, collection, retained, |ctx| {
            parse_saved_conic_entities(
                ctx,
                SAVED_CONIC_LIMIT_INPUT,
                0,
                SAVED_CONIC_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert!(
        matches!(run(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo saved conic body"), |cap| run(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved conic body")
    );
    assert_eq!(run(u64::MAX, u64::MAX).expect("conic admitted").len(), 1);
}

#[test]
fn saved_conic_entity_refuses_before_append() {
    assert!(
        matches!(crate::test_support::last_refusal_at(SAVED_CONIC_LIMIT_INPUT, ResourceDimension::CollectionItems, "creo saved conic entities", |ctx| {
        parse_saved_conic_entities(ctx, SAVED_CONIC_LIMIT_INPUT, 0,
            SAVED_CONIC_LIMIT_INPUT.len(), &scalar::ScalarCache::default())
    }),
CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo saved conic entities")
    );
    assert_eq!(
        with_saved_leaf_limits(SAVED_CONIC_LIMIT_INPUT, u64::MAX, u64::MAX, |ctx| {
            parse_saved_conic_entities(
                ctx,
                SAVED_CONIC_LIMIT_INPUT,
                0,
                SAVED_CONIC_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
            )
        })
        .expect("conic admitted")
        .len(),
        1
    );
}

#[test]
fn saved_arc_negative_dict_forms_supply_ieee_high_bytes() {
    for (bytes, head) in [
        ([0x9b, 1, 2, 3, 4, 5, 6], [0x40, 0x10]),
        ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
        ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
        ([0x9e, 1, 2, 3, 4, 5, 6], [0x40, 0x13]),
        ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
        ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
        ([0x5e, 1, 2, 3, 4, 5, 6], [0x3f, 0xd3]),
        ([0x60, 1, 2, 3, 4, 5, 6], [0x3f, 0xd5]),
        ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
        ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
        ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
        ([0xd5, 1, 2, 3, 4, 5, 6], [0xc0, 0x03]),
        ([0xde, 1, 2, 3, 4, 5, 6], [0xc0, 0x10]),
        ([0xdf, 1, 2, 3, 4, 5, 6], [0xc0, 0x11]),
    ] {
        let expected = f64::from_be_bytes([
            head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
        ]);
        assert_eq!(
            saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (Some(expected), 7)
        );
    }
    let d5 = [0xd5, 1, 2, 3, 4, 5, 6];
    assert_eq!(
        saved_section_scalar(&d5, 0, d5.len(), &scalar::ScalarCache::default()),
        (Some(f64::from_be_bytes([0xbf, 1, 2, 3, 4, 5, 6, 0])), 7)
    );
}

#[test]
fn saved_arc_28_form_supplies_ieee_high_byte() {
    let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
    assert_eq!(
        saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])), 8)
    );
}

#[test]
fn saved_arc_zero_does_not_consume_arc_scalar_opener() {
    let bytes = [0x18, 0x5e, 1, 2, 3, 4, 5, 6];
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        saved_arc_scalar(&bytes, 0, bytes.len(), &cache),
        (Some(0.0), 1)
    );
    assert_eq!(
        saved_arc_scalar(&bytes, 1, bytes.len(), &cache),
        (Some(f64::from_be_bytes([0x3f, 0xd3, 1, 2, 3, 4, 5, 6])), 8)
    );
}

#[test]
fn saved_circular_entities_retain_ids_and_independent_fields() {
    let payload = b"\xe0\x00entity(arc)\0\
            \xe0\x01id\0\x07\xe0\x02center\0\x0f\x0f\x0f\
            \xe0\x00entity(circle)\0\
            \xe0\x01id\0\x08\xe0\x02radius\0\x0f";

    let entities = saved_circular_entities(
        payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        None,
        None,
    );

    let [FeatureSavedEntity::Arc(arc), FeatureSavedEntity::Circle(circle)] = entities.as_slice()
    else {
        panic!("saved circular entities");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, None);
    assert_eq!(arc.endpoints, [[None; 3]; 2]);
    assert_eq!(arc.parameters, [None; 2]);
    let arc_body_start = b"\xe0\x00entity(arc)\0".len();
    let circle_label = b"\xe0\x00entity(circle)\0";
    let circle_offset = payload
        .windows(circle_label.len())
        .position(|window| window == circle_label)
        .expect("circle boundary");
    assert_eq!(arc.body, payload[arc_body_start..circle_offset]);
    assert_eq!(circle.entity_id, 8);
    assert_eq!(circle.center, [None; 3]);
    assert_eq!(circle.radius, Some(0.0));
    assert_eq!(circle.body, payload[circle_offset + circle_label.len()..]);
}

#[test]
fn saved_conic_retains_coefficients_parameters_and_planar_frame() {
    let payload = b"\xe0\x00entity(conic)\0\
            \xe0\x01id\0\x02\xe0\x01type\0\x3a\
            \xe0\x02end1\0\xf8\x03\x18\xe5\
            \xe0\x02end2\0\xf8\x03\x18\xe5\
            \xe0\x02t0\0\x0f\xe0\x02t1\0\xf6\
            \xe0\x02c1\0\xe4\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\
            \xe4\x0f\x0f\x0f\xe4\x18\xe5\x0f\x0f\x0f\x0f\
            \xe0\x01trailing_field\0\x07";

    let entities = saved_conic_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Conic(conic)] = entities.as_slice() else {
        panic!("one saved conic");
    };

    assert_eq!(conic.entity_id, 2);
    assert_eq!(conic.endpoints, [[Some(0.0), Some(1.0), Some(0.0)]; 2]);
    assert_eq!(conic.parameters, [Some(0.0), None]);
    assert_eq!(conic.coefficients, [Some(1.0); 2]);
    assert_eq!(
        conic.local_system.map(cadmpeg_ir::units::FiniteVector::get),
        Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
    );
    assert_eq!(conic.body, payload[b"\xe0\x00entity(conic)\0".len()..]);
}

#[test]
fn saved_arc_replay_uses_order_table_row_boundaries() {
    let mut payload = vec![0xe3, 7, 0xe2];
    payload.extend([0x0f; 12]);
    payload.push(0xe3);
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }]
        .into(),
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![FeatureSegment {
            kind: FeatureSegmentKind::Arc([1, 2]),
            directions: [None; 3],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 0,
        }])
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Arc(arc) = &entities[0] else {
        panic!("expected saved arc");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, Some(0.0));
    assert_eq!(arc.body, payload[1..payload.len() - 1]);
    let mut incomplete_segments = segments.clone();
    incomplete_segments.declared_count += 1;
    let incomplete_entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&incomplete_segments),
    );
    assert_eq!(incomplete_entities, entities);
    let section = positional_saved_section(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    )
    .expect("positional saved section");
    assert_eq!(section.entities.len(), 1);
    assert_eq!(section.offset, 1);

    let named_prefix = b"\xe0\x00entity(arc)\0\xe0\x01id\0\x09";
    let mut named_payload = named_prefix.to_vec();
    named_payload.extend_from_slice(&payload);
    let named_entities = saved_circular_entities(
        &named_payload,
        0,
        named_payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );
    let [FeatureSavedEntity::Arc(named), FeatureSavedEntity::Arc(replay)] =
        named_entities.as_slice()
    else {
        panic!("named arc and replay");
    };
    assert_eq!(
        named.body, b"\xe0\x01id\0\x09",
        "named body must stop before the replay separator"
    );
    assert_eq!(replay.body, payload[1..payload.len() - 1]);
}

#[test]
fn saved_arc_replay_retains_a_structurally_terminated_scalar_prefix() {
    let mut payload = vec![0xe3, 7, 0xe2];
    payload.extend([0x0f; 6]);
    payload.push(0xe3);
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }]
        .into(),
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![FeatureSegment {
            kind: FeatureSegmentKind::Arc([1, 2]),
            directions: [None; 3],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 0,
        }])
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    let [FeatureSavedEntity::Arc(arc)] = entities.as_slice() else {
        panic!("expected saved arc");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, Some(0.0));
    assert_eq!(arc.endpoints[0], [Some(0.0), Some(0.0), None]);
    assert_eq!(arc.endpoints[1], [None; 3]);
    assert_eq!(arc.parameters, [None; 2]);
}

#[test]
fn saved_generated_line_requires_its_orientation_invariant() {
    let payload = [0xe3, 8, 0xe2, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe3];
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 43,
            internal_id: 8,
            bitmask: 0,
            offset: 0,
        }]
        .into(),
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![FeatureSegment {
            kind: FeatureSegmentKind::Line([1, 2]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: Some(0),
            vertical_horizontal: Some(1),
            radius_ref: None,
            radius2_ref: None,
            external_id: 43,
            body: Vec::new(),
            offset: 0,
        }])
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 8);
    assert_eq!(line.endpoints[0], [Some(0.0); 3]);
    assert_eq!(line.endpoints[1], [Some(1.0), Some(0.0), Some(0.0)]);
    assert_eq!(line.body, payload[1..payload.len() - 1]);
}
