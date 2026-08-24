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
    Malformed(Vec<u8>),
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

    const fn defaults_receiver_product_to_sender(self) -> bool {
        matches!(self, Self::V5_0 | Self::V5_1 | Self::V5_2 | Self::V5_3)
    }

    const fn defaults_units_name(self) -> bool {
        matches!(self, Self::V5_0 | Self::V5_1 | Self::V5_2 | Self::V5_3)
    }

    const fn string_byte_is_forbidden(self, byte: u8) -> bool {
        !byte.is_ascii() || (byte.is_ascii_control() && !matches!(self, Self::V4_0))
    }

    /// Whether an empty field has no specification default in this dialect.
    ///
    /// The later profiles apply the data-type implicit defaults to their
    /// required-no-default fields. V4.0 predates that rule and names only
    /// fields 1, 2, and 23 as defaulted. V5.0's Recommended Practices Guide
    /// instead marks its unconditional required fields explicitly; conditional
    /// requirements remain with the consumer that can observe the condition.
    const fn field_requires_value(self, index: usize) -> bool {
        match self {
            Self::V4_0 => {
                index < self.global_field_count() && !matches!(index, 0 | 1 | FIELD_VERSION_FLAG)
            }
            Self::V5_0 => matches!(
                index,
                FIELD_SENDER_PRODUCT
                    | FIELD_NATIVE_SYSTEM
                    | FIELD_PREPROCESSOR_VERSION
                    | FIELD_INTEGER_BITS
                    | FIELD_SINGLE_MAGNITUDE
                    | FIELD_SINGLE_SIGNIFICANCE
                    | FIELD_UNITS_FLAG
                    | FIELD_GENERATION_DATE
                    | FIELD_MINIMUM_RESOLUTION
            ),
            Self::Legacy | Self::V5_1 | Self::V5_2 | Self::V5_3 => false,
        }
    }
}

/// Global field values, fallbacks, and absences after one resolution pass.
#[derive(Debug)]
pub(crate) struct ResolvedGlobal {
    pub(crate) parameter_delimiter: u8,
    pub(crate) record_delimiter: u8,
    sender_product: Option<String>,
    receiver_product: Option<String>,
    native_file_name: Option<String>,
    units_name: Option<String>,
    #[cfg(test)]
    units_flag: Option<i64>,
    precision: RealPrecision,
    numeric_limits: NumericLimits,
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
    mode: LineWeightMode,
}

#[derive(Debug, Clone, Copy)]
enum LineWeightMode {
    Absolute { maximum_width: f64 },
    Relative,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RealPrecision {
    pub(crate) single_significance: u32,
    pub(crate) double_significance: u32,
}

/// Sender numeric range capabilities declared by Global fields 7, 8, and 10.
///
/// An absent or unusable declaration is `None`. It cannot justify a numeric
/// admission, but it also does not create a substitute sender capability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NumericLimits {
    pub(crate) integer_bits: Option<u32>,
    pub(crate) single_magnitude: Option<i64>,
    pub(crate) double_magnitude: Option<i64>,
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

fn layout_hollerith(bytes: &[u8], start: usize) -> Result<Option<(usize, usize)>, CodecError> {
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
        .ok_or_else(|| malformed("Hollerith payload offset overflows"))?;
    let payload_end = payload_start
        .checked_add(count)
        .ok_or_else(|| malformed("Hollerith payload length overflows"))?;
    bytes
        .get(payload_start..payload_end)
        .ok_or_else(|| malformed("Hollerith payload is truncated"))?;
    Ok(Some((cursor + 1, payload_end)))
}

/// Lay out a logical Global stream into 72-column card payloads.
///
/// Spaces inserted before a field are ignored by the Global reader. They let
/// generated output keep a Hollerith count and `H` on one card and keep every
/// non-string field together with its delimiter. Hollerith payload bytes may
/// cross cards.
pub(crate) fn layout_global_cards(bytes: &[u8]) -> Result<Vec<Vec<u8>>, CodecError> {
    let (parameter_delimiter, mut cursor) = if bytes.first() == Some(&b',') {
        (b',', 1)
    } else {
        let Some((header_end, payload_end)) = layout_hollerith(bytes, 0)? else {
            return Err(malformed("parameter delimiter is not a Hollerith string"));
        };
        let payload = bytes
            .get(header_end..payload_end)
            .ok_or_else(|| malformed("parameter delimiter is truncated"))?;
        if payload.len() != 1 {
            return Err(malformed("parameter delimiter must contain one byte"));
        }
        let delimiter = payload[0];
        if bytes.get(payload_end) != Some(&delimiter) {
            return Err(malformed(
                "parameter delimiter does not terminate its Global field",
            ));
        }
        (delimiter, payload_end + 1)
    };

    let record_delimiter = if bytes.get(cursor) == Some(&parameter_delimiter) {
        cursor += 1;
        b';'
    } else {
        let Some((header_end, payload_end)) = layout_hollerith(bytes, cursor)? else {
            return Err(malformed("record delimiter is not a Hollerith string"));
        };
        let payload = bytes
            .get(header_end..payload_end)
            .ok_or_else(|| malformed("record delimiter is truncated"))?;
        if payload.len() != 1 {
            return Err(malformed("record delimiter must contain one byte"));
        }
        let delimiter = payload[0];
        if bytes.get(payload_end) != Some(&parameter_delimiter) {
            return Err(malformed(
                "record delimiter does not terminate its Global field",
            ));
        }
        cursor = payload_end + 1;
        delimiter
    };

    let mut fields = Vec::with_capacity(1);
    fields.push(0..cursor);
    while cursor < bytes.len() {
        let start = cursor;
        let mut end = cursor;
        if let Some((_, payload_end)) = layout_hollerith(bytes, cursor)? {
            end = payload_end;
        }
        while end < bytes.len()
            && bytes[end] != parameter_delimiter
            && bytes[end] != record_delimiter
        {
            end += 1;
        }
        let is_record = bytes
            .get(end)
            .ok_or_else(|| malformed("Global record delimiter is missing"))?
            == &record_delimiter;
        end += 1;
        fields.push(start..end);
        cursor = end;
        if is_record {
            break;
        }
    }

    let mut cards = Vec::new();
    let mut card = Vec::with_capacity(72);
    for field in fields.iter().map(|range| &bytes[range.clone()]) {
        let leading = field
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(field.len());
        let header_end = if leading < field.len() {
            layout_hollerith(field, leading)?.map(|(header_end, _)| header_end)
        } else {
            None
        };
        let minimum = header_end.map_or(field.len(), |end| leading + end);
        if minimum > 72 {
            return Err(malformed("Global field exceeds one card"));
        }
        if card.len() + minimum > 72 {
            card.resize(72, b' ');
            cards.push(std::mem::take(&mut card));
            card = Vec::with_capacity(72);
        }
        for byte in field.iter().copied() {
            if card.len() == 72 {
                cards.push(std::mem::take(&mut card));
                card = Vec::with_capacity(72);
            }
            card.push(byte);
        }
    }
    if !card.is_empty() {
        cards.push(card);
    }
    Ok(cards)
}

fn field_name(index: usize) -> &'static str {
    FIELD_NAMES.get(index).copied().unwrap_or("extra field")
}

const fn prohibited_delimiter_byte(byte: u8) -> bool {
    byte.is_ascii_control()
        || !byte.is_ascii()
        || byte.is_ascii_digit()
        || matches!(byte, b' ' | b'+' | b'-' | b'.' | b'D' | b'E' | b'H')
}

struct GlobalStream {
    bytes: Vec<u8>,
    source_cards: Vec<usize>,
}

type GlobalHollerith = (Vec<u8>, usize, bool);

fn source_span_crosses_card(source_cards: &[usize], start: usize, end: usize) -> bool {
    source_cards
        .get(start..end)
        .is_some_and(|span| span.windows(2).any(|cards| cards[0] != cards[1]))
}

fn hollerith(
    bytes: &[u8],
    source_cards: &[usize],
    start: usize,
) -> Result<Option<GlobalHollerith>, CodecError> {
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
    let header_crosses_card = source_span_crosses_card(source_cards, start, cursor + 1);
    Ok(Some((payload.to_vec(), payload_end, header_crosses_card)))
}

fn first_delimiter(stream: &GlobalStream) -> Result<(u8, usize), CodecError> {
    if stream.bytes.first() == Some(&b',') {
        return Ok((b',', 1));
    }
    let Some((payload, cursor, header_crosses_card)) =
        hollerith(&stream.bytes, &stream.source_cards, 0)?
    else {
        return Err(malformed("parameter delimiter is not a Hollerith string"));
    };
    if header_crosses_card {
        return Err(malformed(
            "parameter delimiter is not a valid one-card Hollerith string",
        ));
    }
    if payload.len() != 1 {
        return Err(malformed("parameter delimiter must contain one byte"));
    }
    let delimiter = payload[0];
    if stream.bytes.get(cursor) != Some(&delimiter) {
        return Err(malformed(
            "parameter delimiter does not terminate its Global field",
        ));
    }
    Ok((delimiter, cursor + 1))
}

fn delimited_value(
    stream: &GlobalStream,
    start: usize,
    parameter_delimiter: u8,
    record_delimiter: Option<u8>,
) -> Result<(Value, usize, bool), CodecError> {
    let bytes = &stream.bytes;
    let value_start = start
        + bytes[start..]
            .iter()
            .position(|byte| *byte != b' ')
            .unwrap_or(bytes.len().saturating_sub(start));
    if bytes.get(value_start) == Some(&parameter_delimiter) {
        return Ok((Value::Omitted, value_start + 1, false));
    }
    if record_delimiter.is_some_and(|delimiter| bytes.get(value_start) == Some(&delimiter)) {
        return Ok((Value::Omitted, value_start + 1, true));
    }
    let (value, end, allow_padding_after) = if let Some((payload, end, header_crosses_card)) =
        hollerith(bytes, &stream.source_cards, value_start)?
    {
        if header_crosses_card {
            (Value::Malformed(payload), end, true)
        } else {
            (Value::String(payload), end, true)
        }
    } else {
        let end = bytes[value_start..]
            .iter()
            .position(|byte| *byte == parameter_delimiter || record_delimiter == Some(*byte))
            .and_then(|relative| value_start.checked_add(relative))
            .ok_or_else(|| malformed("record delimiter is missing"))?;
        let atom = &bytes[value_start..end];
        if atom.iter().all(|byte| *byte == b' ') {
            (Value::Omitted, end, false)
        } else if source_span_crosses_card(&stream.source_cards, value_start, end + 1) {
            (Value::Malformed(atom.to_vec()), end, false)
        } else {
            (Value::Atom(atom.to_vec()), end, false)
        }
    };
    let separator_start = if allow_padding_after {
        end + bytes[end..]
            .iter()
            .position(|byte| *byte != b' ')
            .unwrap_or(bytes.len().saturating_sub(end))
    } else {
        end
    };
    match bytes.get(separator_start).copied() {
        Some(separator) if separator == parameter_delimiter => {
            Ok((value, separator_start + 1, false))
        }
        Some(separator) if record_delimiter == Some(separator) => {
            Ok((value, separator_start + 1, true))
        }
        _ => Err(malformed("value is not followed by a delimiter")),
    }
}

fn global_bytes(scan: &CardScan<'_>) -> GlobalStream {
    let mut bytes = Vec::new();
    let mut source_cards = Vec::new();

    for (card, line) in scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Global))
        .enumerate()
    {
        for byte in line.payload.iter().take(72).copied() {
            bytes.push(byte);
            source_cards.push(card);
        }
    }
    GlobalStream {
        bytes,
        source_cards,
    }
}

fn parse_raw(scan: &CardScan) -> Result<RawGlobal, CodecError> {
    let stream = global_bytes(scan);
    if stream.bytes.is_empty() {
        return Err(malformed("section is missing"));
    }
    let (parameter_delimiter, mut cursor) = first_delimiter(&stream)?;
    let (record_value, next, _) = delimited_value(&stream, cursor, parameter_delimiter, None)?;
    cursor = next;
    let record_delimiter = match record_value {
        Value::Omitted => b';',
        Value::String(value) if value.len() == 1 => value[0],
        Value::String(_) | Value::Malformed(_) | Value::ForbiddenString | Value::Atom(_) => {
            return Err(malformed("record delimiter must contain one byte"));
        }
    };

    for (name, delimiter) in [
        ("parameter", parameter_delimiter),
        ("record", record_delimiter),
    ] {
        if prohibited_delimiter_byte(delimiter) {
            return Err(malformed(format!(
                "{name} delimiter {:?} is prohibited by IGES section 2.2.3.1",
                char::from(delimiter)
            )));
        }
    }

    let mut values = vec![
        Value::String(vec![parameter_delimiter]),
        Value::String(vec![record_delimiter]),
    ];
    loop {
        let (value, next, ended) =
            delimited_value(&stream, cursor, parameter_delimiter, Some(record_delimiter))?;
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

// The resolver uses the newest version table for declarations outside its
// numeric range. This is decoder recovery policy, not an extension of the
// IGES 4.0 version table: preserve the declaration and report a dialect loss
// whenever the recovery changes it.
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

const fn enumerated_unit_name(flag: i64) -> Option<&'static str> {
    match flag {
        1 => Some("IN"),
        2 => Some("MM"),
        4 => Some("FT"),
        5 => Some("MI"),
        6 => Some("M"),
        7 => Some("KM"),
        8 => Some("MIL"),
        9 => Some("UM"),
        10 => Some("CM"),
        11 => Some("UIN"),
        _ => None,
    }
}

struct Resolution {
    values: Vec<Value>,
    losses: Vec<LossNote>,
}

fn numeric_text(bytes: &[u8]) -> Option<&str> {
    let first = bytes.iter().position(|byte| *byte != b' ')?;
    let text = &bytes[first..];
    (!text.iter().any(u8::is_ascii_whitespace))
        .then(|| std::str::from_utf8(text).ok())
        .flatten()
}

impl Resolution {
    fn apply_string_policy(&mut self, dialect: Dialect) {
        for value in &mut self.values {
            let Value::String(bytes) = value else {
                continue;
            };
            if bytes
                .iter()
                .copied()
                .any(|byte| dialect.string_byte_is_forbidden(byte))
            {
                *value = Value::ForbiddenString;
            }
        }
    }

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
            Value::String(bytes) | Value::Malformed(bytes) | Value::Atom(bytes) => {
                String::from_utf8_lossy(bytes).into_owned()
            }
            Value::ForbiddenString => {
                "a string payload contains a byte forbidden by the declared dialect".into()
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
            Value::Malformed(_) | Value::ForbiddenString | Value::Atom(_) => Supplied::Malformed,
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
            Value::Atom(bytes) => numeric_text(bytes)
                .and_then(|text| text.parse::<i64>().ok())
                .map_or(Supplied::Malformed, Supplied::Value),
            Value::String(_) | Value::Malformed(_) | Value::ForbiddenString => Supplied::Malformed,
        }
    }

    fn supplied_real(&self, index: usize) -> Supplied<f64> {
        match self.value(index) {
            Value::Omitted => Supplied::Absent,
            Value::Atom(bytes) => numeric_text(bytes)
                .and_then(|text| text.replace(['D', 'd'], "E").parse::<f64>().ok())
                .filter(|value| value.is_finite())
                .map_or(Supplied::Malformed, Supplied::Value),
            Value::String(_) | Value::Malformed(_) | Value::ForbiddenString => Supplied::Malformed,
        }
    }

    fn metadata_string(&mut self, index: usize, dialect: Dialect) -> Option<String> {
        match self.supplied_string(index) {
            Supplied::Absent if dialect.field_requires_value(index) => {
                self.charge(
                    IgesLossCode::GlobalMetadataFieldUnusable,
                    index,
                    Defect::Absent,
                    METADATA_CONSEQUENCE,
                );
                None
            }
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
        let supplied = self.supplied_date(index, dialect);
        if matches!(&supplied, Supplied::Absent) && dialect.field_requires_value(index) {
            self.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                index,
                Defect::Absent,
                METADATA_CONSEQUENCE,
            );
        } else if matches!(&supplied, Supplied::Malformed) {
            self.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                index,
                Defect::Malformed,
                METADATA_CONSEQUENCE,
            );
        }
    }

    fn metadata_integer(&mut self, index: usize, dialect: Dialect, admits: fn(i64) -> bool) {
        let _ = self.metadata_integer_value(index, dialect, admits);
    }

    fn metadata_integer_value(
        &mut self,
        index: usize,
        dialect: Dialect,
        admits: fn(i64) -> bool,
    ) -> Option<i64> {
        let supplied = self.supplied_integer(index);
        let absent = matches!(&supplied, Supplied::Absent);
        let admitted = match &supplied {
            Supplied::Absent => !dialect.field_requires_value(index),
            Supplied::Value(value) => admits(*value),
            Supplied::Malformed => false,
        };
        if !admitted {
            self.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                index,
                if absent {
                    Defect::Absent
                } else {
                    Defect::Malformed
                },
                METADATA_CONSEQUENCE,
            );
        }
        match supplied {
            Supplied::Value(value) if admitted => Some(value),
            Supplied::Absent | Supplied::Malformed | Supplied::Value(_) => None,
        }
    }

    fn maximum_coordinate(&mut self, dialect: Dialect) -> Option<f64> {
        match self.supplied_real(FIELD_MAXIMUM_COORDINATE) {
            Supplied::Absent if dialect == Dialect::V5_0 => None,
            Supplied::Absent if dialect == Dialect::V4_0 => {
                self.charge(
                    IgesLossCode::GlobalMetadataFieldUnusable,
                    FIELD_MAXIMUM_COORDINATE,
                    Defect::Absent,
                    METADATA_CONSEQUENCE,
                );
                None
            }
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

    fn significance(&mut self, index: usize) -> u32 {
        let defect = match self.supplied_integer(index) {
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
            Supplied::Absent if matches!(dialect, Dialect::V4_0 | Dialect::V5_0) => {
                self.charge(
                    IgesLossCode::GlobalSemanticContextSubstituted,
                    FIELD_MINIMUM_RESOLUTION,
                    Defect::Absent,
                    RESOLUTION_CONSEQUENCE,
                );
                FALLBACK_MINIMUM_RESOLUTION
            }
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
        let supplied_gradations = self.supplied_integer(FIELD_LINE_WEIGHT_GRADATIONS);
        let gradations_was_supplied = !matches!(&supplied_gradations, Supplied::Absent);
        let (gradations, gradations_defect) = match supplied_gradations {
            Supplied::Absent if matches!(dialect, Dialect::V4_0) => (None, Some(Defect::Absent)),
            Supplied::Absent => (Some(1), None),
            Supplied::Value(value)
                if value > 0 && (dialect != Dialect::V4_0 || value <= 32_768) =>
            {
                (Some(value), None)
            }
            Supplied::Value(_) | Supplied::Malformed => (None, Some(Defect::Malformed)),
        };
        let (mode, width_defect) = match self.supplied_real(FIELD_MAXIMUM_LINE_WIDTH) {
            Supplied::Absent if dialect == Dialect::V5_0 && !gradations_was_supplied => {
                (None, None)
            }
            Supplied::Value(0.0) if dialect == Dialect::V5_0 => {
                (Some(LineWeightMode::Relative), None)
            }
            Supplied::Value(value) if value > 0.0 => (
                Some(LineWeightMode::Absolute {
                    maximum_width: value,
                }),
                None,
            ),
            Supplied::Absent => (None, Some(Defect::Absent)),
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
            mode: mode?,
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
            let name = match self.supplied_string(FIELD_UNITS_NAME) {
                Supplied::Absent if dialect.defaults_units_name() => {
                    units_flag.and_then(enumerated_unit_name).map(str::to_owned)
                }
                Supplied::Absent if dialect.field_requires_value(FIELD_UNITS_NAME) => {
                    self.charge(
                        IgesLossCode::GlobalMetadataFieldUnusable,
                        FIELD_UNITS_NAME,
                        Defect::Absent,
                        METADATA_CONSEQUENCE,
                    );
                    None
                }
                Supplied::Absent => None,
                Supplied::Value(name) => Some(name),
                Supplied::Malformed => {
                    self.charge(
                        IgesLossCode::GlobalMetadataFieldUnusable,
                        FIELD_UNITS_NAME,
                        Defect::Malformed,
                        METADATA_CONSEQUENCE,
                    );
                    None
                }
            };
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
    resolution.apply_string_policy(dialect);
    let global_field_count = dialect.global_field_count();

    if field_count > global_field_count {
        resolution
            .losses
            .push(IgesLossCode::GlobalNoncanonicalFraming.note(format!(
                "IGES Global record has {field_count} fields; IGES {} Table 1 defines {global_field_count} and the decoder ignored the rest",
                effective_version(declared_version_flag).1,
            )));
    }

    let sender_product = resolution.metadata_string(FIELD_SENDER_PRODUCT, dialect);
    let native_file_name = resolution.metadata_string(FIELD_FILE_NAME, dialect);
    resolution.metadata_string(FIELD_NATIVE_SYSTEM, dialect);
    resolution.metadata_string(FIELD_PREPROCESSOR_VERSION, dialect);
    let integer_bits = resolution
        .metadata_integer_value(FIELD_INTEGER_BITS, dialect, |_| true)
        .and_then(|value| u32::try_from(value).ok().filter(|value| *value > 0));
    let single_magnitude =
        resolution.metadata_integer_value(FIELD_SINGLE_MAGNITUDE, dialect, |_| true);
    let single_significance = resolution.significance(FIELD_SINGLE_SIGNIFICANCE);
    let double_magnitude =
        resolution.metadata_integer_value(FIELD_DOUBLE_MAGNITUDE, dialect, |_| true);
    let double_significance = resolution.significance(FIELD_DOUBLE_SIGNIFICANCE);
    let receiver_product = match resolution.supplied_string(FIELD_RECEIVER_PRODUCT) {
        Supplied::Absent if dialect.defaults_receiver_product_to_sender() => sender_product.clone(),
        Supplied::Absent if dialect.field_requires_value(FIELD_RECEIVER_PRODUCT) => {
            resolution.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                FIELD_RECEIVER_PRODUCT,
                Defect::Absent,
                METADATA_CONSEQUENCE,
            );
            None
        }
        Supplied::Absent => None,
        Supplied::Value(value) => Some(value),
        Supplied::Malformed => {
            resolution.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                FIELD_RECEIVER_PRODUCT,
                Defect::Malformed,
                METADATA_CONSEQUENCE,
            );
            None
        }
    };
    let (units_flag, units_name, length_factor_mm) = resolution.length_unit(dialect);
    #[cfg(not(test))]
    let _ = units_flag;
    let line_weight_scale = resolution.line_weight_scale(dialect);
    resolution.metadata_date(FIELD_GENERATION_DATE, dialect);
    let minimum_resolution = resolution.minimum_resolution(dialect);
    #[cfg(test)]
    let maximum_coordinate = resolution.maximum_coordinate(dialect);
    #[cfg(not(test))]
    let _ = resolution.maximum_coordinate(dialect);
    resolution.metadata_string(FIELD_AUTHOR, dialect);
    resolution.metadata_string(FIELD_ORGANIZATION, dialect);
    resolution.metadata_integer(FIELD_DRAFTING_STANDARD, dialect, |value| {
        (0..=7).contains(&value)
    });
    if dialect.has_model_date() {
        resolution.metadata_date(FIELD_MODEL_DATE, dialect);
    }
    if dialect.has_application_protocol() {
        resolution.metadata_string(FIELD_APPLICATION_PROTOCOL, dialect);
    }

    let resolved = ResolvedGlobal {
        parameter_delimiter,
        record_delimiter,
        sender_product,
        receiver_product,
        native_file_name,
        units_name,
        #[cfg(test)]
        units_flag,
        precision: RealPrecision {
            single_significance,
            double_significance,
        },
        numeric_limits: NumericLimits {
            integer_bits,
            single_magnitude,
            double_magnitude,
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

    pub(crate) fn numeric_limits(&self) -> NumericLimits {
        self.numeric_limits
    }

    #[cfg(test)]
    pub(crate) fn units_flag(&self) -> Option<i64> {
        self.units_flag
    }

    pub(crate) fn sender_product(&self) -> Option<String> {
        self.sender_product.clone()
    }

    pub(crate) fn receiver_product(&self) -> Option<String> {
        self.receiver_product.clone()
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

    pub(crate) fn dialect(&self) -> Dialect {
        self.dialect
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
        if self.dialect == Dialect::V5_0 {
            if let Some(product) = self.receiver_product() {
                notes.push(format!("receiver_product={product}"));
            }
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
        if !(number > 0 && number <= scale.gradations) {
            return None;
        }
        let LineWeightMode::Absolute { maximum_width } = scale.mode else {
            return None;
        };
        Some(number as f64 * maximum_width * self.length_factor_mm / scale.gradations as f64)
    }

    pub(crate) fn line_weight_number_is_valid(&self, number: i64) -> bool {
        number == 0
            || self
                .line_weight_scale
                .is_some_and(|scale| number > 0 && number <= scale.gradations)
    }
}

#[cfg(test)]
mod tests;
