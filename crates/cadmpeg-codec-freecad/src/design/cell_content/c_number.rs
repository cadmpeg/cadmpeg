// SPDX-License-Identifier: Apache-2.0
//! The complete C-number pass in Spreadsheet/App/Cell.cpp:318–333.

/// A C number prefix was present and, when the whole carrier was accepted,
/// its value. An out-of-range prefix still permits the expression pass.
pub(super) fn read(text: &str) -> (bool, Option<f64>) {
    let text = text.split('\0').next().unwrap_or(text);
    let trimmed = text.trim_start_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let unsigned = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let negative = trimmed.starts_with('-');
    let (length, value) = unsigned_number(unsigned);
    let trailing = |rest: &str| {
        rest.bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    };
    if length == 0 {
        return (false, (!text.is_empty() && trailing(text)).then_some(0.0));
    }
    let value = value
        .filter(|_| trailing(&unsigned[length..]))
        .map(|number| if negative { -number } else { number });
    (true, value)
}

fn unsigned_number(text: &str) -> (usize, Option<f64>) {
    if let Some((length, number)) = special(text) {
        return (length, Some(number));
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        let (length, number) = hexadecimal(hex);
        if length != 0 {
            return (length + 2, number);
        }
    }
    let (length, _) = mantissa(text, u8::is_ascii_digit);
    if length == 0 {
        return (0, None);
    }
    let (power, end) = exponent(text, length, b'e');
    let value = text[..end].parse::<f64>().ok().filter(|number| {
        number.is_normal()
            || number.is_subnormal()
                && power.is_some_and(|power| exact_subnormal(&text[..length], power, *number))
            || *number == 0.0
                && !text[..length]
                    .bytes()
                    .any(|byte| matches!(byte, b'1'..=b'9'))
    });
    (end, value)
}

/// C's range check admits an exactly representable subnormal. Binary64's
/// smallest place is 2^-1074, so 1074 fractional digits give its exact value.
fn exact_subnormal(mantissa: &str, power: i64, number: f64) -> bool {
    const DECIMAL_PLACES: usize = 1074;
    let exact = format!("{number:.DECIMAL_PLACES$}");
    significant_digits(mantissa, power)
        .zip(significant_digits(&exact, 0))
        .is_some_and(|((left_place, left), (right_place, right))| {
            left_place == right_place
                && left
                    .bytes()
                    .filter(|&byte| byte != b'.')
                    .eq(right.bytes().filter(|&byte| byte != b'.'))
        })
}

fn significant_digits(text: &str, power: i64) -> Option<(i64, &str)> {
    let is_significant = |ch| matches!(ch, '1'..='9');
    let first = text.find(is_significant)?;
    let last = text.rfind(is_significant)?;
    let point = text.find('.').unwrap_or(text.len());
    let leading = first - usize::from(first > point);
    let place = i64::try_from(point)
        .ok()?
        .checked_sub(i64::try_from(leading).ok()?)?
        .checked_add(power)?;
    Some((place, &text[first..=last]))
}

fn special(text: &str) -> Option<(usize, f64)> {
    let starts = |word: &str| {
        text.get(..word.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(word))
    };
    if starts("infinity") {
        Some((8, f64::INFINITY))
    } else if starts("inf") {
        Some((3, f64::INFINITY))
    } else if starts("nan") {
        let length = text[3..]
            .strip_prefix('(')
            .and_then(|rest| rest.split_once(')'))
            .filter(|(payload, _)| {
                payload
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
            .map_or(3, |(payload, _)| 5 + payload.len());
        Some((length, f64::NAN))
    } else {
        None
    }
}

fn mantissa(text: &str, is_digit: fn(&u8) -> bool) -> (usize, usize) {
    let whole = text.bytes().take_while(is_digit).count();
    if let Some(fraction) = text[whole..].strip_prefix('.') {
        let fraction = fraction.bytes().take_while(is_digit).count();
        if whole != 0 || fraction != 0 {
            return (whole + 1 + fraction, whole);
        }
    }
    (whole, whole)
}

fn exponent(text: &str, at: usize, marker: u8) -> (Option<i64>, usize) {
    let rest = &text.as_bytes()[at..];
    if rest.first().map(u8::to_ascii_lowercase) != Some(marker) {
        return (Some(0), at);
    }
    let start = at + 1 + usize::from(matches!(rest.get(1), Some(b'+' | b'-')));
    let digits = text[start..].bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return (Some(0), at);
    }
    let end = start + digits;
    (text[at + 1..end].parse().ok(), end)
}

fn hexadecimal(text: &str) -> (usize, Option<f64>) {
    let (length, whole) = mantissa(text, u8::is_ascii_hexdigit);
    if length == 0 {
        return (0, None);
    }
    let (power, end) = exponent(text, length, b'p');
    (end, rounded_hex(&text[..length], whole, power))
}

/// Assemble the binary64 significand before scaling. The C pass gives up an
/// inexact subnormal (ERANGE), while an exact subnormal has a valid bit pattern.
fn rounded_hex(text: &str, whole: usize, power: Option<i64>) -> Option<f64> {
    let nibbles = text
        .chars()
        .filter(|&digit| digit != '.')
        .filter_map(|digit| digit.to_digit(16));
    let mut nibbles = nibbles.enumerate().skip_while(|&(_, digit)| digit == 0);
    let Some((index, digit)) = nibbles.next() else {
        return Some(0.0);
    };
    let leading = digit.ilog2();
    let place = i64::try_from(whole).ok()? - i64::try_from(index).ok()? - 1;
    let mut high = power?
        .checked_add(place.checked_mul(4)?)?
        .checked_add(i64::from(leading))?;
    if !(-1074..=1023).contains(&high) {
        return None;
    }
    let subnormal = high < -1022;
    let kept = if subnormal {
        u32::try_from(high + 1075).ok()?
    } else {
        53
    };
    let bits = (0..=leading)
        .rev()
        .map(|bit| (digit >> bit) & 1)
        .chain(nibbles.flat_map(|(_, digit)| (0..4).rev().map(move |bit| (digit >> bit) & 1)));
    let mut bits = bits.map(u64::from);
    let significand = (0..kept).fold(0, |value, _| (value << 1) | bits.next().unwrap_or(0));
    let round = bits.next().unwrap_or(0) != 0;
    let sticky = bits.any(|bit| bit != 0);
    if subnormal {
        return (!round && !sticky).then(|| f64::from_bits(significand));
    }
    let mut significand = significand + u64::from(round && (sticky || significand & 1 != 0));
    if significand == 1 << 53 {
        high += 1;
        significand >>= 1;
    }
    (high <= 1023).then(|| {
        f64::from_bits(((high + 1023).unsigned_abs() << 52) | (significand & ((1 << 52) - 1)))
    })
}
