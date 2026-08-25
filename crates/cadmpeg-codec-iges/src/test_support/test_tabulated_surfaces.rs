// SPDX-License-Identifier: Apache-2.0
//! Tabulated-surface byte fixtures for crate tests.
#![allow(clippy::unwrap_used)]

use super::test_cards::*;

pub(crate) fn placed_tabulated_hyperbola_file() -> Vec<u8> {
    placed_tabulated_hyperbola_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    )
}

pub(crate) fn placed_tabulated_hyperbola_file_with_global(global: &[u8]) -> Vec<u8> {
    let hyperbola =
        b"104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;";
    let transform = b"124,1,0,0,10,0,1,0,20,0,0,1,30;";
    let tabulated = b"122,1,3.086161269630487,3.525603580931404,2;";
    let hyperbola_count = u32::try_from(parameter_fragment_count(hyperbola)).unwrap();
    let transform_start = 1 + hyperbola_count;
    let transform_count = u32::try_from(parameter_fragment_count(transform)).unwrap();
    let tabulated_start = transform_start + transform_count;
    let tabulated_count = u32::try_from(parameter_fragment_count(tabulated)).unwrap();
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["104", "1", "0", "1", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "104",
            "0",
            "0",
            &hyperbola_count.to_string(),
            "2",
            "",
            "",
            "HYPERBOL",
            "0",
        ],
        2,
    ));
    bytes.extend(directory_card(
        [
            "124",
            &transform_start.to_string(),
            "0",
            "0",
            "0",
            "0",
            "0",
            "0",
            "00000000",
        ],
        3,
    ));
    bytes.extend(directory_card(
        [
            "124",
            "0",
            "0",
            &transform_count.to_string(),
            "0",
            "",
            "",
            "FRAME",
            "0",
        ],
        4,
    ));
    bytes.extend(directory_card(
        [
            "122",
            &tabulated_start.to_string(),
            "0",
            "1",
            "0",
            "0",
            "3",
            "0",
            "00000000",
        ],
        5,
    ));
    bytes.extend(directory_card(
        [
            "122",
            "0",
            "0",
            &tabulated_count.to_string(),
            "0",
            "",
            "",
            "TABULATE",
            "0",
        ],
        6,
    ));
    bytes.extend(parameter_cards(hyperbola, 1, 1));
    bytes.extend(parameter_cards(transform, 3, transform_start));
    bytes.extend(parameter_cards(tabulated, 5, tabulated_start));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D0000006P{:07}",
            tabulated_start + tabulated_count - 1
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn placed_tabulated_line_file() -> Vec<u8> {
    placed_tabulated_line_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    )
}

pub(crate) fn placed_tabulated_line_file_with_global(global: &[u8]) -> Vec<u8> {
    let line = b"110,0,0,0,1,0,0;";
    let transform = b"124,1,0,0,10,0,1,0,20,0,0,1,30;";
    let tabulated = b"122,1,0,0,2;";
    let line_count = u32::try_from(parameter_fragment_count(line)).unwrap();
    let transform_start = 1 + line_count;
    let transform_count = u32::try_from(parameter_fragment_count(transform)).unwrap();
    let tabulated_start = transform_start + transform_count;
    let tabulated_count = u32::try_from(parameter_fragment_count(tabulated)).unwrap();
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["110", "1", "0", "1", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "110",
            "0",
            "0",
            &line_count.to_string(),
            "0",
            "",
            "",
            "DIRECTRX",
            "0",
        ],
        2,
    ));
    bytes.extend(directory_card(
        [
            "124",
            &transform_start.to_string(),
            "0",
            "0",
            "0",
            "0",
            "0",
            "0",
            "00000000",
        ],
        3,
    ));
    bytes.extend(directory_card(
        [
            "124",
            "0",
            "0",
            &transform_count.to_string(),
            "0",
            "",
            "",
            "FRAME",
            "0",
        ],
        4,
    ));
    bytes.extend(directory_card(
        [
            "122",
            &tabulated_start.to_string(),
            "0",
            "1",
            "0",
            "0",
            "3",
            "0",
            "00000000",
        ],
        5,
    ));
    bytes.extend(directory_card(
        [
            "122",
            "0",
            "0",
            &tabulated_count.to_string(),
            "0",
            "",
            "",
            "TABULATE",
            "0",
        ],
        6,
    ));
    bytes.extend(parameter_cards(line, 1, 1));
    bytes.extend(parameter_cards(transform, 3, transform_start));
    bytes.extend(parameter_cards(tabulated, 5, tabulated_start));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D0000006P{:07}",
            tabulated_start + tabulated_count - 1
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}
