// SPDX-License-Identifier: Apache-2.0
//! Hexadecimal rendering with absolute file offsets and an ASCII gutter.

use std::fmt::Write as _;

/// Number of hexadecimal digits used for the offset column.
///
/// Eight digits address 4 GiB. Wider files widen the column so the layout stays
/// aligned instead of ragged.
fn offset_digits(last_offset: u64) -> usize {
    let needed = format!("{last_offset:x}").len();
    needed.max(8)
}

/// Renders `bytes` as a hexadecimal dump whose first byte sits at `base`.
///
/// Each line prints the absolute offset, `width` bytes in hexadecimal grouped in
/// eights, and the printable ASCII for the same bytes between pipes. A short
/// final line is padded so the gutter stays in one column.
pub fn render(base: u64, bytes: &[u8], width: usize) -> String {
    let width = width.max(1);
    let last = base.saturating_add(bytes.len().saturating_sub(1) as u64);
    let digits = offset_digits(last);
    let mut out = String::new();
    for (index, chunk) in bytes.chunks(width).enumerate() {
        let offset = base.saturating_add((index * width) as u64);
        let _ = write!(out, "{offset:0digits$x}  ");
        for column in 0..width {
            if column > 0 && column.is_multiple_of(8) {
                out.push(' ');
            }
            match chunk.get(column) {
                Some(byte) => {
                    let _ = write!(out, "{byte:02x} ");
                }
                None => out.push_str("   "),
            }
        }
        out.push_str(" |");
        for byte in chunk {
            out.push(printable(*byte));
        }
        out.push('|');
        out.push('\n');
    }
    out
}

/// Maps one byte to its ASCII gutter character.
const fn printable(byte: u8) -> char {
    if byte.is_ascii_graphic() || byte == b' ' {
        byte as char
    } else {
        '.'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_one_full_line_with_grouping_and_gutter() {
        let bytes: Vec<u8> = (0x40u8..0x50).collect();
        let text = render(0, &bytes, 16);
        assert_eq!(
            text,
            "00000000  40 41 42 43 44 45 46 47  48 49 4a 4b 4c 4d 4e 4f  |@ABCDEFGHIJKLMNO|\n"
        );
    }

    #[test]
    fn pads_a_short_final_line_so_the_gutter_stays_aligned() {
        let text = render(0x10, b"hi", 16);
        let line = text.trim_end_matches('\n');
        let full = render(0x10, &[0u8; 16], 16);
        let full_line = full.trim_end_matches('\n');
        assert_eq!(
            line.find('|').unwrap(),
            full_line.find('|').unwrap(),
            "gutter column must not move"
        );
        assert!(line.ends_with("|hi|"), "got {line}");
        assert!(line.starts_with("00000010  68 69 "), "got {line}");
    }

    #[test]
    fn non_printable_bytes_become_dots_and_space_stays_a_space() {
        let text = render(0, &[0x00, 0x20, 0x7e, 0x7f, 0xff], 8);
        assert!(text.ends_with("|. ~..|\n"), "got {text}");
    }

    #[test]
    fn offsets_are_absolute_and_widen_past_four_gibibytes() {
        let text = render(0x1_0000_0000, &[0u8; 1], 16);
        assert!(text.starts_with("100000000  00 "), "got {text}");
        let narrow = render(0xff, &[0u8; 1], 16);
        assert!(narrow.starts_with("000000ff  00 "), "got {narrow}");
    }

    #[test]
    fn respects_a_custom_width() {
        let text = render(0, &[1, 2, 3, 4, 5], 2);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("00000000  01 02 "));
        assert!(lines[1].starts_with("00000002  03 04 "));
        assert!(lines[2].starts_with("00000004  05 "));
    }
}
