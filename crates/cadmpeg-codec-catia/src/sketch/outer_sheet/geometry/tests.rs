// SPDX-License-Identifier: Apache-2.0
use super::{chart, flags, integer, line, point, MalformedField};
use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};

const DATA_BLOC_INDEX: u32 = 5;

fn entity(payload: Vec<u8>) -> CatiaEntityRecord {
    CatiaEntityRecord {
        id: "entity".to_owned(),
        object_graph: "graph".to_owned(),
        object_record: "object".to_owned(),
        ordinal: 0,
        byte_offset: 0,
        lead: 0,
        body: CatiaEntityRecordBody::Nested {
            definition_prefix: Vec::new(),
            definition_suffix: Vec::new(),
            value_payload: payload,
            record_suffix: Vec::new(),
        },
        definition_schema_selections: Vec::new(),
        entity_id: 1,
        value_schema_selections: Vec::new(),
        object_production: None,
        value_production: None,
        range_interval: None,
        reference_signature: None,
        suffix: None,
        suffix_schema_selection: None,
    }
}

fn scalar(payload: &mut Vec<u8>, value: f64) {
    match value.to_bits() {
        bits if bits == 0.0_f64.to_bits() => payload.push(0xe8),
        bits if bits == 1.0_f64.to_bits() => payload.push(0xe7),
        bits if bits == (-1.0_f64).to_bits() => payload.push(0xe9),
        _ => {
            payload.push(0xe6);
            payload.extend(value.to_le_bytes());
        }
    }
}

fn program(values: &[f64], value_type: u16, layout: u8, wide: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    if wide {
        payload.push(0x32);
        payload.extend((DATA_BLOC_INDEX + 1).to_le_bytes());
    } else {
        payload.push(0x34 + u8::try_from(DATA_BLOC_INDEX).expect("compact selector"));
    }
    payload.extend([0x86, 0xe8]);
    payload.extend(value_type.to_le_bytes());
    payload.extend([0x37, 0x80 + layout, 0x81]);
    for value in values {
        scalar(&mut payload, *value);
    }
    payload.push(0xfe);
    payload
}

fn frame(values: &[f64], wide: bool) -> Vec<u8> {
    program(values, 0x07e0, 10, wide)
}

fn sample_chart() -> [f64; 9] {
    [0.0, -1.0, 0.0, 0.6, 0.0, 0.8, 3.25, -7.5, 12.125]
}

#[test]
fn translated_rotated_chart_preserves_all_nine_source_values() {
    let values = sample_chart();
    for wide in [false, true] {
        let (origin, horizontal, vertical) =
            chart(&entity(frame(&values, wide)), DATA_BLOC_INDEX).expect("complete chart");
        let actual = [
            horizontal.x,
            horizontal.y,
            horizontal.z,
            vertical.x,
            vertical.y,
            vertical.z,
            origin.x,
            origin.y,
            origin.z,
        ];
        assert_eq!(actual.map(f64::to_bits), values.map(f64::to_bits));
        let normal = horizontal.cross(vertical);
        assert!((normal.x + 0.8).abs() < 1.0e-12);
        assert!((normal.z - 0.6).abs() < 1.0e-12);
    }
    let negative_axis = [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 7.25, -3.5, 11.75];
    assert!(chart(&entity(frame(&negative_axis, true)), DATA_BLOC_INDEX).is_some());
}

#[test]
fn point_controls_and_binary64_preserve_coordinates_and_signed_zero() {
    for values in [[0.0, -1.0], [1.0, 2.25], [-0.0, -7.5]] {
        let value = point(&entity(program(&values, 0x07e4, 3, false)), DATA_BLOC_INDEX)
            .expect("finite point");
        assert_eq!(
            [value.u.to_bits(), value.v.to_bits()],
            values.map(f64::to_bits)
        );
    }
}

#[test]
fn line_preserves_native_origin_direction_and_trim_interval() {
    let values = [3.25, -7.5, 0.6, 0.8, -4.25, 12.125, 1.0];
    for wide in [false, true] {
        let (origin, direction, interval) =
            line(&entity(program(&values, 0x07e4, 8, wide)), DATA_BLOC_INDEX)
                .expect("complete directed carrier");
        let actual = [
            origin.u,
            origin.v,
            direction.u,
            direction.v,
            interval[0],
            interval[1],
        ];
        assert_eq!(
            actual.map(f64::to_bits),
            [values[0], values[1], values[2], values[3], values[4], values[5]].map(f64::to_bits)
        );
    }
}

#[test]
fn selector_is_source_scoped_and_all_counter_forms_are_bounded() {
    let compact = frame(&sample_chart(), false);
    assert!(chart(&entity(compact.clone()), DATA_BLOC_INDEX + 1).is_none());
    for counter in [
        vec![0x81],
        vec![0xd5, 0x42],
        vec![0x80, 0xfe, 0x39, 0x32, 0x00],
    ] {
        let mut payload = compact.clone();
        payload.splice(1..2, counter);
        assert!(chart(&entity(payload), DATA_BLOC_INDEX).is_some());
    }
    let mut inline = entity(compact.clone());
    inline.body = CatiaEntityRecordBody::Inline(compact);
    assert!(chart(&inline, DATA_BLOC_INDEX).is_none());
}

#[test]
fn binary64_and_byte_string_decoys_do_not_select_geometry_fields() {
    for bits in [
        0x3ff0_0000_0000_0632,
        0x3ff0_818a_3707_e0e8,
        0x3ff0_e886_39fe_0000,
    ] {
        let mut values = sample_chart();
        values[6] = f64::from_bits(bits);
        let (origin, _, _) = chart(&entity(frame(&values, true)), DATA_BLOC_INDEX)
            .expect("binary64 remains one scalar");
        assert_eq!(origin.x.to_bits(), bits);
    }
    let valid = frame(&sample_chart(), false);
    let mut blob = vec![0xe5];
    blob.extend(
        u32::try_from(valid.len())
            .expect("fixture length")
            .to_le_bytes(),
    );
    blob.extend(&valid);
    assert!(chart(&entity(blob.clone()), DATA_BLOC_INDEX).is_none());
    blob.push(0xfe);
    blob.extend(&valid);
    assert!(chart(&entity(blob), DATA_BLOC_INDEX).is_some());

    let mut other_field = program(&[2.25, 3.5], 0x07e4, 3, false);
    other_field[0] += 1;
    assert!(point(&entity(other_field.clone()), DATA_BLOC_INDEX).is_none());
    other_field.extend(program(&[1.0, -1.0], 0x07e4, 3, true));
    assert!(point(&entity(other_field), DATA_BLOC_INDEX).is_some());
}

#[test]
fn field_requires_exact_type_layout_scalar_count_and_closure() {
    let values = sample_chart();
    for payload in [
        program(&values, 0x07e4, 10, false),
        program(&values, 0x07e0, 9, false),
        frame(&values[..8], false),
        frame(&[values.as_slice(), &[1.25]].concat(), false),
    ] {
        assert!(chart(&entity(payload), DATA_BLOC_INDEX).is_none());
    }
    let mut extra_atom = frame(&values, false);
    extra_atom.insert(extra_atom.len() - 1, 0x81);
    assert!(chart(&entity(extra_atom), DATA_BLOC_INDEX).is_none());
    let mut wrong_value = frame(&values, false);
    let value_at = wrong_value
        .windows(3)
        .position(|bytes| bytes == [0x37, 0x8a, 0x81])
        .expect("typed packet")
        + 2;
    wrong_value[value_at] = 0x82;
    assert!(chart(&entity(wrong_value), DATA_BLOC_INDEX).is_none());
    let mut unknown = frame(&values, false);
    let component_at = value_at + 1;
    unknown[component_at] = 0xea;
    assert!(chart(&entity(unknown), DATA_BLOC_INDEX).is_none());
}

#[test]
fn truncated_and_nonfinite_programs_have_no_geometry() {
    for valid in [
        frame(&sample_chart(), true),
        program(&[3.25, -7.5], 0x07e4, 3, false),
        program(&[3.25, -7.5, 0.6, 0.8, -4.25, 12.125, 1.0], 0x07e4, 8, true),
    ] {
        for end in 0..valid.len() {
            let partial = entity(valid[..end].to_vec());
            assert!(chart(&partial, DATA_BLOC_INDEX).is_none(), "end {end}");
            assert!(point(&partial, DATA_BLOC_INDEX).is_none(), "end {end}");
            assert!(line(&partial, DATA_BLOC_INDEX).is_none(), "end {end}");
        }
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut values = sample_chart();
        values[6] = value;
        assert!(chart(&entity(frame(&values, false)), DATA_BLOC_INDEX).is_none());
        assert!(point(
            &entity(program(&[value, 2.25], 0x07e4, 3, false)),
            DATA_BLOC_INDEX
        )
        .is_none());
    }
}

#[test]
fn repeated_selected_fields_are_ambiguous_and_malformed_fields_do_not_fall_through() {
    let valid = frame(&sample_chart(), false);
    let mut duplicates = valid.clone();
    duplicates.extend(&valid);
    assert!(chart(&entity(duplicates), DATA_BLOC_INDEX).is_none());
    let mut malformed = frame(&sample_chart()[..8], true);
    malformed.extend(valid);
    assert!(chart(&entity(malformed), DATA_BLOC_INDEX).is_none());
}

#[test]
fn invalid_chart_basis_and_line_invariants_have_no_geometry() {
    for (slot, value) in [(0, 1.0), (1, 0.0), (3, 2.0), (4, -1.0)] {
        let mut values = sample_chart();
        values[slot] = value;
        assert!(chart(&entity(frame(&values, false)), DATA_BLOC_INDEX).is_none());
    }
    let valid = [3.25, -7.5, 0.6, 0.8, -4.25, 12.125, 1.0];
    for (slot, value) in [
        (2, 0.0),
        (3, 2.0),
        (4, 13.0),
        (5, -5.0),
        (6, 0.0),
        (4, f64::NAN),
    ] {
        let mut values = valid;
        values[slot] = value;
        assert!(line(&entity(program(&values, 0x07e4, 8, false)), DATA_BLOC_INDEX).is_none());
    }
}

#[test]
fn inline_field_contents_cannot_start_a_selected_field() {
    let mut foreign = vec![0x3a, 0x86, 0xe8, 0xe0, 0x1a, 0x37, 0x8e, 0xef, 0x84];
    foreign.extend([0xfe, 0x39, 0x86, 0xe8, 0xe4, 0x07, 0x37, 0x83]);
    foreign.extend([0x81, 0xe7, 0xe8, 0xfe]);
    assert!(point(&entity(foreign.clone()), DATA_BLOC_INDEX).is_none());
    foreign.extend(program(&[3.25, -7.5], 0x07e4, 3, false));
    let actual = point(&entity(foreign), DATA_BLOC_INDEX).expect("separate selected field");
    assert_eq!(
        [actual.u.to_bits(), actual.v.to_bits()],
        [3.25_f64.to_bits(), (-7.5_f64).to_bits()]
    );

    for size in 1..=8_u8 {
        let mut truncated = vec![0x8e, 0xe7 + size, 0x84];
        truncated.extend(vec![0xfe; usize::from(size - 1)]);
        assert!(chart(&entity(truncated), DATA_BLOC_INDEX).is_none());
    }
}

fn scalar_field(value_type: u16, body: &[u8]) -> Vec<u8> {
    let mut payload = vec![0x39, 0x86, 0xe8];
    payload.extend(value_type.to_le_bytes());
    payload.push(0x37);
    payload.extend(body);
    payload.push(0xfe);
    payload
}

#[test]
fn source_integer_and_flag_fields_preserve_signed_and_raw_values() {
    for (body, expected) in [(vec![0x81], 0), (vec![0x82], 1), (vec![0xd0], 79)] {
        assert_eq!(
            integer(&entity(scalar_field(0x1cc0, &body)), DATA_BLOC_INDEX),
            Some(expected)
        );
    }
    for expected in [i32::MIN, -1, 0, i32::MAX] {
        let mut body = vec![0x80];
        body.extend(expected.to_le_bytes());
        assert_eq!(
            integer(&entity(scalar_field(0x1cc0, &body)), DATA_BLOC_INDEX),
            Some(expected)
        );
    }
    for expected in [0, 1, 2, 0x48, 0x4a, 0xfe, 0xff] {
        let payload = scalar_field(0x0ae0, &[0x82, 0x81, expected]);
        assert_eq!(
            flags(&entity(payload.clone()), DATA_BLOC_INDEX),
            Ok(Some(expected))
        );
        assert_eq!(flags(&entity(Vec::new()), DATA_BLOC_INDEX), Ok(None));
        for end in 1..payload.len() {
            assert_eq!(
                flags(&entity(payload[..end].to_vec()), DATA_BLOC_INDEX),
                Err(MalformedField)
            );
        }
    }
    assert!(integer(
        &entity(scalar_field(0x0ae0, &[0x82, 0x81, 1])),
        DATA_BLOC_INDEX
    )
    .is_none());
    assert!(flags(&entity(scalar_field(0x1cc0, &[0x82])), DATA_BLOC_INDEX).is_err());
    assert!(flags(
        &entity(scalar_field(0x0ae0, &[0x83, 0x81, 1])),
        DATA_BLOC_INDEX
    )
    .is_err());
    assert!(integer(
        &entity(scalar_field(0x1cc0, &[0x80, 0xff, 0xff, 0xff])),
        DATA_BLOC_INDEX
    )
    .is_none());
}

#[test]
fn absent_flags_and_invalid_or_duplicate_flags_remain_distinct() {
    let other = program(&[3.25, -7.5], 0x07e4, 3, false);
    assert_eq!(flags(&entity(other), DATA_BLOC_INDEX + 1), Ok(None));
    let valid = scalar_field(0x0ae0, &[0x82, 0x81, 0x02]);
    let mut duplicate = valid.clone();
    duplicate.extend(&valid);
    assert_eq!(
        flags(&entity(duplicate), DATA_BLOC_INDEX),
        Err(MalformedField)
    );
    let mut malformed = scalar_field(0x0ae0, &[0x83, 0x81, 0x02]);
    malformed.extend(valid);
    assert_eq!(
        flags(&entity(malformed), DATA_BLOC_INDEX),
        Err(MalformedField)
    );
}
