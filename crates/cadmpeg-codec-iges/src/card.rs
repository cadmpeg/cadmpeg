// SPDX-License-Identifier: Apache-2.0
//! Exact physical-line and fixed-card framing.

use cadmpeg_ir::codec::Confidence;

const CARD_WIDTH: usize = 80;

fn take_line(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let ending_at = input
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))?;
    let ending_len =
        usize::from(input[ending_at] == b'\r' && input.get(ending_at + 1) == Some(&b'\n')) + 1;
    Some((&input[..ending_at], &input[ending_at + ending_len..]))
}

fn sequence(card: &[u8]) -> Option<u32> {
    let field = card.get(73..80)?;
    if field.iter().any(|byte| !matches!(byte, b' ' | b'0'..=b'9')) {
        return None;
    }
    let digits = field.iter().copied().skip_while(|byte| *byte == b' ');
    let mut value = 0_u32;
    let mut count = 0_usize;
    for digit in digits {
        count += 1;
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(digit - b'0'))?;
    }
    (count > 0 && value > 0).then_some(value)
}

fn header(line: &[u8]) -> Option<(u8, u32)> {
    if line.len() != CARD_WIDTH || line.iter().any(|byte| !(b' '..=b'~').contains(byte)) {
        return None;
    }
    Some((*line.get(72)?, sequence(line)?))
}

pub(crate) fn detect_fixed_ascii(prefix: &[u8]) -> Confidence {
    let Some((first, rest)) = take_line(prefix) else {
        return Confidence::No;
    };
    let Some((second, _)) = take_line(rest) else {
        return Confidence::No;
    };
    if header(first) != Some((b'S', 1)) {
        return Confidence::No;
    }
    match header(second) {
        Some((b'S', 2) | (b'G', 1)) => Confidence::High,
        _ => Confidence::No,
    }
}
