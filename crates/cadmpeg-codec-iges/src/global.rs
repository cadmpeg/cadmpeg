// SPDX-License-Identifier: Apache-2.0
//! Global delimiters, count-driven Hollerith values, units, and metadata.

use crate::card::{CardScan, Section};
use cadmpeg_core::CodecError;
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

    fn string_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::String(bytes) => Some(bytes),
            Self::Omitted | Self::Atom(_) => None,
        }
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

#[derive(Clone, Copy)]
pub(crate) struct RealPrecision {
    pub(crate) single_significance: u32,
    pub(crate) double_significance: u32,
}

fn malformed(message: impl Into<String>) -> CodecError {
    crate::error::malformed(format!("IGES Global: {}", message.into()))
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
        let atom = &bytes[start..end];
        if atom.iter().all(u8::is_ascii_whitespace) {
            (Value::Omitted, end)
        } else {
            (Value::Atom(atom.to_vec()), end)
        }
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
    let global = Global {
        parameter_delimiter,
        record_delimiter,
        values,
        value_spans,
        record_end: cursor,
    };
    global.validate()?;
    Ok(global)
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
    fn integer_field(
        &self,
        index: usize,
        name: &str,
        default: Option<i64>,
    ) -> Result<i64, CodecError> {
        match self.values.get(index).unwrap_or(&Value::Omitted) {
            Value::Omitted => default
                .ok_or_else(|| malformed(format!("field {} ({name}) has no value", index + 1))),
            value => value.integer().ok_or_else(|| {
                malformed(format!("field {} ({name}) is not an integer", index + 1))
            }),
        }
    }

    fn real_field(
        &self,
        index: usize,
        name: &str,
        default: Option<f64>,
    ) -> Result<f64, CodecError> {
        match self.values.get(index).unwrap_or(&Value::Omitted) {
            Value::Omitted => default
                .ok_or_else(|| malformed(format!("field {} ({name}) has no value", index + 1))),
            value => value
                .real()
                .ok_or_else(|| malformed(format!("field {} ({name}) is not a real", index + 1))),
        }
    }

    fn validate(&self) -> Result<(), CodecError> {
        for (index, name) in [
            (8, "single-precision significance"),
            (10, "double-precision significance"),
        ] {
            let significance = self.integer_field(index, name, None)?;
            if significance <= 0 || u32::try_from(significance).is_err() {
                return Err(malformed(format!(
                    "field {} ({name}) must be a positive u32",
                    index + 1
                )));
            }
        }
        let scale = self.real_field(12, "model space scale", Some(1.0))?;
        if !scale.is_finite() || scale <= 0.0 {
            return Err(malformed(
                "field 13 (model space scale) must be finite and positive",
            ));
        }
        let units = self.integer_field(13, "units flag", Some(1))?;
        if !(1..=11).contains(&units) {
            return Err(malformed("field 14 (units flag) must be in 1 through 11"));
        }
        if units == 3
            && !matches!(
                self.values.get(14),
                Some(Value::String(value)) if !value.is_empty()
            )
        {
            return Err(malformed(
                "field 15 (units name) is required and nonempty for units flag 3",
            ));
        }
        let gradations = self.integer_field(15, "maximum line-weight gradations", Some(1))?;
        if !(1..=32_768).contains(&gradations) {
            return Err(malformed(
                "field 16 (maximum line-weight gradations) must be in 1 through 32768",
            ));
        }
        let maximum_width = self.real_field(16, "maximum line width", None)?;
        if !maximum_width.is_finite() || maximum_width <= 0.0 {
            return Err(malformed(
                "field 17 (maximum line width) must be finite and positive",
            ));
        }
        let resolution = self.real_field(18, "minimum resolution", None)?;
        if !resolution.is_finite() || resolution < 0.0 {
            return Err(malformed(
                "field 19 (minimum resolution) must be finite and nonnegative",
            ));
        }
        let version = self.integer_field(22, "version flag", Some(3))?;
        if version_name(version).is_none() {
            return Err(malformed(
                "field 23 (version flag) is not a defined IGES version",
            ));
        }
        Ok(())
    }

    pub(crate) fn model_scale(&self) -> f64 {
        self.real_field(12, "model space scale", Some(1.0))
            .expect("validated Global model space scale")
    }

    pub(crate) fn units_flag(&self) -> i64 {
        self.integer_field(13, "units flag", Some(1))
            .expect("validated Global units flag")
    }

    pub(crate) fn single_precision_significance(&self) -> u32 {
        self.integer_field(8, "single-precision significance", None)
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .expect("validated Global single-precision significance")
    }

    pub(crate) fn double_precision_significance(&self) -> u32 {
        self.integer_field(10, "double-precision significance", None)
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .expect("validated Global double-precision significance")
    }

    pub(crate) fn real_precision(&self) -> RealPrecision {
        RealPrecision {
            single_significance: self.single_precision_significance(),
            double_significance: self.double_precision_significance(),
        }
    }

    fn named_unit_factor_mm(&self) -> Option<f64> {
        match self.values.get(14).and_then(Value::string_bytes)? {
            b"IN" | b"INCH" => Some(25.4),
            b"MM" => Some(1.0),
            b"FT" => Some(304.8),
            b"MI" => Some(1_609_344.0),
            b"M" => Some(1_000.0),
            b"KM" => Some(1_000_000.0),
            b"MIL" => Some(0.0254),
            b"UM" => Some(0.001),
            b"CM" => Some(10.0),
            b"UIN" => Some(0.000_025_4),
            _ => None,
        }
    }

    pub(crate) fn has_supported_length_factor(&self) -> bool {
        self.units_flag() != 3 || self.named_unit_factor_mm().is_some()
    }

    pub(crate) fn length_factor_mm(&self) -> f64 {
        let unit = match self.units_flag() {
            1 => 25.4,
            2 => 1.0,
            3 => self
                .named_unit_factor_mm()
                .expect("validated Global named units"),
            4 => 304.8,
            5 => 1_609_344.0,
            6 => 1_000.0,
            7 => 1_000_000.0,
            8 => 0.0254,
            9 => 0.001,
            10 => 10.0,
            11 => 0.000_025_4,
            _ => unreachable!("validated Global units flag"),
        };
        unit / self.model_scale()
    }

    pub(crate) fn minimum_resolution_mm(&self) -> f64 {
        let resolution = self
            .real_field(18, "minimum resolution", None)
            .expect("validated Global minimum resolution");
        resolution * self.length_factor_mm()
    }

    #[cfg(test)]
    pub(crate) fn maximum_coordinate_mm(&self) -> f64 {
        self.real_field(19, "maximum coordinate", Some(0.0))
            .expect("validated Global maximum coordinate")
            * self.length_factor_mm()
    }

    pub(crate) fn line_weight_mm(&self, number: i64) -> Option<f64> {
        let gradations = self
            .integer_field(15, "maximum line-weight gradations", Some(1))
            .expect("validated Global line-weight gradations");
        let maximum = self
            .real_field(16, "maximum line width", None)
            .expect("validated Global maximum line width");
        let factor = self.length_factor_mm();
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

    pub(crate) fn sender_product_bytes(&self) -> Option<&[u8]> {
        self.values.get(2).and_then(Value::string_bytes)
    }

    pub(crate) fn native_file_name(&self) -> Option<String> {
        self.values.get(3).and_then(Value::string)
    }

    pub(crate) fn native_file_name_bytes(&self) -> Option<&[u8]> {
        self.values.get(3).and_then(Value::string_bytes)
    }

    pub(crate) fn units_name(&self) -> Option<String> {
        self.values.get(14).and_then(Value::string)
    }

    pub(crate) fn version_flag(&self) -> i64 {
        self.integer_field(22, "version flag", Some(3))
            .expect("validated Global version flag")
    }

    pub(crate) fn version(&self) -> &'static str {
        version_name(self.version_flag()).expect("validated Global version flag")
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
        notes.push(format!("iges_version={}", self.version()));
        notes
    }
}

#[cfg(test)]
mod tests;
