// SPDX-License-Identifier: Apache-2.0

use super::{all_options, as_double, outcomes, require_all, Class, Fact, Value};
use Class::{Boolean, Dict, Float, Int, List, Object, Placement, Refused, Text, Tuple, Vector};

pub(super) fn remainder(format: Option<&str>, right: &Value, class: Class) -> Value {
    let proved = format.and_then(|format| positional(format, right, class));
    outcomes(Text, proved)
}

enum Arguments<'a> {
    Scalar(Option<(&'a Value, Class)>),
    Tuple(std::slice::Iter<'a, Value>),
}

impl<'a> Arguments<'a> {
    fn next(&mut self) -> Option<(&'a Value, Option<Class>)> {
        match self {
            Self::Scalar(value) => value.take().map(|(value, class)| (value, Some(class))),
            Self::Tuple(items) => items.next().map(|value| (value, None)),
        }
    }
}

/// Prove `CPython`'s positional %-format shape and conversions without
/// constructing the result.
fn positional(format: &str, right: &Value, class: Class) -> Option<bool> {
    let mut args = if class == Tuple {
        Arguments::Tuple(right.item_facts()?.iter())
    } else {
        Arguments::Scalar(Some((right, class)))
    };
    let mut parts = format.split('%');
    parts.next()?;
    let mut checks = Vec::new();
    while let Some(part) = parts.next() {
        if part.is_empty() {
            if parts.next().is_some() {
                continue;
            }
            return Some(false);
        }
        let bytes = part.as_bytes();
        if bytes[0] == b'(' {
            return match class {
                Dict | Object => None,
                _ => Some(false),
            };
        }
        let mut cursor = bytes.iter().copied().peekable();
        while cursor.next_if(|byte| b"#0- +".contains(byte)).is_some() {}
        if cursor.next_if_eq(&b'*').is_some() {
            let Some((value, class)) = args.next() else {
                return Some(false);
            };
            checks.push(check(b'*', value, class));
        } else if !digits(&mut cursor, isize::MAX.unsigned_abs()) {
            return Some(false);
        }
        if cursor.next_if_eq(&b'.').is_some() {
            if cursor.next_if_eq(&b'*').is_some() {
                let Some((value, class)) = args.next() else {
                    return Some(false);
                };
                checks.push(check(b'*', value, class));
            } else if !digits(&mut cursor, 2_147_483_647) {
                return Some(false);
            }
        }
        let _length_modifier = cursor.next_if(|byte| matches!(byte, b'h' | b'l' | b'L'));
        let (Some(conversion), Some((value, class))) = (cursor.next(), args.next()) else {
            return Some(false);
        };
        checks.push(check(conversion, value, class));
    }
    if args.next().is_some() {
        // PyUnicode_Format permits an unused mapping argument.
        return match class {
            Dict | List => Some(true),
            Object => None,
            _ => Some(false),
        };
    }
    require_all(checks)
}

fn digits(cursor: &mut std::iter::Peekable<impl Iterator<Item = u8>>, max: usize) -> bool {
    let mut number = 0_usize;
    while let Some(digit) = cursor.next_if(u8::is_ascii_digit) {
        let Some(next) = number
            .checked_mul(10)
            .and_then(|n| n.checked_add(usize::from(digit - b'0')))
        else {
            return false;
        };
        if next > max {
            return false;
        }
        number = next;
    }
    true
}

fn check(conversion: u8, value: &Value, class: Option<Class>) -> Option<bool> {
    let accepts = |class| {
        if class == Refused && !matches!(value.1, Some(Fact::None)) {
            return None;
        }
        match conversion {
            b's' | b'r' | b'a' => match class {
                Text | Int | Float | Boolean | Class::Quantity(_) | Vector | Placement => {
                    Some(true)
                }
                Refused if matches!(value.1, Some(Fact::None)) => Some(true),
                List | Dict | Tuple | Object | Refused => None,
                Class::Raises => Some(false),
            },
            b'd' | b'i' | b'u' => match class {
                Int | Boolean => Some(true),
                Float => value.numeric_fact().map(f64::is_finite),
                Class::Quantity(_) => value.numeric_fact().and_then(|number| {
                    (number.is_finite() && Value::number(number.trunc()) == Int).then_some(true)
                }),
                Object => None,
                _ => Some(false),
            },
            b'o' | b'x' | b'X' => Some(matches!(class, Int | Boolean)),
            b'*' => match class {
                Boolean => Some(true),
                Int => value.numeric_fact().map(|_| true),
                _ => Some(false),
            },
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
                if class == Refused {
                    Some(false)
                } else {
                    as_double(&Value::numeric(class, value.numeric_fact()))
                }
            }
            b'c' => match class {
                Boolean => Some(true),
                Int => value
                    .numeric_fact()
                    .map(|n| (0.0..=1_114_111.0).contains(&n)),
                Text => value
                    .text_fact()
                    .map(|text| text.chars().take(2).count() == 1),
                Object => None,
                _ => Some(false),
            },
            _ => Some(false),
        }
    };
    match class {
        Some(class) => accepts(class),
        None => all_options(value, accepts),
    }
}
