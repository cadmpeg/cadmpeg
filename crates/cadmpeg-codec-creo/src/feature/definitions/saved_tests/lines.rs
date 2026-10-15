// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn saved_line_body_refuses_before_retained_copy() {
    assert!(
        matches!(saved_line_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo saved line body"), |cap| saved_line_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved line body")
    );
    assert_eq!(
        saved_line_with_limits(
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| saved_line_with_limits(u64::MAX, cap)
            )
        )
        .expect("saved line admitted")
        .len(),
        1
    );
}

#[test]
fn saved_line_accepts_bare_entity_reference_before_coordinates() {
    let payload = b"\xe0\0entity(line)\0\x05\xe2\xf7\x2a\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\xf1\xf7\x2b\xe3";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 5);
    assert_eq!(line.references, [42, 43]);
    assert_eq!(line.endpoints, [[Some(8.0); 3]; 2]);
    let body_start = b"\xe0\0entity(line)\0".len();
    assert_eq!(line.body, payload[body_start..payload.len() - 1]);
}

#[test]
fn saved_line_expands_compact_basis_triple() {
    let payload = b"\xe0\0entity(line)\0\x05\xe2\x18\xe5\x2f\x20\0\x2f\x20\0\x2f\x20\0\xe3";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(
        line.endpoints,
        [
            [Some(0.0), Some(1.0), Some(0.0)],
            [Some(8.0), Some(8.0), Some(8.0)]
        ]
    );
    let body_start = b"\xe0\0entity(line)\0".len();
    assert_eq!(line.body, payload[body_start..payload.len() - 1]);
}

#[test]
fn saved_line_replay_continues_after_point_prototype() {
    let scalar_triple = b"\x2f\x20\0\x2f\x20\0\x2f\x20\0";
    let mut payload = b"\xe0\0entity(line)\0\x05\xe2".to_vec();
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(scalar_triple);
    payload.push(0xe3);
    payload.extend_from_slice(b"\xe0\0entity(point)\0\xe0\x01id\0\x04\xf1\xf7\x2a\xe3\x06\xe2");
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(b"\xe0\0entity(arc)\0");

    let entities = saved_line_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 2);
    assert_eq!(
        entities
            .iter()
            .filter_map(|entity| match entity {
                FeatureSavedEntity::Line(line) => Some(line.entity_id),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [5, 6]
    );
}

#[test]
fn saved_line_accepts_named_record_boundary() {
    let payload = b"\xe0\0entity(line)\0\x03\xe2\xf1\xf7\x80\xc4\
            \x48\x20\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\
            \x48\x1e\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\x8a\x01\x02\x03\x04\x05\x0f\
            \xe0\0entity(point)\0\xf1\xf7\x2a\xe3\xe0\0entity(arc)\0";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 3);
    assert_eq!(line.references, [196]);
    let body_start = b"\xe0\0entity(line)\0".len();
    let body_end = payload[body_start..]
        .windows(b"\xe0\0entity(point)\0".len())
        .position(|window| window == b"\xe0\0entity(point)\0")
        .map(|relative| body_start + relative)
        .expect("point boundary");
    assert_eq!(line.body, payload[body_start..body_end]);
}

#[test]
fn saved_line_retains_its_identity_and_coordinate_prefix() {
    let payload = b"\xe0\0entity(line)\0\x07\xe2\x0f\x0f\x0f\
            \xe0\0entity(arc)\0";

    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
        panic!("saved line");
    };
    assert_eq!(line.entity_id, 7);
    assert_eq!(
        line.endpoints,
        [[Some(0.0), Some(0.0), Some(0.0)], [None; 3]]
    );
}
