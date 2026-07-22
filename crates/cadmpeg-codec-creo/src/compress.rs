// SPDX-License-Identifier: Apache-2.0
//! Bounded decoder for the historical Unix `compress` (`.Z`) LZW stream.

const BLOCK_MODE: u8 = 0x80;
const CLEAR: u16 = 256;

pub(crate) fn decode(data: &[u8], expected_length: usize) -> Option<Vec<u8>> {
    let [0x1f, 0x9d, flags, rest @ ..] = data else {
        return None;
    };
    let max_bits = usize::from(flags & 0x1f);
    if !(9..=16).contains(&max_bits) || flags & !(BLOCK_MODE | 0x1f) != 0 {
        return None;
    }
    let block_mode = flags & BLOCK_MODE != 0;
    let dictionary_limit = 1usize << max_bits;
    let mut prefix = vec![0u16; dictionary_limit];
    let mut suffix = vec![0u8; dictionary_limit];
    for (value, slot) in suffix.iter_mut().take(256).enumerate() {
        *slot = u8::try_from(value).ok()?;
    }

    let mut reader = CodeReader::new(rest, max_bits);
    let mut free_entry = if block_mode { 257usize } else { 256usize };
    let first = usize::from(reader.next(free_entry, false)?);
    if first >= 256 {
        return None;
    }
    let mut old_code = first;
    let mut final_byte = u8::try_from(first).ok()?;
    let mut output = Vec::with_capacity(expected_length);
    output.push(final_byte);
    let mut stack = Vec::new();

    while let Some(raw_code) = reader.next(free_entry, false) {
        if block_mode && raw_code == CLEAR {
            free_entry = 257;
            let Some(code) = reader.next(free_entry, true) else {
                break;
            };
            let code = usize::from(code);
            if code >= 256 {
                return None;
            }
            old_code = code;
            final_byte = u8::try_from(code).ok()?;
            output.push(final_byte);
            if output.len() > expected_length {
                return None;
            }
            continue;
        }

        let input_code = usize::from(raw_code);
        let mut code = input_code;
        if code >= free_entry {
            if code != free_entry {
                return None;
            }
            stack.push(final_byte);
            code = old_code;
        }
        while code >= 256 {
            if code >= free_entry || code >= dictionary_limit {
                return None;
            }
            stack.push(suffix[code]);
            code = usize::from(prefix[code]);
        }
        final_byte = u8::try_from(code).ok()?;
        output.push(final_byte);
        output.extend(stack.drain(..).rev());
        if output.len() > expected_length {
            return None;
        }

        if free_entry < dictionary_limit {
            prefix[free_entry] = u16::try_from(old_code).ok()?;
            suffix[free_entry] = final_byte;
            free_entry += 1;
        }
        old_code = input_code;
    }

    (output.len() == expected_length).then_some(output)
}

struct CodeReader<'a> {
    data: &'a [u8],
    cursor: usize,
    block: &'a [u8],
    bit_offset: usize,
    start_limit: usize,
    width: usize,
    max_bits: usize,
}

impl<'a> CodeReader<'a> {
    fn new(data: &'a [u8], max_bits: usize) -> Self {
        Self {
            data,
            cursor: 0,
            block: &[],
            bit_offset: 0,
            start_limit: 0,
            width: 9,
            max_bits,
        }
    }

    fn next(&mut self, free_entry: usize, clear: bool) -> Option<u16> {
        let max_code = (1usize << self.width) - 1;
        if clear {
            self.width = 9;
            self.block = &[];
            self.bit_offset = 0;
            self.start_limit = 0;
        } else if free_entry > max_code && self.width < self.max_bits {
            self.width += 1;
            self.block = &[];
            self.bit_offset = 0;
            self.start_limit = 0;
        }
        if self.bit_offset >= self.start_limit {
            if self.cursor >= self.data.len() {
                return None;
            }
            let end = self.cursor.saturating_add(self.width).min(self.data.len());
            self.block = &self.data[self.cursor..end];
            self.cursor = end;
            self.bit_offset = 0;
            self.start_limit = self
                .block
                .len()
                .checked_mul(8)?
                .checked_sub(self.width - 1)?;
            if self.start_limit == 0 {
                return None;
            }
        }
        let mut code = 0u16;
        for bit in 0..self.width {
            let source = self.bit_offset + bit;
            let value = (self.block[source / 8] >> (source % 8)) & 1;
            code |= u16::from(value) << bit;
        }
        self.bit_offset += self.width;
        Some(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_literal_non_block_stream() {
        // Nine-bit LSB-first codes 65, 66, 67.
        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        assert_eq!(decode(&stream, 3), Some(b"ABC".to_vec()));
        assert_eq!(decode(&stream, 4), None);
    }

    #[test]
    fn rejects_invalid_header_flags() {
        assert_eq!(decode(&[0x1f, 0x9d, 0x08], 0), None);
        assert_eq!(decode(&[0x1f, 0x9d, 0x30], 0), None);
    }

    #[test]
    fn rejects_truncated_code_block() {
        assert_eq!(decode(&[0x1f, 0x9d, 0x09, 0x00], 1), None);
    }

    #[test]
    fn decodes_block_mode_dictionary_references() {
        let stream = [
            0x1f, 0x9d, 0x90, 0x54, 0x9e, 0x08, 0x29, 0xf2, 0x44, 0x8a, 0x93, 0x27, 0x54, 0x02,
            0x0e, 0x2c, 0xa8, 0x90, 0xa0, 0x41, 0x84, 0x0a, 0x00,
        ];
        assert_eq!(
            decode(&stream, 25),
            Some(b"TOBEORNOTTOBEORTOBEORNOT\n".to_vec())
        );
    }

    #[test]
    fn block_mode_clear_reserves_the_clear_code() {
        fn codes(values: &[u16]) -> Vec<u8> {
            let mut bytes = vec![0; values.len().saturating_mul(9).div_ceil(8)];
            for (index, value) in values.iter().copied().enumerate() {
                for bit in 0..9 {
                    bytes[(index * 9 + bit) / 8] |= u8::try_from((value >> bit) & 1)
                        .expect("required invariant")
                        << ((index * 9 + bit) % 8);
                }
            }
            bytes
        }

        let mut stream = vec![0x1f, 0x9d, 0x90];
        let mut first_block = codes(&[u16::from(b'A'), CLEAR]);
        first_block.resize(9, 0);
        stream.extend(first_block);
        stream.extend(codes(&[u16::from(b'B'), u16::from(b'C'), 257]));
        assert_eq!(decode(&stream, 5), Some(b"ABCBC".to_vec()));
    }
}
