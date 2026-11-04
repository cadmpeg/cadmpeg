// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::{derive, records, CatiaE5NurbsLimitRecord, LimitProgram};
use crate::test_support::{test_e5::append_e5_record, with_entity_limit, with_service_context};

fn axis(bytes: &mut Vec<u8>) {
    for word in [1_u32, 0, 0, 2, 0] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for knot in [0.0_f64, 1.0] {
        bytes.extend_from_slice(&knot.to_le_bytes());
    }
    for multiplicity in [2_u32, 2] {
        bytes.extend_from_slice(&multiplicity.to_le_bytes());
    }
}

fn curve() -> Vec<u8> {
    let mut payload = vec![0x81, 0x87, 0, 0];
    axis(&mut payload);
    for coordinate in [0.0_f64, 0.0, 1.0, 1.0] {
        payload.extend_from_slice(&coordinate.to_le_bytes());
    }
    for arc in [1_u16, 1] {
        payload.extend_from_slice(&arc.to_le_bytes());
    }
    for bits in [
        (-0.0_f64).to_bits(),
        1.0_f64.to_bits(),
        0x7ff8_0000_0000_002a,
        f64::INFINITY.to_bits(),
    ] {
        payload.extend_from_slice(&bits.to_le_bytes());
    }
    payload.push(7);
    payload
}

fn surface(mode: u16) -> Vec<u8> {
    let mut payload = vec![0x80];
    axis(&mut payload);
    axis(&mut payload);
    payload.extend_from_slice(&mode.to_le_bytes());
    for coordinate in [
        0.0_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0,
    ] {
        payload.extend_from_slice(&coordinate.to_le_bytes());
    }
    if mode == 1 {
        for weight in [1.0_f64, 2.0, -1.0, 4.0] {
            payload.extend_from_slice(&weight.to_le_bytes());
        }
    }
    for patch in [1_u16, 2, 3, 4] {
        payload.extend_from_slice(&patch.to_le_bytes());
    }
    for bits in 1_u64..=8 {
        payload.extend_from_slice(&bits.to_le_bytes());
    }
    for flag in [5_u16, 6, 7] {
        payload.extend_from_slice(&flag.to_le_bytes());
    }
    for bits in 9_u64..=16 {
        payload.extend_from_slice(&bits.to_le_bytes());
    }
    payload.extend_from_slice(&[8, 9, 10, 11, 12, 13]);
    payload
}

fn frame(class: u8, id: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    append_e5_record(&mut bytes, class, id, payload);
    bytes
}

#[test]
fn complete_curve_cores_locate_limits_and_retain_support_frame_identity() {
    let payload = curve();
    let bytes = frame(0xaa, 23, &payload);
    let values: Vec<_> = records(&bytes).collect();
    assert_eq!(values.len(), 1);
    let value = &values[0];
    assert_eq!(value.id, "catia:e5:nurbs-limits#0");
    assert_eq!(value.byte_offset, 0);
    assert_eq!(value.byte_len, 130);
    assert_eq!(value.record_id, 23);
    assert_eq!(value.class_word, 0xaa);
    assert_eq!(value.limit_payload_offset, 80);
    let LimitProgram::Pcurve(program) = &value.program else {
        panic!("curve program")
    };
    assert_eq!(program.lower_arc, 1);
    assert_eq!(program.upper_arc, 1);
    assert_eq!(program.lower_local_bits, (-0.0_f64).to_bits());
    assert_eq!(program.upper_local_bits, 1.0_f64.to_bits());
    assert_eq!(program.coefficient_bits, 0x7ff8_0000_0000_002a);
    assert_eq!(program.shift_bits, f64::INFINITY.to_bits());
    assert_eq!(program.extrapolation_control, 7);
    assert!(!program.is_disabled_identity());
    let mut another_support = payload;
    another_support[1] = 0x89;
    assert_eq!(records(&frame(0xaa, 24, &another_support)).count(), 1);
}

#[test]
fn complete_surface_cores_cover_polynomial_and_rational_weights() {
    for (mode, offset) in [(0_u16, 187_u64), (1, 219)] {
        let payload = surface(mode);
        let values: Vec<_> = records(&frame(0xe7, 29, &payload)).collect();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].limit_payload_offset, offset);
        let LimitProgram::Surface(program) = &values[0].program else {
            panic!("surface program")
        };
        assert_eq!(program.patches, [1, 2, 3, 4]);
        assert_eq!(program.local_bits, [1, 2, 3, 4]);
        assert_eq!(program.affine_bits, [5, 6, 7, 8]);
        assert_eq!(program.extrapolation_flags, [5, 6, 7]);
        assert_eq!(program.continuation_bits, [9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(
            program.suffix_controls.map(|control| control.0),
            [8, 9, 10, 11, 12, 13]
        );
        assert!(!program.is_disabled_identity());
    }
    for invalid in [0.0_f64, f64::NAN, f64::INFINITY] {
        let mut payload = surface(1);
        payload[187..195].copy_from_slice(&invalid.to_le_bytes());
        assert_eq!(records(&frame(0xe7, 1, &payload)).count(), 0);
    }
    let mut payload = surface(0);
    payload[89..91].copy_from_slice(&2_u16.to_le_bytes());
    assert_eq!(records(&frame(0xe7, 1, &payload)).count(), 0);
}

#[test]
fn physical_record_ids_do_not_merge_repeated_persistent_ids() {
    let first = frame(0xaa, 7, &curve());
    let mut bytes = first.clone();
    bytes.extend_from_slice(&first);
    let values = with_service_context(|ctx| derive(ctx, &bytes)).unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].record_id, 7);
    assert_eq!(values[1].record_id, 7);
    assert_eq!(values[0].id, "catia:e5:nurbs-limits#0");
    assert_eq!(values[1].id, "catia:e5:nurbs-limits#130");
    assert_eq!(values[1].byte_offset, 130);
    assert_eq!(values[0].program, values[1].program);
}

#[test]
fn native_projection_refuses_class_aliases_wrong_families_and_malformed_cores() {
    let good = frame(0xaa, 1, &curve());
    for (offset, byte) in [(0, 0xa5), (1, 0x0c), (4, 1)] {
        let mut bytes = good.clone();
        bytes[offset] = byte;
        assert_eq!(records(&bytes).count(), 0);
    }
    for (offset, replacement) in [
        (0, vec![0x80]),
        (1, vec![0x00]),
        (2, vec![1]),
        (4, 0_u32.to_le_bytes().to_vec()),
        (8, 1_u32.to_le_bytes().to_vec()),
        (16, u32::MAX.to_le_bytes().to_vec()),
        (24, 1.0_f64.to_le_bytes().to_vec()),
        (40, 0_u32.to_le_bytes().to_vec()),
        (48, f64::NAN.to_le_bytes().to_vec()),
    ] {
        let mut payload = curve();
        payload[offset..offset + replacement.len()].copy_from_slice(&replacement);
        assert_eq!(
            records(&frame(0xaa, 1, &payload)).count(),
            0,
            "core byte {offset}"
        );
    }
    for class in [0xaa, 0xe7] {
        let payload = if class == 0xaa { curve() } else { surface(1) };
        for length in 0..payload.len() {
            assert_eq!(records(&frame(class, 1, &payload[..length])).count(), 0);
        }
        let mut extra = payload;
        extra.push(0);
        assert_eq!(records(&frame(class, 1, &extra)).count(), 0);
    }
}

#[test]
fn native_limit_records_charge_entities_before_allocation() {
    let bytes = frame(0xaa, 1, &curve());
    assert!(with_entity_limit(0, |ctx| derive(ctx, &bytes)).is_err());
    assert_eq!(
        with_entity_limit(1, |ctx| derive(ctx, &bytes))
            .unwrap()
            .len(),
        1
    );
    let mut two = bytes.clone();
    two.extend_from_slice(&bytes);
    assert!(with_entity_limit(1, |ctx| derive(ctx, &two)).is_err());
    assert_eq!(
        with_entity_limit(2, |ctx| derive(ctx, &two)).unwrap().len(),
        2
    );
    assert!(with_entity_limit(0, |ctx| derive(ctx, &[]))
        .unwrap()
        .is_empty());
}

#[test]
fn stored_limit_programs_round_trip_exact_raw_bits() {
    let mut bytes = frame(0xaa, 7, &curve());
    bytes.extend(frame(0xe7, 7, &surface(1)));
    let expected = with_service_context(|ctx| derive(ctx, &bytes)).unwrap();
    let wire = serde_json::to_vec(&expected).unwrap();
    let loaded: Vec<CatiaE5NurbsLimitRecord> = serde_json::from_slice(&wire).unwrap();
    assert_eq!(loaded, expected);
    assert_eq!(serde_json::to_vec(&loaded).unwrap(), wire);
    let mut value = serde_json::to_value(&expected[0]).unwrap();
    value["unexpected"] = true.into();
    assert!(serde_json::from_value::<CatiaE5NurbsLimitRecord>(value).is_err());
}
