// SPDX-License-Identifier: Apache-2.0
//! Generates bounded IGES 5.3 seeds from fixed-field and entity semantics.

use std::fs;
use std::io;

use cadmpeg_fuzz::seed_paths::seed_dir;

const GLOBAL: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";

/// Columns 1 through 72 of a card carry its data.
const CARD_DATA_COLUMNS: usize = 72;

/// Columns 1 through 64 of a parameter card carry its data; columns 65 through
/// 72 carry the directory entry pointer.
const PARAMETER_DATA_COLUMNS: usize = 64;

fn card(data: &[u8], section: u8, sequence: u32) -> io::Result<Vec<u8>> {
    if data.len() > CARD_DATA_COLUMNS {
        return Err(io::Error::other(format!(
            "a card holds {CARD_DATA_COLUMNS} data columns; this one states {}",
            data.len()
        )));
    }
    if sequence > 9_999_999 {
        return Err(io::Error::other("card sequence exceeds seven columns"));
    }
    let mut result = vec![b' '; 80];
    result[..data.len()].copy_from_slice(data);
    result[CARD_DATA_COLUMNS] = section;
    result[CARD_DATA_COLUMNS + 1..].copy_from_slice(format!("{sequence:>7}").as_bytes());
    result.push(b'\n');
    Ok(result)
}

fn directory_card(fields: [&str; 9], sequence: u32) -> io::Result<Vec<u8>> {
    let data = fields.into_iter().fold(String::new(), |mut data, field| {
        data.push_str(&format!("{field:>8}"));
        data
    });
    card(data.as_bytes(), b'D', sequence)
}

fn parameter_card(data: &[u8], directory_sequence: u32, sequence: u32) -> io::Result<Vec<u8>> {
    if data.len() > PARAMETER_DATA_COLUMNS {
        return Err(io::Error::other(format!(
            "a parameter card holds {PARAMETER_DATA_COLUMNS} data columns; this one states {}",
            data.len()
        )));
    }
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    payload[PARAMETER_DATA_COLUMNS..]
        .copy_from_slice(format!("{directory_sequence:>8}").as_bytes());
    card(&payload, b'P', sequence)
}

fn prefix() -> io::Result<Vec<u8>> {
    let mut bytes = card(b"cadmpeg generated fuzz seed", b'S', 1)?;
    // A card ordinal is one-based and bounded by the fixed global section, so
    // the ordinal carries its own width.
    for (index, chunk) in GLOBAL.chunks(CARD_DATA_COLUMNS).enumerate() {
        bytes.extend(card(chunk, b'G', index as u32 + 1)?);
    }
    Ok(bytes)
}

fn terminate(directory_cards: u32, parameter_cards: u32) -> io::Result<Vec<u8>> {
    card(
        format!(
            "S0000001G{:07}D{directory_cards:07}P{parameter_cards:07}",
            GLOBAL.len().div_ceil(CARD_DATA_COLUMNS)
        )
        .as_bytes(),
        b'T',
        1,
    )
}

fn point() -> io::Result<Vec<u8>> {
    let mut bytes = prefix()?;
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    )?);
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    )?);
    bytes.extend(parameter_card(b"116,1.0,2.0,3.0;", 1, 1)?);
    bytes.extend(terminate(2, 1)?);
    Ok(bytes)
}

fn trimmed_plane() -> io::Result<Vec<u8>> {
    let mut bytes = prefix()?;
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
        )?);
        bytes.extend(directory_card(
            [&entity_type, "0", "0", "1", &form, "", "", label, "0"],
            sequence + 1,
        )?);
    }
    bytes.extend(parameter_card(b"108,0,0,1,0,0,0,0,0,0;", 1, 1)?);
    let square = b"106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    bytes.extend(parameter_card(square, 3, 2)?);
    bytes.extend(parameter_card(square, 5, 3)?);
    bytes.extend(parameter_card(b"142,0,1,5,3,3;", 7, 4)?);
    bytes.extend(parameter_card(b"144,1,1,0,7;", 9, 5)?);
    bytes.extend(terminate(10, 5)?);
    Ok(bytes)
}

fn main() -> io::Result<()> {
    let directory = seed_dir("seeds/iges_container");
    fs::create_dir_all(&directory)?;
    for (name, bytes) in [
        ("point_5_3", point()?),
        ("trimmed_plane_5_3", trimmed_plane()?),
    ] {
        fs::write(directory.join(name), &bytes)?;
        println!("iges/{name} ({} bytes)", bytes.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::card;

    #[test]
    fn card_refuses_sequence_that_exceeds_seven_columns() {
        assert!(card(b"", b'D', 10_000_000).is_err());
        let card = card(b"", b'D', 9_999_999).expect("largest seven-column sequence");
        assert_eq!(&card[73..80], b"9999999");
    }
}
