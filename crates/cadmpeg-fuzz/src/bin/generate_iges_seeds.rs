// SPDX-License-Identifier: Apache-2.0
//! Generates bounded IGES 5.3 seeds from fixed-field and entity semantics.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const GLOBAL: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";

fn card(data: &[u8], section: u8, sequence: u32) -> Vec<u8> {
    assert!(data.len() <= 72);
    let mut result = vec![b' '; 80];
    result[..data.len()].copy_from_slice(data);
    result[72] = section;
    result[73..].copy_from_slice(format!("{sequence:>7}").as_bytes());
    result.push(b'\n');
    result
}

fn directory_card(fields: [&str; 9], sequence: u32) -> Vec<u8> {
    let data = fields.into_iter().fold(String::new(), |mut data, field| {
        write!(data, "{field:>8}").expect("required invariant");
        data
    });
    card(data.as_bytes(), b'D', sequence)
}

fn parameter_card(data: &[u8], directory_sequence: u32, sequence: u32) -> Vec<u8> {
    assert!(data.len() <= 64);
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    payload[64..].copy_from_slice(format!("{directory_sequence:>8}").as_bytes());
    card(&payload, b'P', sequence)
}

fn prefix() -> Vec<u8> {
    let mut bytes = card(b"cadmpeg generated fuzz seed", b'S', 1);
    for (index, chunk) in GLOBAL.chunks(72).enumerate() {
        bytes.extend(card(chunk, b'G', u32::try_from(index + 1).expect("required invariant")));
    }
    bytes
}

fn terminate(directory_cards: u32, parameter_cards: u32) -> Vec<u8> {
    card(
        format!(
            "S0000001G{:07}D{directory_cards:07}P{parameter_cards:07}",
            GLOBAL.len().div_ceil(72)
        )
        .as_bytes(),
        b'T',
        1,
    )
}

fn point() -> Vec<u8> {
    let mut bytes = prefix();
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"116,1.0,2.0,3.0;", 1, 1));
    bytes.extend(terminate(2, 1));
    bytes
}

fn trimmed_plane() -> Vec<u8> {
    let mut bytes = prefix();
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
    bytes.extend(terminate(10, 5));
    bytes
}

fn main() {
    let directory = Path::new("seeds/iges_container");
    fs::create_dir_all(directory).expect("required invariant");
    for (name, bytes) in [
        ("point_5_3", point()),
        ("trimmed_plane_5_3", trimmed_plane()),
    ] {
        fs::write(directory.join(name), &bytes).expect("required invariant");
        println!("iges/{name} ({} bytes)", bytes.len());
    }
}
