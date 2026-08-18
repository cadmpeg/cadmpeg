// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

fn valid_global_fields() -> Vec<String> {
    [
        "1H,",
        "1H;",
        "7Hproduct",
        "8Hpart.igs",
        "7Hcadmpeg",
        "3H0.1",
        "32",
        "38",
        "6",
        "308",
        "15",
        "0H",
        "1.0",
        "2",
        "2HMM",
        "1",
        "1.0",
        "15H20260714.000000",
        "0.001",
        "1000.0",
        "6Hauthor",
        "3Horg",
        "11",
        "0",
        "0H",
        "0H",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn parse_global_fields(fields: &[String]) -> Result<crate::global::Global, CodecError> {
    let mut global = fields.join(",");
    global.push(';');
    let bytes = fixed_ascii_with_global(global.as_bytes());
    crate::global::parse(&crate::card::scan(&bytes)?)
}

fn global_with_units(units_flag: i64, units_name: &str) -> Vec<u8> {
    let global = format!(
        "1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,{units_flag},{units_name},1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;"
    );
    fixed_ascii_with_global(global.as_bytes())
}

#[test]
fn inspect_parses_alternate_delimiters_and_cross_card_hollerith() {
    let product = format!("p^!,{}", "x".repeat(66));
    let global = format!(
        "1H^^1H!^70H{product}^8Hpart.igs^7Hcadmpeg^3H0.1^32^38^6^308^15^0H^1.0^2^2HMM^1^1.0^15H20260714.000000^0.001^1000.0^6Hauthor^3Horg^11^0^0H^0H!"
    );
    let bytes = fixed_ascii_with_global(global.as_bytes());

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"parameter_delimiter=^".into()));
    assert!(summary.notes.contains(&"record_delimiter=!".into()));
    assert!(summary.notes.contains(&format!("sender_product={product}")));
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(summary.notes.contains(&"units=MM".into()));
}

#[test]
fn global_strings_admit_printable_ascii_and_retain_invalid_bytes() {
    for invalid in [0xff, 0x1f, 0x7f] {
        let mut global = b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;".to_vec();
        let product = global
            .windows(3)
            .position(|window| window == b"1Hp")
            .expect("sender product");
        global[product + 2] = invalid;

        let bytes = fixed_ascii_with_global(&global);
        let scan = crate::card::scan(&bytes).unwrap();
        let parsed = crate::global::parse(&scan).unwrap();

        assert_eq!(parsed.sender_product(), None);
        assert_eq!(parsed.sender_product_bytes(), Some(&[invalid][..]));
        assert_eq!(parsed.invalid_string_fields().collect::<Vec<_>>(), vec![3]);

        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(bytes),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(summary
            .notes
            .contains(&"invalid_global_string_fields=3".into()));
    }
}

#[test]
fn global_defaults_apply_only_to_omitted_fields() {
    let global =
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,,,2HIN,1,1.0,15H20260714.000000,0,1,1Ha,1Ho,,0,0H,0H;";
    let bytes = fixed_ascii_with_global(global);
    let scan = crate::card::scan(&bytes).unwrap();
    let parsed = crate::global::parse(&scan).unwrap();

    assert_eq!(parsed.model_scale(), 1.0);
    assert_eq!(parsed.units_flag(), 1);
    assert_eq!(parsed.version_flag(), 3);
    assert_eq!(parsed.minimum_resolution_mm(), 0.0);
}

#[test]
fn global_field_categories_apply_defaults_and_require_no_default_fields() {
    let mut fields = valid_global_fields();
    for index in [11, 12, 13, 14, 15, 19, 20, 21, 22, 23, 24, 25] {
        fields[index].clear();
    }

    let parsed = parse_global_fields(&fields).unwrap();
    assert_eq!(parsed.model_scale(), 1.0);
    assert_eq!(parsed.units_flag(), 1);
    assert_eq!(parsed.version_flag(), 3);
    assert!((parsed.minimum_resolution_mm() - 0.0254).abs() <= f64::EPSILON * 64.0);
    assert_eq!(parsed.maximum_coordinate_mm(), 0.0);

    for index in [2, 3, 4, 5, 6, 7, 8, 9, 10, 16, 17, 18] {
        let mut fields = valid_global_fields();
        fields[index].clear();
        assert!(
            matches!(parse_global_fields(&fields), Err(CodecError::Malformed(_))),
            "field {} unexpectedly selected a default",
            index + 1
        );
    }
}

#[test]
fn malformed_global_values_do_not_select_defaults() {
    for (index, value) in [
        (2, "1"),
        (3, "1"),
        (4, "1"),
        (5, "1"),
        (6, "1Hx"),
        (7, "1Hx"),
        (8, "1Hx"),
        (9, "1Hx"),
        (10, "1Hx"),
        (11, "1"),
        (12, "1Hx"),
        (13, "1Hx"),
        (14, "1"),
        (15, "1Hx"),
        (16, "1Hx"),
        (17, "1"),
        (18, "1Hx"),
        (19, "1Hx"),
        (20, "1"),
        (21, "1"),
        (22, "1Hx"),
        (23, "1Hx"),
        (24, "1"),
        (25, "1"),
    ] {
        let mut fields = valid_global_fields();
        fields[index] = value.to_owned();
        assert!(
            matches!(parse_global_fields(&fields), Err(CodecError::Malformed(_))),
            "field {} unexpectedly selected a default",
            index + 1
        );
    }

    let mut fields = valid_global_fields();
    fields.push("0H".into());
    assert!(matches!(
        parse_global_fields(&fields),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
fn version_flags_clamp_unrecognized_values() {
    for (value, expected) in [("-1", 3), ("0", 3), ("12", 11), ("99", 11)] {
        let mut fields = valid_global_fields();
        fields[22] = value.into();
        let parsed = parse_global_fields(&fields).unwrap();
        assert_eq!(parsed.version_flag(), expected);
    }
}

#[test]
fn global_timestamps_and_scalar_ranges_follow_the_specification() {
    for (timestamp, valid) in [
        ("15H20260714.000000", true),
        ("13H240714.000000", true),
        ("15H20261314.000000", false),
        ("15H20260714.240000", false),
    ] {
        let mut fields = valid_global_fields();
        fields[17] = timestamp.into();
        assert_eq!(parse_global_fields(&fields).is_ok(), valid, "{timestamp}");
    }

    let mut fields = valid_global_fields();
    fields[24] = "15H20260714.000000".into();
    assert!(parse_global_fields(&fields).is_ok());

    for (index, value) in [(15, "0"), (19, "-1"), (23, "8")] {
        let mut fields = valid_global_fields();
        fields[index] = value.into();
        assert!(matches!(
            parse_global_fields(&fields),
            Err(CodecError::Malformed(_))
        ));
    }
}

#[test]
fn malformed_global_integer_does_not_select_its_default() {
    let global = b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,2.,2HMM,1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;";
    let error = IgesCodec
        .inspect(
            &mut Cursor::new(fixed_ascii_with_global(global)),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "malformed container: IGES Global: field 14 (units flag) is not an integer"
    );
}

#[test]
fn real_significance_fields_are_required_and_positive() {
    for (global, field) in [
        (
            b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;".as_slice(),
            9,
        ),
        (
            b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,0,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;".as_slice(),
            11,
        ),
    ] {
        let error = IgesCodec
            .inspect(
                &mut Cursor::new(fixed_ascii_with_global(global)),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap_err();

        assert!(
            error.to_string().contains(&format!("field {field}")),
            "{error}"
        );
    }
}

#[test]
fn standard_units_names_match_the_flag_three_table() {
    for (encoded_name, decoded_name, expected_factor_mm) in [
        ("2HIN", "IN", 25.4),
        ("4HINCH", "INCH", 25.4),
        ("2HMM", "MM", 1.0),
        ("2HFT", "FT", 304.8),
        ("2HMI", "MI", 1_609_344.0),
        ("1HM", "M", 1_000.0),
        ("2HKM", "KM", 1_000_000.0),
        ("3HMIL", "MIL", 0.0254),
        ("2HUM", "UM", 0.001),
        ("2HCM", "CM", 10.0),
        ("3HUIN", "UIN", 0.000_025_4),
    ] {
        let bytes = global_with_units(3, encoded_name);
        let scan = crate::card::scan(&bytes).unwrap();
        let parsed = crate::global::parse(&scan).unwrap();

        assert_eq!(parsed.units_name().as_deref(), Some(decoded_name));
        assert!((parsed.length_factor_mm() - expected_factor_mm).abs() <= f64::EPSILON * 16.0);
    }
}

#[test]
fn flag_three_requires_a_nonempty_name() {
    for units_name in ["", "0H"] {
        let bytes = global_with_units(3, units_name);
        let scan = crate::card::scan(&bytes).unwrap();
        let error = crate::global::parse(&scan).unwrap_err();

        match error {
            CodecError::Malformed(_) => {}
            other => panic!("{units_name:?}: expected malformed input, got {other}"),
        }
    }
}

#[test]
fn unknown_flag_three_unit_name_is_inspectable_but_not_projected() {
    let bytes = global_with_units(3, "3HFOO");
    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"units=FOO".into()));

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(matches!(error, CodecError::NotImplemented(_)));
}

#[test]
fn non_three_units_flags_ignore_inconsistent_names() {
    let bytes = global_with_units(2, "3HFOO");
    let scan = crate::card::scan(&bytes).unwrap();
    let parsed = crate::global::parse(&scan).unwrap();

    assert_eq!(parsed.units_flag(), 2);
    assert_eq!(parsed.units_name().as_deref(), Some("FOO"));
    assert!((parsed.length_factor_mm() - 1.0).abs() <= f64::EPSILON * 16.0);
}

#[test]
fn minimum_resolution_is_required_and_cannot_be_negative() {
    for (resolution, expected) in [
        ("", "field 19 (minimum resolution) has no value"),
        (
            "-0.001",
            "field 19 (minimum resolution) must be finite and nonnegative",
        ),
    ] {
        let global = format!(
            "1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,{resolution},1,1Ha,1Ho,11,0,0H,0H;"
        );
        let error = IgesCodec
            .inspect(
                &mut Cursor::new(fixed_ascii_with_global(global.as_bytes())),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn minimum_resolution_uses_a_strict_positive_boundary_and_exact_zero() {
    assert!(crate::global::coincident_distance(0.0, 0.0));
    assert!(!crate::global::coincident_distance(f64::EPSILON, 0.0));
    assert!(crate::global::coincident_distance(
        0.125 - f64::EPSILON,
        0.125
    ));
    assert!(!crate::global::coincident_distance(0.125, 0.125));
    assert!(!crate::global::coincident_distance(
        0.125 + f64::EPSILON,
        0.125
    ));
    assert!(!crate::global::coincident_distance(f64::NAN, 0.125));
    assert!(!crate::global::coincident_distance(0.0, -0.125));
}

#[test]
fn non_utf8_global_identifiers_are_preserved_as_exact_hex_attributes() {
    let mut bytes = point_file();
    let product = bytes
        .windows(9)
        .position(|window| window == b"7Hproduct")
        .expect("sender product");
    bytes[product + 5] = 0xff;
    let file_name = bytes
        .windows(10)
        .position(|window| window == b"8Hpart.igs")
        .expect("native file name");
    bytes[file_name + 4] = 0xfe;

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.clone()),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    assert_eq!(attributes["sender_product_bytes_hex"], "70726fff756374");
    assert_eq!(attributes["native_file_name_bytes_hex"], "7061fe742e696773");
    assert!(!attributes.contains_key("sender_product"));
    assert!(!attributes.contains_key("native_file_name"));

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn ascii_control_global_identifiers_are_retained_only_by_container_decode() {
    for (invalid, expected) in [(0x1f, "70726f1f756374"), (0x7f, "70726f7f756374")] {
        let mut bytes = point_file();
        let product = bytes
            .windows(9)
            .position(|window| window == b"7Hproduct")
            .expect("sender product");
        bytes[product + 5] = invalid;

        let result = IgesCodec
            .decode(
                &mut Cursor::new(bytes.clone()),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let attributes = &result.ir().source.as_ref().unwrap().attributes;
        assert_eq!(attributes["sender_product_bytes_hex"], expected);
        assert!(!attributes.contains_key("sender_product"));

        let error = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err();
        assert!(matches!(error, CodecError::Malformed(_)));
    }
}

#[test]
fn fixed_ascii_5_1_and_5_2_decode_under_the_supported_profile() {
    for (encoded_version, version_name) in [(b"09", "5.1"), (b"10", "5.2")] {
        let mut bytes = point_file();
        let version = bytes
            .windows(b",11,0,".len())
            .position(|window| window == b",11,0,")
            .unwrap();
        bytes[version + 1..version + 3].copy_from_slice(encoded_version);

        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(bytes.clone()),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(summary
            .notes
            .contains(&format!("iges_version={version_name}")));
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.ir().source.as_ref().unwrap().attributes["iges_version"],
            version_name
        );
        assert_eq!(result.ir().model.points.len(), 1);
        assert!(
            result.report().losses.is_empty(),
            "{version_name}: {:#?}",
            result.report().losses
        );
        assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok());
    }
}
