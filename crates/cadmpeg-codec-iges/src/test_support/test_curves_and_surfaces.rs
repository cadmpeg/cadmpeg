// SPDX-License-Identifier: Apache-2.0
//! Curve and surface byte fixtures for crate tests.
#![allow(clippy::unwrap_used)]

use super::test_cards::*;
use super::test_owned::*;
pub(crate) use super::test_surface_fixtures::*;

pub(crate) fn point_file() -> Vec<u8> {
    point_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    )
}

pub(crate) fn point_file_with_global(global: &[u8]) -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn direction_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["123", "1", "0", "0", "0", "0", "0", "0", "00030000"],
        1,
    ));
    bytes.extend(directory_card(
        ["123", "0", "0", "1", "0", "", "", "VECTOR", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"123,2,-3,4;", 1, 1));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn line_file(form: i64) -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn circular_arc_file() -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn transformed_circular_arc_file(matrix: &[u8], arc: &[u8]) -> Vec<u8> {
    transformed_circular_arc_file_with_form(0, matrix, arc)
}

pub(crate) fn transformed_circular_arc_file_with_form(
    form: i64,
    matrix: &[u8],
    arc: &[u8],
) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    transformed_circular_arc_file_with_global(form, matrix, arc, global)
}

/// A transformed circular arc with the supplied Global record.
pub(crate) fn transformed_circular_arc_file_with_global(
    form: i64,
    matrix: &[u8],
    arc: &[u8],
    global: &[u8],
) -> Vec<u8> {
    let form = form.to_string();
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["124", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", &form, "", "", "FRAME", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["100", "2", "0", "0", "0", "0", "1", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["100", "0", "0", "1", "0", "", "", "ARC", "0"],
        4,
    ));
    bytes.extend(parameter_card(matrix, 1, 1));
    bytes.extend(parameter_card(arc, 3, 2));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn uniform_offset_circle_file() -> Vec<u8> {
    uniform_offset_circle_file_with_parameters(b"130,1,1,0,,,0.5,,,,0,0,1,0,1.5707963267948966;")
}

pub(crate) fn uniform_offset_circle_file_with_parameters(offset: &[u8]) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["100", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["100", "0", "0", "1", "0", "", "", "ARC", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["130", "2", "0", "0", "0", "0", "0", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["130", "0", "0", "1", "0", "", "", "OFFSET", "0"],
        4,
    ));
    bytes.extend(parameter_card(b"100,0,0,0,2,0,0,2;", 1, 1));
    bytes.extend(parameter_card(offset, 3, 2));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn placed_uniform_offset_circle_file(form: i64, matrix: &[u8]) -> Vec<u8> {
    placed_uniform_offset_file(
        "100",
        b"100,0,0,0,2,0,0,2;",
        b"130,1,1,0,,,0.5,,,,0,0,1,0,1.5707963267948966;",
        form,
        matrix,
    )
}

pub(crate) fn placed_uniform_offset_line_file(form: i64, matrix: &[u8]) -> Vec<u8> {
    placed_uniform_offset_file(
        "110",
        b"110,0,0,0,2,0,0;",
        b"130,1,1,0,,,0.5,,,,0,0,1,0,1;",
        form,
        matrix,
    )
}

fn placed_uniform_offset_file(
    source_type: &str,
    source_parameters: &[u8],
    offset_parameters: &[u8],
    form: i64,
    matrix: &[u8],
) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let form = form.to_string();
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        [source_type, "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        [source_type, "0", "0", "1", "0", "", "", "SOURCE", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["130", "2", "0", "0", "0", "0", "5", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["130", "0", "0", "1", "0", "", "", "OFFSET", "0"],
        4,
    ));
    bytes.extend(directory_card(
        ["124", "3", "0", "0", "0", "0", "0", "0", "00000000"],
        5,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", &form, "", "", "FRAME", "0"],
        6,
    ));
    bytes.extend(parameter_card(source_parameters, 1, 1));
    bytes.extend(parameter_card(offset_parameters, 3, 2));
    bytes.extend(parameter_card(matrix, 5, 3));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn offset_quarter_circle_with_absolute_native_parameters() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "ARC".into(),
            status: "00010000",
            parameters: "100,0,0,0,0,2,-2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 130,
            form: 0,
            label: "OFFSET".into(),
            status: "00000000",
            parameters: format!(
                "130,1,1,0,,,0.5,,,,0,0,1,{},{};",
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI
            ),
        },
    ])
}

pub(crate) fn linear_offset_line_file(basis: i64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["110", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["110", "0", "0", "1", "0", "", "", "LINE", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["130", "2", "0", "0", "0", "0", "0", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["130", "0", "0", "1", "0", "", "", "OFFSET", "0"],
        4,
    ));
    bytes.extend(parameter_card(b"110,0,0,0,10,0,0;", 1, 1));
    let control_end = if basis == 1 { 10 } else { 1 };
    bytes.extend(parameter_card(
        format!("130,1,2,0,0,{basis},1,0,3,{control_end},0,0,1,0,1;").as_bytes(),
        3,
        2,
    ));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn function_offset_line_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, 110, "LINE", "00010000"),
        (3, 2, 126, "LAW", "00010000"),
        (5, 3, 130, "OFFSET", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = parameter_start.to_string();
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
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
            [&entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,10,0,0;", 1, 1));
    bytes.extend(parameter_card(
        b"126,1,1,1,0,1,0,0,0,1,1,1,1,0,1,0,1,3,0,0,1,0,0,1;",
        3,
        2,
    ));
    bytes.extend(parameter_card(b"130,1,3,3,2,2,0,0,0,0,0,0,1,0,1;", 5, 3));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn composite_curve_file() -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn composite_curve_with_join_gap(gap: f64) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CHILD1".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CHILD2".into(),
            status: "00010000",
            parameters: format!("110,{},0,0,2,0,0;", 1.0 + gap),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "COMPOSIT".into(),
            status: "00000000",
            parameters: "102,2,1,3;".into(),
        },
    ])
}

pub(crate) fn mixed_analytic_composite_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "100", "ARC", "00010000"),
        (3, 2, "110", "LINE", "00010000"),
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
    bytes.extend(parameter_card(b"100,0,0,0,1,0,0,1;", 1, 1));
    bytes.extend(parameter_card(b"110,0,1,0,0,2,0;", 3, 2));
    bytes.extend(parameter_card(b"102,2,1,3;", 5, 3));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn heterogeneous_composite_curve_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 104,
            form: 0,
            label: "ELLIPSE".into(),
            status: "00010000",
            parameters: "104,0.25,0,1,0,0,-1,0,2,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00010000",
            parameters: "110,0,1,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "COMPOSIT".into(),
            status: "00000000",
            parameters: "102,2,1,3;".into(),
        },
    ])
}

pub(crate) fn mixed_degree_composite_pcurve_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "CUBIC".into(),
            status: "00010000",
            parameters:
                "126,3,3,1,0,1,0,0,0,0,0,1,1,1,1,1,1,1,1,0,0,0,0,1,0,1,1,0,1,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00010000",
            parameters: "110,1,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "BCURVE".into(),
            status: "00010500",
            parameters: "102,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: "142,0,1,7,7,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "TRIMMED".into(),
            status: "00000000",
            parameters: "144,1,1,0,9;".into(),
        },
    ])
}

pub(crate) fn parametric_spline_composite_curve_file() -> Vec<u8> {
    let values = [
        "112", "3", "1", "3", "1", "0", "1", // Header and breakpoints.
        "0", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Polynomial.
        "1.5", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0",
        "0", // Inconsistent terminal block.
    ];
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 112,
            form: 0,
            label: "SPLINE".into(),
            status: "00010000",
            parameters: format!("{};", values.join(",")),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "COMPOSIT".into(),
            status: "00000000",
            parameters: "102,1,1;".into(),
        },
    ])
}

pub(crate) fn copious_data_file(form: i64, parameters: &[u8], status: &str) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameter_fragment_count(parameters);
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn conic_arc_file(form: i64, parameters: &[u8]) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameter_fragment_count(parameters);
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn nurbs_curve_file() -> Vec<u8> {
    polynomial_nurbs_curve_file(b"126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1,0,0,1;")
}

pub(crate) fn polynomial_nurbs_curve_file(parameters: &[u8]) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameter_fragment_count(parameters);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["126", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "126",
            "0",
            "0",
            &parameter_count.to_string(),
            "1",
            "",
            "",
            "NURBS",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn parametric_spline_curve_file() -> Vec<u8> {
    let values = [
        "112", "3", "1", "3", "2", "0", "1", "2", // Header and breakpoints.
        "0", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Segment 1.
        "1", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Segment 2.
        "2", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Terminal block.
    ];
    let parameters = format!("{};", values.join(","));
    parametric_spline_curve_file_with_parameters(parameters.as_bytes())
}

pub(crate) fn parametric_spline_curve_file_with_parameters(parameters: &[u8]) -> Vec<u8> {
    parametric_spline_curve_file_with_parameters_and_resolution(parameters, "0.001")
}

pub(crate) fn parametric_spline_curve_file_with_parameters_and_resolution(
    parameters: &[u8],
    resolution: &str,
) -> Vec<u8> {
    let global = format!(
        "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,{resolution},1000.0,6Hauthor,3Horg,11,0,0H,0H;"
    );
    let parameter_count = parameter_fragment_count(parameters);
    let mut bytes = fixed_ascii_with_global(global.as_bytes());
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
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global_card_count(global.as_bytes());
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn nonlinear_parametric_spline_curve_file() -> Vec<u8> {
    let values = [
        "112", "3", "1", "3", "1", "2", "5", // Header and breakpoints.
        "1", "2", "3", "4", // x(w)
        "-1", "0.5", "-2", "1", // y(w)
        "2", "-1", "0.25", "-0.5", // z(w)
        "142", "128", "39", "4", // x terminal jet at w=3
        "9.5", "15.5", "7", "1", // y terminal jet at w=3
        "-12.25", "-13", "-4.25", "-0.5", // z terminal jet at w=3
    ];
    let parameters = format!("{};", values.join(","));
    parametric_spline_curve_file_with_parameters(parameters.as_bytes())
}

pub(crate) fn parametric_spline_surface_file() -> Vec<u8> {
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
    let parameter_count = parameter_fragment_count(parameters.as_bytes());
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn nonlinear_parametric_spline_surface_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut values = vec![
        "114".to_owned(),
        "3".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "3".to_owned(),
        "-2".to_owned(),
        "1".to_owned(),
    ];
    values.extend((1..=16).map(|value| value.to_string()));
    values.extend((17..=32).map(|value| value.to_string()));
    values.extend((1..=16).map(|value| (-value).to_string()));
    values.extend((0..48 * 3).map(|_| "0".to_owned()));
    let parameters = format!("{};", values.join(","));
    let parameter_count = parameter_fragment_count(parameters.as_bytes());
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn rational_nurbs_curve_file() -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn equal_weight_rational_nurbs_curve_file() -> Vec<u8> {
    let mut bytes = rational_nurbs_curve_file();
    let unequal = b",1,0.5,1,";
    let start = bytes
        .windows(unequal.len())
        .position(|window| window == unequal)
        .unwrap();
    bytes[start..start + unequal.len()].copy_from_slice(b",1,1.0,1,");
    bytes
}
