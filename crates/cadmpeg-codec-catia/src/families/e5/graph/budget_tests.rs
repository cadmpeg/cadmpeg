// SPDX-License-Identifier: Apache-2.0
//! Work admission at malformed E5 prefixes.

use super::*;

#[test]
fn e5_rejection_reads_only_the_first_invalid_counted_reference() {
    for count in [1, 127] {
        let bounds = Record {
            class: 0x0e,
            id: 1,
            payload: &[0x80 + count],
        };
        assert!(
            crate::test_support::with_work_limit(1, |ctx| parse_bounds(ctx, &bounds))
                .expect("first missing reference ends the bound")
                .is_none()
        );
        let face = Record {
            class: 0x00,
            id: 1,
            payload: &[0x81 + count.min(126), 0x80],
        };
        assert!(
            crate::test_support::with_work_limit(1, |ctx| parse_face(ctx, &face))
                .expect("first missing loop ends the face")
                .is_none()
        );
        let root = [0x08, count];
        assert!(
            crate::test_support::with_work_limit(1, |ctx| parse_body_root(ctx, &root))
                .expect("first missing face ends the body")
                .is_none()
        );
        let loop_ = Record {
            class: 0xa2,
            id: 1,
            payload: &[0x81 + 2 * count.min(63)],
        };
        assert!(
            crate::test_support::with_work_limit(1, |ctx| parse_loop(ctx, &loop_))
                .expect("first missing member ends the loop")
                .is_none()
        );
    }
}

#[test]
fn e5_bound_rejection_does_not_charge_unread_parameters() {
    let count = 127_u8;
    let mut payload = vec![0x80 + count];
    payload.extend(std::iter::repeat_n(0x80, usize::from(count)));
    payload.push(0x80 + count);
    payload.extend(f64::NAN.to_le_bytes());
    payload.extend(0_u32.to_le_bytes());
    payload.resize(payload.len() + 12 * (usize::from(count) - 1), 0);
    let record = Record {
        class: 0x0e,
        id: 1,
        payload: &payload,
    };
    // 127 references and the end probe, then one invalid parameter visit.
    assert!(
        crate::test_support::with_work_limit(129, |ctx| parse_bounds(ctx, &record))
            .expect("only the first parameter is read")
            .is_none()
    );
}

#[test]
fn e5_loop_sign_rejection_does_not_charge_the_sign_suffix() {
    let count = 63;
    let mut trailing = vec![0x80 + count];
    trailing.extend(2_i16.to_le_bytes());
    trailing.resize(1 + 2 * (3 * usize::from(count) + 4), 0);
    assert!(matches!(
        crate::test_support::with_work_limit(1, |ctx| {
            parse_loop_signs(ctx, &trailing, usize::from(count))
        })
        .expect("the first sign rejects"),
        Err(LoopSignError::Sign)
    ));
}

#[test]
fn e5_duplicate_identity_charges_one_visit_at_a_time() {
    let mut stream = Vec::new();
    for id in [1, 1].into_iter().chain(2..200) {
        crate::test_support::test_e5::append_e5_record(&mut stream, 0xfe, id, &[]);
    }
    let refusal = crate::test_support::with_work_refusal("catia_e5_record_id_scan", |ctx| {
        parse_topology(ctx, &stream)
    });
    let Err(CodecError::ResourceLimit(limit)) = refusal else {
        panic!("work refusal")
    };
    assert_eq!(limit.additional, 1);
    assert!(
        crate::test_support::with_service_context(|ctx| parse_topology(ctx, &stream))
            .expect("duplicate identity rejection")
            .is_none()
    );
}
