// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;

use super::{attributes, Value};

const VALUE_TOLERANCE: f64 = 1.0e-12;

#[test]
fn logical_values_preserve_true_false_and_unknown() {
    for (bytes, expected) in [
        (&[0][..], Some(false)),
        (&[1][..], Some(true)),
        (&[2][..], None),
        (&[0xf6][..], None),
    ] {
        let mut view = View::over_retained(bytes);
        assert!(matches!(super::typed_value(&mut view, 0x35, false),
            Some(Value::Logical(actual)) if actual == expected));
        assert!(view.unread().is_empty());
    }
    for bytes in [&[][..], &[0xeb, 0, 0, 0][..]] {
        assert!(super::typed_value(&mut View::over_retained(bytes), 0x35, false).is_none());
    }
}

fn fixture() -> Vec<u8> {
    let prototype = b"\xe3\xe0\x0aname\0PART_NUMBER\0\xe0\x00value\0\xe2\xe0\x01type\0\x33\xe0\x0avalue(s_val)\0PART\0\xe0\x01ref_id\0\xf1\xf6\xe0\x01ref_type\0\x02\xe0\x0aref_name\0\xe1\xe0\x0aunit\0\xe1\xe0\x01designated\0\x01\xf1\xf7\x1b";
    let kind = b"\xe3PTC_GTOL_TYPE\0\xe2\x33\xf7\x1fPOSITION\0\x2a\x03\xe1\xe1\x00";
    let display = b"\xe3PTC_GTOL_1_TOL_STRING\0\xe2\x33\xe8\xe2\x8c\x800.75\0\x2a\x03\xe1\xe1\x00";
    let magnitude = b"\xe3PTC_GTOL_PRIMARY_TOL\0\xe2\x32\xe0\x02value(d_val)\0\x2a\xe8\x00\xf1\xf7\x20\x2a\x03\xe1\xe1\x00";
    let rows: [&[u8]; 4] = [prototype, kind, display, magnitude];
    let mut data = b"\xe0\x00attributes\0\xf4\x05\xf8".to_vec();
    data.push(u8::try_from(rows.len()).expect("small fixture"));
    data.extend_from_slice(b"\xf7\x1b\xfb");
    for row in rows {
        data.extend_from_slice(row);
    }
    data
}

#[test]
fn reads_counted_rows_with_a_non_utf8_display_prefix() {
    let data = fixture();
    crate::decode::with_service_context(|ctx| {
        let rows = attributes(ctx, View::over_retained(&data))
            .expect("allocation")
            .expect("complete table");
        let kind = rows[1].expect("type row");
        assert_eq!(kind.owner, Some(42));
        assert!(matches!(kind.value, Value::Text(b"POSITION")));
        let magnitude = rows[3].expect("magnitude row");
        assert!(
            matches!(magnitude.value, Value::Real(Some(value)) if (value - 0.75).abs() < VALUE_TOLERANCE)
        );
    });
}

#[test]
fn a_truncated_last_row_does_not_form_a_complete_table() {
    let mut data = fixture();
    data.pop();
    crate::decode::with_service_context(|ctx| {
        assert!(attributes(ctx, View::over_retained(&data))
            .expect("allocation")
            .is_none());
    });
}

#[test]
fn retains_the_attribute_unit() {
    let mut data = fixture();
    let row_end = data.len();
    data.splice(row_end - 2..row_end - 1, b"mm\0".iter().copied());
    crate::decode::with_service_context(|ctx| {
        let rows = attributes(ctx, View::over_retained(&data))
            .expect("allocation")
            .expect("complete table");
        assert_eq!(rows[3].expect("magnitude row").unit, b"mm");
    });
}
