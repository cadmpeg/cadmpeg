// SPDX-License-Identifier: Apache-2.0
//! Placement atom, frame, and wire admission invariants.

use super::*;

fn frame() -> Vec<u8> {
    let mut bytes = MAGIC_VALUE.to_vec();
    bytes.extend([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]);
    bytes.extend([0, 0, 0, 1, 63, 0xf0, 0, 0, 0]);
    bytes.extend(b"\x03\x03A\0\x03\x2601234567-89ab-cdef-0123-456789abcdef\0");
    bytes
}

#[test]
fn placement_atoms_use_truncated_binary64_and_preserve_signed_zero() {
    for (raw, value) in [
        (&[0][..], 0.0_f64),
        (&[1][..], 1.0),
        (&[0x80][..], -0.0),
        (&[0x81][..], -1.0),
        (&[0xd0, 0, 0, 0][..], -0.5),
        (&[0x50, 0x18, 0, 0][..], 1.5),
        (&[0x30, 0x28, 0, 0, 0, 0, 0, 0][..], 3.0),
    ] {
        let mut view = View::over_retained(raw);
        assert_eq!(matrix_atom(&mut view).unwrap().to_bits(), value.to_bits());
        assert!(view.is_empty());
        for length in 0..raw.len() {
            assert!(matrix_atom(&mut View::over_retained(&raw[..length])).is_none());
        }
    }
    assert!(matrix_atom(&mut View::over_retained(&[0x10])).is_none());
    assert!(matrix_atom(&mut View::over_retained(&[0x6f, 0xff, 0, 0])).is_none());
}

#[test]
fn placement_frame_requires_complete_text_terminators_and_proper_rotation() {
    let bytes = frame();
    let parsed = parse_frame(View::over_retained(&bytes)).unwrap();
    assert_eq!(parsed.length, bytes.len());
    assert_eq!(parsed.label, "A");
    assert_eq!(
        parsed.matrix,
        [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.]
    );
    for length in 0..bytes.len() {
        assert!(parse_frame(View::over_retained(&bytes[..length])).is_none());
    }
    let mut reflection = bytes.clone();
    reflection[6] = 0x81;
    assert!(parse_frame(View::over_retained(&reflection)).is_none());
    let mut malformed = bytes;
    *malformed.last_mut().unwrap() = 1;
    assert!(parse_frame(View::over_retained(&malformed)).is_none());
}

#[test]
fn placement_wire_rejects_derived_field_and_source_identity_changes() {
    let raw = frame();
    let wire = serde_json::json!({
        "id":"nx:assembly:placement#123", "raw_frame":NativeBytes::new(raw.as_slice()),
        "matrix":[1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0.],
        "label":"A", "uuid":"01234567-89ab-cdef-0123-456789abcdef",
        "source_entry":ENTRY_NAME, "source_offset":123,
    });
    let record: AssemblyPlacement = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), wire);
    let mut null_reference = wire.clone();
    null_reference["object_record"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<AssemblyPlacement>(null_reference).is_err());
    for (field, value) in [
        ("id", serde_json::json!("nx:assembly:placement#124")),
        (
            "source_entry",
            serde_json::json!("/Root/FastLoad/Structure"),
        ),
        ("label", serde_json::json!("B")),
        (
            "uuid",
            serde_json::json!("11234567-89ab-cdef-0123-456789abcdef"),
        ),
    ] {
        let mut changed = wire.clone();
        changed[field] = value;
        assert!(serde_json::from_value::<AssemblyPlacement>(changed).is_err());
    }
    let mut changed = wire;
    changed["matrix"][3] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<AssemblyPlacement>(changed).is_err());
}

#[test]
fn placement_object_binding_requires_one_complete_record_at_the_frame_start() {
    let raw = frame();
    let placement = |offset: u64| -> AssemblyPlacement {
        serde_json::from_value(serde_json::json!({
            "id":format!("nx:assembly:placement#{offset}"), "raw_frame":NativeBytes::new(raw.as_slice()),
            "matrix":[1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0.],
            "label":"A", "uuid":"01234567-89ab-cdef-0123-456789abcdef",
            "source_entry":ENTRY_NAME, "source_offset":offset,
        })).unwrap()
    };
    let object = |id: &str, offset: u64, length: usize| -> ObjectRecord {
        serde_json::from_value(serde_json::json!({
            "id":id, "object_id":17, "object_id_source_offset":16,
            "section_ordinal":0, "record_ordinal":0, "section_offset":0,
            "byte_len":length, "sha256":"0000000000000000000000000000000000000000000000000000000000000000",
            "source_entry":ENTRY_NAME, "source_offset":offset,
        })).unwrap()
    };
    let mut placements = vec![placement(100), placement(300), placement(500)];
    let records = vec![
        object("second", 300, raw.len()),
        object("first", 100, raw.len() + 7),
        object("short", 500, raw.len() - 1),
    ];
    bind_object_records(&mut placements, &records);
    assert_eq!(placements[0].object_record.as_deref(), Some("first"));
    assert_eq!(placements[1].object_record.as_deref(), Some("second"));
    assert_eq!(placements[2].object_record, None);

    let mut ambiguous = records;
    ambiguous.push(object("duplicate", 100, raw.len()));
    bind_object_records(&mut placements, &ambiguous);
    assert_eq!(placements[0].object_record, None);
    assert_eq!(placements[1].object_record.as_deref(), Some("second"));
}
