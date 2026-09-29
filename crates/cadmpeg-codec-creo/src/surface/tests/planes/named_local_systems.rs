// SPDX-License-Identifier: Apache-2.0

use super::super::{named_prototype_records, named_surface_value};
use super::sequential_named_local_system_slots;
use crate::scalar;
use crate::surface::{
    scalar_slots_with_tokens_and_end, ScalarBodyRefusal, SurfaceNamedValue, SurfacePrototypeFamily,
};

#[test]
fn terminal_positional_slot_zero_occupies_one_byte() {
    let slots = crate::decode::with_test_decode_ctx(|ctx| {
        scalar_slots_with_tokens_and_end(ctx, &[0xe4, 0x18], 2, &scalar::ScalarCache::default())
            .map(|result| result.map(|table| (table.slots, table.consumed)))
    })
    .expect("scalar slots are admitted")
    .expect("a complete two-slot table")
    .0;

    assert_eq!(slots, [(Some(1.0), &[0xe4][..]), (Some(0.0), &[0x18][..])]);
}

#[test]
fn named_local_system_expands_row_lane_zero_forms() {
    let body = [
        0xf9, 0x04, 0x03, 0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4, 0x43, 0xe0, 0x00,
        0x18, 0xe4,
    ];

    assert_eq!(
        named_surface_value(
            &SurfacePrototypeFamily::Plane,
            "local_sys",
            &body,
            &scalar::ScalarCache::default(),
            &"prototype fixture",
            &mut crate::lane_refusal::LaneRefusals::new()
        ),
        SurfaceNamedValue::ScalarArray({
            let mut array = crate::surface::arrays::DimensionedScalars::empty(4, 3)
                .expect("valid scalar array");
            array
                .fill_values(vec![
                    Some(0.0),
                    Some(1.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(1.0),
                    Some(-0.5),
                    Some(0.0),
                    Some(1.0),
                ])
                .expect("matching scalar extent");
            array
        })
    );
}

#[test]
fn named_local_system_splits_zero_before_coordinate_token() {
    let body = [
        0x41, 0xd2, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2, 0x79, 0xac, 0x53, 0x1a, 0x28, 0x66, 0x9d,
        0x18, 0x79, 0xac, 0x53, 0x1a, 0x28, 0x66, 0x9d, 0x5d, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2,
        0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f,
    ];

    let slots = sequential_named_local_system_slots(
        &body,
        12,
        &scalar::ScalarCache::default(),
        &mut ScalarBodyRefusal::default(),
    )
    .expect("complete local system");

    assert_eq!(slots[2], Some(0.0));
    assert_eq!(slots[3], slots[1]);
    assert_eq!(slots[4], slots[0].map(|value| -value));
    assert_eq!(slots[5..], [Some(0.0); 7]);
}

#[test]
fn named_local_system_decodes_terminal_zero_slot() {
    let payload = b"srf_prim_ptr(cylinder)\0\xe0\x02local_sys\0\xf9\x04\x03\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18\xe0\x01radius\0\xe4";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("local_sys").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarArray({
            let mut array = crate::surface::arrays::DimensionedScalars::empty(4, 3)
                .expect("valid scalar array");
            array
                .fill_values(vec![
                    Some(0.0),
                    Some(1.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(1.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(15.0),
                    Some(0.0),
                ])
                .expect("matching scalar extent");
            array
        }))
    );
}

#[test]
fn named_local_system_advances_across_inherited_slots() {
    let body = [
        0xe4, 0x0f, 0xe7, 0x03, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f,
    ];

    assert_eq!(
        sequential_named_local_system_slots(
            &body,
            12,
            &scalar::ScalarCache::default(),
            &mut ScalarBodyRefusal::default()
        ),
        Some(vec![
            Some(1.0),
            Some(0.0),
            None,
            None,
            None,
            Some(1.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
        ])
    );
}

/// The bounded `local_sys` scalar body obeys the rules of the bounded scalar
/// body it is: a body that ends before its declared count is refused rather
/// than read as trailing absent slots, and a body with a byte left after its
/// last declared slot is refused.
#[test]
fn a_named_local_system_body_that_is_not_exactly_its_declared_slots_is_refused() {
    let cache = scalar::ScalarCache::default();
    // The twelve-slot body of `named_local_system_advances_across_inherited_slots`.
    let body = [
        0xe4, 0x0f, 0xe7, 0x03, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f,
    ];

    assert_eq!(
        sequential_named_local_system_slots(&body, 12, &cache, &mut ScalarBodyRefusal::default())
            .map(|slots| slots.len()),
        Some(12)
    );
    // Thirteen declared slots: the body ends one slot early.
    assert_eq!(
        sequential_named_local_system_slots(&body, 13, &cache, &mut ScalarBodyRefusal::default()),
        None
    );
    // Eleven declared slots: the last byte is left over.
    assert_eq!(
        sequential_named_local_system_slots(&body, 11, &cache, &mut ScalarBodyRefusal::default()),
        None
    );
    // An inherited run that ends the body far short of the declaration.
    assert_eq!(
        sequential_named_local_system_slots(
            &[0xe4, 0xe7, 0x02],
            12,
            &cache,
            &mut ScalarBodyRefusal::default()
        ),
        None
    );
}

#[test]
fn named_local_system_rejects_invalid_inherited_slot_transitions() {
    for body in [
        &[0xe7][..],
        &[0xe7, 0x00],
        &[0xe7, 0x0d],
        &[0xe4, 0xe7, 0x0c],
    ] {
        assert_eq!(
            sequential_named_local_system_slots(
                body,
                12,
                &scalar::ScalarCache::default(),
                &mut ScalarBodyRefusal::default()
            ),
            None
        );
    }
}

#[test]
fn named_local_system_rejects_an_unknown_byte_before_complete_slots() {
    let payload = b"srf_prim_ptr(cylinder)\0\
        \xe0\x02local_sys\0\xf9\x04\x03\xfb\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18\
        \xe0\x01radius\0\xe4";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("local_sys").map(|field| &field.value),
        Some(&SurfaceNamedValue::Opaque(
            b"\xf9\x04\x03\xfb\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18".to_vec()
        ))
    );
}

#[test]
fn named_local_system_uses_the_signed_coordinate_dict_lane() {
    let payload = b"srf_prim_ptr(torus)\0\
        \xe0\x02local_sys\0\xf9\x04\x03\
        \x7a\xeb\xb6\x28\xd0\x03\x82\
        \x28\xb2\x01\x83\xce\x09\x70\xf1\
        \x18\xe5\x10\
        \x41\xb2\x01\x83\xce\x09\x70\xf1\
        \x7a\xeb\xb6\x28\xd0\x03\x82\x18\
        \x48\x66\x80\x48\x08\x00\x2f\x44\x00";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());
    let SurfaceNamedValue::ScalarArray(array) =
        &records[0].field("local_sys").expect("local system").value
    else {
        panic!("scalar local system");
    };
    let values = array.values();

    assert_eq!(values[0], Some(0.997_523_383_819_597_8));
    assert_eq!(values[1], Some(0.070_335_614_969_227_37));
    assert_eq!(values[6], Some(-0.070_335_614_969_227_37));
    assert_eq!(values[7], Some(0.997_523_383_819_597_8));
    assert_eq!(&values[9..12], &[Some(-180.0), Some(-3.0), Some(40.0)]);
}

/// `0x0e` is the positive compact half coordinate of the `local_sys` scalar
/// lane. The body declares three slots and encodes three.
#[test]
fn named_local_system_decodes_positive_compact_half_coordinate_over_a_complete_body() {
    let body = [0xf9, 0x01, 0x03, 0x0e, 0x0f, 0x0f];
    let SurfaceNamedValue::ScalarArray(array) = named_surface_value(
        &SurfacePrototypeFamily::Plane,
        "local_sys",
        &body,
        &scalar::ScalarCache::default(),
        &"prototype fixture",
        &mut crate::lane_refusal::LaneRefusals::new(),
    ) else {
        panic!("scalar local system");
    };
    let values = array.values();

    assert_eq!(values, [Some(0.5), Some(0.0), Some(0.0)]);
}

#[test]
fn dimensioned_scalar_arrays_decode_compact_extents() {
    let mut body = vec![0xf9, 0x80, 0x88, 0x03];
    body.extend([0x0f; 136 * 3]);
    let SurfaceNamedValue::ScalarArray(array) = named_surface_value(
        &SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline),
        "i_points",
        &body,
        &scalar::ScalarCache::default(),
        &"prototype fixture",
        &mut crate::lane_refusal::LaneRefusals::new(),
    ) else {
        panic!("dimensioned scalar array");
    };
    let dimensions = array.dimensions();
    let count = array.count();
    let values = array.values();

    assert_eq!(dimensions, 136);
    assert_eq!(count, 3);
    assert_eq!(values.len(), 408);
    assert!(values.iter().all(|value| *value == Some(0.0)));
}
