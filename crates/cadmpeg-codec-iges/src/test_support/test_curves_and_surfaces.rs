// SPDX-License-Identifier: Apache-2.0
//! Curve and surface byte fixtures for crate tests.
#![allow(clippy::unwrap_used)]

use super::test_cards::*;
use super::test_owned::*;

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

pub(crate) fn nurbs_surface_file() -> Vec<u8> {
    nurbs_surface_file_with_parameters(
        b"128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;",
    )
}

pub(crate) fn degree_zero_nurbs_surface_file() -> Vec<u8> {
    nurbs_surface_file_with_parameters(b"128,0,0,0,0,1,1,1,0,0,0,1,0,1,1,1,2,3,0,1,0,1;")
}

pub(crate) fn multispan_degree_zero_nurbs_surface_file() -> Vec<u8> {
    nurbs_surface_file_with_parameters(b"128,1,0,0,0,0,1,1,0,0,0,1,2,0,1,1,1,1,2,3,4,5,6,0,2,0,1;")
}

pub(crate) fn nurbs_surface_file_with_parameters(parameters: &[u8]) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameter_fragment_count(parameters);
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn ruled_surface_file() -> Vec<u8> {
    ruled_surface_file_with_developable_flag(1)
}

pub(crate) fn ruled_surface_file_with_developable_flag(developable_flag: i64) -> Vec<u8> {
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
    bytes.extend(parameter_card(
        format!("118,1,3,0,{developable_flag};").as_bytes(),
        5,
        3,
    ));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn rational_ruled_surface_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "RAT1".into(),
            status: "00000000",
            parameters: "126,2,2,1,0,0,0,0,0,0,1,1,1,1,0.5,1,0,0,0,1,1,0,2,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "RAT2".into(),
            status: "00000000",
            parameters: "126,2,2,1,0,0,0,0,0,0,1,1,1,1,0.25,1,0,0,1,1,1,1,2,0,1,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 118,
            form: 1,
            label: "RRULED".into(),
            status: "00000000",
            parameters: "118,1,3,0,1;".into(),
        },
    ])
}

pub(crate) fn circular_ruled_surface_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "CIRC1".into(),
            status: "00000000",
            parameters: "100,0,0,0,1,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "CIRC2".into(),
            status: "00000000",
            parameters: "100,0,0,2,1,2,1,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 118,
            form: 0,
            label: "CRULED".into(),
            status: "00000000",
            parameters: "118,1,3,0,1;".into(),
        },
    ])
}

pub(crate) fn composite_ruled_surface_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "RAIL1A".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "RAIL1B".into(),
            status: "00010000",
            parameters: "110,1,0,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "RAIL1".into(),
            status: "00000000",
            parameters: "102,2,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "RAIL2A".into(),
            status: "00010000",
            parameters: "110,0,1,0,1,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "RAIL2B".into(),
            status: "00010000",
            parameters: "110,1,1,0,2,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "RAIL2".into(),
            status: "00000000",
            parameters: "102,2,7,9;".into(),
        },
        OwnedTestEntity {
            entity_type: 118,
            form: 1,
            label: "RULED".into(),
            status: "00000000",
            parameters: "118,5,11,0,1;".into(),
        },
    ])
}

pub(crate) fn composite_tabulated_cylinder_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "DIRECT1".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "DIRECT2".into(),
            status: "00010000",
            parameters: "110,1,0,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "DIRECT".into(),
            status: "00000000",
            parameters: "102,2,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 122,
            form: 0,
            label: "TABULATE".into(),
            status: "00000000",
            parameters: "122,5,0,0,2;".into(),
        },
    ])
}

pub(crate) fn tabulated_cylinder_file() -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn tabulated_hyperbola_file() -> Vec<u8> {
    owned_test_file(&tabulated_hyperbola_entities())
}

pub(crate) fn tabulated_hyperbola_file_with_global(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global_and_line_fonts(
        &tabulated_hyperbola_entities(),
        global,
        &[(1, 1), (3, 1)],
    )
}

fn tabulated_hyperbola_entities() -> Vec<OwnedTestEntity> {
    vec![
        OwnedTestEntity {
            entity_type: 104,
            form: 2,
            label: "HYPERBOL".into(),
            status: "00010000",
            parameters:
                "104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 122,
            form: 0,
            label: "TABULATE".into(),
            status: "00000000",
            parameters: "122,1,3.086161269630487,3.525603580931404,2;".into(),
        },
    ]
}

pub(crate) fn surface_of_revolution_file() -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn ellipse_surface_of_revolution_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "110,0,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 104,
            form: 0,
            label: "ELLIPSE".into(),
            status: "00010000",
            parameters: "104,0.25,0,1,0,0,-1,0,2,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 120,
            form: 0,
            label: "REVOLVE".into(),
            status: "00000000",
            parameters: "120,1,3,0,1.5707963267948966;".into(),
        },
    ])
}

pub(crate) fn line_surface_of_revolution_file() -> Vec<u8> {
    owned_test_file(&line_surface_of_revolution_entities())
}

pub(crate) fn line_surface_of_revolution_file_with_global(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global(&line_surface_of_revolution_entities(), global)
}

fn line_surface_of_revolution_entities() -> Vec<OwnedTestEntity> {
    vec![
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "110,0,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "PROFILE".into(),
            status: "00010000",
            parameters:
                "110,-108.9812949,6.814348186,-2.592356749,-108.9812949,11.76210922,-6.969522429;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 120,
            form: 0,
            label: "REVOLVE".into(),
            status: "00000000",
            parameters: "120,1,3,0,1.5707963267948966;".into(),
        },
    ]
}

pub(crate) fn hyperbola_surface_of_revolution_file() -> Vec<u8> {
    owned_test_file(&hyperbola_surface_of_revolution_entities())
}

pub(crate) fn hyperbola_surface_of_revolution_file_with_global(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global_and_line_fonts(
        &hyperbola_surface_of_revolution_entities(),
        global,
        &[(1, 1), (3, 1), (5, 1)],
    )
}

fn hyperbola_surface_of_revolution_entities() -> Vec<OwnedTestEntity> {
    vec![
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "110,0,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 104,
            form: 2,
            label: "HYPERBOL".into(),
            status: "00010000",
            parameters:
                "104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 120,
            form: 0,
            label: "REVOLVE".into(),
            status: "00000000",
            parameters: "120,1,3,0,1.5707963267948966;".into(),
        },
    ]
}

pub(crate) fn trimmed_surface_of_revolution_file() -> Vec<u8> {
    let angle = 0.3_f64;
    let pcurve = format!(
        "126,1,1,1,0,1,0,0,0,1,1,1,1,0.5,{angle},0,0.5,{},0,0,1,0,0,1;",
        angle + std::f64::consts::TAU
    );
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "110,0,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "PROFILE".into(),
            status: "00010000",
            parameters: "110,1,0,0,1,0,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 120,
            form: 0,
            label: "REVOLVE".into(),
            status: "00000000",
            parameters: format!("120,1,3,0,{};", std::f64::consts::TAU),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "MODEL".into(),
            status: "00010000",
            parameters: format!(
                "100,1,0,0,{},{},{},{};",
                angle.cos(),
                angle.sin(),
                angle.cos(),
                angle.sin()
            ),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE".into(),
            status: "00010500",
            parameters: pcurve,
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "ON_SURF".into(),
            status: "00010000",
            parameters: "142,0,5,9,7,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "TRIMMED".into(),
            status: "00000000",
            parameters: "144,5,1,0,11;".into(),
        },
    ])
}

pub(crate) fn placed_surface_of_revolution_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, transform, label, status) in [
        (1, 1, "110", "0", "AXIS", "00010000"),
        (3, 2, "110", "0", "PROFILE", "00010000"),
        (5, 3, "124", "0", "PLACE", "00010000"),
        (7, 4, "120", "5", "REVOLVE", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                transform,
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
    bytes.extend(parameter_card(b"124,1,0,0,10,0,1,0,0,0,0,1,0;", 5, 3));
    bytes.extend(parameter_card(b"120,1,3,0,1.5707963267948966;", 7, 4));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000008P0000004").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn plane_file() -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn bounded_plane_entity_file(
    global: &[u8],
    boundary_type: i64,
    boundary_parameters: &str,
) -> Vec<u8> {
    owned_test_file_with_global_and_line_fonts(
        &[
            OwnedTestEntity {
                entity_type: 108,
                form: 1,
                label: "PLANE".into(),
                status: "00010000",
                parameters: "108,0,0,1,0,3,0,0,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: boundary_type,
                form: 0,
                label: "BOUNDARY".into(),
                status: "00010000",
                parameters: boundary_parameters.into(),
            },
        ],
        global,
        &[(1, 1), (3, 1)],
    )
}

pub(crate) fn offset_plane_file(indicator_z: f64, distance: f64) -> Vec<u8> {
    offset_plane_file_with_indicator("0", "0", &indicator_z.to_string(), distance)
}

pub(crate) fn offset_nurbs_surface_file(indicator: &str) -> Vec<u8> {
    let surface_parameters = [
        "128", "1", "1", "1", "1", "0", "0", "1", "0", "0", "0", "0", "1", "1", "0", "0", "1", "1",
        "1", "1", "1", "1", "0", "0", "0", "1", "0", "0", "0", "1", "0", "1", "1", "1", "0", "1",
        "0", "1",
    ]
    .join(",");
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "SADDLE".into(),
            status: "00000000",
            parameters: format!("{surface_parameters};"),
        },
        OwnedTestEntity {
            entity_type: 140,
            form: 0,
            label: "OFFSET".into(),
            status: "00000000",
            parameters: format!("140,{indicator},1,1;"),
        },
    ])
}

pub(crate) fn offset_plane_file_with_indicator(
    indicator_x: &str,
    indicator_y: &str,
    indicator_z: &str,
    distance: f64,
) -> Vec<u8> {
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
        format!("140,{indicator_x},{indicator_y},{indicator_z},{distance},1;").as_bytes(),
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

pub(crate) fn offset_cylinder_file(indicator_x: f64) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "ORIGIN".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "123,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 192,
            form: 0,
            label: "CYLINDER".into(),
            status: "00010000",
            parameters: "192,1,3,10;".into(),
        },
        OwnedTestEntity {
            entity_type: 140,
            form: 0,
            label: "OFFSET".into(),
            status: "00000000",
            parameters: format!("140,{indicator_x},0,0,2,5;"),
        },
    ])
}

pub(crate) fn pointer_defined_surface_file(entity_type: i64, form: i64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    let include_axis = !(entity_type == 196 && form == 0);
    let include_reference = form == 1;
    let surface_sequence = if include_reference {
        7
    } else if include_axis {
        5
    } else {
        3
    };
    let surface_parameter_start = if include_reference {
        4
    } else if include_axis {
        3
    } else {
        2
    };
    let mut directory_entries = vec![(1, 1, 116, "LOCATION")];
    if include_axis {
        directory_entries.push((3, 2, 123, "AXIS"));
    }
    if include_reference {
        directory_entries.push((5, 3, 123, "REFDIR"));
    }
    directory_entries.push((
        surface_sequence,
        surface_parameter_start,
        entity_type,
        "SURFACE",
    ));
    for (sequence, parameter_start, kind, label) in directory_entries {
        let kind = kind.to_string();
        let parameter_start = parameter_start.to_string();
        let surface_form = form.to_string();
        bytes.extend(directory_card(
            [
                &kind,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                if sequence == surface_sequence {
                    "00000000"
                } else {
                    "00010000"
                },
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                &kind,
                "0",
                "0",
                "1",
                if sequence == surface_sequence {
                    &surface_form
                } else {
                    "0"
                },
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"116,1,2,3,0;", 1, 1));
    if include_axis {
        bytes.extend(parameter_card(b"123,0,0,1;", 3, 2));
    }
    if include_reference {
        bytes.extend(parameter_card(b"123,1,0,0;", 5, 3));
    }
    let parameters = match (entity_type, form) {
        (190, 0) => "190,1,3;",
        (190, 1) => "190,1,3,5;",
        (192, 0) => "192,1,3,2;",
        (192, 1) => "192,1,3,2,5;",
        (194, 0) => "194,1,3,2,30;",
        (194, 1) => "194,1,3,2,30,5;",
        (196, 0) => "196,1,2;",
        (196, 1) => "196,1,2,3,5;",
        (198, 0) => "198,1,3,4,1;",
        (198, 1) => "198,1,3,4,1,5;",
        _ => unreachable!(),
    };
    bytes.extend(parameter_card(
        parameters.as_bytes(),
        surface_sequence,
        surface_parameter_start,
    ));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D{:07}P{:07}",
            surface_sequence + 1,
            surface_parameter_start,
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn pointer_defined_surface_with_reference(
    entity_type: i64,
    reference_pointer: &str,
    reference_type: i64,
    reference_status: &'static str,
    reference_parameters: &str,
) -> Vec<u8> {
    let surface_parameters = match entity_type {
        190 => format!("190,1,3,{reference_pointer};"),
        192 => format!("192,1,3,2,{reference_pointer};"),
        194 => format!("194,1,3,2,30,{reference_pointer};"),
        196 => format!("196,1,2,3,{reference_pointer};"),
        198 => format!("198,1,3,4,1,{reference_pointer};"),
        _ => unreachable!(),
    };
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "LOCATION".into(),
            status: "00010000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "123,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: reference_type,
            form: 0,
            label: "REFDIR".into(),
            status: reference_status,
            parameters: reference_parameters.into(),
        },
        OwnedTestEntity {
            entity_type,
            form: 1,
            label: "SURFACE".into(),
            status: "00000000",
            parameters: surface_parameters,
        },
    ])
}

pub(crate) fn trimmed_plane_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, form, label, status) in [
        (1_u32, 108, 0, "PLANE", "00010000"),
        (3, 106, 63, "MODEL", "00010000"),
        (5, 106, 63, "PCURVE", "00010500"),
        (7, 142, 0, "ON_SURF", "00010000"),
        (9, 144, 0, "TRIMMED", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = sequence.div_ceil(2).to_string();
        let form = form.to_string();
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
            [&entity_type, "0", "0", "1", &form, "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"108,0,0,1,0,0,0,0,0,0;", 1, 1));
    let square = b"106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    bytes.extend(parameter_card(square, 3, 2));
    bytes.extend(parameter_card(square, 5, 3));
    bytes.extend(parameter_card(b"142,0,1,5,3,3;", 7, 4));
    bytes.extend(parameter_card(b"144,1,1,0,7;", 9, 5));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000010P0000005").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn trimmed_circle_pcurve_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "MODEL".into(),
            status: "00010000",
            parameters: "100,0,0,0,0.5,0.5,0.5,0.5;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "PCURVE".into(),
            status: "00010500",
            parameters: "100,0,0,0,0.5,0.5,0.5,0.5;".into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "ON_SURF".into(),
            status: "00010000",
            parameters: "142,0,1,5,3,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "TRIMMED".into(),
            status: "00000000",
            parameters: "144,1,1,0,7;".into(),
        },
    ])
}

pub(crate) fn model_curve_only_trimmed_plane_file() -> Vec<u8> {
    let mut bytes = trimmed_plane_file();
    let parameter = b"142,0,1,5,3,3;";
    let start = bytes
        .windows(parameter.len())
        .position(|window| window == parameter)
        .unwrap();
    bytes[start..start + parameter.len()].copy_from_slice(b"142,0,1,0,3,2;");
    bytes
}

pub(crate) fn bounded_plane_file() -> Vec<u8> {
    bounded_plane_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    )
}

fn bounded_plane_file_with_global(global: &[u8]) -> Vec<u8> {
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, label, status) in [
        (1_u32, 108, "PLANE", "00010000"),
        (3, 110, "EDGE1", "00010000"),
        (5, 110, "EDGE2", "00010000"),
        (7, 110, "EDGE3", "00010000"),
        (9, 110, "EDGE4", "00010000"),
        (11, 141, "BOUNDARY", "00010000"),
        (13, 143, "BOUNDED", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = sequence.div_ceil(2).to_string();
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
    for (sequence, parameter_sequence, parameters) in [
        (1, 1, "108,0,0,1,0,0,0,0,0,0;"),
        (3, 2, "110,0,0,0,1,0,0;"),
        (5, 3, "110,1,1,0,1,0,0;"),
        (7, 4, "110,1,1,0,0,1,0;"),
        (9, 5, "110,0,1,0,0,0,0;"),
        (11, 6, "141,0,1,1,4,3,1,0,5,2,0,7,1,0,9,1,0;"),
        (13, 7, "143,0,1,1,11;"),
    ] {
        bytes.extend(parameter_card(
            parameters.as_bytes(),
            sequence,
            parameter_sequence,
        ));
    }
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000014P0000007").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn bounded_plane_with_resolution_gap_file() -> Vec<u8> {
    bounded_plane_with_resolution_gap(bounded_plane_file())
}

fn bounded_plane_with_resolution_gap(mut bytes: Vec<u8>) -> Vec<u8> {
    let original = b"110,1,1,0,1,0,0;";
    let replacement = b"110,1,1,0,1,0.000999,0;";
    let start = bytes
        .windows(original.len())
        .position(|window| window == original)
        .expect("bounded-plane edge parameter record");
    let line_start = bytes[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    assert_eq!(start, line_start);
    let payload_end = line_start + 64;
    assert!(replacement.len() <= payload_end - start);
    bytes[start..start + replacement.len()].copy_from_slice(replacement);
    bytes[start + replacement.len()..payload_end].fill(b' ');
    bytes
}

pub(crate) fn centimetre_bounded_plane_with_resolution_gap_file() -> Vec<u8> {
    bounded_plane_with_resolution_gap(bounded_plane_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,3,2Hcm,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    ))
}

pub(crate) fn bounded_plane_with_significance_gap_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "EDGE1".into(),
            status: "00010000",
            parameters: "110,1000,1000,0,1001,1000,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "EDGE2".into(),
            status: "00010000",
            parameters: "110,1001,1000.005,0,1001,1001,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "EDGE3".into(),
            status: "00010000",
            parameters: "110,1001,1001,0,1000,1001,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "EDGE4".into(),
            status: "00010000",
            parameters: "110,1000,1001,0,1000,1000,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: "141,0,1,1,4,3,1,0,5,1,0,7,1,0,9,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 143,
            form: 0,
            label: "BOUNDED".into(),
            status: "00000000",
            parameters: "143,0,1,1,11;".into(),
        },
    ])
}
