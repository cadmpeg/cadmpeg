// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use std::fmt::Write as _;
use std::io::Cursor;

use crate::IgesCodec;

fn card(data: &[u8], section: u8, sequence: u32) -> Vec<u8> {
    card_with_ending(data, section, sequence, b"\n")
}

fn card_with_ending(data: &[u8], section: u8, sequence: u32, ending: &[u8]) -> Vec<u8> {
    assert!(data.len() <= 72);
    let mut card = vec![b' '; 80];
    card[..data.len()].copy_from_slice(data);
    card[72] = section;
    card[73..80].copy_from_slice(format!("{sequence:>7}").as_bytes());
    card.extend_from_slice(ending);
    card
}

#[test]
fn fixed_ascii_detection_requires_two_consistent_cards() {
    let mut valid = card(b"generated fixture", b'S', 1);
    valid.extend(card(b"", b'G', 1));
    assert_eq!(IgesCodec.detect(&valid), Confidence::High);

    assert_eq!(IgesCodec.detect(&valid[..81]), Confidence::No);

    let mut arbitrary = vec![b'x'; 72];
    arbitrary.extend_from_slice(b"S      1\nsecond line\n");
    assert_eq!(IgesCodec.detect(&arbitrary), Confidence::No);
}

#[test]
fn inspect_reports_sections_and_physical_line_endings() {
    let mut bytes = card_with_ending(b"original fixture", b'S', 1, b"\r\n");
    bytes.extend(card_with_ending(b"1H,,1H;,,;", b'G', 1, b"\n"));
    bytes.extend(card_with_ending(
        b"S0000001G0000001D0000000P0000000",
        b'T',
        1,
        b"\r",
    ));

    let summary = IgesCodec.inspect(&mut Cursor::new(bytes)).unwrap();

    assert_eq!(summary.format, "iges");
    assert_eq!(summary.container_kind, "fixed-ascii");
    assert_eq!(summary.entries.len(), 3);
    assert_eq!(summary.entries[0].name, "start");
    assert_eq!(summary.entries[0].attributes["line_endings"], "crlf:1");
    assert_eq!(summary.entries[1].attributes["line_endings"], "lf:1");
    assert_eq!(summary.entries[2].attributes["line_endings"], "cr:1");
}

fn fixed_ascii_with_global(global: &[u8]) -> Vec<u8> {
    let mut bytes = card(b"original fixture", b'S', 1);
    let chunks = global.chunks(72).collect::<Vec<_>>();
    for (index, chunk) in chunks.iter().enumerate() {
        bytes.extend(card(chunk, b'G', u32::try_from(index + 1).unwrap()));
    }
    bytes.extend(card(
        format!("S0000001G{:07}D0000000P0000000", chunks.len()).as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn directory_card(fields: [&str; 9], sequence: u32) -> Vec<u8> {
    let data = fields.into_iter().fold(String::new(), |mut data, field| {
        write!(data, "{field:>8}").unwrap();
        data
    });
    card(data.as_bytes(), b'D', sequence)
}

fn parameter_card(data: &[u8], directory_sequence: u32, sequence: u32) -> Vec<u8> {
    assert!(data.len() <= 64);
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    payload[64..72].copy_from_slice(format!("{directory_sequence:>8}").as_bytes());
    card(&payload, b'P', sequence)
}

fn parameter_cards(data: &[u8], directory_sequence: u32, first_sequence: u32) -> Vec<u8> {
    data.chunks(64)
        .enumerate()
        .flat_map(|(index, chunk)| {
            parameter_card(
                chunk,
                directory_sequence,
                first_sequence + u32::try_from(index).unwrap(),
            )
        })
        .collect()
}

#[test]
fn inspect_parses_alternate_delimiters_and_cross_card_hollerith() {
    let product = "p".repeat(70);
    let global = format!(
        "1H^^1H!^70H{product}^8Hpart.igs^7Hcadmpeg^3H0.1^32^38^6^308^15^0H^1.0^2^2HMM^1^1.0^15H20260714.000000^0.001^1000.0^6Hauthor^3Horg^11^0^0H^0H!"
    );
    let bytes = fixed_ascii_with_global(global.as_bytes());

    let summary = IgesCodec.inspect(&mut Cursor::new(bytes)).unwrap();

    assert!(summary.notes.contains(&"parameter_delimiter=^".into()));
    assert!(summary.notes.contains(&"record_delimiter=!".into()));
    assert!(summary.notes.contains(&format!("sender_product={product}")));
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(summary.notes.contains(&"units=MM".into()));
}

#[test]
fn inspect_reports_directory_entity_and_form_census() {
    let bytes = point_file();

    let summary = IgesCodec.inspect(&mut Cursor::new(bytes)).unwrap();

    assert!(summary.notes.contains(&"entities=1".into()));
    assert!(summary.notes.contains(&"entity.116.form.0=1".into()));
    assert!(summary.notes.contains(&"parameter_records=1".into()));
    assert!(summary.notes.contains(&"parameter_tokens=4".into()));
}

fn point_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"116,1.0,2.0,3.0;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn direction_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["123", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["123", "0", "0", "1", "0", "", "", "VECTOR", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"123,2,-3,4;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_retains_a_typed_dimensionless_direction() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report.geometry_transferred);
    assert!(result.report.losses.is_empty());
    let native = result.ir.native.namespace("iges").unwrap();
    assert_eq!(native.arenas["directions"].len(), 1);
    let components = native.arenas["directions"][0].fields["components"]
        .as_array()
        .unwrap();
    assert_eq!(components[0], 2.0);
    assert_eq!(components[1], -3.0);
    assert_eq!(components[2], 4.0);
    assert_eq!(
        native.arenas["directions"][0].fields["physically_dependent"],
        true
    );
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn line_file(form: i64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        [
            "110",
            "1",
            "0",
            "0",
            "4",
            "0",
            "0",
            "0",
            if form == 0 { "00000000" } else { "00000600" },
        ],
        1,
    ));
    bytes.extend(directory_card(
        ["110", "0", "0", "1", &form.to_string(), "", "", "LINE", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"110,1,2,3,4,6,3;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn circular_arc_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["100", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["100", "0", "0", "1", "0", "", "", "ARC", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"100,0,0,0,1,0,0,1;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn composite_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "110", "CHILD1", "00010000"),
        (3, 2, "110", "CHILD2", "00010000"),
        (5, 3, "102", "COMPOSIT", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,1,0,0;", 1, 1));
    bytes.extend(parameter_card(b"110,1,0,0,1,1,0;", 3, 2));
    bytes.extend(parameter_card(b"102,2,1,3;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_concatenates_ordered_composite_curve_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    let composite = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected a concatenated NURBS cache");
    };
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(nurbs.control_points.len(), 3);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &nurbs.knots, &nurbs.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn copious_data_file(form: i64, parameters: &[u8], status: &str) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["106", "1", "0", "0", "0", "0", "0", "0", status],
        1,
    ));
    bytes.extend(directory_card(
        [
            "106",
            "0",
            "0",
            &parameter_count.to_string(),
            &form.to_string(),
            "",
            "",
            "COPIOUS",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_projects_copious_linear_paths_with_segment_parameters() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                12,
                b"106,2,3,0,0,0,1,0,0,1,2,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(path) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a degree-one path carrier");
    };
    assert_eq!(path.degree, 1);
    assert_eq!(path.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &path.knots, &path.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0))
    );
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 2.0]));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_separates_copious_points_vectors_and_presentation_forms() {
    let points = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                3,
                b"106,3,2,1,2,3,0,0,1,4,5,6,1,0,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(points.ir.model.points.len(), 2);
    assert_eq!(points.ir.model.vertices.len(), 2);
    let native = points.ir.native.namespace("iges").unwrap();
    assert_eq!(native.arenas["copious_data"].len(), 1);
    assert_eq!(native.arenas["copious_data"][0].fields["tuples"][0][5], 1.0);
    assert!(points.report.losses.is_empty());

    let witness = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(40, b"106,1,3,0,0,0,1,0,2,0;", "00000100")),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!witness.report.geometry_transferred);
    assert!(witness.ir.model.curves.is_empty());
    assert!(witness.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&witness.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn conic_arc_file(form: i64, parameters: &[u8]) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["104", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "104",
            "0",
            "0",
            &parameter_count.to_string(),
            &form.to_string(),
            "",
            "",
            "CONIC",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_classifies_and_bounds_all_standard_conic_arc_families() {
    let fixtures: [(i64, &[u8]); 4] = [
        (0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        (1, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        (
            2,
            b"104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;",
        ),
        (3, b"104,1,0,0,0,-4,0,0,2,1,-2,1;"),
    ];
    for (form, parameters) in fixtures {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(conic_arc_file(form, parameters)),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir.model.curves.len(), 1, "form {form}");
        assert_eq!(result.ir.model.edges.len(), 1, "form {form}");
        match (&result.ir.model.curves[0].geometry, form) {
            (cadmpeg_ir::geometry::CurveGeometry::Ellipse { .. }, 0 | 1)
            | (cadmpeg_ir::geometry::CurveGeometry::Hyperbola { .. }, 2)
            | (cadmpeg_ir::geometry::CurveGeometry::Parabola { .. }, 3) => {}
            (geometry, _) => panic!("unexpected form {form} geometry {geometry:?}"),
        }
        assert!(result.report.losses.is_empty(), "form {form}");
        let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
        assert!(
            validation.is_ok(),
            "form {form}: {:#?}",
            validation.findings
        );
    }
}

fn nurbs_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["126", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["126", "0", "0", "1", "1", "", "", "NURBS", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        b"126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1,0,0,1;",
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn parametric_spline_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let values = [
        "112", "3", "1", "3", "2", "0", "1", "2", // Header and breakpoints.
        "0", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Segment 1.
        "1", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Segment 2.
        "2", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Terminal block.
    ];
    let parameters = format!("{};", values.join(","));
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["112", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "112",
            "0",
            "0",
            &parameter_count.to_string(),
            "0",
            "",
            "",
            "SPLINE",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters.as_bytes(), 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn parametric_spline_surface_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut values = vec![
        "114".to_owned(),
        "3".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "0".to_owned(),
        "1".to_owned(),
        "0".to_owned(),
        "1".to_owned(),
    ];
    let mut patch = vec!["0".to_owned(); 48];
    patch[1] = "1".into();
    patch[16 + 4] = "1".into();
    values.extend(patch);
    values.extend((0..48 * 3).map(|_| "0".to_owned()));
    let parameters = format!("{};", values.join(","));
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["114", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "114",
            "0",
            "0",
            &parameter_count.to_string(),
            "0",
            "",
            "",
            "SPLSURF",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters.as_bytes(), 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_converts_bicubic_power_patches_to_an_exact_nurbs_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected a bicubic NURBS carrier");
    };
    assert_eq!((surface.u_degree, surface.v_degree), (3, 3));
    assert_eq!((surface.u_count, surface.v_count), (4, 4));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_converts_piecewise_power_splines_to_exact_cubic_nurbs() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a cubic NURBS carrier");
    };
    assert_eq!(nurbs.degree, 3);
    assert_eq!(
        nurbs.knots,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]
    );
    assert_eq!(nurbs.control_points.len(), 7);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            None,
            1.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.5, 0.0, 0.0))
    );
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 2.0]));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn rational_nurbs_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["126", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["126", "0", "0", "1", "0", "", "", "RNURBS", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        b"126,2,2,1,0,0,0,0,0,0,1,1,1,1,0.5,1,0,0,0,1,1,0,2,0,0,0,1,0,0,1;",
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn nurbs_surface_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameters =
        b"128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;";
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["128", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "128",
            "0",
            "0",
            &parameter_count.to_string(),
            "0",
            "",
            "",
            "SURFACE",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn ruled_surface_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, form, label) in [
        (1, 1, "110", 0, "RAIL1"),
        (3, 2, "110", 0, "RAIL2"),
        (5, 3, "118", 1, "RULED"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                if entity_type == "110" {
                    "00010000"
                } else {
                    "00000000"
                },
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                entity_type,
                "0",
                "0",
                "1",
                &form.to_string(),
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,1,0,0;", 1, 1));
    bytes.extend(parameter_card(b"110,0,1,0,1,1,0;", 3, 2));
    bytes.extend(parameter_card(b"118,1,3,0,1;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_solves_a_parameter_matched_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected an exact NURBS ruled cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn tabulated_cylinder_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "110", "DIRECTRX", "00010000"),
        (3, 2, "122", "TABULATE", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,1,0,0;", 1, 1));
    bytes.extend(parameter_card(b"122,1,0,0,2;", 3, 2));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn surface_of_revolution_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "110", "AXIS", "00010000"),
        (3, 2, "110", "PROFILE", "00010000"),
        (5, 3, "120", "REVOLVE", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,0,0,2;", 1, 1));
    bytes.extend(parameter_card(b"110,1,0,0,1,0,2;", 3, 2));
    bytes.extend(parameter_card(b"120,1,3,0,1.5707963267948966;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_solves_a_surface_of_revolution_as_rational_quadratic_spans() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.v_degree, 2);
    assert_eq!(surface.weights.as_ref().unwrap().len(), 6);
    let point =
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, std::f64::consts::FRAC_PI_4).unwrap();
    let expected = 0.5_f64.sqrt();
    assert!((point.x - expected).abs() < 1.0e-12);
    assert!((point.y - expected).abs() < 1.0e-12);
    assert!((point.z - 1.0).abs() < 1.0e-12);
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_tabulated_cylinder_as_an_exact_extrusion() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(tabulated_cylinder_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected an exact NURBS extrusion cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5),
        Some(cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn plane_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["108", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["108", "0", "0", "1", "0", "", "", "PLANE", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"108,0,0,1,2,0,0,0,2,0;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_projects_an_unbounded_plane_from_implicit_coefficients() {
    let result = IgesCodec
        .decode(&mut Cursor::new(plane_file()), &DecodeOptions::default())
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected a plane carrier");
    };
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 2.0));
    assert_eq!(*normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(
        cadmpeg_ir::eval::surface_point(&result.ir.model.surfaces[0].geometry, 1.0, 3.0),
        Some(cadmpeg_ir::math::Point3::new(1.0, 3.0, 2.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn offset_plane_file(indicator_z: f64, distance: f64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["108", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["108", "0", "0", "1", "0", "", "", "PLANE", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["140", "2", "0", "0", "0", "0", "0", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["140", "0", "0", "1", "0", "", "", "OFFSET", "0"],
        4,
    ));
    bytes.extend(parameter_card(b"108,0,0,1,0,0,0,0,0,0;", 1, 1));
    bytes.extend(parameter_card(
        format!("140,0,0,{indicator_z},{distance},1;").as_bytes(),
        3,
        2,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_solves_signed_analytic_offset_surfaces() {
    for (indicator_z, expected_z) in [(1.0, 2.0), (-1.0, -2.0)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_plane_file(indicator_z, 2.0)),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.0 == "iges:model:surface#D3")
            .unwrap();
        let cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, .. } = offset.geometry else {
            panic!("expected an exact plane offset carrier");
        };
        assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, expected_z));
        assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset { distance, .. } =
            result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected an offset dependency");
        };
        assert_eq!(distance, expected_z);
        assert!(result.report.losses.is_empty());
        let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_projects_a_bspline_surface_with_u_major_control_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
    assert_eq!(
        nurbs.control_points,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ]
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(nurbs, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_rational_bspline_weights_and_multiplicities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.weights, Some(vec![1.0, 0.5, 1.0]));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0 / 3.0, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_bounded_polynomial_bspline_curve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree, 1);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(nurbs.control_points.len(), 2);
    assert_eq!(nurbs.weights, None);
    assert!(!nurbs.periodic);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 1.0]));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_counterclockwise_circular_arc() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(circular_arc_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.curves.len(), 1);
    let cadmpeg_ir::geometry::CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    } = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a circle carrier");
    };
    assert_eq!(*center, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        *ref_direction,
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    );
    assert_eq!(*radius, 1.0);
    assert_eq!(
        result.ir.model.edges[0].param_range,
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert!(result
        .ir
        .model
        .points
        .iter()
        .any(|point| point.position == cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_line_as_a_normalized_bounded_wire_edge() {
    let result = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.curves.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.points.len(), 2);
    let cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } =
        &result.ir.model.curves[0].geometry
    else {
        panic!("expected a line carrier");
    };
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.6, 0.8, 0.0));
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 5.0]));
    assert_eq!(result.ir.model.shells[0].wire_edges.len(), 1);
    assert!(result.ir.model.shells[0].free_vertices.is_empty());
    assert_eq!(
        result.ir.model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .object_id,
        "D1"
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_semi_bounded_and_unbounded_line_domains_natively() {
    for form in [1, 2] {
        let result = IgesCodec
            .decode(&mut Cursor::new(line_file(form)), &DecodeOptions::default())
            .unwrap();

        assert_eq!(result.ir.model.curves.len(), 1);
        assert!(result.ir.model.edges.is_empty());
        assert!(result.ir.model.bodies.is_empty());
        assert_eq!(
            result.ir.model.curves[0]
                .source_object
                .as_ref()
                .unwrap()
                .object_id,
            "D1"
        );
        assert!(result.report.losses.is_empty());
        let native = result.ir.native.namespace("iges").unwrap();
        assert_eq!(native.arenas["entities"][0].fields["form"], form);
        let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

fn nested_transformed_point_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,0.5,10,2HCM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, transform, entity_type, form, label) in [
        (1, 1, 0, "124", 0, "PARENT"),
        (3, 2, 1, "124", 1, "LOCAL"),
        (5, 3, 3, "116", 0, "POINT"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                &transform.to_string(),
                "0",
                "00000000",
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                entity_type,
                "0",
                "0",
                "1",
                &form.to_string(),
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"124,1,0,0,0,0,1,0,2,0,0,1,0;", 1, 1));
    bytes.extend(parameter_card(b"124,-1,0,0,1,0,1,0,0,0,0,1,0;", 3, 2));
    bytes.extend(parameter_card(b"116,1,2,3;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_applies_nested_transforms_reflection_units_and_model_scale_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_transformed_point_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 0.0);
    assert_eq!(result.ir.model.points[0].position.y, 80.0);
    assert_eq!(result.ir.model.points[0].position.z, 60.0);
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["transformations"].len(),
        2
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn inspect_rejects_terminate_count_mismatch() {
    let mut bytes = card(b"original fixture", b'S', 1);
    bytes.extend(card(b"1H,,1H;,,;", b'G', 1));
    bytes.extend(card(b"S0000001G0000002D0000000P0000000", b'T', 1));

    let error = IgesCodec.inspect(&mut Cursor::new(bytes)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "malformed container: IGES Terminate count for global is 2, actual 1"
    );
}

#[test]
fn decode_preserves_native_entities_graph_and_complete_byte_ledger() {
    let bytes = point_file();
    let source_length = u64::try_from(bytes.len()).unwrap();

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.source.as_ref().unwrap().format, "iges");
    assert_eq!(result.ir.byte_ledger.source_length, source_length);
    assert_eq!(result.ir.byte_ledger.spans.first().unwrap().start, 0);
    assert_eq!(
        result.ir.byte_ledger.spans.last().unwrap().end,
        source_length
    );
    let native = result.ir.native.namespace("iges").unwrap();
    assert_eq!(native.version, 1);
    assert_eq!(native.arenas["cards"].len(), 7);
    assert_eq!(native.arenas["entities"].len(), 1);
    assert_eq!(native.arenas["entities"][0].id, "iges:entity:directory#1");
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 1.0);
    assert_eq!(result.ir.model.points[0].position.y, 2.0);
    assert_eq!(result.ir.model.points[0].position.z, 3.0);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert!(result.report.geometry_transferred);
    assert!(!result.report.losses.iter().any(|loss| {
        loss.message == "IGES entity type 116 form 0 retained without neutral projection"
    }));
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn inspect_preserves_transform_cycles_as_named_reference_states() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["124", "1", "0", "0", "0", "0", "3", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "1"],
        2,
    ));
    bytes.extend(directory_card(
        ["124", "2", "0", "0", "0", "0", "1", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "2"],
        4,
    ));
    let matrix = b"124,1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.;";
    bytes.extend(parameter_card(matrix, 1, 1));
    bytes.extend(parameter_card(matrix, 3, 2));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));

    let summary = IgesCodec.inspect(&mut Cursor::new(bytes)).unwrap();

    assert!(summary.notes.contains(&"references.cyclic=2".into()));
}

#[test]
fn compressed_and_binary_representations_are_detected_inspected_and_refused() {
    let mut compressed = vec![b' '; 80];
    compressed[72] = b'C';
    compressed.push(b'\n');
    compressed.extend(card(b"compressed fixture", b'S', 1));
    assert_eq!(IgesCodec.detect(&compressed), Confidence::High);
    let summary = IgesCodec
        .inspect(&mut Cursor::new(compressed.clone()))
        .unwrap();
    assert_eq!(summary.container_kind, "compressed-ascii");
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(compressed), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "not implemented yet: IGES Compressed ASCII representation decode"
    );

    let mut binary = vec![0_u8; 80];
    binary[0] = b'B';
    binary[1..5].copy_from_slice(&75_u32.to_be_bytes());
    binary[72] = b'B';
    binary[79] = b'1';
    assert_eq!(IgesCodec.detect(&binary), Confidence::High);
    let summary = IgesCodec.inspect(&mut Cursor::new(binary.clone())).unwrap();
    assert_eq!(summary.container_kind, "binary");
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(binary), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "not implemented yet: IGES Binary representation decode"
    );
}

#[test]
fn legacy_fixed_ascii_is_reported_but_not_decoded_as_iges_5_3() {
    let mut bytes = point_file();
    let version = bytes
        .windows(b",11,0,".len())
        .position(|window| window == b",11,0,")
        .unwrap();
    bytes[version + 1..version + 3].copy_from_slice(b"10");

    let summary = IgesCodec.inspect(&mut Cursor::new(bytes.clone())).unwrap();
    assert!(summary.notes.contains(&"iges_version=5.2".into()));
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "not implemented yet: IGES Fixed ASCII version 5.2 decode; target envelope is 5.3"
    );
}

#[test]
fn decode_retains_and_accounts_for_post_terminate_records() {
    let mut bytes = point_file();
    bytes.extend_from_slice(b"transport padding\r\n");
    let source_length = u64::try_from(bytes.len()).unwrap();

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.byte_ledger.source_length, source_length);
    assert_eq!(
        result.ir.byte_ledger.spans.last().unwrap().end,
        source_length
    );
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["cards"].len(),
        8
    );
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
