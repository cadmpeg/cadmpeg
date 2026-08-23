// SPDX-License-Identifier: Apache-2.0
//! Solid, B-rep, and structure byte fixtures for crate tests.
#![allow(clippy::unwrap_used)]

use super::test_cards::*;
use super::test_owned::*;

pub(crate) fn parametrically_bounded_plane_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, form, label, status) in [
        (1_u32, 108, 0, "PLANE", "00010000"),
        (3, 106, 63, "MODEL", "00010000"),
        (5, 106, 63, "PCURVE", "00010500"),
        (7, 141, 0, "BOUNDARY", "00010000"),
        (9, 143, 0, "BOUNDED", "00000000"),
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
    bytes.extend(parameter_card(b"141,1,3,1,1,3,1,1,5;", 7, 4));
    bytes.extend(parameter_card(b"143,1,1,1,7;", 9, 5));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000010P0000005").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn explicit_open_shell_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, form, label, status) in [
        (1_u32, 116, 0, "LOCATION", "00010000"),
        (3, 123, 0, "NORMAL", "00010000"),
        (5, 190, 0, "SURFACE", "00010000"),
        (7, 110, 0, "EDGE1", "00010000"),
        (9, 110, 0, "EDGE2", "00010000"),
        (11, 110, 0, "EDGE3", "00010000"),
        (13, 110, 0, "EDGE4", "00010000"),
        (15, 502, 1, "VERTICES", "00010000"),
        (17, 504, 1, "EDGES", "00010001"),
        (19, 508, 1, "LOOP", "00010000"),
        (21, 510, 1, "FACE", "00010000"),
        (23, 514, 2, "SHELL", "00000000"),
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
    for (sequence, parameter_sequence, parameters) in [
        (1, 1, "116,0,0,0,0;"),
        (3, 2, "123,0,0,1;"),
        (5, 3, "190,1,3;"),
        (7, 4, "110,0,0,0,1,0,0;"),
        (9, 5, "110,1,0,0,1,1,0;"),
        (11, 6, "110,1,1,0,0,1,0;"),
        (13, 7, "110,0,1,0,0,0,0;"),
        (15, 8, "502,4,0,0,0,1,0,0,1,1,0,0,1,0;"),
        (
            17,
            9,
            "504,4,7,15,1,15,2,9,15,2,15,3,11,15,3,15,4,13,15,4,15,1;",
        ),
        (19, 10, "508,4,0,17,1,1,0,0,17,2,1,0,0,17,3,1,0,0,17,4,1,0;"),
        (21, 11, "510,5,1,1,19;"),
        (23, 12, "514,1,21,1;"),
    ] {
        bytes.extend(parameter_card(
            parameters.as_bytes(),
            sequence,
            parameter_sequence,
        ));
    }
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000024P0000012").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn explicit_non_manifold_open_shell_file() -> Vec<u8> {
    let mut entities = vec![
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "LOCATION".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: "NORMAL".into(),
            status: "00010000",
            parameters: "123,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 190,
            form: 0,
            label: "SURFACE".into(),
            status: "00010000",
            parameters: "190,1,3;".into(),
        },
    ];
    for (index, parameters) in [
        "110,0,0,0,1,0,0;",
        "110,1,0,0,0,1,0;",
        "110,0,1,0,0,0,0;",
        "110,0,0,0,0,-1,0;",
        "110,0,-1,0,1,0,0;",
        "110,1,0,0,0.5,1,0;",
        "110,0.5,1,0,0,0,0;",
    ]
    .into_iter()
    .enumerate()
    {
        entities.push(OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: format!("EDGE{}", index + 1),
            status: "00010000",
            parameters: parameters.into(),
        });
    }
    entities.extend([
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "VERTICES".into(),
            status: "00010000",
            parameters: "502,5,0,0,0,1,0,0,0,1,0,0,-1,0,0.5,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 504,
            form: 1,
            label: "EDGES".into(),
            status: "00010001",
            parameters: "504,7,7,21,1,21,2,9,21,2,21,3,11,21,3,21,1,13,21,1,21,4,15,21,4,21,2,17,21,2,21,5,19,21,5,21,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP1".into(),
            status: "00010000",
            parameters: "508,3,0,23,1,1,0,0,23,2,1,0,0,23,3,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP2".into(),
            status: "00010000",
            parameters: "508,3,0,23,1,0,0,0,23,4,1,0,0,23,5,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP3".into(),
            status: "00010000",
            parameters: "508,3,0,23,1,1,0,0,23,6,1,0,0,23,7,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE1".into(),
            status: "00010000",
            parameters: "510,5,1,1,25;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE2".into(),
            status: "00010000",
            parameters: "510,5,1,1,27;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE3".into(),
            status: "00010000",
            parameters: "510,5,1,1,29;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,3,31,1,33,1,35,1;".into(),
        },
    ]);
    owned_test_file(&entities)
}

pub(crate) fn explicit_tetrahedron_solid_file() -> Vec<u8> {
    explicit_tetrahedron_solid_file_with_options(false, false)
}

pub(crate) fn explicit_tetrahedron_solid_file_with_transform(transformed: bool) -> Vec<u8> {
    explicit_tetrahedron_solid_file_with_options(transformed, false)
}

pub(crate) fn explicit_tetrahedron_solid_file_with_options(
    transformed: bool,
    inconsistent_radial_sense: bool,
) -> Vec<u8> {
    explicit_tetrahedron_solid_file_extended(transformed, inconsistent_radial_sense, false)
}

pub(crate) fn explicit_tetrahedron_solid_with_boolean_file() -> Vec<u8> {
    explicit_tetrahedron_solid_file_extended(false, false, true)
}

pub(crate) fn explicit_tetrahedron_solid_file_extended(
    transformed: bool,
    inconsistent_radial_sense: bool,
    with_boolean: bool,
) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut entities = vec![
        (116, 0, "POINTA", "00010000", "116,0,0,0,0;"),
        (116, 0, "POINTB", "00010000", "116,1,0,0,0;"),
        (123, 0, "NEGZ", "00010000", "123,0,0,-1;"),
        (123, 0, "NEGY", "00010000", "123,0,-1,0;"),
        (123, 0, "NEGX", "00010000", "123,-1,0,0;"),
        (
            123,
            0,
            "DIAG",
            "00010000",
            "123,0.5773502691896258,0.5773502691896258,0.5773502691896258;",
        ),
        (190, 0, "SURF1", "00010000", "190,1,5;"),
        (190, 0, "SURF2", "00010000", "190,1,7;"),
        (190, 0, "SURF3", "00010000", "190,1,9;"),
        (190, 0, "SURF4", "00010000", "190,3,11;"),
        (110, 0, "AB", "00010000", "110,0,0,0,1,0,0;"),
        (110, 0, "AC", "00010000", "110,0,0,0,0,1,0;"),
        (110, 0, "AD", "00010000", "110,0,0,0,0,0,1;"),
        (110, 0, "BC", "00010000", "110,1,0,0,0,1,0;"),
        (110, 0, "BD", "00010000", "110,1,0,0,0,0,1;"),
        (110, 0, "CD", "00010000", "110,0,1,0,0,0,1;"),
        (
            502,
            1,
            "VERTICES",
            "00010000",
            "502,4,0,0,0,1,0,0,0,1,0,0,0,1;",
        ),
        (
            504,
            1,
            "EDGES",
            "00010001",
            "504,6,21,33,1,33,2,23,33,1,33,3,25,33,1,33,4,27,33,2,33,3,29,33,2,33,4,31,33,3,33,4;",
        ),
        (
            508,
            1,
            "LOOP1",
            "00010000",
            "508,3,0,35,2,1,0,0,35,4,0,0,0,35,1,0,0;",
        ),
        (
            508,
            1,
            "LOOP2",
            "00010000",
            "508,3,0,35,1,1,0,0,35,5,1,0,0,35,3,0,0;",
        ),
        (
            508,
            1,
            "LOOP3",
            "00010000",
            "508,3,0,35,3,1,0,0,35,6,0,0,0,35,2,0,0;",
        ),
        if inconsistent_radial_sense {
            (
                508,
                1,
                "LOOP4",
                "00010000",
                "508,3,0,35,4,1,0,0,35,6,0,0,0,35,5,0,0;",
            )
        } else {
            (
                508,
                1,
                "LOOP4",
                "00010000",
                "508,3,0,35,4,1,0,0,35,6,1,0,0,35,5,0,0;",
            )
        },
        (510, 1, "FACE1", "00010000", "510,13,1,1,37;"),
        (510, 1, "FACE2", "00010000", "510,15,1,1,39;"),
        (510, 1, "FACE3", "00010000", "510,17,1,1,41;"),
        (510, 1, "FACE4", "00010000", "510,19,1,1,43;"),
        (514, 1, "SHELL", "00010000", "514,4,45,1,47,1,49,1,51,1;"),
        (186, 0, "SOLID", "00000000", "186,53,1,0;"),
    ];
    if transformed {
        entities.push((
            124,
            0,
            "PLACE",
            "00010000",
            "124,1,0,0,10,0,1,0,20,0,0,1,30;",
        ));
    }
    if with_boolean {
        entities.extend([
            (158, 0, "SPHERE", "00000000", "158,1,2,2,2;"),
            (180, 1, "MIXED", "00000000", "180,3,-55,-57,1;"),
            (184, 1, "ASSEMBLY", "00000200", "184,2,55,57,0,0;"),
            (430, 1, "BREPINST", "00000000", "430,55;"),
        ]);
    }
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    let mut parameter_sequence = 1_u32;
    for (index, (entity_type, form, label, status, parameters)) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        let line_count = parameter_fragment_count(parameters.as_bytes());
        let entity_type = entity_type.to_string();
        let form = form.to_string();
        let parameter_start = parameter_sequence.to_string();
        let line_count_string = line_count.to_string();
        let transform = if transformed && entity_type == "186" {
            "57"
        } else {
            "0"
        };
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
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
            [
                &entity_type,
                "0",
                "0",
                &line_count_string,
                &form,
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
        parameter_sequence += u32::try_from(line_count).unwrap();
    }
    parameter_sequence = 1;
    for (index, (_, _, _, _, parameters)) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        bytes.extend(parameter_cards(
            parameters.as_bytes(),
            sequence,
            parameter_sequence,
        ));
        parameter_sequence +=
            u32::try_from(parameter_fragment_count(parameters.as_bytes())).unwrap();
    }
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D{:07}P{:07}",
            entities.len() * 2,
            parameter_sequence - 1
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn explicit_vertex_loop_file() -> Vec<u8> {
    explicit_vertex_loop_file_with_outer_flag(true)
}

pub(crate) fn explicit_vertex_loop_file_with_outer_flag(has_outer_loop: bool) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "CENTER".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 196,
            form: 0,
            label: "SPHERE".into(),
            status: "00010000",
            parameters: "196,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "POLE".into(),
            status: "00010000",
            parameters: "502,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "VLOOP".into(),
            status: "00010000",
            parameters: "508,1,1,5,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE".into(),
            status: "00010000",
            parameters: format!("510,3,1,{},7;", i32::from(has_outer_loop)),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,1,9,1;".into(),
        },
    ])
}

pub(crate) fn colored_explicit_vertex_loop_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "CENTER".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 196,
            form: 0,
            label: "SPHERE".into(),
            status: "00010000",
            parameters: "196,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "POLE".into(),
            status: "00010000",
            parameters: "502,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "VLOOP".into(),
            status: "00010000",
            parameters: "508,1,1,5,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE".into(),
            status: "00010000",
            parameters: "510,3,1,1,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,1,9,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 314,
            form: 0,
            label: "COLOR".into(),
            status: "00000200",
            parameters: "314,20,40,60,6Hcustom;".into(),
        },
    ];
    owned_test_file_with_colors(&entities, &[(9, -13), (11, 2), (13, 2)])
}

pub(crate) fn line_font_definitions_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "TEMPLATE".into(),
            status: "00000200",
            parameters: "308,0,4HMARK,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 304,
            form: 1,
            label: "SYMBOLS".into(),
            status: "00000200",
            parameters: "304,1,1,2,0.5;".into(),
        },
        OwnedTestEntity {
            entity_type: 304,
            form: 2,
            label: "PATTERN".into(),
            status: "00000200",
            parameters: "304,5,2,1,2,1,2,2H16;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
    ];
    owned_test_file_with_display(&entities, &[], &[(3, 1), (5, 2), (7, -5)])
}

pub(crate) fn definition_levels_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 406,
            form: 1,
            label: "LEVELS".into(),
            status: "00000200",
            parameters: "406,3,2,7,11;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
    ];
    owned_test_file_with_levels(&entities, &[(3, -1)])
}

pub(crate) fn weighted_line_file() -> Vec<u8> {
    let entities = [OwnedTestEntity {
        entity_type: 110,
        form: 0,
        label: "LINE".into(),
        status: "00000000",
        parameters: "110,0,0,0,1,0,0;".into(),
    }];
    owned_test_file_with_line_weights(&entities, &[(1, 1)])
}

pub(crate) fn primitive_solids_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 150,
            form: 0,
            label: "BLOCK".into(),
            status: "00000000",
            parameters: "150,2,3,4,1,2,3,1,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 150,
            form: 0,
            label: "DEFAULT".into(),
            status: "00000000",
            parameters: "150,1,2,3,,,,,,,,,;".into(),
        },
        OwnedTestEntity {
            entity_type: 152,
            form: 0,
            label: "WEDGE".into(),
            status: "00000000",
            parameters: "152,4,3,2,1,0,0,0,1,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 154,
            form: 0,
            label: "CYLINDER".into(),
            status: "00000000",
            parameters: "154,5,2,1,2,3,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 156,
            form: 0,
            label: "FRUSTUM".into(),
            status: "00000000",
            parameters: "156,5,3,1,1,2,3,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,2,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 160,
            form: 0,
            label: "TORUS".into(),
            status: "00000000",
            parameters: "160,4,1,1,2,3,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 168,
            form: 0,
            label: "ELLIPSO".into(),
            status: "00000000",
            parameters: "168,4,3,2,1,2,3,1,0,0,0,0,1;".into(),
        },
    ])
}

pub(crate) fn procedural_and_boolean_solids_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "PROFILE1".into(),
            status: "00010000",
            parameters: "110,1,0,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "PROFILE2".into(),
            status: "00010000",
            parameters: "100,0,0,0,1,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 162,
            form: 0,
            label: "REVOPEN".into(),
            status: "00000000",
            parameters: "162,1,0.5,0,0,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 162,
            form: 1,
            label: "REVCLOSE".into(),
            status: "00000000",
            parameters: "162,3,1,0,0,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 164,
            form: 0,
            label: "EXTRUDE".into(),
            status: "00000000",
            parameters: "164,3,5,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE1".into(),
            status: "00000000",
            parameters: "158,2,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE2".into(),
            status: "00000000",
            parameters: "158,1,3,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "UNION".into(),
            status: "00000000",
            parameters: "180,3,-11,-13,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 182,
            form: 0,
            label: "SELECT".into(),
            status: "00000300",
            parameters: "182,15,1,0,0;".into(),
        },
    ])
}

pub(crate) fn solid_assembly_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE1".into(),
            status: "00000000",
            parameters: "158,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE2".into(),
            status: "00000000",
            parameters: "158,1,3,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 124,
            form: 0,
            label: "MOVE".into(),
            status: "00010000",
            parameters: "124,1,0,0,10,0,1,0,0,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 184,
            form: 0,
            label: "ASSEMBLY".into(),
            status: "00000200",
            parameters: "184,2,1,3,0,5;".into(),
        },
    ])
}

pub(crate) fn solid_instance_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,2,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 430,
            form: 0,
            label: "INSTANCE".into(),
            status: "00000000",
            parameters: "430,1;".into(),
        },
    ])
}

pub(crate) fn nested_brep_boolean_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 186,
            form: 0,
            label: "BREP".into(),
            status: "00000000",
            parameters: "186,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 430,
            form: 1,
            label: "BREPINST".into(),
            status: "00000000",
            parameters: "430,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "INSF0".into(),
            status: "00000000",
            parameters: "180,3,-3,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 1,
            label: "INSF1".into(),
            status: "00000000",
            parameters: "180,3,-3,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 1,
            label: "DIRECT".into(),
            status: "00000000",
            parameters: "180,3,-1,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "NESTF0".into(),
            status: "00000000",
            parameters: "180,3,-11,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 1,
            label: "NESTF1".into(),
            status: "00000000",
            parameters: "180,3,-11,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "BADNEST".into(),
            status: "00000000",
            parameters: "180,3,-9,-5,1;".into(),
        },
    ])
}

pub(crate) fn patterned_instance_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "BASE".into(),
            status: "00000000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 412,
            form: 0,
            label: "RECT".into(),
            status: "00000000",
            parameters: "412,1,2,1,2,3,2,3,10,5,0.25,1,0,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 414,
            form: 0,
            label: "CIRCLE".into(),
            status: "00000000",
            parameters: "414,3,4,10,20,30,8,0.5,1.25,2,1,1,3;".into(),
        },
    ])
}

pub(crate) fn external_reference_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 416,
            form: 0,
            label: "EXTDEF".into(),
            status: "00000000",
            parameters: "416,8Hpart.igs,7HBRACKET;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 1,
            label: "EXTFILE".into(),
            status: "00000000",
            parameters: "416,12Hassembly.igs;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 2,
            label: "EXTLOGIC".into(),
            status: "00000000",
            parameters: "416,9Hsheet.igs,7HFLANGE1;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 3,
            label: "NATIVE".into(),
            status: "00000000",
            parameters: "416,5HMOTOR;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 4,
            label: "LIBRARY".into(),
            status: "00000000",
            parameters: "416,7HDEVICES,5HRELAY;".into(),
        },
    ])
}

pub(crate) fn group_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "ORDERED".into(),
            status: "00000000",
            parameters: "116,1,2,3,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 14,
            label: "GROUP1".into(),
            status: "00000000",
            parameters: "402,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "UNORDER".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 7,
            label: "GROUP2".into(),
            status: "00000000",
            parameters: "402,1,5;".into(),
        },
    ])
}

pub(crate) fn attribute_definition_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 322,
            form: 0,
            label: "ATTRDEF".into(),
            status: "00000000",
            parameters: "322,4HMETA,1,2,10,1,1,11,3,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 1,
            label: "ATTRROW".into(),
            status: "00000000",
            parameters: "322,4HROW1,1,2,10,1,1,42,11,3,1,5HSTEEL;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 2,
            label: "ATTRDSP".into(),
            status: "00000000",
            parameters: "322,4HROW2,1,2,10,2,1,3.5,0,11,6,1,1,0;".into(),
        },
    ])
}

pub(crate) fn attribute_instance_forms_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 322,
            form: 0,
            label: "ATTRDEF".into(),
            status: "00000000",
            parameters: "322,4HMETA,1,2,10,1,1,11,3,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 422,
            form: 0,
            label: "ATTRONE".into(),
            status: "00000000",
            parameters: "422,7,5HSTEEL;".into(),
        },
        OwnedTestEntity {
            entity_type: 422,
            form: 1,
            label: "ATTRTAB".into(),
            status: "00000000",
            parameters: "422,2,8,4HIRON,9,5HBRASS;".into(),
        },
    ];
    owned_test_file_with_structures(&entities, &[(3, -1), (5, -1)])
}

pub(crate) fn attribute_instance_ignored_structures_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 322,
            form: 0,
            label: "ATTRDEF".into(),
            status: "00000000",
            parameters: "322,4HMETA,1,1,10,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 422,
            form: 0,
            label: "BLANK".into(),
            status: "00000000",
            parameters: "422,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 422,
            form: 0,
            label: "POSITIVE".into(),
            status: "00000000",
            parameters: "422,8;".into(),
        },
    ];
    owned_test_file_with_structures(&entities, &[(5, 1)])
}

pub(crate) fn structure_target_rules_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 322,
            form: 1,
            label: "WRNGATTR".into(),
            status: "00000000",
            parameters: "322,1HX,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 422,
            form: 0,
            label: "ATTRINST".into(),
            status: "00000000",
            parameters: "422;".into(),
        },
        OwnedTestEntity {
            entity_type: 302,
            form: 5001,
            label: "ASSOCDEF".into(),
            status: "00000200",
            parameters: "302,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 5001,
            label: "ASSOC".into(),
            status: "00000200",
            parameters: "402;".into(),
        },
        OwnedTestEntity {
            entity_type: 302,
            form: 5002,
            label: "OTHERDEF".into(),
            status: "00000200",
            parameters: "302,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 5001,
            label: "BADASSOC".into(),
            status: "00000200",
            parameters: "402;".into(),
        },
        OwnedTestEntity {
            entity_type: 306,
            form: 0,
            label: "MACRODEF".into(),
            status: "00000200",
            parameters: "306;".into(),
        },
        OwnedTestEntity {
            entity_type: 600,
            form: 0,
            label: "MACRO".into(),
            status: "00000000",
            parameters: "600;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "BADOWNER".into(),
            status: "00000000",
            parameters: "116,0,0,0,0;".into(),
        },
    ];
    owned_test_file_with_structures(
        &entities,
        &[(3, -1), (7, -5), (11, -9), (15, -13), (17, -1)],
    )
}

pub(crate) fn product_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "COMP".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,0,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 7,
            label: "REFDES".into(),
            status: "00010000",
            parameters: "406,1,2HR1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 15,
            label: "NAME".into(),
            status: "00010000",
            parameters: "406,1,7HBRACKET;".into(),
        },
    ])
}

pub(crate) fn scalar_property_forms_file() -> Vec<u8> {
    let cases = [
        (2, "406,3,0,1,2;"),
        (3, "406,2,17,5HPOWER;"),
        (4, "406,2,1,0;"),
        (5, "406,5,0.25,0,2,1,0.1;"),
        (6, "406,5,0.5,0.45,1,2,8;"),
        (8, "406,1,3HPA7;"),
        (9, "406,4,7HGENERIC,6HMIL123,6HVEND42,5HINT99;"),
        (10, "406,6,1,0,1,0,1,0;"),
        (12, "406,2,8HBASE.IGS,10HDETAIL.IGS;"),
        (13, "406,3,2.5,3HAWG,7HANSI123;"),
        (14, "406,2,4HMAIN,3HHOT;"),
        (18, "406,1,12.5;"),
        (19, "406,1,223;"),
        (20, "406,1,1;"),
        (21, "406,1,0;"),
    ];
    let entities = cases
        .into_iter()
        .map(|(form, parameters)| OwnedTestEntity {
            entity_type: 406,
            form,
            label: format!("PROP{form}"),
            status: "00000000",
            parameters: parameters.into(),
        })
        .collect::<Vec<_>>();
    owned_test_file(&entities)
}

pub(crate) fn invalid_drilled_hole_layer_order_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 406,
        form: 6,
        label: "DRILL".into(),
        status: "00000000",
        parameters: "406,5,0.5,0.45,1,8,2;".into(),
    }])
}

pub(crate) fn equal_drilled_hole_layer_range_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 406,
        form: 6,
        label: "DRILL".into(),
        status: "00000000",
        parameters: "406,5,0.5,0.45,1,2,2;".into(),
    }])
}

pub(crate) fn grid_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00020100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 22,
            label: "GRID".into(),
            status: "00010000",
            parameters: "406,9,1,1,0,0,0,5,10,20,30;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING".into(),
            status: "00000100",
            parameters: "404,1,1,0,0,0,0,0,1,3;".into(),
        },
    ])
}

pub(crate) fn group_type_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "ITEM".into(),
            status: "00000000",
            parameters: "116,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 7,
            label: "GROUP".into(),
            status: "00000000",
            parameters: "402,1,1,0,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 23,
            label: "GROUPTYP".into(),
            status: "00010000",
            parameters: "406,2,5,5HDRILL;".into(),
        },
    ])
}

pub(crate) fn lep_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 406,
            form: 24,
            label: "LAYERMAP".into(),
            status: "00000000",
            parameters: "406,9,2,10,4HTOP1,1,8HSIGNAL_T,20,4HCORE,0,9HUNDEFINED;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 25,
            label: "STACKUP".into(),
            status: "00000000",
            parameters: "406,5,5HBOARD,3,10,20,30;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "HOLE".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,0,1,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 26,
            label: "DRILL".into(),
            status: "00010000",
            parameters: "406,3,0.8,0.7,5;".into(),
        },
    ])
}

pub(crate) fn variable_schema_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "OWNER".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 27,
            label: "GENERIC".into(),
            status: "00010000",
            parameters: "406,14,4HMETA,6,0,,1,42,2,3.5,3,5HSTEEL,4,1,6,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 11,
            label: "TABULAR".into(),
            status: "00000000",
            parameters: "406,9,5,1,1,2,2,50,25,33,46;".into(),
        },
    ])
}

pub(crate) fn dimension_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "DIMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "ARROW".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "DIMENS".into(),
            status: "00000100",
            parameters: "216,1,3,3,0,0,0,4,7,9,11,13;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 28,
            label: "DIMUNITS".into(),
            status: "00000000",
            parameters: "406,6,0,2,,2HMM,0,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 29,
            label: "DIMTOL".into(),
            status: "00000000",
            parameters: "406,8,0,2,,0.1,-0.1,0,0,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 31,
            label: "BASICDIM".into(),
            status: "00010000",
            parameters: "406,8,0,0,2,0,2,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 30,
            label: "DIMDISP".into(),
            status: "00010000",
            parameters: "406,14,2,1,1,3HDIA,0,1.5707963267948966,1,0,0,0,12.5,1,1,1,1;".into(),
        },
    ])
}

pub(crate) fn drawing_metadata_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00020100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 32,
            label: "APPROVAL".into(),
            status: "00000000",
            parameters: "406,3,4HJANE,3HENG,15H20260714.123456;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 33,
            label: "SHEETID".into(),
            status: "00000000",
            parameters: "406,2,2,1HC;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING".into(),
            status: "00000100",
            parameters: "404,1,1,0,0,0,0,0,2,3,5;".into(),
        },
    ])
}

pub(crate) fn duplicate_drawing_sheet_ids_file() -> Vec<u8> {
    drawing_sheet_ids_file("C", false)
}

pub(crate) fn distinct_drawing_sheet_ids_file() -> Vec<u8> {
    drawing_sheet_ids_file("D", false)
}

pub(crate) fn shared_drawing_sheet_id_file() -> Vec<u8> {
    drawing_sheet_ids_file("C", true)
}

fn drawing_sheet_ids_file(second_sid: &str, share_first_property: bool) -> Vec<u8> {
    let second_drawing_property = if share_first_property { 3 } else { 9 };
    let mut entities = vec![
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW1".into(),
            status: "00020100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 33,
            label: "SHEET1".into(),
            status: "00000000",
            parameters: "406,2,2,1HC;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING1".into(),
            status: "00000100",
            parameters: "404,1,1,0,0,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW2".into(),
            status: "00020100",
            parameters: "410,2,1,0,0,0,0,0,0;".into(),
        },
    ];
    if !share_first_property {
        entities.push(OwnedTestEntity {
            entity_type: 406,
            form: 33,
            label: "SHEET2".into(),
            status: "00000000",
            parameters: format!("406,2,2,1H{second_sid};"),
        });
    }
    entities.push(OwnedTestEntity {
        entity_type: 404,
        form: 1,
        label: "DRAWING2".into(),
        status: "00000100",
        parameters: format!("404,1,7,0,0,0,0,0,1,{second_drawing_property};"),
    });
    owned_test_file(&entities)
}

pub(crate) fn text_score_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,5,1,1,1,1.5707963267948966,0,0,0,0,0,0,5HABCDE,0,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 34,
            label: "UNDER".into(),
            status: "00010000",
            parameters: "406,4,1,1,2,4;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 35,
            label: "OVER".into(),
            status: "00010000",
            parameters: "406,4,1,1,3,5;".into(),
        },
    ])
}

pub(crate) fn closure_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "SURFACE".into(),
            status: "00000000",
            parameters: "108,0,0,1,0,0,0,0,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 36,
            label: "CLOSURE".into(),
            status: "00010000",
            parameters: "406,2,0,1;".into(),
        },
    ])
}

pub(crate) fn view_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "ORTHO".into(),
            status: "00000100",
            parameters: "410,1,,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 1,
            label: "PERSP".into(),
            status: "00000100",
            parameters: "410,2,1.5,0,0,1,0,0,0,0,0,10,0,1,0,5,-2,2,-1,1,3,-5,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 1,
            label: "SCALE".into(),
            status: "00000100",
            parameters: "410,3,1,0,0,1E-200,0,0,0,0,0,10,0,1E-200,0,5,-2,2,-1,1,0,0,0;".into(),
        },
    ])
}

pub(crate) fn out_of_table_depth_clipping_view_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 410,
        form: 1,
        label: "BADDCI".into(),
        status: "00000100",
        parameters: "410,2,1.5,0,0,1,0,0,0,0,0,10,0,1,0,5,-2,2,-1,1,4,-5,5;".into(),
    }])
}

pub(crate) fn out_of_table_segmented_display_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 19,
            label: "BADDF".into(),
            status: "00000100",
            parameters: "402,1,1,0.5,2,0,0,0;".into(),
        },
    ])
}

pub(crate) fn defaulted_text_and_view_fields_file() -> Vec<u8> {
    let note_parameters = [
        "212", "1", "1", "1", "1", "", "", "", "", "", "", "", "", "1HA",
    ]
    .join(",")
        + ";";
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00000100",
            parameters: note_parameters,
        },
        OwnedTestEntity {
            entity_type: 312,
            form: 0,
            label: "TEMPLATE".into(),
            status: "00000200",
            parameters: "312,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "EMPTY".into(),
            status: "00000100",
            parameters: "212,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "NEWNOTE".into(),
            status: "00000100",
            parameters: "213,0,0,0,0,0,0,0,0,0,0,0,1,0,2,3,-0.5,0,18,0,4HTUNL,1,0,0,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000100",
            parameters: "410,1,1.,,,,,,;".into(),
        },
    ])
}

pub(crate) fn view_visibility_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW1".into(),
            status: "00000100",
            parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 3,
            label: "VISIBLE".into(),
            status: "00000100",
            parameters: "402,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW2".into(),
            status: "00000100",
            parameters: "410,2,1,0,0,0,0,0,0,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 4,
            label: "DISPLAY".into(),
            status: "00000100",
            parameters: "402,1,0,5,1,0,2,3;".into(),
        },
    ])
}

pub(crate) fn segmented_view_visibility_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 19,
            label: "SEGMENTS".into(),
            status: "00000100",
            parameters: "402,2,1,0.5,0,,,1,1,1.0,1,2,3,4;".into(),
        },
    ])
}

pub(crate) fn drawing_with_properties_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00020100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "NOTELOC".into(),
            status: "00010100",
            parameters: "116,5,6,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 16,
            label: "SIZE".into(),
            status: "00010000",
            parameters: "406,2,210,297;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 17,
            label: "UNITS".into(),
            status: "00010000",
            parameters: "406,2,2,2HMM;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 15,
            label: "NAME".into(),
            status: "00010000",
            parameters: "406,1,7HDETAIL1;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING".into(),
            status: "00000100",
            parameters: "404,1,1,10,20,0.5,1,3,0,3,5,7,9;".into(),
        },
    ])
}

pub(crate) fn drawing_with_conflicting_size_properties_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00020100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "NOTELOC".into(),
            status: "00010100",
            parameters: "116,5,6,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 16,
            label: "SIZEA".into(),
            status: "00010000",
            parameters: "406,2,210,297;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 17,
            label: "UNITS".into(),
            status: "00010000",
            parameters: "406,2,2,2HMM;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 15,
            label: "NAME".into(),
            status: "00010000",
            parameters: "406,1,7HDETAIL1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 16,
            label: "SIZEB".into(),
            status: "00010000",
            parameters: "406,2,216,297;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING".into(),
            status: "00000100",
            parameters: "404,1,1,10,20,0.5,1,3,0,4,5,7,9,11;".into(),
        },
    ])
}

pub(crate) fn text_annotation_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00000100",
            parameters: "212,2,5,20,4,1,1.5707963267948966,0,0,0,1,2,0,5HALPHA,3,12,3,18,1.5707963267948966,0.25,1,1,4,5,0,3HBET;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "NEWNOTE".into(),
            status: "00000100",
            parameters: "213,40,20,2,0,20,0,0,0,18,0,-5,1,0,2,3,-0.5,0,18,0,4HTUNL,4,12,3,1,1.5707963267948966,0,0,0,2,18,0,4HTOL!;".into(),
        },
    ])
}

pub(crate) fn defaulted_new_general_note_file() -> Vec<u8> {
    new_general_note_file_with_character_metrics("1", "1")
}

pub(crate) fn omitted_font_style_new_general_note_file() -> Vec<u8> {
    new_general_note_file_with_font("0", "1", "1", "", "0", "")
}

pub(crate) fn variable_spacing_default_new_general_note_file() -> Vec<u8> {
    new_general_note_file_with_fields("1", "1", "1", "", "0")
}

pub(crate) fn zero_character_metrics_new_general_note_file() -> Vec<u8> {
    new_general_note_file_with_character_metrics("0", "0")
}

pub(crate) fn omitted_character_metrics_new_general_note_file() -> Vec<u8> {
    new_general_note_file_with_character_metrics("", "")
}

pub(crate) fn omitted_character_count_new_general_note_file() -> Vec<u8> {
    new_general_note_file_with_fields("0", "1", "1", "", "")
}

pub(crate) fn new_general_note_character_set_file(character_set: &str) -> Vec<u8> {
    new_general_note_file_with_font_and_character_set("0", "1", "1", "0", "0", "1", character_set)
}

pub(crate) fn new_general_note_type_310_character_set_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "FONT".into(),
            status: "00000200",
            parameters: "310,101,4HBASE,,10,1,65,8,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "CUSTOM".into(),
            status: "00000100",
            parameters: new_general_note_parameters("0", "1", "1", "0", "0", "1", "-1") + ";",
        },
    ])
}

pub(crate) fn new_general_note_even_character_set_pointer_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "FONT".into(),
            status: "00000200",
            parameters: "310,101,4HBASE,,10,1,65,8,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "EVEN".into(),
            status: "00000100",
            parameters: new_general_note_parameters("0", "1", "1", "0", "0", "1", "-2") + ";",
        },
    ])
}

pub(crate) fn new_general_note_wrong_type_character_set_pointer_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 314,
            form: 0,
            label: "COLOR".into(),
            status: "00000200",
            parameters: "314,20,40,60,6HCUSTOM;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "WRONG".into(),
            status: "00000100",
            parameters: new_general_note_parameters("0", "1", "1", "0", "0", "1", "-1") + ";",
        },
    ])
}

pub(crate) fn malformed_general_note_parameter_types_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "BAD212".into(),
            status: "00000100",
            parameters: "212,1,1,1,1,1.0,,,,,,,,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "BAD213".into(),
            status: "00000100",
            parameters: "213,0,0,2.0,0,0,0,0,0,0,0,0,1,0,1,1,,,,,0,,,,,,,,,,;".into(),
        },
    ])
}

pub(crate) fn malformed_view_parameter_type_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 410,
        form: 0,
        label: "BADVIEW".into(),
        status: "00000100",
        parameters: "410,1.0,1.,,,,,,;".into(),
    }])
}

pub(crate) fn negative_text_box_dimensions_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NEG212".into(),
            status: "00000100",
            parameters: "212,1,1,-1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "NEG213".into(),
            status: "00000100",
            parameters: "213,-10,10,0,0,0,0,0,0,0,0,0,1,0,1,1,0,0,1,0,0,1,-2,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
    ])
}

fn new_general_note_file_with_character_metrics(
    character_width: &str,
    character_height: &str,
) -> Vec<u8> {
    new_general_note_file_with_fields("0", character_width, character_height, "", "0")
}

fn new_general_note_file_with_fields(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
) -> Vec<u8> {
    new_general_note_file_with_font(
        fixed_or_variable,
        character_width,
        character_height,
        character_spacing,
        character_count,
        "1",
    )
}

fn new_general_note_file_with_font(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
    font_style: &str,
) -> Vec<u8> {
    new_general_note_file_with_font_and_character_set(
        fixed_or_variable,
        character_width,
        character_height,
        character_spacing,
        character_count,
        font_style,
        "",
    )
}

fn new_general_note_file_with_font_and_character_set(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
    font_style: &str,
    character_set: &str,
) -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 213,
        form: 0,
        label: "DEFAULTS".into(),
        status: "00000100",
        parameters: new_general_note_parameters(
            fixed_or_variable,
            character_width,
            character_height,
            character_spacing,
            character_count,
            font_style,
            character_set,
        ) + ";",
    }])
}

fn new_general_note_parameters(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
    font_style: &str,
    character_set: &str,
) -> String {
    let mut fields = vec![String::from("213")];
    fields.extend((0..11).map(|_| String::new()));
    fields.push(String::from("1"));
    fields.extend(
        [
            fixed_or_variable,
            character_width,
            character_height,
            character_spacing,
            "", // LSPACE
            font_style,
            "", // CHRANG
            "", // CCTEXT
            character_count,
            "", // WT
            "", // HT
            character_set,
            "", // SL
            "", // A
            "", // M
            "", // VH
            "", // XS
            "", // YS
            "", // ZS
            "", // TEXT
        ]
        .into_iter()
        .map(str::to_owned),
    );
    fields.join(",")
}

pub(crate) fn out_of_table_annotation_font_values_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "BAD212".into(),
            status: "00000100",
            parameters: "212,1,1,1,4,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "BAD213".into(),
            status: "00000100",
            parameters: "213,40,20,2,0,20,0,0,0,18,0,-5,1,0,2,3,-0.5,0,4,0,4HTUNL,4,12,3,1,1.5707963267948966,0,0,0,2,18,0,4HTOL!;".into(),
        },
    ])
}

pub(crate) fn leader_forms_file() -> Vec<u8> {
    let entities = (1..=12)
        .map(|form| {
            let (height, width) = match form {
                4 => (0, 0),
                5 | 6 | 12 => (2, 2),
                _ => (2, 1),
            };
            OwnedTestEntity {
                entity_type: 214,
                form,
                label: format!("LEAD{form}"),
                status: "00000100",
                parameters: format!("214,2,{height},{width},3,0,0,5,0,5,4;"),
            }
        })
        .collect::<Vec<_>>();
    owned_test_file(&entities)
}
