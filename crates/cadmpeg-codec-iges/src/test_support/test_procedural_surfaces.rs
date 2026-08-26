// SPDX-License-Identifier: Apache-2.0
//! Procedural-surface byte fixtures for crate tests.
#![allow(clippy::unwrap_used)]

use super::test_owned::*;

pub(crate) fn cacheless_line_tabulated_surface_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "DIRECTRX".into(),
            status: "00010000",
            parameters:
                "110,-108.9812949,6.814348186,-2.592356749,-108.9812949,11.76210922,-6.969522429;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 122,
            form: 0,
            label: "TABULATE".into(),
            status: "00000000",
            parameters: "122,1,-108.9812949,6.814348186,-0.592356749;".into(),
        },
    ])
}

pub(crate) fn trimmed_procedural_line_surface_of_revolution_file() -> Vec<u8> {
    trimmed_procedural_line_surface_of_revolution_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,64,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    )
}

pub(crate) fn trimmed_procedural_line_surface_of_revolution_file_with_global(
    global: &[u8],
) -> Vec<u8> {
    let profile_start = [-108.981_294_9_f64, 6.814_348_186, -2.592_356_749];
    let profile_end = [-108.981_294_9_f64, 11.762_109_22, -6.969_522_429];
    let profile_mid: [f64; 3] =
        std::array::from_fn(|index| (profile_start[index] + profile_end[index]) * 0.5);
    let pcurve = format!(
        "126,1,1,1,0,1,0,0,0,1,1,1,1,0.5,0,0,0.5,{},0,0,1,0,0,1;",
        std::f64::consts::TAU
    );
    owned_test_file_with_global(
        &[
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
                parameters: format!(
                    "110,{},{},{},{},{},{};",
                    profile_start[0],
                    profile_start[1],
                    profile_start[2],
                    profile_end[0],
                    profile_end[1],
                    profile_end[2]
                ),
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
                    "100,{},{},{},{},{},{},{};",
                    profile_mid[2],
                    0.0,
                    0.0,
                    profile_mid[0],
                    profile_mid[1],
                    profile_mid[0],
                    profile_mid[1]
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
        ],
        global,
    )
}

pub(crate) fn interval_certified_linear_bezier_ruled_surface_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "BEZIER1".into(),
            status: "00000000",
            parameters:
                "126,3,3,0,0,1,0,0,0,0,0,1,1,1,1,1,1,1,1,0,0,0,1.000002,0,0,2.000004,0,0,3,0,0,0,1;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "BEZIER2".into(),
            status: "00000000",
            parameters:
                "126,3,3,0,0,1,0,0,0,0,0,1,1,1,1,1,1,1,1,0,1,0,1.000002,1,0,2.000004,1,0,3,1,0,0,1;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 118,
            form: 0,
            label: "BZRULED".into(),
            status: "00000000",
            parameters: "118,1,3,0,0;".into(),
        },
    ])
}
