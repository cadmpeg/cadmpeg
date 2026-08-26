// SPDX-License-Identifier: Apache-2.0
//! Card, directory, and parameter byte builders for crate tests.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;

// A card is columns 1-72 of data, the section letter in column 73, the
// right-aligned sequence in columns 74-80, and then the line ending.
pub(crate) const CARD_COLUMNS: usize = 80;
pub(crate) const CARD_DATA_COLUMNS: usize = 72;
pub(crate) const CARD_LINE_BYTES: usize = CARD_COLUMNS + 1;

pub(crate) fn card(data: &[u8], section: u8, sequence: u32) -> Vec<u8> {
    card_with_ending(data, section, sequence, b"\n")
}

pub(crate) fn card_with_ending(data: &[u8], section: u8, sequence: u32, ending: &[u8]) -> Vec<u8> {
    assert!(data.len() <= CARD_DATA_COLUMNS);
    let mut card = vec![b' '; CARD_COLUMNS];
    card[..data.len()].copy_from_slice(data);
    card[CARD_DATA_COLUMNS] = section;
    card[CARD_DATA_COLUMNS + 1..CARD_COLUMNS].copy_from_slice(format!("{sequence:>7}").as_bytes());
    card.extend_from_slice(ending);
    card
}

pub(crate) fn fixed_ascii_with_global(global: &[u8]) -> Vec<u8> {
    match crate::global::layout_global_cards(global) {
        Ok(cards) => {
            fixed_ascii_with_global_cards(&cards.iter().map(Vec::as_slice).collect::<Vec<_>>())
        }
        Err(_) => {
            fixed_ascii_with_global_cards(&global.chunks(CARD_DATA_COLUMNS).collect::<Vec<_>>())
        }
    }
}

pub(crate) fn fixed_ascii_with_global_cards(cards: &[&[u8]]) -> Vec<u8> {
    let mut bytes = card(b"original fixture", b'S', 1);
    for (index, chunk) in cards.iter().enumerate() {
        bytes.extend(card(chunk, b'G', u32::try_from(index + 1).unwrap()));
    }
    bytes.extend(card(
        format!("S0000001G{:07}D0000000P0000000", cards.len()).as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn global_card_count(global: &[u8]) -> usize {
    crate::global::layout_global_cards(global).map_or_else(
        |_| global.len().div_ceil(CARD_DATA_COLUMNS),
        |cards| cards.len(),
    )
}

pub(crate) fn directory_card(fields: [&str; 9], sequence: u32) -> Vec<u8> {
    let data = fields.into_iter().fold(String::new(), |mut data, field| {
        write!(data, "{field:>8}").unwrap();
        data
    });
    card(data.as_bytes(), b'D', sequence)
}

pub(crate) fn parameter_card(data: &[u8], directory_sequence: u32, sequence: u32) -> Vec<u8> {
    assert!(data.len() <= 64);
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    payload[64..72].copy_from_slice(format!("{directory_sequence:>8}").as_bytes());
    card(&payload, b'P', sequence)
}

pub(crate) fn parameter_cards(
    data: &[u8],
    directory_sequence: u32,
    first_sequence: u32,
) -> Vec<u8> {
    parameter_fragments(data)
        .into_iter()
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

pub(crate) fn parameter_fragment_count(data: &[u8]) -> usize {
    parameter_fragments(data).len()
}

pub(crate) fn raw_parameter_cards(
    data: &[u8],
    directory_sequence: u32,
    first_sequence: u32,
) -> Vec<u8> {
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

pub(crate) fn raw_parameter_fragment_count(data: &[u8]) -> usize {
    data.len().div_ceil(64)
}

fn parameter_fragments(data: &[u8]) -> Vec<&[u8]> {
    let mut fragments = Vec::new();
    let mut remainder = data;
    while remainder.len() > 64 {
        let window = &remainder[..64];
        let split = if window.contains(&b';') {
            64
        } else {
            window
                .iter()
                .rposition(|byte| *byte == b',')
                .map_or(64, |index| index + 1)
        };
        fragments.push(&remainder[..split]);
        remainder = &remainder[split..];
    }
    if !remainder.is_empty() {
        fragments.push(remainder);
    }
    fragments
}
