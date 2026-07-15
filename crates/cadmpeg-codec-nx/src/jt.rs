// SPDX-License-Identifier: Apache-2.0
//! Siemens JT integer packet decoding used by embedded NX display models.

#[derive(Debug, Clone, Copy)]
struct ProbabilityEntry {
    symbol: i32,
    occurrence_count: u32,
    value: i32,
}

struct MsbBitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> MsbBitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn read(&mut self, count: u8) -> Option<u32> {
        if count > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..count {
            let byte = *self.bytes.get(self.bit / 8)?;
            value = (value << 1) | u32::from((byte >> (7 - self.bit % 8)) & 1);
            self.bit += 1;
        }
        Some(value)
    }

    fn finish_zero_padding(self) -> Option<usize> {
        let byte_len = self.bit.div_ceil(8);
        if !self.bit.is_multiple_of(8) {
            let used = self.bit % 8;
            let last = *self.bytes.get(byte_len - 1)?;
            if last & ((1 << (8 - used)) - 1) != 0 {
                return None;
            }
        }
        Some(byte_len)
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset.checked_add(4)?)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Predictor {
    Lag1,
    Lag2,
    Stride1,
    Stride2,
    StripIndex,
    Ramp,
    Xor1,
    Xor2,
    Null,
}

/// Reconstruct JT primal integers from predictor residuals.
pub(crate) fn unpack_predictor_residuals(residuals: &[i32], predictor: Predictor) -> Vec<i32> {
    if predictor == Predictor::Null {
        return residuals.to_vec();
    }

    let mut values = Vec::with_capacity(residuals.len());
    for (index, &residual) in residuals.iter().enumerate() {
        if index < 4 {
            values.push(residual);
            continue;
        }
        let v1 = values[index - 1];
        let v2 = values[index - 2];
        let v4 = values[index - 4];
        let predicted = match predictor {
            Predictor::Lag1 | Predictor::Xor1 => v1,
            Predictor::Lag2 | Predictor::Xor2 => v2,
            Predictor::Stride1 => v1.wrapping_add(v1.wrapping_sub(v2)),
            Predictor::Stride2 => v2.wrapping_add(v2.wrapping_sub(v4)),
            Predictor::StripIndex => {
                let stride = v2.wrapping_sub(v4);
                if (-7..=7).contains(&stride) {
                    v2.wrapping_add(stride)
                } else {
                    v2.wrapping_add(2)
                }
            }
            Predictor::Ramp => index as i32,
            Predictor::Null => unreachable!(),
        };
        values.push(if matches!(predictor, Predictor::Xor1 | Predictor::Xor2) {
            residual ^ predicted
        } else {
            residual.wrapping_add(predicted)
        });
    }
    values
}

/// Bound one complete JT Int32 Compressed Data Packet Mk. 2 without interpreting its symbols.
pub(crate) fn frame_int32_cdp2(bytes: &[u8], depth: u8) -> Option<(u32, u8, usize)> {
    if depth > 3 {
        return None;
    }
    let value_count = read_u32(bytes, 0)?;
    if value_count == 0 {
        return Some((0, 0, 4));
    }
    let &codec = bytes.get(4)?;
    if codec == 4 {
        let &chop_bits = bytes.get(5)?;
        if chop_bits == 0 {
            let (nested_count, _, nested_len) = frame_int32_cdp2(bytes.get(6..)?, depth + 1)?;
            return (nested_count == value_count).then_some((value_count, codec, 6 + nested_len));
        }
        let &span_bits = bytes.get(10)?;
        if chop_bits > span_bits || span_bits > 32 {
            return None;
        }
        let (msb_count, _, msb_len) = frame_int32_cdp2(bytes.get(11..)?, depth + 1)?;
        let (lsb_count, _, lsb_len) = frame_int32_cdp2(bytes.get(11 + msb_len..)?, depth + 1)?;
        return (msb_count == value_count && lsb_count == value_count).then_some((
            value_count,
            codec,
            11 + msb_len + lsb_len,
        ));
    }
    if !matches!(codec, 1 | 3) {
        return None;
    }
    let code_bit_len = usize::try_from(read_u32(bytes, 5)?).ok()?;
    let code_byte_len = code_bit_len.div_ceil(32).checked_mul(4)?;
    let mut cursor = 9_usize.checked_add(code_byte_len)?;
    bytes.get(..cursor)?;
    if codec == 1 {
        return Some((value_count, codec, cursor));
    }
    let (entries, context_len) = parse_probability_context(bytes.get(cursor..)?)?;
    cursor = cursor.checked_add(context_len)?;
    let code_words = bytes.get(9..9 + code_byte_len)?;
    let symbols = decode_arithmetic(
        code_words,
        code_bit_len,
        usize::try_from(value_count).ok()?,
        &entries,
    )?;
    let escape_count = symbols.iter().filter(|value| value.is_none()).count();
    if escape_count != 0 {
        let (out_of_band_count, _, out_of_band_len) =
            frame_int32_cdp2(bytes.get(cursor..)?, depth + 1)?;
        if usize::try_from(out_of_band_count).ok()? != escape_count {
            return None;
        }
        cursor = cursor.checked_add(out_of_band_len)?;
    }
    Some((value_count, codec, cursor))
}

fn parse_probability_context(bytes: &[u8]) -> Option<(Vec<ProbabilityEntry>, usize)> {
    let entry_count = usize::from(u16::from_be_bytes(bytes.get(..2)?.try_into().ok()?));
    let mut bits = MsbBitReader::new(bytes.get(2..)?);
    let symbol_bits = u8::try_from(bits.read(6)?).ok()?;
    let occurrence_bits = u8::try_from(bits.read(6)?).ok()?;
    let value_bits = u8::try_from(bits.read(6)?).ok()?;
    let minimum = bits.read(32)? as i32;
    if symbol_bits > 32 || occurrence_bits > 32 || value_bits > 32 {
        return None;
    }
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let symbol = bits.read(symbol_bits)? as i32 - 2;
        let occurrence_count = bits.read(occurrence_bits)?;
        let value = (bits.read(value_bits)? as i32).wrapping_add(minimum);
        if occurrence_count == 0 {
            return None;
        }
        entries.push(ProbabilityEntry {
            symbol,
            occurrence_count,
            value,
        });
    }
    let bit_bytes = bits.finish_zero_padding()?;
    Some((entries, 2 + bit_bytes))
}

struct CodeBits<'a> {
    words: &'a [u8],
    bit_len: usize,
    bit: usize,
}

impl CodeBits<'_> {
    fn next(&mut self) -> u16 {
        if self.bit >= self.bit_len {
            return 0;
        }
        let word_index = self.bit / 32;
        let bit_index = self.bit % 32;
        let offset = word_index * 4;
        let word = self
            .words
            .get(offset..offset + 4)
            .and_then(|value| value.try_into().ok())
            .map_or(0, u32::from_le_bytes);
        self.bit += 1;
        ((word >> (31 - bit_index)) & 1) as u16
    }
}

/// Upper bound on values a single arithmetic-coded lane may declare.
const MAX_ARITHMETIC_VALUES: usize = 1_000_000;

fn decode_arithmetic(
    code_words: &[u8],
    code_bit_len: usize,
    value_count: usize,
    entries: &[ProbabilityEntry],
) -> Option<Vec<Option<i32>>> {
    // Arithmetic symbols can consume zero code bits, so the stream length puts
    // no floor under `value_count`; an absolute cap bounds the allocation and
    // the per-value decode work instead.
    if value_count > MAX_ARITHMETIC_VALUES {
        return None;
    }
    let total: u32 = entries
        .iter()
        .try_fold(0u32, |sum, entry| sum.checked_add(entry.occurrence_count))?;
    if total == 0 || total > u32::from(u16::MAX) {
        return None;
    }
    let mut bits = CodeBits {
        words: code_words,
        bit_len: code_bit_len,
        bit: 0,
    };
    let mut code = 0u16;
    for _ in 0..16 {
        code = (code << 1) | bits.next();
    }
    let mut low = 0u16;
    let mut high = u16::MAX;
    let mut values = Vec::with_capacity(value_count);
    for _ in 0..value_count {
        let range = u32::from(high.wrapping_sub(low)) + 1;
        let scaled = ((u32::from(code.wrapping_sub(low)) + 1) * total - 1) / range;
        let mut cumulative = 0u32;
        let entry = entries.iter().find(|entry| {
            let end = cumulative + entry.occurrence_count;
            let contains = scaled >= cumulative && scaled < end;
            if !contains {
                cumulative = end;
            }
            contains
        })?;
        let entry_high = cumulative + entry.occurrence_count;
        high = low.wrapping_add(((range * entry_high) / total - 1) as u16);
        low = low.wrapping_add(((range * cumulative) / total) as u16);
        loop {
            if ((high ^ low) & 0x8000) == 0 {
            } else if low & 0x4000 != 0 && high & 0x4000 == 0 {
                code ^= 0x4000;
                low &= 0x3fff;
                high |= 0x4000;
            } else {
                break;
            }
            low = low.wrapping_shl(1);
            high = high.wrapping_shl(1) | 1;
            code = code.wrapping_shl(1) | bits.next();
        }
        values.push(if entry.symbol == -2 {
            None
        } else {
            Some(entry.value)
        });
    }
    Some(values)
}

fn decode_bitlength(
    code_words: &[u8],
    code_bit_len: usize,
    value_count: usize,
) -> Option<Vec<i32>> {
    let mut bits = CodeBits {
        words: code_words,
        bit_len: code_bit_len,
        bit: 0,
    };
    let mut width = 0i32;
    // Each emitted value consumes at least the one leading run bit, so no more than
    // code_bit_len values can decode; a larger declared count cannot fill the run.
    let value_count = cadmpeg_ir::cursor::bounded_len(value_count as u64, 1, code_bit_len)?;
    let mut values = Vec::with_capacity(value_count);
    while bits.bit < bits.bit_len && values.len() < value_count {
        if bits.next() != 0 {
            let mut previous = 2u16;
            loop {
                let bit = bits.next();
                if previous != 2 && bit != previous {
                    break;
                }
                width += if bit == 1 { 2 } else { -2 };
                if !(0..=32).contains(&width) {
                    return None;
                }
                previous = bit;
            }
        }
        let mut raw = 0u32;
        for _ in 0..width {
            raw = (raw << 1) | u32::from(bits.next());
        }
        let value = if width == 0 {
            0
        } else if width == 32 {
            raw as i32
        } else {
            ((raw << (32 - width)) as i32) >> (32 - width)
        };
        values.push(value);
    }
    (values.len() == value_count).then_some(values)
}

/// Decode one complete JT Int32 Compressed Data Packet Mk. 2.
pub(crate) fn decode_int32_cdp2(bytes: &[u8], depth: u8) -> Option<(Vec<i32>, usize)> {
    if depth > 3 {
        return None;
    }
    let value_count = usize::try_from(read_u32(bytes, 0)?).ok()?;
    if value_count == 0 {
        return Some((Vec::new(), 4));
    }
    let &codec = bytes.get(4)?;
    if codec == 4 {
        let &chop_bits = bytes.get(5)?;
        if chop_bits == 0 {
            let (values, nested_len) = decode_int32_cdp2(bytes.get(6..)?, depth + 1)?;
            return (values.len() == value_count).then_some((values, 6 + nested_len));
        }
        let bias = read_u32(bytes, 6)? as i32;
        let &span_bits = bytes.get(10)?;
        if chop_bits > span_bits || span_bits > 32 {
            return None;
        }
        let (msb, msb_len) = decode_int32_cdp2(bytes.get(11..)?, depth + 1)?;
        let (lsb, lsb_len) = decode_int32_cdp2(bytes.get(11 + msb_len..)?, depth + 1)?;
        if msb.len() != value_count || lsb.len() != value_count {
            return None;
        }
        let shift = span_bits - chop_bits;
        let low_mask = if shift == 32 {
            u32::MAX
        } else {
            (1_u32 << shift) - 1
        };
        if lsb
            .iter()
            .any(|value| *value < 0 || (*value as u32) > low_mask)
        {
            return None;
        }
        let values = msb
            .into_iter()
            .zip(lsb)
            .map(|(high, low)| (low | high.wrapping_shl(u32::from(shift))).wrapping_add(bias))
            .collect();
        return Some((values, 11 + msb_len + lsb_len));
    }
    if !matches!(codec, 1 | 3) {
        return None;
    }
    let code_bit_len = usize::try_from(read_u32(bytes, 5)?).ok()?;
    let word_count = code_bit_len.div_ceil(32);
    let code_byte_len = word_count.checked_mul(4)?;
    let code_words = bytes.get(9..9 + code_byte_len)?;
    let mut cursor = 9 + code_byte_len;
    if codec == 1 {
        let values = decode_bitlength(code_words, code_bit_len, value_count)?;
        return Some((values, cursor));
    }
    let (entries, context_len) = parse_probability_context(bytes.get(cursor..)?)?;
    cursor += context_len;
    let symbols = decode_arithmetic(code_words, code_bit_len, value_count, &entries)?;
    let escape_count = symbols.iter().filter(|value| value.is_none()).count();
    let (out_of_band, oob_len) = if escape_count == 0 {
        (Vec::new(), 0)
    } else {
        decode_int32_cdp2(bytes.get(cursor..)?, depth + 1)?
    };
    if out_of_band.len() != escape_count {
        return None;
    }
    cursor += oob_len;
    let mut out_of_band = out_of_band.into_iter();
    let values = symbols
        .into_iter()
        .map(|value| value.or_else(|| out_of_band.next()))
        .collect::<Option<Vec<_>>>()?;
    Some((values, cursor))
}
