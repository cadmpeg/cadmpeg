// SPDX-License-Identifier: Apache-2.0
//! Global delimiters, count-driven Hollerith values, units, and metadata.

use crate::card::{CardScan, Section};
use cadmpeg_ir::codec::CodecError;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Omitted,
    String(Vec<u8>),
    Atom(Vec<u8>),
}

impl Value {
    fn string(&self) -> Option<String> {
        match self {
            Self::String(bytes) => String::from_utf8(bytes.clone()).ok(),
            Self::Omitted | Self::Atom(_) => None,
        }
    }

    fn integer(&self) -> Option<i64> {
        let Self::Atom(bytes) = self else {
            return None;
        };
        std::str::from_utf8(bytes).ok()?.trim().parse::<i64>().ok()
    }

    fn real(&self) -> Option<f64> {
        match self {
            Self::Atom(bytes) => std::str::from_utf8(bytes)
                .ok()?
                .trim()
                .replace(['D', 'd'], "E")
                .parse::<f64>()
                .ok(),
            Self::Omitted | Self::String(_) => None,
        }
    }
}

/// Parsed Global metadata required by inspection and projection.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Global {
    pub(crate) parameter_delimiter: u8,
    pub(crate) record_delimiter: u8,
    values: Vec<Value>,
    pub(crate) value_spans: Vec<Range<usize>>,
    pub(crate) record_end: usize,
}

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(format!("IGES Global: {}", message.into()))
}

fn hollerith(bytes: &[u8], start: usize) -> Result<Option<(Vec<u8>, usize)>, CodecError> {
    let mut cursor = start;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == start || !matches!(bytes.get(cursor), Some(b'H' | b'h')) {
        return Ok(None);
    }
    let count = std::str::from_utf8(&bytes[start..cursor])
        .map_err(|_| malformed("Hollerith count is not ASCII"))?
        .parse::<usize>()
        .map_err(|_| malformed("Hollerith count is out of range"))?;
    let payload_start = cursor
        .checked_add(1)
        .ok_or_else(|| malformed("Hollerith payload offset overflow"))?;
    let payload_end = payload_start
        .checked_add(count)
        .ok_or_else(|| malformed("Hollerith payload length overflow"))?;
    let payload = bytes
        .get(payload_start..payload_end)
        .ok_or_else(|| malformed("Hollerith payload is truncated"))?;
    Ok(Some((payload.to_vec(), payload_end)))
}

fn first_delimiter(bytes: &[u8]) -> Result<(u8, usize), CodecError> {
    if bytes.first() == Some(&b',') {
        return Ok((b',', 1));
    }
    let Some((payload, cursor)) = hollerith(bytes, 0)? else {
        return Err(malformed("parameter delimiter is not a Hollerith string"));
    };
    if payload.len() != 1 {
        return Err(malformed("parameter delimiter must contain one byte"));
    }
    let delimiter = payload[0];
    if bytes.get(cursor) != Some(&delimiter) {
        return Err(malformed(
            "parameter delimiter does not terminate its Global field",
        ));
    }
    Ok((delimiter, cursor + 1))
}

fn delimited_value(
    bytes: &[u8],
    start: usize,
    parameter_delimiter: u8,
    record_delimiter: Option<u8>,
) -> Result<(Value, Range<usize>, usize, bool), CodecError> {
    if bytes.get(start) == Some(&parameter_delimiter) {
        return Ok((Value::Omitted, start..start, start + 1, false));
    }
    if record_delimiter.is_some_and(|delimiter| bytes.get(start) == Some(&delimiter)) {
        return Ok((Value::Omitted, start..start, start + 1, true));
    }
    let (value, end) = if let Some((payload, end)) = hollerith(bytes, start)? {
        (Value::String(payload), end)
    } else {
        let end = bytes[start..]
            .iter()
            .position(|byte| *byte == parameter_delimiter || record_delimiter == Some(*byte))
            .and_then(|relative| start.checked_add(relative))
            .ok_or_else(|| malformed("record delimiter is missing"))?;
        (Value::Atom(bytes[start..end].to_vec()), end)
    };
    match bytes.get(end).copied() {
        Some(separator) if separator == parameter_delimiter => {
            Ok((value, start..end, end + 1, false))
        }
        Some(separator) if record_delimiter == Some(separator) => {
            Ok((value, start..end, end + 1, true))
        }
        _ => Err(malformed("value is not followed by a delimiter")),
    }
}

pub(crate) fn parse(scan: &CardScan) -> Result<Global, CodecError> {
    let bytes = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Global))
        .flat_map(|line| line.payload.iter().take(72).copied())
        .collect::<Vec<_>>();
    if bytes.is_empty() {
        return Err(malformed("section is missing"));
    }
    let (parameter_delimiter, mut cursor) = first_delimiter(&bytes)?;
    let mut value_spans = Vec::with_capacity(26);
    value_spans.push(0..cursor.saturating_sub(1));
    let (record_value, record_span, next, ended) =
        delimited_value(&bytes, cursor, parameter_delimiter, None)?;
    if ended {
        return Err(malformed("record ends before the record delimiter field"));
    }
    cursor = next;
    value_spans.push(record_span);
    let record_delimiter = match record_value {
        Value::Omitted => b';',
        Value::String(value) if value.len() == 1 => value[0],
        Value::String(_) | Value::Atom(_) => {
            return Err(malformed("record delimiter must contain one byte"));
        }
    };

    let mut values = vec![
        Value::String(vec![parameter_delimiter]),
        Value::String(vec![record_delimiter]),
    ];
    loop {
        let (value, span, next, ended) =
            delimited_value(&bytes, cursor, parameter_delimiter, Some(record_delimiter))?;
        values.push(value);
        value_spans.push(span);
        cursor = next;
        if ended {
            break;
        }
    }
    Ok(Global {
        parameter_delimiter,
        record_delimiter,
        values,
        value_spans,
        record_end: cursor,
    })
}

fn version_name(flag: i64) -> Option<&'static str> {
    match flag {
        1 => Some("1.0"),
        2 => Some("ANSI-Y14.26M-1981"),
        3 => Some("2.0"),
        4 => Some("3.0"),
        5 => Some("ASME-ANSI-Y14.26M-1987"),
        6 => Some("4.0"),
        7 => Some("ASME-Y14.26M-1989"),
        8 => Some("5.0"),
        9 => Some("5.1"),
        10 => Some("5.2"),
        11 => Some("5.3"),
        _ => None,
    }
}

impl Global {
    pub(crate) fn model_scale(&self) -> f64 {
        self.values.get(12).and_then(Value::real).unwrap_or(1.0)
    }

    pub(crate) fn units_flag(&self) -> i64 {
        self.values.get(13).and_then(Value::integer).unwrap_or(1)
    }

    pub(crate) fn length_factor_mm(&self) -> Option<f64> {
        let unit = match self.units_flag() {
            1 => 25.4,
            2 => 1.0,
            3 => match self.units_name()?.as_str() {
                "IN" | "INCH" => 25.4,
                "MM" => 1.0,
                "FT" => 304.8,
                "MI" => 1_609_344.0,
                "M" => 1_000.0,
                "KM" => 1_000_000.0,
                "MIL" => 0.0254,
                "UM" => 0.001,
                "CM" => 10.0,
                "UIN" => 0.000_025_4,
                _ => return None,
            },
            4 => 304.8,
            5 => 1_609_344.0,
            6 => 1_000.0,
            7 => 1_000_000.0,
            8 => 0.0254,
            9 => 0.001,
            10 => 10.0,
            11 => 0.000_025_4,
            _ => return None,
        };
        let scale = self.model_scale();
        (scale.is_finite() && scale > 0.0).then_some(unit / scale)
    }

    pub(crate) fn minimum_resolution_mm(&self) -> Option<f64> {
        let resolution = self.values.get(18).and_then(Value::real)?;
        let factor = self.length_factor_mm()?;
        (resolution.is_finite() && resolution > 0.0).then_some(resolution * factor)
    }

    pub(crate) fn line_weight_mm(&self, number: i64) -> Option<f64> {
        let gradations = self.values.get(15).and_then(Value::integer).unwrap_or(1);
        let maximum = self.values.get(16).and_then(Value::real)?;
        let factor = self.length_factor_mm()?;
        (number > 0
            && number <= gradations
            && gradations > 0
            && maximum.is_finite()
            && maximum > 0.0)
            .then_some(number as f64 * maximum * factor / gradations as f64)
    }

    pub(crate) fn sender_product(&self) -> Option<String> {
        self.values.get(2).and_then(Value::string)
    }

    pub(crate) fn native_file_name(&self) -> Option<String> {
        self.values.get(3).and_then(Value::string)
    }

    pub(crate) fn units_name(&self) -> Option<String> {
        self.values.get(14).and_then(Value::string)
    }

    pub(crate) fn version_flag(&self) -> Option<i64> {
        self.values.get(22).and_then(Value::integer).or(Some(3))
    }

    pub(crate) fn version(&self) -> Option<&'static str> {
        self.version_flag().and_then(version_name)
    }

    pub(crate) fn summary_notes(&self) -> Vec<String> {
        let mut notes = vec![
            format!(
                "parameter_delimiter={}",
                char::from(self.parameter_delimiter)
            ),
            format!("record_delimiter={}", char::from(self.record_delimiter)),
        ];
        if let Some(product) = self.sender_product() {
            notes.push(format!("sender_product={product}"));
        }
        if let Some(units) = self.units_name() {
            notes.push(format!("units={units}"));
        }
        if let Some(version) = self.version() {
            notes.push(format!("iges_version={version}"));
        }
        notes
    }
}
