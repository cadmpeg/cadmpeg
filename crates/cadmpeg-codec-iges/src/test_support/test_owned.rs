// SPDX-License-Identifier: Apache-2.0
//! Owned-entity fixture assembler for crate tests.
#![allow(clippy::unwrap_used)]

use super::test_cards::*;

pub(crate) struct OwnedTestEntity {
    pub(crate) entity_type: i64,
    pub(crate) form: i64,
    pub(crate) label: String,
    pub(crate) status: &'static str,
    pub(crate) parameters: String,
}

struct DirectoryFields<'a> {
    colors: &'a [(u32, i64)],
    line_fonts: &'a [(u32, i64)],
    levels: &'a [(u32, i64)],
    line_weights: &'a [(u32, i64)],
    structures: &'a [(u32, i64)],
}

pub(crate) fn owned_test_file(entities: &[OwnedTestEntity]) -> Vec<u8> {
    owned_test_file_with_colors(entities, &[])
}

pub(crate) fn owned_test_file_with_raw_parameters(entities: &[OwnedTestEntity]) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    owned_test_file_with_parameter_layout(
        entities,
        global,
        &DirectoryFields {
            colors: &[],
            line_fonts: &[],
            levels: &[],
            line_weights: &[],
            structures: &[],
        },
        true,
    )
}

pub(crate) fn owned_test_file_with_colors(
    entities: &[OwnedTestEntity],
    colors: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_display(entities, colors, &[])
}

pub(crate) fn owned_test_file_with_display(
    entities: &[OwnedTestEntity],
    colors: &[(u32, i64)],
    line_fonts: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_attributes(entities, colors, line_fonts, &[], &[])
}

pub(crate) fn owned_test_file_with_levels(
    entities: &[OwnedTestEntity],
    levels: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_attributes(entities, &[], &[], levels, &[])
}

pub(crate) fn owned_test_file_with_line_weights(
    entities: &[OwnedTestEntity],
    line_weights: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_attributes(entities, &[], &[], &[], line_weights)
}

pub(crate) fn owned_test_file_with_attributes(
    entities: &[OwnedTestEntity],
    colors: &[(u32, i64)],
    line_fonts: &[(u32, i64)],
    levels: &[(u32, i64)],
    line_weights: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_directory_fields(entities, colors, line_fonts, levels, line_weights, &[])
}

pub(crate) fn owned_test_file_with_structures(
    entities: &[OwnedTestEntity],
    structures: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_directory_fields(entities, &[], &[], &[], &[], structures)
}

pub(crate) fn owned_test_file_with_directory_fields(
    entities: &[OwnedTestEntity],
    colors: &[(u32, i64)],
    line_fonts: &[(u32, i64)],
    levels: &[(u32, i64)],
    line_weights: &[(u32, i64)],
    structures: &[(u32, i64)],
) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    owned_test_file_with_parameter_layout(
        entities,
        global,
        &DirectoryFields {
            colors,
            line_fonts,
            levels,
            line_weights,
            structures,
        },
        false,
    )
}

pub(crate) fn owned_test_file_with_global(entities: &[OwnedTestEntity], global: &[u8]) -> Vec<u8> {
    owned_test_file_with_parameter_layout(
        entities,
        global,
        &DirectoryFields {
            colors: &[],
            line_fonts: &[],
            levels: &[],
            line_weights: &[],
            structures: &[],
        },
        false,
    )
}

fn owned_test_file_with_parameter_layout(
    entities: &[OwnedTestEntity],
    global: &[u8],
    fields: &DirectoryFields<'_>,
    raw_parameters: bool,
) -> Vec<u8> {
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    let mut parameter_sequence = 1_u32;
    for (index, entity) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        let line_count = if raw_parameters {
            raw_parameter_fragment_count(entity.parameters.as_bytes())
        } else {
            parameter_fragment_count(entity.parameters.as_bytes())
        };
        bytes.extend(directory_card(
            [
                &entity.entity_type.to_string(),
                &parameter_sequence.to_string(),
                &fields
                    .structures
                    .iter()
                    .find_map(|(entry, structure)| (*entry == sequence).then_some(*structure))
                    .unwrap_or(0)
                    .to_string(),
                &fields
                    .line_fonts
                    .iter()
                    .find_map(|(entry, line_font)| (*entry == sequence).then_some(*line_font))
                    .unwrap_or(0)
                    .to_string(),
                &fields
                    .levels
                    .iter()
                    .find_map(|(entry, level)| (*entry == sequence).then_some(*level))
                    .unwrap_or(0)
                    .to_string(),
                "0",
                "0",
                "0",
                entity.status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                &entity.entity_type.to_string(),
                &fields
                    .line_weights
                    .iter()
                    .find_map(|(entry, weight)| (*entry == sequence).then_some(*weight))
                    .unwrap_or(0)
                    .to_string(),
                &fields
                    .colors
                    .iter()
                    .find_map(|(entry, color)| (*entry == sequence).then_some(*color))
                    .unwrap_or(0)
                    .to_string(),
                &line_count.to_string(),
                &entity.form.to_string(),
                "",
                "",
                &entity.label,
                "0",
            ],
            sequence + 1,
        ));
        parameter_sequence += u32::try_from(line_count).unwrap();
    }
    parameter_sequence = 1;
    for (index, entity) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        let line_count = if raw_parameters {
            raw_parameter_fragment_count(entity.parameters.as_bytes())
        } else {
            parameter_fragment_count(entity.parameters.as_bytes())
        };
        if raw_parameters {
            bytes.extend(raw_parameter_cards(
                entity.parameters.as_bytes(),
                sequence,
                parameter_sequence,
            ));
        } else {
            bytes.extend(parameter_cards(
                entity.parameters.as_bytes(),
                sequence,
                parameter_sequence,
            ));
        }
        parameter_sequence += u32::try_from(line_count).unwrap();
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

pub(crate) fn append_tetrahedral_shell(
    entities: &mut Vec<OwnedTestEntity>,
    label: &str,
    origin: [f64; 3],
    size: f64,
) -> u32 {
    let sequence = |index: usize| u32::try_from(index * 2 + 1).unwrap();
    let first = entities.len();
    let vertices = [
        origin,
        [origin[0] + size, origin[1], origin[2]],
        [origin[0], origin[1] + size, origin[2]],
        [origin[0], origin[1], origin[2] + size],
    ];
    for (index, point) in vertices.iter().enumerate().take(2) {
        entities.push(OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: format!("{label}P{index}"),
            status: "00010000",
            parameters: format!("116,{},{},{},0;", point[0], point[1], point[2]),
        });
    }
    for (index, normal) in [
        [0.0, 0.0, -1.0],
        [0.0, -1.0, 0.0],
        [-1.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    ]
    .iter()
    .enumerate()
    {
        entities.push(OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: format!("{label}N{index}"),
            status: "00010000",
            parameters: format!("123,{},{},{};", normal[0], normal[1], normal[2]),
        });
    }
    for (index, (point_offset, normal_offset)) in
        [(0, 2), (0, 3), (0, 4), (1, 5)].into_iter().enumerate()
    {
        entities.push(OwnedTestEntity {
            entity_type: 190,
            form: 0,
            label: format!("{label}S{index}"),
            status: "00010000",
            parameters: format!(
                "190,{},{};",
                sequence(first + point_offset),
                sequence(first + normal_offset)
            ),
        });
    }
    for (index, (start, end)) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        .into_iter()
        .enumerate()
    {
        let a = vertices[start];
        let b = vertices[end];
        entities.push(OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: format!("{label}E{index}"),
            status: "00010000",
            parameters: format!("110,{},{},{},{},{},{};", a[0], a[1], a[2], b[0], b[1], b[2]),
        });
    }
    let vertex_list = sequence(entities.len());
    entities.push(OwnedTestEntity {
        entity_type: 502,
        form: 1,
        label: format!("{label}VERT"),
        status: "00010000",
        parameters: format!(
            "502,4,{},{},{},{},{},{},{},{},{},{},{},{};",
            vertices[0][0],
            vertices[0][1],
            vertices[0][2],
            vertices[1][0],
            vertices[1][1],
            vertices[1][2],
            vertices[2][0],
            vertices[2][1],
            vertices[2][2],
            vertices[3][0],
            vertices[3][1],
            vertices[3][2]
        ),
    });
    let edge_list = sequence(entities.len());
    let curve = |offset: usize| sequence(first + 10 + offset);
    entities.push(OwnedTestEntity {
        entity_type: 504,
        form: 1,
        label: format!("{label}EDGE"),
        status: "00010001",
        parameters: format!(
            "504,6,{}, {},1,{},2,{}, {},1,{},3,{}, {},1,{},4,{}, {},2,{},3,{}, {},2,{},4,{}, {},3,{},4;",
            curve(0), vertex_list, vertex_list,
            curve(1), vertex_list, vertex_list,
            curve(2), vertex_list, vertex_list,
            curve(3), vertex_list, vertex_list,
            curve(4), vertex_list, vertex_list,
            curve(5), vertex_list, vertex_list,
        ).replace(' ', ""),
    });
    let mut loop_sequences = Vec::new();
    for (index, uses) in [
        [(2, 1), (4, 0), (1, 0)],
        [(1, 1), (5, 1), (3, 0)],
        [(3, 1), (6, 0), (2, 0)],
        [(4, 1), (6, 1), (5, 0)],
    ]
    .into_iter()
    .enumerate()
    {
        let loop_sequence = sequence(entities.len());
        loop_sequences.push(loop_sequence);
        entities.push(OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: format!("{label}L{index}"),
            status: "00010000",
            parameters: format!(
                "508,3,0,{edge_list},{}, {},0,0,{edge_list},{}, {},0,0,{edge_list},{}, {},0;",
                uses[0].0, uses[0].1, uses[1].0, uses[1].1, uses[2].0, uses[2].1
            )
            .replace(' ', ""),
        });
    }
    let mut face_sequences = Vec::new();
    for (index, loop_sequence) in loop_sequences.into_iter().enumerate() {
        let face_sequence = sequence(entities.len());
        face_sequences.push(face_sequence);
        entities.push(OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: format!("{label}F{index}"),
            status: "00010000",
            parameters: format!("510,{},1,1,{loop_sequence};", sequence(first + 6 + index)),
        });
    }
    let shell = sequence(entities.len());
    entities.push(OwnedTestEntity {
        entity_type: 514,
        form: 1,
        label: format!("{label}SH"),
        status: "00010000",
        parameters: format!(
            "514,4,{},1,{},1,{},1,{},1;",
            face_sequences[0], face_sequences[1], face_sequences[2], face_sequences[3]
        ),
    });
    shell
}

pub(crate) fn explicit_void_solid_file() -> (Vec<u8>, u32, u32, u32) {
    let mut entities = Vec::new();
    let outer = append_tetrahedral_shell(&mut entities, "OUT", [0.0, 0.0, 0.0], 4.0);
    let void = append_tetrahedral_shell(&mut entities, "VOID", [0.5, 0.5, 0.5], 0.5);
    let solid = u32::try_from(entities.len() * 2 + 1).unwrap();
    entities.push(OwnedTestEntity {
        entity_type: 186,
        form: 0,
        label: "VOIDBODY".into(),
        status: "00000000",
        parameters: format!("186,{outer},1,1,{void},0;"),
    });

    (owned_test_file(&entities), solid, outer, void)
}
