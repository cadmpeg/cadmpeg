// SPDX-License-Identifier: Apache-2.0
//! Drawing, annotation, and trimming byte fixtures for crate tests.
#![allow(clippy::unwrap_used)]

use super::test_cards::*;
use super::test_owned::*;

pub(crate) fn dimension_forms_file() -> Vec<u8> {
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
            parameters: "214,3,2,1,0,0,0,2,0,2,2,4,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 40,
            label: "WITNESS".into(),
            status: "00010100",
            parameters: "106,1,3,0,0,0,1,0,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "ENCLOSE".into(),
            status: "00010100",
            parameters: "100,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 4,
            label: "NOARROW".into(),
            status: "00010100",
            parameters: "214,1,0,0,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "LINEAR0".into(),
            status: "00000100",
            parameters: "216,1,3,3,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 1,
            label: "LINEAR1".into(),
            status: "00000100",
            parameters: "216,1,3,3,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 2,
            label: "LINEAR2".into(),
            status: "00000100",
            parameters: "216,1,3,3,5,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 218,
            form: 0,
            label: "ORD0".into(),
            status: "00000100",
            parameters: "218,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 218,
            form: 1,
            label: "ORD1".into(),
            status: "00000100",
            parameters: "218,1,5,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 220,
            form: 0,
            label: "POINTDIM".into(),
            status: "00000100",
            parameters: "220,1,3,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 222,
            form: 0,
            label: "RADIUS0".into(),
            status: "00000100",
            parameters: "222,1,3,10,20;".into(),
        },
        OwnedTestEntity {
            entity_type: 222,
            form: 1,
            label: "RADIUS1".into(),
            status: "00000100",
            parameters: "222,1,3,10,20,9;".into(),
        },
    ])
}

pub(crate) fn legacy_dimension_and_label_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LEADER1".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LEADER2".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,1,0,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 40,
            label: "WITNESS".into(),
            status: "00010100",
            parameters: "106,1,3,0,0,0,1,0,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CURVE1".into(),
            status: "00010100",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "CURVE2".into(),
            status: "00010100",
            parameters: "100,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 202,
            form: 0,
            label: "ANGULAR".into(),
            status: "00000100",
            parameters: "202,1,7,0,0,0,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 204,
            form: 0,
            label: "CURVEDIM".into(),
            status: "00000100",
            parameters: "204,1,9,11,3,5,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 206,
            form: 0,
            label: "DIAMETER".into(),
            status: "00000100",
            parameters: "206,1,3,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 208,
            form: 0,
            label: "FLAGNOTE".into(),
            status: "00000100",
            parameters: "208,0,0,0,0,1,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 210,
            form: 0,
            label: "LABEL".into(),
            status: "00000100",
            parameters: "210,1,1,3;".into(),
        },
    ])
}

pub(crate) fn symbol_and_sectioned_area_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "SYMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HS;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "SYMGEOM".into(),
            status: "00010100",
            parameters: "100,0,0,0,1,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 2,
            label: "SYMLEAD".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 228,
            form: 0,
            label: "SYMBOL".into(),
            status: "00000100",
            parameters: "228,1,1,3,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00000000",
            parameters: "100,0,0,0,5,0,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "ISLAND".into(),
            status: "00000000",
            parameters: "100,0,0,0,1,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 230,
            form: 0,
            label: "SECTION".into(),
            status: "00000100",
            parameters: "230,9,2,0,0,0,1,0.7853981633974483,1,11;".into(),
        },
    ])
}

pub(crate) fn general_symbol_form_file(form: i64, global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global_and_line_fonts(
        &[
            OwnedTestEntity {
                entity_type: 212,
                form: 0,
                label: "SYMNOTE".into(),
                status: "00010100",
                parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HS;".into(),
            },
            OwnedTestEntity {
                entity_type: 100,
                form: 0,
                label: "SYMGEOM".into(),
                status: "00010100",
                parameters: "100,0,0,0,1,0,1,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 214,
                form: 2,
                label: "SYMLEAD".into(),
                status: "00010100",
                parameters: "214,1,2,1,0,0,0,2,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 228,
                form,
                label: "SYMBOL".into(),
                status: "00000100",
                parameters: "228,1,1,3,1,5;".into(),
            },
        ],
        global,
        &[(3, 1)],
    )
}

pub(crate) fn inverted_sectioned_area_file() -> Vec<u8> {
    inverted_sectioned_area_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    )
}

pub(crate) fn inverted_sectioned_area_file_with_global(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global(
        &[
            OwnedTestEntity {
                entity_type: 100,
                form: 0,
                label: "ISLAND".into(),
                status: "00000000",
                parameters: "100,0,0,0,1,0,1,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 230,
                form: 1,
                label: "INVERTED".into(),
                status: "00000100",
                parameters: "230,0,2,0,0,0,1,0.7853981633974483,1,1;".into(),
            },
        ],
        global,
    )
}

pub(crate) fn out_of_table_sectioned_area_pattern_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00000000",
            parameters: "100,0,0,0,5,0,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 230,
            form: 0,
            label: "BADPAT".into(),
            status: "00000100",
            parameters: "230,1,21,0,0,0,0,0;".into(),
        },
    ])
}

pub(crate) fn associativity_definition_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 302,
        form: 5001,
        label: "ASSOCDEF".into(),
        status: "00000200",
        parameters: "302,2,1,1,2,1,2,2,2,1,3;".into(),
    }])
}

pub(crate) fn bounded_associativity_forms_file() -> Vec<u8> {
    bounded_associativity_forms_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    )
}

pub(crate) fn bounded_associativity_forms_file_with_global(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global(
        &[
            OwnedTestEntity {
                entity_type: 410,
                form: 0,
                label: "VIEW".into(),
                status: "00000100",
                parameters: "410,1,1,0,0,0,0,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 214,
                form: 1,
                label: "LABELARR".into(),
                status: "00010100",
                parameters: "214,1,2,1,0,0,0,2,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "LABELED".into(),
                status: "00000000",
                parameters: "116,1,2,3,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 5,
                label: "LABELDSP".into(),
                status: "00000200",
                parameters: "402,1,1,1,2,3,3,0,5;".into(),
            },
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "PARENT".into(),
                status: "00000000",
                parameters: "116,0,0,0,0,1,13,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "CHILD".into(),
                status: "00000000",
                parameters: "116,1,0,0,0,1,13,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 9,
                label: "PARENTCH".into(),
                status: "00000200",
                parameters: "402,1,1,9,11;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 12,
                label: "EXTINDEX".into(),
                status: "00000200",
                parameters: "402,1,4HNAME,9;".into(),
            },
            OwnedTestEntity {
                entity_type: 212,
                form: 0,
                label: "DIMNOTE".into(),
                status: "00010100",
                parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HD;".into(),
            },
            OwnedTestEntity {
                entity_type: 214,
                form: 1,
                label: "DIMARR".into(),
                status: "00010100",
                parameters: "214,1,2,1,0,0,0,2,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 216,
                form: 0,
                label: "DIMENS".into(),
                status: "00000100",
                parameters: "216,17,19,19,0,0,1,23,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 13,
                label: "DIMGEOM".into(),
                status: "00000200",
                parameters: "402,1,1,21,9;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 16,
                label: "PLANAR".into(),
                status: "00000200",
                parameters: "402,1,2,0,9,11;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 2,
                label: "EXTLOGIC".into(),
                status: "00000200",
                parameters: "402,1,4HNAME,9;".into(),
            },
        ],
        global,
    )
}

pub(crate) fn legacy_perforated_plane_file(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global_and_line_fonts(
        &[
            OwnedTestEntity {
                entity_type: 108,
                form: 1,
                label: "PPLANE".into(),
                status: "00000000",
                parameters: "108,0,0,1,0,5,0,0,0,0,1,9,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 108,
                form: -1,
                label: "CPLANE".into(),
                status: "00010000",
                parameters: "108,0,0,1,0,7,0,0,0,0,1,9,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 106,
                form: 63,
                label: "OUTER".into(),
                status: "00010000",
                parameters: "106,1,5,0,0,0,10,0,10,10,0,10,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 106,
                form: 63,
                label: "INNER".into(),
                status: "00010000",
                parameters: "106,1,5,0,2,2,4,2,4,4,2,4,2,2;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 9,
                label: "PERFOR8".into(),
                status: "00000200",
                parameters: "402,1,1,1,3;".into(),
            },
        ],
        global,
        &[(1, 1), (3, 1), (5, 1), (7, 1)],
    )
}

pub(crate) fn legacy_generic_single_parent_file(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global_and_line_fonts(
        &[
            OwnedTestEntity {
                entity_type: 108,
                form: 1,
                label: "GENPARN".into(),
                status: "00000000",
                parameters: "108,0,0,1,0,3,0,0,0,0,1,7,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 106,
                form: 63,
                label: "OUTER".into(),
                status: "00010000",
                parameters: "106,1,5,0,0,0,10,0,10,10,0,10,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "CHILD".into(),
                status: "00000000",
                parameters: "116,1,2,3,0,1,7,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form: 9,
                label: "GENASSOC".into(),
                status: "00000200",
                parameters: "402,1,1,1,5;".into(),
            },
        ],
        global,
        &[(1, 1), (3, 1)],
    )
}

pub(crate) fn label_display_without_leader_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000100",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "LABELED".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 5,
            label: "LABELDSP".into(),
            status: "00000200",
            parameters: "402,1,1,1,2,3,0,0,3;".into(),
        },
    ])
}

fn view_list_associativity_entities(back_pointers: bool) -> Vec<OwnedTestEntity> {
    let suffix = if back_pointers { ",1,3,0" } else { "" };
    vec![
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000100",
            parameters: format!("410,1,1,0,0,0,0,0,0{suffix};"),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 6,
            label: "VIEWLIST".into(),
            status: "00000200",
            parameters: "402,1,1,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "VISIBLE".into(),
            status: "00000000",
            parameters: format!("116,1,2,3,0{suffix};"),
        },
    ]
}

pub(crate) fn view_list_associativity_file(back_pointers: bool) -> Vec<u8> {
    owned_test_file(&view_list_associativity_entities(back_pointers))
}

pub(crate) fn view_list_associativity_file_with_global(
    back_pointers: bool,
    global: &[u8],
) -> Vec<u8> {
    owned_test_file_with_global(&view_list_associativity_entities(back_pointers), global)
}

fn legacy_associativity_entities() -> Vec<OwnedTestEntity> {
    vec![
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "NODEPT".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 11,
            label: "NODE".into(),
            status: "00000400",
            parameters: "402,1,2,1,6HCONSTR,42,1,9,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "TEXTPT".into(),
            status: "00000000",
            parameters: "116,1,2,3,0,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 10,
            label: "TEXTNODE".into(),
            status: "00000400",
            parameters: "402,1,1,5,1.0,2.0,1,1.5708,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 8,
            label: "SIGNAL".into(),
            status: "00000400",
            parameters: "402,1,1,1,1,3HNET,3,11,11;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "SIGCURVE".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0,1,9,0;".into(),
        },
    ]
}

pub(crate) fn legacy_associativity_forms_file() -> Vec<u8> {
    owned_test_file_with_display(&legacy_associativity_entities(), &[], &[(11, 1)])
}

pub(crate) fn legacy_associativity_forms_file_with_global(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global_and_line_fonts(&legacy_associativity_entities(), global, &[(11, 1)])
}

pub(crate) fn legacy_text_node_font_pointer_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "FONT".into(),
            status: "00000200",
            parameters: "310,101,4HBASE,,10,1,65,8,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "TEXTPT".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,1,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 10,
            label: "TEXTNODE".into(),
            status: "00000400",
            parameters: "402,1,1,3,1.0,2.0,-1,1.5708,0,0,0;".into(),
        },
    ])
}

pub(crate) fn flow_associativity_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "SIGNALPT".into(),
            status: "00000400",
            parameters: "132,0,0,0,0,101,1,2HP1,0,3HPIN,0,1,1,0,0,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "SIGNAL".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "FLOWNAME".into(),
            status: "00000100",
            parameters: "212,1,4,4,1,1,1.5707963267948966,0,0,0,0,0,0,4HFLOW;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 18,
            label: "FLOW".into(),
            status: "00000200",
            parameters: "402,2,0,1,1,1,1,1,1,2,1,3,4HFLOW,5,9;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 18,
            label: "FLOWTAIL".into(),
            status: "00000200",
            parameters: "402,2,0,0,0,1,0,0,1,2,4HTAIL,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "PIPEPT".into(),
            status: "00000400",
            parameters: "132,0,0,0,0,101,1,2HP2,0,4HPIPE,0,2,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "PIPE".into(),
            status: "00000000",
            parameters: "110,0,0,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 20,
            label: "PIPEFLOW".into(),
            status: "00000200",
            parameters: "402,1,0,1,1,1,0,1,2,11,13,4HPIPE,17;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 20,
            label: "PIPETAIL".into(),
            status: "00000200",
            parameters: "402,1,0,0,0,1,0,0,2,4HTAIL;".into(),
        },
    ])
}

pub(crate) fn recalculable_dimension_associativity_file() -> Vec<u8> {
    recalculable_dimension_associativity_file_with_orientation(4)
}

pub(crate) fn recalculable_dimension_associativity_file_with_orientation(
    orientation: i64,
) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "DIMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HD;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 2,
            label: "ARROW1".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 2,
            label: "ARROW2".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,4,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "GEOM1".into(),
            status: "00000000",
            parameters: "110,0,0,0,0,4,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "GEOM2".into(),
            status: "00000000",
            parameters: "110,4,0,0,4,4,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "DIMENS".into(),
            status: "00000100",
            parameters: "216,1,3,5,0,0,1,13,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 21,
            label: "RECALCD".into(),
            status: "00010200",
            parameters: format!("402,1,2,11,{orientation},0,7,0,0,0,0,9,1,4,0,0;"),
        },
    ])
}

pub(crate) fn text_display_template_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 312,
            form: 0,
            label: "ABSTEXT".into(),
            status: "00000200",
            parameters: "312,4,2,1,1.5707963267948966,0,0,0,10,20,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 312,
            form: 1,
            label: "INCTEXT".into(),
            status: "00000200",
            parameters: "312,3,1,18,1.5707963267948966,0.25,1,1,2,-1,0;".into(),
        },
    ])
}

pub(crate) fn out_of_table_text_template_font_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 312,
        form: 0,
        label: "BADFONT".into(),
        status: "00000200",
        parameters: "312,4,2,4,1.5707963267948966,0,0,0,0,0,0;".into(),
    }])
}

pub(crate) fn text_font_definition_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "BASEFONT".into(),
            status: "00000200",
            parameters: "310,101,4HBASE,,10,2,65,8,0,3,,0,0,0,4,10,0,8,0,66,8,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "MODFONT".into(),
            status: "00000200",
            parameters: "310,102,3HMOD,-1,10,1,67,8,0,2,1,0,0,,8,10;".into(),
        },
        OwnedTestEntity {
            entity_type: 312,
            form: 0,
            label: "FONTUSE".into(),
            status: "00000200",
            parameters: "312,4,2,-3,1.5707963267948966,0,0,0,0,0,0;".into(),
        },
    ])
}

pub(crate) fn units_data_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "MEASURED".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 316,
            form: 0,
            label: "UNITS".into(),
            status: "00000200",
            parameters: "316,3,6HLENGTH,2HKN,1852,4HTIME,1HS,1,5HPLANE,1HD,0.017453292519943295;"
                .into(),
        },
    ])
}

pub(crate) fn units_data_scope_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "MEASURED".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 316,
            form: 0,
            label: "UNITS".into(),
            status: "00000200",
            parameters: "316,1,6HLENGTH,2HKN,1852;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "UNMEAS".into(),
            status: "00000000",
            parameters: "110,0,0,0,2,0,0;".into(),
        },
    ])
}

pub(crate) fn nested_subfigure_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "MEMBER".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "CHILD".into(),
            status: "00000200",
            parameters: "308,0,5HCHILD,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "CHILDINS".into(),
            status: "00000000",
            parameters: "408,3,1,2,3,0.5;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "PARENT".into(),
            status: "00000200",
            parameters: "308,1,6HPARENT,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "PARENTIN".into(),
            status: "00000000",
            parameters: "408,7,10,20,30,2;".into(),
        },
    ])
}

pub(crate) fn transformed_subfigure_definition_file(
    definition_type: i64,
    instance_type: i64,
    global: &[u8],
    line_font: i64,
    label_display: i64,
    definition_transform: i64,
) -> Vec<u8> {
    struct Entry<'a> {
        entity_type: i64,
        form: i64,
        transform: i64,
        label_display: i64,
        label: &'a str,
        status: &'a str,
        parameters: &'a [u8],
    }
    assert!(matches!(
        (definition_type, instance_type),
        (308, 408) | (320, 420)
    ));
    let definition_parameters: &[u8] = match definition_type {
        308 => b"308,0,3HDEF,1,3;",
        320 => b"320,0,3HNET,1,3,1,,,0;",
        _ => unreachable!(),
    };
    let instance_parameters: &[u8] = match instance_type {
        408 => b"408,5,0,0,0,1;",
        420 => b"420,5,0,0,0,1,,,1,,,0;",
        _ => unreachable!(),
    };
    let mut entries = Vec::from([
        Entry {
            entity_type: 124,
            form: 0,
            transform: 0,
            label_display: 0,
            label: "MATRIX",
            status: "00010000",
            parameters: b"124,1,0,0,10,0,1,0,20,0,0,1,30;",
        },
        Entry {
            entity_type: 110,
            form: 0,
            transform: 0,
            label_display: 0,
            label: "MEMBER",
            status: "00010000",
            parameters: b"110,0,0,0,1,0,0;",
        },
        Entry {
            entity_type: definition_type,
            form: 0,
            transform: definition_transform,
            label_display,
            label: "DEF",
            status: "00000200",
            parameters: definition_parameters,
        },
        Entry {
            entity_type: instance_type,
            form: 0,
            transform: 0,
            label_display: 0,
            label: "INSTANCE",
            status: "00000000",
            parameters: instance_parameters,
        },
    ]);
    if label_display != 0 {
        entries.extend([
            Entry {
                entity_type: 402,
                form: 5,
                transform: 0,
                label_display: 0,
                label: "LABELDSP",
                status: "00000200",
                parameters: b"402,1,11,1,2,3,13,0,5;",
            },
            Entry {
                entity_type: 410,
                form: 0,
                transform: 0,
                label_display: 0,
                label: "VIEW",
                status: "00000100",
                parameters: b"410,1,1,0,0,0,0,0,0;",
            },
            Entry {
                entity_type: 214,
                form: 1,
                transform: 0,
                label_display: 0,
                label: "LEADER",
                status: "00010100",
                parameters: b"214,1,2,1,0,0,0,2,0;",
            },
        ]);
    }
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (index, entry) in entries.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        let parameter_start = u32::try_from(index + 1).unwrap().to_string();
        let entity_type = entry.entity_type.to_string();
        let form = entry.form.to_string();
        let transform = entry.transform.to_string();
        let line_font = if line_font != 0 {
            line_font.to_string()
        } else {
            "0".into()
        };
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
                "0",
                &line_font,
                "0",
                "0",
                &transform,
                &entry.label_display.to_string(),
                entry.status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [&entity_type, "0", "0", "1", &form, "", "", entry.label, "0"],
            sequence + 1,
        ));
    }
    for (index, entry) in entries.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        bytes.extend(parameter_card(
            entry.parameters,
            sequence,
            u32::try_from(index + 1).unwrap(),
        ));
    }
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000008P0000004").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn malformed_occurrence_placement_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "MEMBER".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "DEF".into(),
            status: "00000200",
            parameters: "308,0,10HDEFINITION,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "INSTANCE".into(),
            status: "00000000",
            parameters: "408,3,0,3Hbad,0,1;".into(),
        },
    ])
}

pub(crate) fn occurrence_limit_file() -> Vec<u8> {
    let mut entities = vec![OwnedTestEntity {
        entity_type: 308,
        form: 0,
        label: "EMPTYDEF".into(),
        status: "00000200",
        parameters: "308,0,8HEMPTYDEF,0;".into(),
    }];
    entities.extend((0..101).map(|_| OwnedTestEntity {
        entity_type: 408,
        form: 0,
        label: "INSTANCE".into(),
        status: "00000000",
        parameters: "408,1,0,0,0,1;".into(),
    }));
    owned_test_file(&entities)
}

pub(crate) fn occurrence_depth_limit_file() -> Vec<u8> {
    const INSTANCE_COUNT: usize = 65;
    let mut entities = Vec::with_capacity(INSTANCE_COUNT * 2);
    for index in 0..INSTANCE_COUNT {
        let definition_sequence = 1 + u32::try_from(index).unwrap() * 4;
        let member = if index + 1 < INSTANCE_COUNT {
            format!(",{}", definition_sequence + 6)
        } else {
            String::new()
        };
        let member_count = usize::from(index + 1 < INSTANCE_COUNT);
        entities.push(OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: format!("DEF{index}"),
            status: "00000200",
            parameters: format!(
                "308,{},1HD,{member_count}{member};",
                INSTANCE_COUNT - index - 1
            ),
        });
        entities.push(OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: format!("INS{index}"),
            status: "00000000",
            parameters: format!("408,{definition_sequence},0,0,0,1;"),
        });
    }
    owned_test_file(&entities)
}

pub(crate) fn malformed_occurrence_definition_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "CHILD".into(),
            status: "00000200",
            parameters: "308,0,5HCHILD,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "CHILDINS".into(),
            status: "00000000",
            parameters: "408,1,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "BROKEN".into(),
            status: "00000200",
            parameters: "308,1,6HBROKEN,1,3.;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "DANGLING".into(),
            status: "00000200",
            parameters: "308,1,8HDANGLING,1,99;".into(),
        },
    ])
}

pub(crate) fn malformed_network_occurrence_definition_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "BROKEN".into(),
            status: "00000200",
            parameters: "320,0,9HBROKENNET,1,3.,0,2HR1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETINST".into(),
            status: "00000000",
            parameters: "420,1,0,0,0,1,,,,2HU1,0,0;".into(),
        },
    ])
}

pub(crate) fn invalid_subfigure_depth_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "CHILD".into(),
            status: "00000200",
            parameters: "308,0,5HCHILD,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "INSTANCE".into(),
            status: "00000000",
            parameters: "408,1,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "PARENT".into(),
            status: "00000200",
            parameters: "308,0,6HPARENT,1,3;".into(),
        },
    ])
}

pub(crate) fn invalid_top_level_occurrence_structure_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "BADDEF".into(),
            status: "00000100",
            parameters: "308,0,6HBADDEF,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "BADINS".into(),
            status: "00000000",
            parameters: "408,1,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETDEF".into(),
            status: "00000200",
            parameters: "320,0,6HNETDEF,0,0,3HREF,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "WRONGTGT".into(),
            status: "00000000",
            parameters: "408,5,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETBAD".into(),
            status: "00000000",
            parameters: "420,1,0,0,0,1,,,,2HNI,0,0;".into(),
        },
    ])
}

fn containing_subfigure_file(containing_status: &'static str) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "CHILD".into(),
            status: "00000200",
            parameters: "308,0,5HCHILD,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "CHILDINS".into(),
            status: "00000000",
            parameters: "408,1,10,20,30,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "CONTNR".into(),
            status: containing_status,
            parameters: "308,1,9HCONTAINER,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "CONTINS".into(),
            status: "00000000",
            parameters: "408,5,0,0,0,1;".into(),
        },
    ])
}

pub(crate) fn rejected_containing_subfigure_file() -> Vec<u8> {
    containing_subfigure_file("00000100")
}

pub(crate) fn admitted_containing_subfigure_file() -> Vec<u8> {
    containing_subfigure_file("00000200")
}

fn containing_network_file(containing_status: &'static str) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "CHILD".into(),
            status: "00000200",
            parameters: "320,0,5HCHILD,0,0,3HREF,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "CHILDINS".into(),
            status: "00000000",
            parameters: "420,1,10,20,30,2,,,,5HCHILD,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "CONTNR".into(),
            status: containing_status,
            parameters: "320,1,9HCONTAINER,1,3,0,3HREF,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "CONTINS".into(),
            status: "00000000",
            parameters: "420,5,0,0,0,1,,,,6HCONTNR,0,0;".into(),
        },
    ])
}

pub(crate) fn rejected_containing_network_file() -> Vec<u8> {
    containing_network_file("00000100")
}

pub(crate) fn admitted_containing_network_file() -> Vec<u8> {
    containing_network_file("00000200")
}

pub(crate) fn network_subfigure_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETWORK".into(),
            status: "00000200",
            parameters: "320,0,3HNET,0,1,2HR1,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETINST".into(),
            status: "00000000",
            parameters: "420,1,1,2,3,2,,,,2HU1,0,2,0,0;".into(),
        },
    ])
}

pub(crate) fn wrong_typed_network_instance_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETWORK".into(),
            status: "00000200",
            parameters: "320,0,3HNET,0,1,2HR1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETINST".into(),
            status: "00000000",
            parameters: "420,1,1,2,3,2,,,1HX,2HU1,0,0;".into(),
        },
    ])
}

pub(crate) fn wrong_typed_network_definition_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETWORK".into(),
            status: "00000200",
            parameters: "320,0,3HNET,0,1HX,2HR1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETINST".into(),
            status: "00000000",
            parameters: "420,1,1,2,3,2,,,,2HU1,0,0;".into(),
        },
    ])
}

pub(crate) fn connected_network_subfigure_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "DEFPIN".into(),
            status: "00000400",
            parameters: "132,0,0,0,0,101,1,2HP1,0,3HPIN,0,1,1,0,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETWORK".into(),
            status: "00000200",
            parameters: "320,0,3HNET,0,1,2HR1,0,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "INSTPIN".into(),
            status: "00000400",
            parameters: "132,1,2,3,0,101,1,2HP1,0,3HPIN,0,2,1,0,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETINST".into(),
            status: "00000000",
            parameters: "420,3,10,20,30,1,,,1,2HU1,0,1,5;".into(),
        },
    ])
}

pub(crate) fn explicit_multi_pcurve_loop_file() -> Vec<u8> {
    explicit_multi_pcurve_loop_file_with_first_pcurve(
        "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,0.5,0,0,0,1,0,0,1;",
    )
}

pub(crate) fn explicit_multi_pcurve_loop_file_with_first_pcurve(first_pcurve: &str) -> Vec<u8> {
    explicit_multi_pcurve_loop_file_with_carriers(first_pcurve, "110,0,0,0,1,0,0;")
}

pub(crate) fn explicit_multi_pcurve_loop_file_with_first_edge(first_edge: &str) -> Vec<u8> {
    explicit_multi_pcurve_loop_file_with_carriers(
        "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,0.5,0,0,0,1,0,0,1;",
        first_edge,
    )
}

pub(crate) fn explicit_multi_pcurve_loop_file_with_carriers(
    first_pcurve: &str,
    first_edge: &str,
) -> Vec<u8> {
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
        first_edge,
        "110,1,0,0,1,1,0;",
        "110,1,1,0,0,1,0;",
        "110,0,1,0,0,0,0;",
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
            parameters: "502,4,0,0,0,1,0,0,1,1,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 504,
            form: 1,
            label: "EDGES".into(),
            status: "00010001",
            parameters: "504,4,7,15,1,15,2,9,15,2,15,3,11,15,3,15,4,13,15,4,15,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE1".into(),
            status: "00010500",
            parameters: first_pcurve.into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE2".into(),
            status: "00010500",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,0.5,0,0,1,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP".into(),
            status: "00010000",
            parameters: "508,5,0,17,1,1,2,1,19,0,21,1,15,2,0,0,0,17,2,1,0,0,17,3,1,0,0,17,4,1,0;"
                .into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE".into(),
            status: "00010000",
            parameters: "510,5,1,1,23;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,1,25,1;".into(),
        },
    ]);
    owned_test_file(&entities)
}

pub(crate) fn explicit_cylinder_seam_file() -> Vec<u8> {
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
            parameters: "192,1,3,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "SEAMEDGE".into(),
            status: "00010000",
            parameters: "110,1,0,0,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "VERTICES".into(),
            status: "00010000",
            parameters: "502,2,1,0,0,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 504,
            form: 1,
            label: "EDGES".into(),
            status: "00010001",
            parameters: "504,1,7,9,1,9,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "SEAMUV0".into(),
            status: "00010500",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,0,1,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "SEAMUV1".into(),
            status: "00010500",
            parameters: format!(
                "126,1,1,1,0,1,0,0,0,1,1,1,1,{},{},0,{},0,0,0,1,0,0,1;",
                std::f64::consts::TAU,
                1,
                std::f64::consts::TAU
            ),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "SEAMLOOP".into(),
            status: "00010000",
            parameters: "508,2,0,11,1,1,1,1,13,0,11,1,0,1,1,15;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "SEAMFACE".into(),
            status: "00010000",
            parameters: "510,5,1,1,17;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SEAMSHEL".into(),
            status: "00000000",
            parameters: "514,1,19,1;".into(),
        },
    ])
}

pub(crate) fn multi_pcurve_boundary_file() -> Vec<u8> {
    multi_pcurve_boundary_file_with_first_pcurve(
        "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,1,1,0,0,1,0,0,1;",
    )
}

pub(crate) fn multi_pcurve_boundary_file_with_first_pcurve(first_pcurve: &str) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "MODEL".into(),
            status: "00010000",
            parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE1".into(),
            status: "00010500",
            parameters: first_pcurve.into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE2".into(),
            status: "00010500",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,1,1,0,0,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: "141,1,3,1,1,3,1,2,5,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 143,
            form: 0,
            label: "BOUNDED".into(),
            status: "00000000",
            parameters: "143,1,1,1,9;".into(),
        },
    ])
}

pub(crate) fn trimmed_plane_with_inner_loop_file() -> Vec<u8> {
    let outer = "106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    trimmed_plane_with_inner_loop_and_outer_pcurve(outer)
}

pub(crate) fn trimmed_plane_with_inner_loop_and_outer_pcurve(outer_pcurve: &str) -> Vec<u8> {
    trimmed_plane_with_boundaries(outer_pcurve, "144,1,1,1,7,13;")
}

pub(crate) fn trimmed_plane_with_boundaries(
    outer_pcurve: &str,
    trimmed_parameters: &str,
) -> Vec<u8> {
    let outer = "106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    let inner = "106,1,5,0,0.25,0.25,0.75,0.25,0.75,0.75,0.25,0.75,0.25,0.25;";
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "OUTMODEL".into(),
            status: "00010000",
            parameters: outer.into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "OUTPCURV".into(),
            status: "00010500",
            parameters: outer_pcurve.into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "OUTBOUND".into(),
            status: "00010000",
            parameters: "142,0,1,5,3,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "INMODEL".into(),
            status: "00010000",
            parameters: inner.into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "INPCURVE".into(),
            status: "00010500",
            parameters: inner.into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "INBOUND".into(),
            status: "00010000",
            parameters: "142,0,1,11,9,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "ANNULUS".into(),
            status: "00000000",
            parameters: trimmed_parameters.into(),
        },
    ])
}

pub(crate) fn parameter_domain_trimmed_surface_file(trimmed_surface_parameters: &str) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "SURFACE".into(),
            status: "00010000",
            parameters:
                "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "DOMAIN".into(),
            status: "00000000",
            parameters: trimmed_surface_parameters.into(),
        },
    ])
}

pub(crate) fn subrange_nurbs_surface_boundary_file(preference: i64) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "SURFACE".into(),
            status: "00010000",
            parameters:
                "128,1,1,1,1,0,0,1,0,0,0,0,1,1,-1,-1,1,1,1,1,1,1,0,-1,0,1,-1,0,0,1,0,1,1,0,0.2,0.8,-1,1;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "MODEL".into(),
            status: "00010000",
            parameters: "106,1,5,0,0.2,0.2,0.8,0.2,0.8,0.8,0.2,0.8,0.2,0.2;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 2,
            label: "PCURVE".into(),
            status: "00010500",
            parameters:
                "126,2,2,1,1,1,0,0,0,0,1,1,1,1,1,1,0.2,0.2,0,0.1,0.5,0,0.2,0.2,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "ONSURF".into(),
            status: "00010000",
            parameters: format!("142,0,1,5,3,{preference};"),
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

pub(crate) fn independent_boundary_entities_file(include_failing_owner: bool) -> Vec<u8> {
    let mut entities = vec![
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "MODEL".into(),
            status: "00010000",
            parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "PCURVE".into(),
            status: "00010500",
            parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "CURVSRF".into(),
            status: "00000000",
            parameters: "142,0,1,5,3,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00000000",
            parameters: "141,1,0,1,1,3,1,1,5;".into(),
        },
    ];
    if include_failing_owner {
        entities.push(OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE2".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        });
        entities.push(OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "TRIMMED".into(),
            status: "00000000",
            parameters: "144,11,1,0,7;".into(),
        });
    }
    owned_test_file(&entities)
}

pub(crate) fn asymmetric_parameter_domain_surface_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00010000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,-2,-2,2,2,1,1,1,1,0,0,0,0,1,0,0,1,0,1,0,1,0,1,-2,2;"
                .into(),
    }])
}

pub(crate) fn alternate_asymmetric_parameter_domain_surface_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00010000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,-2,-2,2,2,1,1,1,1,0,0,0,0,1,0,0,1,0,1,0,1,0,-2,1,2;"
                .into(),
    }])
}

pub(crate) fn subrange_nurbs_surface_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00010000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,-2,-2,2,2,1,1,1,1,0,0,0,0,1,0,0,1,0,1,0,1,0.2,0.8,-1,1;"
                .into(),
    }])
}

pub(crate) fn nested_transformed_point_file() -> Vec<u8> {
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
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn transform_chain_overflow_file(transform_count: u32) -> Vec<u8> {
    assert!(transform_count > 0);
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    let identity = b"124,1,0,0,0,0,1,0,0,0,0,1,0;";
    for index in 0..transform_count {
        let sequence = 1 + index * 2;
        let transform = if index + 1 < transform_count {
            sequence + 2
        } else {
            0
        };
        bytes.extend(directory_card(
            [
                "124",
                &(index + 1).to_string(),
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
            ["124", "0", "0", "1", "0", "", "", "TRANS", "0"],
            sequence + 1,
        ));
    }
    let point_sequence = 1 + transform_count * 2;
    bytes.extend(directory_card(
        [
            "116",
            &(transform_count + 1).to_string(),
            "0",
            "0",
            "0",
            "0",
            "1",
            "0",
            "00000000",
        ],
        point_sequence,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        point_sequence + 1,
    ));
    for index in 0..transform_count {
        let sequence = 1 + index * 2;
        bytes.extend(parameter_card(identity, sequence, index + 1));
    }
    bytes.extend(parameter_card(
        b"116,1,2,3;",
        point_sequence,
        transform_count + 1,
    ));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D{:07}P{:07}",
            (transform_count + 1) * 2,
            transform_count + 1
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}
