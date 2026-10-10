// SPDX-License-Identifier: Apache-2.0

use crate::scalar::ScalarCache;
use crate::surface::{
    parsed_named_surface_value, ScalarBodyRefusal, SurfaceNamedValue, SurfacePrototypeFamily,
};

fn check_steps(name: &str, body: &[u8], expected: &Option<SurfaceNamedValue>) {
    let value = super::work_output(|ctx| {
        parsed_named_surface_value(
            ctx,
            &SurfacePrototypeFamily::Plane,
            name,
            body,
            &ScalarCache::default(),
            &mut ScalarBodyRefusal::default(),
            None,
        )
        .transpose()
    });
    assert_eq!(&value, expected);
}

fn compact_count(count: usize) -> Vec<u8> {
    if count <= 127 {
        vec![u8::try_from(count).expect("direct count")]
    } else {
        assert_eq!(count, 257);
        vec![0x81, 0x01]
    }
}

#[test]
fn named_compact_array_dispatch_owns_each_integer_and_skips_absent_slots() {
    for count in [0_usize, 1, 4, 7, 17, 257] {
        let mut body = vec![0xf8];
        body.extend(compact_count(count));
        body.extend(std::iter::repeat_n(7, count));

        check_steps(
            "dum_array",
            &body,
            &Some(SurfaceNamedValue::CompactIntArray(vec![7; count])),
        );
    }
    check_steps(
        "dum_array",
        &[0xf8, 3, 0x80, 0x80, 7, 8],
        &Some(SurfaceNamedValue::CompactIntArray(vec![128, 7, 8])),
    );
    // Two encoded integers and the existing scalar fallback dispatch. The
    // absent third and fourth integers cause no visit or allocation.
    check_steps("dum_array", &[0xf8, 4, 7, 8], &None);
    check_steps("dum_array", &[0xf8, 0x81, 1], &None);
}

#[test]
fn named_contiguous_references_admit_exact_generated_range_before_writes() {
    for count in [0_usize, 1, 4, 7, 17, 257] {
        let mut body = vec![0xf8];
        body.extend(compact_count(count));
        body.extend_from_slice(&[0xf7, 0x80, 0x80, 0xfb]);

        let end = 128 + u32::try_from(count).expect("small count");
        check_steps(
            "c_pnts",
            &body,
            &Some(SurfaceNamedValue::ContiguousEntityReferences(
                (128..end).collect(),
            )),
        );
    }
}

#[test]
fn named_scalar_sequence_dispatch_stops_at_first_boundary_and_keeps_interiors() {
    for count in [0_usize, 1, 4, 7, 17, 257] {
        let body = vec![0xe4; count];

        let expected = if count == 0 {
            SurfaceNamedValue::Empty
        } else {
            SurfaceNamedValue::ScalarSequence(vec![1.0; count])
        };
        check_steps("data_dbls", &body, &Some(expected));
        let mut bounded = body;
        bounded.extend(std::iter::repeat_n(0xe3, 257));

        let expected = (count != 0).then(|| SurfaceNamedValue::ScalarSequence(vec![1.0; count]));
        check_steps("data_dbls", &bounded, &expected);
    }
    let raw = [0x46, 0x00, 0xe3, 0xe0, 0xf7, 0xe4, 0x0f, 0x18];
    let value = f64::from_bits(0x4000_e3e0_f7e4_0f18);
    check_steps(
        "data_dbls",
        &raw,
        &Some(SurfaceNamedValue::ScalarSequence(vec![value])),
    );
    check_steps(
        "radius",
        &[0x0d, 0x0e],
        &Some(SurfaceNamedValue::ScalarSequence(vec![0.25, 0.5])),
    );
}

#[test]
fn named_value_empty_and_fixed_metadata_routes_preserve_original_refusal() {
    for (name, body, expected) in [
        ("data_dbls", &[][..], Some(SurfaceNamedValue::Empty)),
        ("id", &[7][..], Some(SurfaceNamedValue::CompactInt(7))),
        (
            "flip",
            &[0xf1, 7][..],
            Some(SurfaceNamedValue::CompactInt(7)),
        ),
        ("flip", &[0xf1][..], None),
        (
            "offset_type",
            &[1, 0xf1, 0xf7, 2][..],
            Some(SurfaceNamedValue::CompactInt(1)),
        ),
        ("offset_type", &[1, 0xf1, 0xf7][..], None),
        ("data_dbls", &[0xf9][..], None),
    ] {
        check_steps(name, body, &expected);
    }
}
