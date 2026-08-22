// SPDX-License-Identifier: Apache-2.0
//! Global delimiters, count-driven Hollerith values, units, and metadata.

use crate::card::{CardScan, Section};
use crate::loss::IgesLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::LossNote;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Omitted,
    String(Vec<u8>),
    ForbiddenString,
    Atom(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Defect {
    Absent,
    Malformed,
}

impl Defect {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Malformed => "malformed",
        }
    }
}

enum Supplied<T> {
    Absent,
    Value(T),
    Malformed,
}

/// Global field boundaries recovered from the card stream, with no values resolved.
#[derive(Debug)]
pub(crate) struct RawGlobal {
    parameter_delimiter: u8,
    record_delimiter: u8,
    values: Vec<Value>,
}

/// The effective specification family selected by Global field 23.
///
/// The older declarations remain grouped as `Legacy` until their own
/// specifications are verified. The 4.0 and 5.0 families are separate because
/// their Global tables stop at fields 24 and 25 respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    Legacy,
    V4_0,
    V5_0,
    V5_1,
    V5_2,
    V5_3,
}

impl Dialect {
    const fn from_effective_flag(flag: i64) -> Self {
        match flag {
            6 => Self::V4_0,
            8 => Self::V5_0,
            9 => Self::V5_1,
            10 => Self::V5_2,
            11 => Self::V5_3,
            _ => Self::Legacy,
        }
    }

    const fn global_field_count(self) -> usize {
        match self {
            Self::Legacy | Self::V5_1 | Self::V5_2 | Self::V5_3 => 26,
            Self::V4_0 => 24,
            Self::V5_0 => 25,
        }
    }

    const fn accepts_four_digit_date(self) -> bool {
        matches!(self, Self::Legacy | Self::V5_1 | Self::V5_2 | Self::V5_3)
    }

    const fn default_model_scale(self) -> Option<f64> {
        match self {
            Self::V4_0 => None,
            Self::Legacy | Self::V5_0 | Self::V5_1 | Self::V5_2 | Self::V5_3 => Some(1.0),
        }
    }

    const fn default_units_flag(self) -> Option<i64> {
        match self {
            Self::V4_0 | Self::V5_0 => None,
            Self::Legacy | Self::V5_1 | Self::V5_2 | Self::V5_3 => Some(1),
        }
    }

    const fn has_model_date(self) -> bool {
        !matches!(self, Self::V4_0)
    }

    const fn has_application_protocol(self) -> bool {
        matches!(self, Self::Legacy | Self::V5_1 | Self::V5_2 | Self::V5_3)
    }
}

/// Global field values, fallbacks, and absences after one resolution pass.
#[derive(Debug)]
pub(crate) struct ResolvedGlobal {
    pub(crate) parameter_delimiter: u8,
    pub(crate) record_delimiter: u8,
    sender_product: Option<String>,
    native_file_name: Option<String>,
    units_name: Option<String>,
    #[cfg(test)]
    units_flag: Option<i64>,
    precision: RealPrecision,
    minimum_resolution: f64,
    #[cfg(test)]
    maximum_coordinate: Option<f64>,
    length_factor_mm: Option<f64>,
    line_weight_scale: Option<LineWeightScale>,
    declared_version_flag: i64,
    unreadable_version_declaration: Option<String>,
    dialect: Dialect,
}

/// Length-valued Global view. It exists only when the millimetre factor resolved.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProjectedGlobal {
    length_factor_mm: f64,
    minimum_resolution_mm: f64,
    precision: RealPrecision,
    line_weight_scale: Option<LineWeightScale>,
    dialect: Dialect,
}

#[derive(Debug, Clone, Copy)]
struct LineWeightScale {
    gradations: i64,
    maximum_width: f64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RealPrecision {
    pub(crate) single_significance: u32,
    pub(crate) double_significance: u32,
}

const FIELD_SENDER_PRODUCT: usize = 2;
const FIELD_FILE_NAME: usize = 3;
const FIELD_NATIVE_SYSTEM: usize = 4;
const FIELD_PREPROCESSOR_VERSION: usize = 5;
const FIELD_INTEGER_BITS: usize = 6;
const FIELD_SINGLE_MAGNITUDE: usize = 7;
const FIELD_SINGLE_SIGNIFICANCE: usize = 8;
const FIELD_DOUBLE_MAGNITUDE: usize = 9;
const FIELD_DOUBLE_SIGNIFICANCE: usize = 10;
const FIELD_RECEIVER_PRODUCT: usize = 11;
const FIELD_MODEL_SCALE: usize = 12;
const FIELD_UNITS_FLAG: usize = 13;
const FIELD_UNITS_NAME: usize = 14;
const FIELD_LINE_WEIGHT_GRADATIONS: usize = 15;
const FIELD_MAXIMUM_LINE_WIDTH: usize = 16;
const FIELD_GENERATION_DATE: usize = 17;
const FIELD_MINIMUM_RESOLUTION: usize = 18;
const FIELD_MAXIMUM_COORDINATE: usize = 19;
const FIELD_AUTHOR: usize = 20;
const FIELD_ORGANIZATION: usize = 21;
const FIELD_VERSION_FLAG: usize = 22;
const FIELD_DRAFTING_STANDARD: usize = 23;
const FIELD_MODEL_DATE: usize = 24;
const FIELD_APPLICATION_PROTOCOL: usize = 25;
const TABLE_1_FIELD_COUNT: usize = 26;

const FIELD_NAMES: [&str; TABLE_1_FIELD_COUNT] = [
    "parameter delimiter",
    "record delimiter",
    "product identification from sender",
    "file name",
    "native system ID",
    "preprocessor version",
    "integer representation bits",
    "single-precision magnitude",
    "single-precision significance",
    "double-precision magnitude",
    "double-precision significance",
    "product identification for the receiver",
    "model space scale",
    "units flag",
    "units name",
    "maximum line-weight gradations",
    "maximum line width",
    "date and time of exchange file generation",
    "minimum user-intended resolution",
    "approximate maximum coordinate",
    "author name",
    "author organization",
    "version flag",
    "drafting standard flag",
    "date and time the model was created or modified",
    "application protocol",
];

const FALLBACK_SIGNIFICANCE: u32 = 17;
const FALLBACK_MINIMUM_RESOLUTION: f64 = 0.0;
const VERIFIED_VERSIONS: [&str; 3] = ["5.1", "5.2", "5.3"];

const METADATA_CONSEQUENCE: &str = "its value was not transferred";
const SIGNIFICANCE_CONSEQUENCE: &str =
    "the decoder substituted 17 significant decimal digits from its own specification";
const RESOLUTION_CONSEQUENCE: &str =
    "the decoder substituted 0.0 millimetres as the minimum user-intended resolution";
const LENGTH_CONSEQUENCE: &str =
    "the decoder resolved no millimetre length factor, suppressed every geometry projection, and retained the native records";
const LINE_WEIGHT_CONSEQUENCE: &str =
    "the line-weight scale is unavailable, so no entity carries a display width";

fn malformed(message: impl Into<String>) -> CodecError {
    crate::error::malformed(format!("IGES Global: {}", message.into()))
}

fn field_name(index: usize) -> &'static str {
    FIELD_NAMES.get(index).copied().unwrap_or("extra field")
}

const fn forbidden_string_byte(byte: u8) -> bool {
    !byte.is_ascii() || byte.is_ascii_control()
}

const fn prohibited_delimiter_byte(byte: u8) -> bool {
    byte.is_ascii_control()
        || !byte.is_ascii()
        || byte.is_ascii_digit()
        || matches!(byte, b' ' | b'+' | b'-' | b'.' | b'D' | b'E' | b'H')
}

fn hollerith(bytes: &[u8], start: usize) -> Result<Option<(Vec<u8>, usize, bool)>, CodecError> {
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
    let forbidden = payload.iter().copied().any(forbidden_string_byte);
    Ok(Some((payload.to_vec(), payload_end, forbidden)))
}

fn first_delimiter(bytes: &[u8]) -> Result<(u8, usize), CodecError> {
    if bytes.first() == Some(&b',') {
        return Ok((b',', 1));
    }
    let Some((payload, cursor, forbidden)) = hollerith(bytes, 0)? else {
        return Err(malformed("parameter delimiter is not a Hollerith string"));
    };
    if forbidden {
        return Err(malformed(
            "parameter delimiter is a non-ASCII or control character",
        ));
    }
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
) -> Result<(Value, usize, bool), CodecError> {
    if bytes.get(start) == Some(&parameter_delimiter) {
        return Ok((Value::Omitted, start + 1, false));
    }
    if record_delimiter.is_some_and(|delimiter| bytes.get(start) == Some(&delimiter)) {
        return Ok((Value::Omitted, start + 1, true));
    }
    let (value, end) = if let Some((payload, end, forbidden)) = hollerith(bytes, start)? {
        if forbidden {
            (Value::ForbiddenString, end)
        } else {
            (Value::String(payload), end)
        }
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
        Some(separator) if separator == parameter_delimiter => Ok((value, end + 1, false)),
        Some(separator) if record_delimiter == Some(separator) => Ok((value, end + 1, true)),
        _ => Err(malformed("value is not followed by a delimiter")),
    }
}

fn global_bytes(scan: &CardScan<'_>) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    let mut pending_digits = Vec::new();
    let mut hollerith_remaining = 0_usize;

    for line in scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Global))
    {
        for byte in line.payload.iter().take(72).copied() {
            if hollerith_remaining > 0 {
                bytes.push(byte);
                hollerith_remaining -= 1;
                continue;
            }

            if byte == b' ' {
                continue;
            }
            if byte.is_ascii_digit() {
                pending_digits.push(byte);
                continue;
            }
            if !pending_digits.is_empty() && matches!(byte, b'H' | b'h') {
                let count = std::str::from_utf8(&pending_digits)
                    .map_err(|_| malformed("Hollerith count is not ASCII"))?
                    .parse::<usize>()
                    .map_err(|_| malformed("Hollerith count is out of range"))?;
                bytes.extend_from_slice(&pending_digits);
                bytes.push(byte);
                pending_digits.clear();
                hollerith_remaining = count;
                continue;
            }

            bytes.extend_from_slice(&pending_digits);
            pending_digits.clear();
            bytes.push(byte);
        }
    }
    bytes.extend_from_slice(&pending_digits);
    Ok(bytes)
}

fn parse_raw(scan: &CardScan) -> Result<RawGlobal, CodecError> {
    let bytes = global_bytes(scan)?;
    if bytes.is_empty() {
        return Err(malformed("section is missing"));
    }
    let (parameter_delimiter, mut cursor) = first_delimiter(&bytes)?;
    let (record_value, next, _) = delimited_value(&bytes, cursor, parameter_delimiter, None)?;
    cursor = next;
    let record_delimiter = match record_value {
        Value::Omitted => b';',
        Value::String(value) if value.len() == 1 => value[0],
        Value::String(_) | Value::Atom(_) => {
            return Err(malformed("record delimiter must contain one byte"));
        }
        Value::ForbiddenString => {
            return Err(malformed(
                "record delimiter is a non-ASCII or control character",
            ));
        }
    };

    let mut values = vec![
        Value::String(vec![parameter_delimiter]),
        Value::String(vec![record_delimiter]),
    ];
    loop {
        let (value, next, ended) =
            delimited_value(&bytes, cursor, parameter_delimiter, Some(record_delimiter))?;
        values.push(value);
        cursor = next;
        if ended {
            break;
        }
    }
    Ok(RawGlobal {
        parameter_delimiter,
        record_delimiter,
        values,
    })
}

/// Recover the Global field boundaries, then resolve every field once.
pub(crate) fn parse(scan: &CardScan) -> Result<(ResolvedGlobal, Vec<LossNote>), CodecError> {
    Ok(resolve(parse_raw(scan)?))
}

fn date_value_is_valid(bytes: &[u8], accepts_four_digit_date: bool) -> bool {
    let dot = match bytes.len() {
        13 => 6,
        15 if accepts_four_digit_date => 8,
        _ => return false,
    };
    if bytes.get(dot) != Some(&b'.')
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != dot && !byte.is_ascii_digit())
    {
        return false;
    }
    let number = |start: usize, end: usize| {
        std::str::from_utf8(&bytes[start..end])
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
    };
    let (month_start, day_start, hour_start, minute_start, second_start) = if dot == 6 {
        (2, 4, 7, 9, 11)
    } else {
        (4, 6, 9, 11, 13)
    };
    number(month_start, month_start + 2).is_some_and(|month| (1..=12).contains(&month))
        && number(day_start, day_start + 2).is_some_and(|day| (1..=31).contains(&day))
        && number(hour_start, hour_start + 2).is_some_and(|hour| hour < 24)
        && number(minute_start, minute_start + 2).is_some_and(|minute| minute < 60)
        && number(second_start, second_start + 2).is_some_and(|second| second < 60)
}

const fn effective_version(declared: i64) -> (i64, &'static str) {
    match declared {
        1 => (1, "1.0"),
        2 => (2, "ANSI-Y14.26M-1981"),
        4 => (4, "3.0"),
        5 => (5, "ASME-ANSI-Y14.26M-1987"),
        6 => (6, "4.0"),
        7 => (7, "ASME-Y14.26M-1989"),
        8 => (8, "5.0"),
        9 => (9, "5.1"),
        10 => (10, "5.2"),
        value if value >= 11 => (11, "5.3"),
        _ => (3, "2.0"),
    }
}

fn delegated_unit_factor_mm(name: &str) -> Option<f64> {
    match name.as_bytes() {
        b"A" => Some(0.000_000_1),
        b"in" => Some(25.4),
        b"ft" => Some(304.8),
        b"mi" => Some(1_609_344.0),
        b"mil" => Some(0.0254),
        b"uin" => Some(0.000_025_4),
        b"yd" => Some(914.4),
        b"nmi" => Some(1_852_000.0),
        b"dam" => Some(10_000.0),
        b"hm" => Some(100_000.0),
        b"km" => Some(1_000_000.0),
        b"Mm" => Some(1_000_000_000.0),
        b"Gm" => Some(1_000_000_000_000.0),
        b"Tm" => Some(1_000_000_000_000_000.0),
        b"Pm" => Some(1_000_000_000_000_000_000.0),
        b"Em" => Some(1_000_000_000_000_000_000_000.0),
        b"m" => Some(1_000.0),
        b"dm" => Some(100.0),
        b"cm" => Some(10.0),
        b"mm" => Some(1.0),
        b"um" => Some(0.001),
        b"nm" => Some(0.000_001),
        b"pm" => Some(0.000_000_001),
        b"fm" => Some(0.000_000_000_001),
        b"am" => Some(0.000_000_000_000_001),
        _ => None,
    }
}

const fn enumerated_unit_factor_mm(flag: i64) -> Option<f64> {
    match flag {
        1 => Some(25.4),
        2 => Some(1.0),
        4 => Some(304.8),
        5 => Some(1_609_344.0),
        6 => Some(1_000.0),
        7 => Some(1_000_000.0),
        8 => Some(0.0254),
        9 => Some(0.001),
        10 => Some(10.0),
        11 => Some(0.000_025_4),
        _ => None,
    }
}

struct Resolution {
    values: Vec<Value>,
    losses: Vec<LossNote>,
}

impl Resolution {
    fn value(&self, index: usize) -> &Value {
        self.values.get(index).unwrap_or(&Value::Omitted)
    }

    fn charge(&mut self, code: IgesLossCode, index: usize, defect: Defect, consequence: &str) {
        self.losses.push(code.note(format!(
            "IGES Global field {} ({}) is {}; {consequence}",
            index + 1,
            field_name(index),
            defect.as_str()
        )));
    }

    fn declaration_text(&self, index: usize) -> String {
        match self.value(index) {
            Value::Omitted => String::new(),
            Value::String(bytes) | Value::Atom(bytes) => {
                String::from_utf8_lossy(bytes).into_owned()
            }
            Value::ForbiddenString => {
                "a payload carrying a byte IGES 5.3 section 2.2.2.3 forbids".into()
            }
        }
    }

    fn supplied_string(&self, index: usize) -> Supplied<String> {
        match self.value(index) {
            Value::Omitted => Supplied::Absent,
            Value::String(bytes) if bytes.is_empty() => Supplied::Absent,
            Value::String(bytes) => {
                String::from_utf8(bytes.clone()).map_or(Supplied::Malformed, Supplied::Value)
            }
            Value::ForbiddenString | Value::Atom(_) => Supplied::Malformed,
        }
    }

    fn supplied_date(&self, index: usize, dialect: Dialect) -> Supplied<String> {
        match self.supplied_string(index) {
            Supplied::Value(text)
                if date_value_is_valid(text.as_bytes(), dialect.accepts_four_digit_date()) =>
            {
                Supplied::Value(text)
            }
            Supplied::Value(_) => Supplied::Malformed,
            Supplied::Absent => Supplied::Absent,
            Supplied::Malformed => Supplied::Malformed,
        }
    }

    fn supplied_integer(&self, index: usize) -> Supplied<i64> {
        match self.value(index) {
            Value::Omitted => Supplied::Absent,
            Value::Atom(bytes) => std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| text.trim().parse::<i64>().ok())
                .map_or(Supplied::Malformed, Supplied::Value),
            Value::String(_) | Value::ForbiddenString => Supplied::Malformed,
        }
    }

    fn supplied_real(&self, index: usize) -> Supplied<f64> {
        match self.value(index) {
            Value::Omitted => Supplied::Absent,
            Value::Atom(bytes) => std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| text.trim().replace(['D', 'd'], "E").parse::<f64>().ok())
                .filter(|value| value.is_finite())
                .map_or(Supplied::Malformed, Supplied::Value),
            Value::String(_) | Value::ForbiddenString => Supplied::Malformed,
        }
    }

    fn metadata_string(&mut self, index: usize) -> Option<String> {
        match self.supplied_string(index) {
            Supplied::Absent => None,
            Supplied::Value(text) => Some(text),
            Supplied::Malformed => {
                self.charge(
                    IgesLossCode::GlobalMetadataFieldUnusable,
                    index,
                    Defect::Malformed,
                    METADATA_CONSEQUENCE,
                );
                None
            }
        }
    }

    fn metadata_date(&mut self, index: usize, dialect: Dialect) {
        if matches!(self.supplied_date(index, dialect), Supplied::Malformed) {
            self.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                index,
                Defect::Malformed,
                METADATA_CONSEQUENCE,
            );
        }
    }

    fn metadata_integer(&mut self, index: usize, admits: fn(i64) -> bool) {
        let admitted = match self.supplied_integer(index) {
            Supplied::Absent => true,
            Supplied::Value(value) => admits(value),
            Supplied::Malformed => false,
        };
        if !admitted {
            self.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                index,
                Defect::Malformed,
                METADATA_CONSEQUENCE,
            );
        }
    }

    fn maximum_coordinate(&mut self) -> Option<f64> {
        match self.supplied_real(FIELD_MAXIMUM_COORDINATE) {
            Supplied::Absent => Some(0.0),
            Supplied::Value(value) if value >= 0.0 => Some(value),
            Supplied::Value(_) | Supplied::Malformed => {
                self.charge(
                    IgesLossCode::GlobalMetadataFieldUnusable,
                    FIELD_MAXIMUM_COORDINATE,
                    Defect::Malformed,
                    METADATA_CONSEQUENCE,
                );
                None
            }
        }
    }

    fn significance(&mut self, index: usize, dialect: Dialect) -> u32 {
        let defect = match self.supplied_integer(index) {
            Supplied::Absent if matches!(dialect, Dialect::V4_0) => return 0,
            Supplied::Absent => Defect::Absent,
            Supplied::Value(value) => match u32::try_from(value).ok().filter(|value| *value > 0) {
                Some(value) => return value,
                None => Defect::Malformed,
            },
            Supplied::Malformed => Defect::Malformed,
        };
        self.charge(
            IgesLossCode::GlobalSemanticContextSubstituted,
            index,
            defect,
            SIGNIFICANCE_CONSEQUENCE,
        );
        FALLBACK_SIGNIFICANCE
    }

    fn minimum_resolution(&mut self, dialect: Dialect) -> f64 {
        match self.supplied_real(FIELD_MINIMUM_RESOLUTION) {
            Supplied::Absent if matches!(dialect, Dialect::V4_0) => 0.0,
            Supplied::Absent => FALLBACK_MINIMUM_RESOLUTION,
            Supplied::Value(value) if value >= 0.0 => value,
            Supplied::Value(_) | Supplied::Malformed => {
                self.charge(
                    IgesLossCode::GlobalSemanticContextSubstituted,
                    FIELD_MINIMUM_RESOLUTION,
                    Defect::Malformed,
                    RESOLUTION_CONSEQUENCE,
                );
                FALLBACK_MINIMUM_RESOLUTION
            }
        }
    }

    fn line_weight_scale(&mut self, dialect: Dialect) -> Option<LineWeightScale> {
        let (gradations, gradations_defect) = match self
            .supplied_integer(FIELD_LINE_WEIGHT_GRADATIONS)
        {
            Supplied::Absent if matches!(dialect, Dialect::V4_0) => (None, Some(Defect::Absent)),
            Supplied::Absent => (Some(1), None),
            Supplied::Value(value)
                if value > 0 && (dialect != Dialect::V4_0 || value <= 32_768) =>
            {
                (Some(value), None)
            }
            Supplied::Value(_) | Supplied::Malformed => (None, Some(Defect::Malformed)),
        };
        let (maximum_width, width_defect) = match self.supplied_real(FIELD_MAXIMUM_LINE_WIDTH) {
            Supplied::Absent => (None, Some(Defect::Absent)),
            Supplied::Value(value) if value > 0.0 => (Some(value), None),
            Supplied::Value(_) | Supplied::Malformed => (None, Some(Defect::Malformed)),
        };
        if let Some((index, defect)) = [
            (FIELD_LINE_WEIGHT_GRADATIONS, gradations_defect),
            (FIELD_MAXIMUM_LINE_WIDTH, width_defect),
        ]
        .into_iter()
        .find_map(|(index, defect)| defect.map(|defect| (index, defect)))
        {
            self.charge(
                IgesLossCode::LineWeightScaleUnavailable,
                index,
                defect,
                LINE_WEIGHT_CONSEQUENCE,
            );
        }
        Some(LineWeightScale {
            gradations: gradations?,
            maximum_width: maximum_width?,
        })
    }

    fn length_unit(&mut self, dialect: Dialect) -> (Option<i64>, Option<String>, Option<f64>) {
        let (scale, scale_defect) = match self.supplied_real(FIELD_MODEL_SCALE) {
            Supplied::Absent => (dialect.default_model_scale(), None),
            Supplied::Value(value) if value > 0.0 => (Some(value), None),
            Supplied::Value(_) | Supplied::Malformed => (None, Some(Defect::Malformed)),
        };
        let (units_flag, flag_defect) = match self.supplied_integer(FIELD_UNITS_FLAG) {
            Supplied::Absent => (dialect.default_units_flag(), None),
            Supplied::Value(value) if (1..=11).contains(&value) => (Some(value), None),
            Supplied::Value(_) | Supplied::Malformed => (None, Some(Defect::Malformed)),
        };
        let (units_name, unit_mm, name_defect) = if units_flag == Some(3) {
            match self.supplied_string(FIELD_UNITS_NAME) {
                Supplied::Absent => (None, None, Some(Defect::Absent)),
                Supplied::Value(name) => match delegated_unit_factor_mm(&name) {
                    Some(factor) => (Some(name), Some(factor), None),
                    None => (Some(name), None, Some(Defect::Malformed)),
                },
                Supplied::Malformed => (None, None, Some(Defect::Malformed)),
            }
        } else {
            let name = self.metadata_string(FIELD_UNITS_NAME);
            (name, units_flag.and_then(enumerated_unit_factor_mm), None)
        };
        let length_factor_mm = match (unit_mm, scale) {
            (Some(unit), Some(scale)) => {
                Some(unit / scale).filter(|factor| factor.is_finite() && *factor > 0.0)
            }
            _ => None,
        };
        if length_factor_mm.is_none() {
            match [
                (FIELD_MODEL_SCALE, scale_defect),
                (FIELD_UNITS_FLAG, flag_defect),
                (FIELD_UNITS_NAME, name_defect),
            ]
            .into_iter()
            .find_map(|(index, defect)| defect.map(|defect| (index, defect)))
            {
                Some((index, defect)) => self.charge(
                    IgesLossCode::GlobalLengthUnitUnresolved,
                    index,
                    defect,
                    LENGTH_CONSEQUENCE,
                ),
                None => self
                    .losses
                    .push(IgesLossCode::GlobalLengthUnitUnresolved.note(format!(
                        "IGES Global fields {} ({}) and {} ({}) produce no finite positive millimetre length factor; {LENGTH_CONSEQUENCE}",
                        FIELD_MODEL_SCALE + 1,
                        field_name(FIELD_MODEL_SCALE),
                        FIELD_UNITS_FLAG + 1,
                        field_name(FIELD_UNITS_FLAG),
                    ))),
            }
        }
        (units_flag, units_name, length_factor_mm)
    }
}

fn resolve(raw: RawGlobal) -> (ResolvedGlobal, Vec<LossNote>) {
    let RawGlobal {
        parameter_delimiter,
        record_delimiter,
        values,
    } = raw;
    let field_count = values.len();
    let mut resolution = Resolution {
        values,
        losses: Vec::new(),
    };

    let (declared_version_flag, unreadable_version_declaration) =
        match resolution.supplied_integer(FIELD_VERSION_FLAG) {
            Supplied::Absent => (3, None),
            Supplied::Value(value) => (value, None),
            Supplied::Malformed => (3, Some(resolution.declaration_text(FIELD_VERSION_FLAG))),
        };
    let effective_flag = effective_version(declared_version_flag).0;
    let dialect = Dialect::from_effective_flag(effective_flag);
    let global_field_count = dialect.global_field_count();

    for (index, byte) in [(0_usize, parameter_delimiter), (1_usize, record_delimiter)] {
        if prohibited_delimiter_byte(byte) {
            resolution
                .losses
                .push(IgesLossCode::GlobalNoncanonicalFraming.note(format!(
                    "IGES Global field {} ({}) declares the byte {:?} that IGES 5.3 section 2.2.3.1 prohibits; the declaration was honored for every later field",
                    index + 1,
                    field_name(index),
                    char::from(byte),
                )));
        }
    }
    if field_count > global_field_count {
        resolution
            .losses
            .push(IgesLossCode::GlobalNoncanonicalFraming.note(format!(
                "IGES Global record has {field_count} fields; IGES {} Table 1 defines {global_field_count} and the decoder ignored the rest",
                effective_version(declared_version_flag).1,
            )));
    }

    let sender_product = resolution.metadata_string(FIELD_SENDER_PRODUCT);
    let native_file_name = resolution.metadata_string(FIELD_FILE_NAME);
    resolution.metadata_string(FIELD_NATIVE_SYSTEM);
    resolution.metadata_string(FIELD_PREPROCESSOR_VERSION);
    resolution.metadata_integer(FIELD_INTEGER_BITS, |_| true);
    resolution.metadata_integer(FIELD_SINGLE_MAGNITUDE, |_| true);
    let single_significance = resolution.significance(FIELD_SINGLE_SIGNIFICANCE, dialect);
    resolution.metadata_integer(FIELD_DOUBLE_MAGNITUDE, |_| true);
    let double_significance = resolution.significance(FIELD_DOUBLE_SIGNIFICANCE, dialect);
    resolution.metadata_string(FIELD_RECEIVER_PRODUCT);
    let (units_flag, units_name, length_factor_mm) = resolution.length_unit(dialect);
    #[cfg(not(test))]
    let _ = units_flag;
    let line_weight_scale = resolution.line_weight_scale(dialect);
    resolution.metadata_date(FIELD_GENERATION_DATE, dialect);
    let minimum_resolution = resolution.minimum_resolution(dialect);
    #[cfg(test)]
    let maximum_coordinate = resolution.maximum_coordinate();
    #[cfg(not(test))]
    let _ = resolution.maximum_coordinate();
    resolution.metadata_string(FIELD_AUTHOR);
    resolution.metadata_string(FIELD_ORGANIZATION);
    resolution.metadata_integer(FIELD_DRAFTING_STANDARD, |value| (0..=7).contains(&value));
    if dialect.has_model_date() {
        resolution.metadata_date(FIELD_MODEL_DATE, dialect);
    }
    if dialect.has_application_protocol() {
        resolution.metadata_string(FIELD_APPLICATION_PROTOCOL);
    }

    let resolved = ResolvedGlobal {
        parameter_delimiter,
        record_delimiter,
        sender_product,
        native_file_name,
        units_name,
        #[cfg(test)]
        units_flag,
        precision: RealPrecision {
            single_significance,
            double_significance,
        },
        minimum_resolution,
        #[cfg(test)]
        maximum_coordinate,
        length_factor_mm,
        line_weight_scale,
        declared_version_flag,
        unreadable_version_declaration,
        dialect,
    };
    (resolved, resolution.losses)
}

impl ResolvedGlobal {
    /// The projection view, present only when the millimetre length factor resolved.
    pub(crate) fn length_context(&self) -> Option<ProjectedGlobal> {
        let length_factor_mm = self.length_factor_mm?;
        Some(ProjectedGlobal {
            length_factor_mm,
            minimum_resolution_mm: self.minimum_resolution * length_factor_mm,
            precision: self.precision,
            line_weight_scale: self.line_weight_scale,
            dialect: self.dialect,
        })
    }

    pub(crate) fn real_precision(&self) -> RealPrecision {
        self.precision
    }

    #[cfg(test)]
    pub(crate) fn units_flag(&self) -> Option<i64> {
        self.units_flag
    }

    pub(crate) fn sender_product(&self) -> Option<String> {
        self.sender_product.clone()
    }

    pub(crate) fn native_file_name(&self) -> Option<String> {
        self.native_file_name.clone()
    }

    pub(crate) fn units_name(&self) -> Option<String> {
        self.units_name.clone()
    }

    #[cfg(test)]
    pub(crate) fn maximum_coordinate_mm(&self) -> Option<f64> {
        Some(self.maximum_coordinate? * self.length_factor_mm?)
    }

    /// The version flag as declared, with the specification default for an absent field.
    pub(crate) fn declared_version_flag(&self) -> i64 {
        self.declared_version_flag
    }

    /// The declaration text of a field 23 that does not read as an integer.
    fn unreadable_version_declaration(&self) -> Option<&str> {
        self.unreadable_version_declaration.as_deref()
    }

    /// The declared version flag after the specification's postprocessor clamp.
    fn effective_version_flag(&self) -> i64 {
        effective_version(self.declared_version_flag).0
    }

    pub(crate) fn version(&self) -> &'static str {
        match self.dialect {
            Dialect::V4_0 => "4.0",
            Dialect::V5_0 => "5.0",
            Dialect::V5_1 => "5.1",
            Dialect::V5_2 => "5.2",
            Dialect::V5_3 => "5.3",
            Dialect::Legacy => effective_version(self.declared_version_flag).1,
        }
    }

    /// The loss charged when field 23 does not name a verified specification version.
    ///
    /// It is `None` only for a readable, unclamped flag whose effective version
    /// is one this codec verified against that version's own specification.
    pub(crate) fn dialect_loss(&self) -> Option<LossNote> {
        let declared = self.declared_version_flag;
        let effective = self.effective_version_flag();
        let version = self.version();
        let clamped = declared != effective;
        let unreadable = self.unreadable_version_declaration();
        if !clamped && unreadable.is_none() && VERIFIED_VERSIONS.contains(&version) {
            return None;
        }
        let declaration = match unreadable {
            Some(text) => format!(
                "IGES Global field 23 (version flag) is malformed: the declaration {text} does not read as an integer, so the specification default {declared}"
            ),
            None => format!("IGES Global version flag {declared}"),
        };
        let clamp = if clamped {
            format!(
                " after the clamp to {effective} that IGES 5.3 section 2.2.4.3.23 requires of a postprocessor"
            )
        } else {
            String::new()
        };
        Some(IgesLossCode::SourceDialectUnverified.note(format!(
            "{declaration} names effective specification version {version}{clamp}; this decode interpreted the file with the semantics verified for versions {}",
            VERIFIED_VERSIONS.join(", ")
        )))
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
        if self.declared_version_flag != self.effective_version_flag() {
            notes.push(format!("iges_version_flag={}", self.declared_version_flag));
        }
        notes
    }
}

impl ProjectedGlobal {
    pub(crate) fn dialect(&self) -> Dialect {
        self.dialect
    }

    pub(crate) fn length_factor_mm(&self) -> f64 {
        self.length_factor_mm
    }

    pub(crate) fn minimum_resolution_mm(&self) -> f64 {
        self.minimum_resolution_mm
    }

    pub(crate) fn real_precision(&self) -> RealPrecision {
        self.precision
    }

    pub(crate) fn single_precision_significance(&self) -> u32 {
        self.precision.single_significance
    }

    pub(crate) fn line_weight_mm(&self, number: i64) -> Option<f64> {
        let scale = self.line_weight_scale?;
        (number > 0 && number <= scale.gradations).then_some(
            number as f64 * scale.maximum_width * self.length_factor_mm / scale.gradations as f64,
        )
    }
}

#[cfg(test)]
mod tests;
