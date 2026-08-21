// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{self, Layout};
use crate::loss::CreoLossCode;
use crate::test_support::*;
use crate::CreoCodec;

use super::*;

#[test]
fn scan_resolves_declarations_values_and_continuations() {
    let data = b"#P_OBJECT 6\n@root 1 0\n0 1 ->\n@matrix 2 2\n1 2 [2][2]\n\
                     $3FF,0\n$0,3FF\n#END_OF_UGC\n";

    let persistence = scan(data, std::iter::once(0..data.len()));
    let scope = &persistence.scopes[0];

    assert_eq!(scope.declarations.len(), 2);
    assert_eq!(scope.declarations[1].name, "matrix");
    assert_eq!(scope.declarations[1].type_code, 2);
    assert_eq!(scope.values.len(), 2);
    assert_eq!(&data[scope.values[0].payload.clone()], b"->");
    assert_eq!(&data[scope.values[1].payload.clone()], b"[2][2]");
    assert_eq!(scope.values[1].continuation_count, 2);
    let continuation = scope.values[1]
        .continuation_rows
        .clone()
        .expect("continuations");
    assert_eq!(&data[continuation], b"$3FF,0\n$0,3FF");
    assert_eq!(persistence.unresolved_value_count(), 0);
    assert_eq!(persistence.conflicting_declaration_count(), 0);
}

#[test]
fn scan_resolves_identifiers_within_independent_scopes() {
    let data = b"@field 7 1\n1 7 4\n@other 7 2\n2 7 5\n";
    let second = data
        .windows(b"@other".len())
        .position(|window| window == b"@other")
        .expect("second scope");

    let persistence = scan(data, [0..second, second..data.len()]);

    assert_eq!(persistence.scopes.len(), 2);
    assert_eq!(persistence.declaration_count(), 2);
    assert_eq!(persistence.value_count(), 2);
    assert_eq!(persistence.conflicting_declaration_count(), 0);
    assert_eq!(persistence.real_values.len(), 1);
    assert_eq!(persistence.real_values[0].scope_offset, second);
}

#[test]
fn principal_unit_requires_one_complete_known_type_10_scalar() {
    let millimeter = b"@principal_sys_units 25 10\n2 25 millimeter Newton Second (mmNs)\n";
    let persistence = scan(millimeter, std::iter::once(0..millimeter.len()));
    assert_eq!(
        persistence.principal_unit_system(),
        Some(PrincipalUnitSystem::MillimeterNewtonSecond)
    );
    assert_eq!(
        persistence
            .principal_unit_system()
            .and_then(PrincipalUnitSystem::length_scale_mm),
        Some(1.0)
    );

    let inch = b"@principal_sys_units 25 10\n2 25 Inch lbm Second (Pro/E Default)\n";
    let persistence = scan(inch, std::iter::once(0..inch.len()));
    assert_eq!(
        persistence.principal_unit_system(),
        Some(PrincipalUnitSystem::InchPoundMassSecond)
    );
    assert_eq!(
        persistence
            .principal_unit_system()
            .and_then(PrincipalUnitSystem::length_scale_mm),
        Some(25.4)
    );

    let mut repeated = millimeter.to_vec();
    repeated.extend_from_slice(millimeter);
    let persistence = scan(&repeated, std::iter::once(0..repeated.len()));
    assert_eq!(persistence.principal_unit_system(), None);
}

#[test]
fn type_2_reals_decode_compact_bits_runs_and_child_rows() {
    let data = b"@scalar 1 2\n0 1 3FF\n\
            @scale 2 2\n0 2 40396R\n\
            @matrix 3 2\n0 3 [2][2]\n$3FF,2*0,\n$3FF\n\
            @single 4 2\n0 4 [1]\n1 4 400\n";
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.real_values.len(), 4);
    assert_eq!(persistence.unresolved_real_value_count, 0);
    assert_eq!(
        persistence.real_values[0].payload,
        RealPayload::Scalar {
            value: Real(1.0f64.to_bits())
        }
    );
    assert_eq!(
        persistence.real_values[1].payload,
        RealPayload::Scalar {
            value: Real(25.4f64.to_bits())
        }
    );
    assert_eq!(
        persistence.real_values[2].payload,
        RealPayload::Array {
            dimensions: vec![2, 2],
            runs: vec![
                RealRun {
                    count: 1,
                    value: Real(1.0f64.to_bits()),
                },
                RealRun {
                    count: 2,
                    value: Real(0.0f64.to_bits()),
                },
                RealRun {
                    count: 1,
                    value: Real(1.0f64.to_bits()),
                },
            ],
        }
    );
    assert_eq!(persistence.real_values[2].payload.element_count(), 4);
    assert_eq!(
        persistence.real_values[3].payload,
        RealPayload::Array {
            dimensions: vec![1],
            runs: vec![RealRun {
                count: 1,
                value: Real(2.0f64.to_bits()),
            }],
        }
    );
}

#[test]
fn type_2_reals_withhold_incomplete_or_nonfinite_values() {
    let data = b"@short 1 2\n0 1 [3]\n$2*0\n\
            @lower 2 2\n0 2 3ff\n\
            @infinite 3 2\n0 3 7FF\n";
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert!(persistence.real_values.is_empty());
    assert_eq!(persistence.unresolved_real_value_count, 3);
}

#[test]
fn type_1_integers_decode_signed_scalars_runs_and_child_rows() {
    let data = b"@minimum 1 1\n0 1 -2147483648\n\
            @array 2 1\n0 2 [4]\n$1,2*-1,0\n\
            @single 3 1\n0 3 [1]\n1 3 42\n";
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.integer_values.len(), 3);
    assert_eq!(persistence.unresolved_integer_value_count, 0);
    assert_eq!(
        persistence.integer_values[0].payload,
        IntegerPayload::Scalar { value: i32::MIN }
    );
    assert_eq!(
        persistence.integer_values[1].payload,
        IntegerPayload::Array {
            dimensions: vec![4],
            runs: vec![
                IntegerRun { count: 1, value: 1 },
                IntegerRun {
                    count: 2,
                    value: -1,
                },
                IntegerRun { count: 1, value: 0 },
            ],
        }
    );
    assert_eq!(
        persistence.integer_values[2].payload,
        IntegerPayload::Array {
            dimensions: vec![1],
            runs: vec![IntegerRun {
                count: 1,
                value: 42,
            }],
        }
    );
}

#[test]
fn type_1_integers_withhold_incomplete_arrays_and_overflow() {
    let data = b"@short 1 1\n0 1 [2]\n$0\n\
            @overflow 2 1\n0 2 2147483648\n";
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert!(persistence.integer_values.is_empty());
    assert_eq!(persistence.unresolved_integer_value_count, 2);
}

#[test]
fn remaining_numeric_types_decode_their_scalar_and_array_grammars() {
    let data = b"@root 1 0\n@five 2 5\n@five_array 3 5\n@six 4 6\n@six_array 5 6\n\
            @seven 6 7\n@seven_array 7 7\n@nine 8 9\n@eleven 9 11\n@eleven_single 10 11\n\
            0 1 ->\n1 2 2700\n1 3 [3]\n$0,2*144\n1 4 400\n1 5 [2][2]\n$3FF,3*0\n\
            1 6 7\n1 7 [4]\n$0,1,2,8\n1 8 [4]\n$0,67108864,2*1\n\
            1 9 [3]\n$3,4,5\n1 10 [1]\n2 10 14633\n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.type_5_values.len(), 2);
    assert_eq!(persistence.unresolved_type_5_value_count, 0);
    assert_eq!(
        persistence.type_5_values[1].payload,
        UnsignedPayload::Array {
            dimensions: vec![3],
            runs: vec![
                NumericRun { count: 1, value: 0 },
                NumericRun {
                    count: 2,
                    value: 144,
                },
            ],
        }
    );
    assert_eq!(persistence.type_6_values.len(), 2);
    assert_eq!(persistence.unresolved_type_6_value_count, 0);
    assert_eq!(
        persistence.type_6_values[0].payload,
        RealPayload::Scalar {
            value: Real(2.0f64.to_bits())
        }
    );
    assert_eq!(persistence.type_7_values.len(), 2);
    assert_eq!(persistence.unresolved_type_7_value_count, 0);
    assert_eq!(persistence.type_9_values.len(), 1);
    assert_eq!(persistence.unresolved_type_9_value_count, 0);
    assert_eq!(persistence.type_11_values.len(), 2);
    assert_eq!(persistence.unresolved_type_11_value_count, 0);
    assert_eq!(
        persistence.type_11_values[1].payload,
        UnsignedPayload::Array {
            dimensions: vec![1],
            runs: vec![NumericRun {
                count: 1,
                value: 14633,
            }],
        }
    );
    assert_eq!(
        persistence.type_11_values[1].parent.as_deref(),
        Some(object_node_id(root_offset).as_str())
    );
}

#[test]
fn remaining_numeric_types_withhold_undefined_values() {
    let data = b"@negative 1 5\n0 1 -1\n@nonfinite 2 6\n0 2 7FF\n\
            @short 3 11\n0 3 [2]\n$1\n";
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert!(persistence.type_5_values.is_empty());
    assert_eq!(persistence.unresolved_type_5_value_count, 1);
    assert!(persistence.type_6_values.is_empty());
    assert_eq!(persistence.unresolved_type_6_value_count, 1);
    assert!(persistence.type_11_values.is_empty());
    assert_eq!(persistence.unresolved_type_11_value_count, 1);
}

#[test]
fn type_3_and_type_4_decode_exact_scalar_bytes() {
    let data = b"@root 1 0\n@three_null 2 3\n@three_text 3 3\n@three_bytes 4 3\n\
            @four_null_text 5 4\n@four_empty 6 4\n@continued 7 3\n0 1 ->\n1 2 NULL\n\
            1 3 texture-name\n1 4 \xff\n1 5 NULL\n1 6\n1 7 first\n$second\n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.type_3_values.len(), 3);
    assert_eq!(persistence.unresolved_type_3_value_count, 1);
    assert_eq!(persistence.type_3_values[0].payload, StringValue::Null);
    assert_eq!(
        persistence.type_3_values[1].payload,
        StringValue::Utf8 {
            text: "texture-name".to_string(),
        }
    );
    assert_eq!(
        persistence.type_3_values[2].payload,
        StringValue::Bytes { bytes: vec![0xff] }
    );
    assert_eq!(
        persistence.type_3_values[0].parent.as_deref(),
        Some(object_node_id(root_offset).as_str())
    );

    assert_eq!(persistence.type_4_values.len(), 2);
    assert_eq!(persistence.unresolved_type_4_value_count, 0);
    assert_eq!(
        persistence.type_4_values[0].payload,
        StringValue::Utf8 {
            text: "NULL".to_string(),
        }
    );
    assert_eq!(
        persistence.type_4_values[1].payload,
        StringValue::Utf8 {
            text: String::new(),
        }
    );
}

#[test]
fn type_10_strings_decode_null_bytes_and_direct_element_arrays() {
    let data = b"@root 1 0\n@label 2 10\n@empty 3 10\n@missing 4 10\n\
            @encoded 5 10\n@names 6 10\n0 1 ->\n1 2 alpha beta\n1 3 \n1 4 NULL\n\
            1 5 \xE9\n1 6 [2][81]\n2 6 first\n2 6 \n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.string_values.len(), 5);
    assert_eq!(persistence.incomplete_string_array_count, 0);
    assert_eq!(persistence.unresolved_string_value_count, 0);
    assert_eq!(
        persistence.string_values[0].payload,
        StringPayload::Scalar {
            value: StringValue::Utf8 {
                text: "alpha beta".to_string()
            }
        }
    );
    assert_eq!(
        persistence.string_values[1].payload,
        StringPayload::Scalar {
            value: StringValue::Utf8 {
                text: String::new()
            }
        }
    );
    assert_eq!(
        persistence.string_values[2].payload,
        StringPayload::Scalar {
            value: StringValue::Null
        }
    );
    assert_eq!(
        persistence.string_values[3].payload,
        StringPayload::Scalar {
            value: StringValue::Bytes { bytes: vec![0xe9] }
        }
    );
    assert_eq!(
        persistence.string_values[4].payload,
        StringPayload::Array {
            dimensions: vec![2, 81],
            values: vec![
                StringValue::Utf8 {
                    text: "first".to_string()
                },
                StringValue::Utf8 {
                    text: String::new()
                },
            ],
            complete: true,
        }
    );
    assert_eq!(persistence.string_values[4].payload.element_count(), 2);
    assert_eq!(
        persistence.string_values[3]
            .payload
            .undecoded_encoding_count(),
        1
    );
    assert_eq!(
        persistence.string_values[4].parent.as_deref(),
        Some(object_node_id(root_offset).as_str())
    );
}

#[test]
fn type_10_strings_retain_incomplete_arrays_and_withhold_continuations() {
    let data = b"@names 1 10\n0 1 [2]\n1 1 only\n\
            @continued 2 10\n0 2 first\n$second\n";
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.string_values.len(), 1);
    assert_eq!(persistence.incomplete_string_array_count, 1);
    assert_eq!(persistence.unresolved_string_value_count, 1);
    assert_eq!(
        persistence.string_values[0].payload,
        StringPayload::Array {
            dimensions: vec![2],
            values: vec![StringValue::Utf8 {
                text: "only".to_string()
            }],
            complete: false,
        }
    );
}

#[test]
fn type_0_objects_define_scoped_ownership_and_array_elements() {
    let data = b"@root 1 0\n@number 2 1\n@children 3 0\n@weight 4 2\n\
            0 1 ->\n1 2 7\n1 3 [2]\n2 3 ->\n3 4 3FF\n2 3 NULL\n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let array_offset = data
        .windows(b"1 3 [2]".len())
        .position(|window| window == b"1 3 [2]")
        .expect("array offset");
    let first_child_offset = data
        .windows(b"2 3 ->".len())
        .position(|window| window == b"2 3 ->")
        .expect("first child offset");
    let second_child_offset = data
        .windows(b"2 3 NULL".len())
        .position(|window| window == b"2 3 NULL")
        .expect("second child offset");
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.objects.len(), 4);
    assert_eq!(persistence.incomplete_object_array_count, 0);
    assert_eq!(persistence.unresolved_object_value_count, 0);
    assert_eq!(
        persistence.objects[1].parent.as_deref(),
        Some(object_node_id(root_offset).as_str())
    );
    assert_eq!(
        persistence.objects[1].payload,
        ObjectPayload::Array {
            dimensions: vec![2],
            elements: vec![
                object_node_id(first_child_offset),
                object_node_id(second_child_offset),
            ],
            complete: true,
        }
    );
    assert_eq!(persistence.integer_values.len(), 1);
    assert_eq!(
        persistence.integer_values[0].parent.as_deref(),
        Some(object_node_id(root_offset).as_str())
    );
    assert_eq!(persistence.real_values.len(), 1);
    assert_eq!(
        persistence.real_values[0].parent.as_deref(),
        Some(object_node_id(first_child_offset).as_str())
    );
    assert_eq!(persistence.objects[1].offset, array_offset);
}

#[test]
fn type_0_objects_retain_incomplete_and_opaque_forms() {
    let data = b"@array 1 0\n0 1 [2]\n@future 2 0\n0 2 token\n";
    let persistence = scan(data, std::iter::once(0..data.len()));

    assert_eq!(persistence.objects.len(), 2);
    assert_eq!(persistence.incomplete_object_array_count, 1);
    assert_eq!(persistence.unresolved_object_value_count, 1);
    assert!(matches!(
        persistence.objects[0].payload,
        ObjectPayload::Array {
            complete: false,
            ..
        }
    ));
    assert_eq!(
        persistence.objects[1].payload,
        ObjectPayload::Opaque {
            bytes: b"token".to_vec()
        }
    );
}

#[test]
fn scan_withholds_ambiguous_and_undeclared_values() {
    let data = b"#P_OBJECT 6\n$orphan\n@field 7 1\n@other 7 2\n1 7 4\n2 99 5\n";

    let persistence = scan(data, std::iter::once(0..data.len()));
    let scope = &persistence.scopes[0];

    assert!(scope.values.is_empty());
    assert_eq!(scope.declarations.len(), 1);
    assert_eq!(persistence.conflicting_declaration_count(), 1);
    assert_eq!(persistence.unresolved_value_count(), 2);
}

#[test]
fn scan_decodes_active_principal_unit() {
    let mut payload = visibgeom_payload(5, 12);
    payload.extend_from_slice(b"_principal_sys_units_id\0\x33");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data);

    assert_eq!(
        scan.framing
            .principal_unit
            .map(crate::legacy::PrincipalUnitSystem::token)
            .as_deref(),
        Some("mmNs")
    );
}

#[test]
fn legacy_principal_unit_sets_the_source_length_scale() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @root 28 0\n1 28 ->\n\
        @principal_sys_units 25 10\n2 25 Inch lbm Second (Pro/E Default)\n\
        @rel_accuracy 26 2\n2 26 3FF\n\
        @feat_id 27 1\n2 27 42\n\
        @encoded 29 10\n2 29 \xE9\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy unit decode");
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["principal_unit"], "inLbmS");
    assert_eq!(source.attributes["source_length_scale_mm"], "25.4");
    assert_eq!(
        result.report().coverage["decoded_legacy_principal_unit_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_real_scalar_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_real_element_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_integer_scalar_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_object_arrow_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_string_scalar_count"],
        2
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_string_element_count"],
        2
    );
    assert_eq!(
        result.report().coverage["undecoded_legacy_string_encoding_count"],
        1
    );
    let reals = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas["legacy_real_values"];
    assert_eq!(reals.len(), 1);
    assert_eq!(
        reals[0].field("name"),
        Some(serde_json::json!("rel_accuracy"))
    );
    assert_eq!(
        reals[0].field("payload"),
        Some(serde_json::json!({"form": "scalar", "value": 1.0}))
    );
    let integers = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas["legacy_integer_values"];
    assert_eq!(integers.len(), 1);
    assert_eq!(
        integers[0].field("name"),
        Some(serde_json::json!("feat_id"))
    );
    assert_eq!(
        integers[0].field("payload"),
        Some(serde_json::json!({"form": "scalar", "value": 42}))
    );
    let objects = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas["legacy_objects"];
    assert_eq!(objects.len(), 1);
    assert_eq!(
        integers[0].field("parent"),
        Some(serde_json::json!(objects[0].id()))
    );
    let strings = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas["legacy_string_values"];
    assert_eq!(strings.len(), 2);
    let principal = strings
        .iter()
        .find(|record| record.field("name") == Some(serde_json::json!("principal_sys_units")))
        .expect("principal-unit string");
    let encoded = strings
        .iter()
        .find(|record| record.field("name") == Some(serde_json::json!("encoded")))
        .expect("encoded string");
    assert_eq!(
        principal.field("payload"),
        Some(serde_json::json!({
            "form": "scalar",
            "value": {
                "form": "utf8",
                "text": "Inch lbm Second (Pro/E Default)"
            }
        }))
    );
    assert_eq!(
        encoded.field("payload"),
        Some(serde_json::json!({
            "form": "scalar",
            "value": {"form": "bytes", "bytes": [233]}
        }))
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == CreoLossCode::LegacyStringEncodingRetained.kind())
            .count(),
        1
    );
}

#[test]
fn legacy_numbered_numeric_families_emit_exact_native_values() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @root 1 0\n@five 2 5\n@five_array 3 5\n@six 4 6\n@six_array 5 6\n\
        @seven 6 7\n@nine 7 9\n@eleven 8 11\n0 1 ->\n1 2 2700\n\
        1 3 [3]\n$0,2*144\n1 4 400\n1 5 [2][2]\n$3FF,3*0\n1 6 7\n\
        1 7 [4]\n$0,67108864,2*1\n1 8 [1]\n2 8 14633\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy numbered numeric decode");
    assert_eq!(
        result.report().coverage["decoded_legacy_type_5_scalar_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_5_array_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_5_element_count"],
        4
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_6_scalar_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_6_array_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_6_element_count"],
        5
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_7_scalar_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_9_array_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_11_array_count"],
        1
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_11_element_count"],
        1
    );

    let native = result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace");
    assert_eq!(native.arenas["legacy_type_5_values"].len(), 2);
    assert_eq!(native.arenas["legacy_type_6_values"].len(), 2);
    assert_eq!(native.arenas["legacy_type_7_values"].len(), 1);
    assert_eq!(native.arenas["legacy_type_9_values"].len(), 1);
    assert_eq!(native.arenas["legacy_type_11_values"].len(), 1);
    assert_eq!(
        native.arenas["legacy_type_6_values"]
            .iter()
            .find(|record| record.field("name") == Some(serde_json::json!("six")))
            .and_then(|record| record.field("payload")),
        Some(serde_json::json!({"form": "scalar", "value": 2.0}))
    );
    assert_eq!(
        native.arenas["legacy_type_11_values"][0].field("payload"),
        Some(serde_json::json!({
            "form": "array",
            "dimensions": [1],
            "runs": [{"count": 1, "value": 14633}]
        }))
    );
    assert_eq!(
        native.arenas["legacy_type_11_values"][0].field("parent"),
        Some(serde_json::json!(native.arenas["legacy_objects"][0].id()))
    );
}

#[test]
fn legacy_type_3_and_type_4_emit_exact_scalar_bytes() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @root 1 0\n@three_null 2 3\n@three_text 3 3\n@four 4 4\n0 1 ->\n\
        1 2 NULL\n1 3 texture-name\n1 4 NULL\n#END_OF_P_OBJECT\n\
        #Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy type-3/type-4 decode");
    assert_eq!(
        result.report().coverage["decoded_legacy_type_3_scalar_count"],
        2
    );
    assert_eq!(
        result.report().coverage["decoded_legacy_type_4_scalar_count"],
        1
    );
    assert_eq!(
        result.report().coverage["unresolved_legacy_type_3_value_count"],
        0
    );
    assert_eq!(
        result.report().coverage["unresolved_legacy_type_4_value_count"],
        0
    );

    let native = result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace");
    assert_eq!(native.arenas["legacy_type_3_values"].len(), 2);
    assert_eq!(native.arenas["legacy_type_4_values"].len(), 1);
    assert_eq!(
        native.arenas["legacy_type_3_values"][0].field("payload"),
        Some(serde_json::json!({"form": "null"}))
    );
    assert_eq!(
        native.arenas["legacy_type_3_values"][1].field("payload"),
        Some(serde_json::json!({"form": "utf8", "text": "texture-name"}))
    );
    assert_eq!(
        native.arenas["legacy_type_4_values"][0].field("payload"),
        Some(serde_json::json!({"form": "utf8", "text": "NULL"}))
    );
    assert_eq!(
        native.arenas["legacy_type_4_values"][0].field("parent"),
        Some(serde_json::json!(native.arenas["legacy_objects"][0].id()))
    );
}

#[test]
fn incomplete_legacy_values_are_reported() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @values 7 2\n0 7 [2]\n$0\n\
        @ids 8 1\n0 8 [2]\n$1\n\
        @objects 9 0\n0 9 [2]\n\
        @future 10 0\n0 10 token\n\
        @names 11 10\n0 11 [2]\n1 11 only\n\
        @continued 12 10\n0 12 first\n$second\n\
        @type_three_continued 13 3\n0 13 first\n$second\n\
        @type_four_bytes 14 4\n0 14 \xff\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy incomplete-array decode");
    assert_eq!(
        result.report().coverage["unresolved_legacy_real_value_count"],
        1
    );
    assert_eq!(
        result.report().coverage["unresolved_legacy_integer_value_count"],
        1
    );
    assert_eq!(
        result.report().coverage["incomplete_legacy_object_array_count"],
        1
    );
    assert_eq!(
        result.report().coverage["unresolved_legacy_object_value_count"],
        1
    );
    assert_eq!(
        result.report().coverage["incomplete_legacy_string_array_count"],
        1
    );
    assert_eq!(
        result.report().coverage["unresolved_legacy_string_value_count"],
        1
    );
    assert_eq!(
        result.report().coverage["unresolved_legacy_type_3_value_count"],
        1
    );
    assert_eq!(
        result.report().coverage["undecoded_legacy_type_4_encoding_count"],
        1
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| { loss.code.taxonomy() == cadmpeg_ir::LossTaxonomy::RecordNotTyped })
            .count(),
        7
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == CreoLossCode::LegacyByteStringEncodingRetained.kind())
            .count(),
        1
    );
}

#[test]
fn complete_header_adjacent_p_object_selects_legacy_ascii_layout() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n\
        #P_OBJECT 6\n@P_object 1 0\n0 1 ->\n@value #END_OF_P_OBJECT\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";
    let scan = container::scan_bytes(data);

    assert_eq!(scan.framing.layout, Layout::LegacyAscii);
    assert_eq!(scan.framing.layout.token(), "LEGACY_ASCII");
    assert!(scan.framing.sections.is_empty());
    let legacy = scan.framing.legacy_ascii.as_ref().expect("legacy framing");
    assert_eq!(legacy.schema, "6");
    assert_eq!(legacy.product_release.as_deref(), Some("H-01-21"));
    assert_eq!(legacy.persistence.declaration_count(), 1);
    assert_eq!(legacy.persistence.value_count(), 1);
    assert!(container::summarize(&scan).notes.iter().any(|note| {
        note.contains("legacy ASCII persistence: schema 6; product release H-01-21")
    }));
}

#[test]
fn legacy_ascii_toc_is_authoritative_for_named_section_extents() {
    let mut data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n"
        .to_vec();
    let banner_offset = data.len();
    data.extend_from_slice(
        b"#Pro/ENGINEER  TM  Version H-01-21\n@Toc 52 0\n0 52 ->\n\
          @entry 53 10\n1 53 [2]\n",
    );
    let section = b"#BasicData\n@field 1 1\n0 1 4\n#FakeSection\nembedded";
    let row_tail = format!(" {:08x} 0 983####\n2 53 ####\n", section.len());
    let relative_offset =
        data.len() + b"2 53 BasicData ".len() + 8 + row_tail.len() - banner_offset;
    data.extend_from_slice(format!("2 53 BasicData {relative_offset:08x}{row_tail}").as_bytes());
    let section_offset = data.len();
    data.extend_from_slice(section);

    let scan = container::scan_bytes(data);

    assert_eq!(scan.framing.layout, Layout::LegacyAscii);
    assert_eq!(scan.framing.sections.len(), 1);
    assert_eq!(scan.framing.sections[0].name, "BasicData");
    assert_eq!(scan.framing.sections[0].offset, section_offset);
    assert_eq!(scan.framing.sections[0].length, section.len());
    let persistence = &scan
        .framing
        .legacy_ascii
        .as_ref()
        .expect("legacy framing")
        .persistence;
    assert_eq!(persistence.scopes.len(), 2);
    assert_eq!(persistence.declaration_count(), 3);
    assert_eq!(persistence.value_count(), 5);
}

#[test]
fn legacy_release_banner_and_unspecified_banner_preserve_framing_metadata() {
    let release = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 12\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Release 16.0  All Rights Reserved\n";
    let scan = container::scan_bytes(release.as_slice());
    let legacy = scan.framing.legacy_ascii.as_ref().expect("legacy framing");
    assert_eq!(legacy.schema, "12");
    assert_eq!(legacy.product_release.as_deref(), Some("16.0"));

    let result = CreoCodec
        .decode(&mut Cursor::new(release), &DecodeOptions::default())
        .expect("legacy container decode");
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["legacy_ascii_schema"], "12");
    assert_eq!(source.attributes["legacy_ascii_product_release"], "16.0");
    assert_eq!(source.attributes["legacy_ascii_declaration_count"], "0");
    assert_eq!(source.attributes["legacy_ascii_scope_count"], "1");
    assert_eq!(source.attributes["legacy_ascii_value_count"], "0");
    assert_eq!(
        source.attributes["legacy_ascii_conflicting_declaration_count"],
        "0"
    );

    let concatenated_release = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Release18.0  All Rights Reserved\n";
    let scan = container::scan_bytes(concatenated_release.as_slice());
    let legacy = scan.framing.legacy_ascii.as_ref().expect("legacy framing");
    assert_eq!(legacy.product_release.as_deref(), Some("18.0"));

    let unspecified = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER\n";
    let scan = container::scan_bytes(unspecified.as_slice());
    let legacy = scan.framing.legacy_ascii.as_ref().expect("legacy framing");
    assert_eq!(legacy.product_release, None);
}

#[test]
fn incomplete_or_payload_embedded_p_object_does_not_select_legacy_ascii_layout() {
    let incomplete = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n@P_object 1 0\n".to_vec();
    assert_eq!(
        container::scan_bytes(incomplete).framing.layout,
        Layout::Unknown
    );
    let empty_schema = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT \n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER";
    assert_eq!(
        container::scan_bytes(empty_schema).framing.layout,
        Layout::Unknown
    );

    let embedded = build_prt_raw(
        "c",
        &[(
            "VisibGeom",
            b"#P_OBJECT 6\n#END_OF_P_OBJECT\n#Pro/ENGINEER".to_vec(),
        )],
    );
    assert_eq!(
        container::scan_bytes(embedded).framing.layout,
        Layout::Unknown
    );
}
