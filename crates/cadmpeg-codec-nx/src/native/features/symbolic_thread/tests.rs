// SPDX-License-Identifier: Apache-2.0
use super::SymbolicThreadConstruction;
use cadmpeg_core::serde_erase::json;

#[derive(serde::Serialize)]
struct Record<'a> {
    id: &'static str,
    construction: &'a SymbolicThreadConstruction,
}

fn wire() -> serde_json::Value {
    cadmpeg_core::json!({"parameter_class_index":3,"coordinates":[{"value":0.0,"raw":"00"},{"value":0.0,"raw":"00"},{"value":1.0,"raw":"2ff0000000000000"}],"tail_references":[{"object_index":0,"raw_object_index":"f000"},{"object_index":255,"raw_object_index":"f0ff"},{"object_index":256,"raw_object_index":"f10100"}],"tail_sentinel":true,"source_offset":100,"byte_len":30})
}

#[test]
fn symbolic_thread_wire_requires_exact_atoms_reference_widths_and_span() {
    let wire = wire();
    let construction: SymbolicThreadConstruction = json::from_value(wire.clone()).unwrap();
    assert_eq!(construction.point_mm(), [0., 0., 250.]);
    assert_eq!(json::to_value(&construction).unwrap(), wire);
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &Record {
            id: "nx:test:thread#0",
            construction: &construction,
        },
        cadmpeg_core::json!({"id":"nx:test:thread#0","construction":wire}),
    );
    for (key, value) in [
        ("parameter_class_index", cadmpeg_core::json!(0)),
        ("byte_len", cadmpeg_core::json!(29)),
        ("source_offset", cadmpeg_core::json!(u64::MAX)),
    ] {
        let mut invalid = wire.clone();
        invalid[key] = value;
        assert!(json::from_value::<SymbolicThreadConstruction>(invalid).is_err());
    }
    for coordinate in [
        cadmpeg_core::json!({"value":0.0,"raw":"0000"}),
        cadmpeg_core::json!({"value":2.0,"raw":"2ff0000000000000"}),
        cadmpeg_core::json!({"value":1.0,"raw":"4f800000"}),
    ] {
        let mut invalid = wire.clone();
        invalid["coordinates"][0] = coordinate;
        assert!(json::from_value::<SymbolicThreadConstruction>(invalid).is_err());
    }
    let mut invalid = wire;
    invalid["tail_references"][0] = cadmpeg_core::json!({"object_index":0,"raw_object_index":"00"});
    assert!(json::from_value::<SymbolicThreadConstruction>(invalid).is_err());
}
