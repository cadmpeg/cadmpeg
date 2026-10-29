// SPDX-License-Identifier: Apache-2.0
//! Global delimiters, count-driven Hollerith values, units, and metadata.

use crate::card::{CardScan, Section};
use crate::loss::IgesLossCode;
use crate::version::{DialectRecovery, UnverifiedDialectRecovery, VersionFlag};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, PositiveReal};

pub(crate) mod parts;

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

#[derive(Debug, Clone, Copy)]
#[must_use]
enum Supplied<T> {
    Absent,
    Value(T),
    Malformed,
}

impl<T: Copy> Supplied<T> {
    fn value(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Absent | Self::Malformed => None,
        }
    }
}

#[derive(Debug)]
enum VersionDeclaration {
    Exact(VersionFlag),
    Unreadable(String),
    Clamped(i64),
}

impl VersionDeclaration {
    const fn declared_flag(&self) -> i64 {
        match self {
            Self::Exact(version) => version.value(),
            Self::Unreadable(_) => 3,
            Self::Clamped(value) => *value,
        }
    }

    const fn effective_version(&self) -> VersionFlag {
        match self {
            Self::Exact(version) => *version,
            Self::Unreadable(_) => VersionFlag::V2_0,
            Self::Clamped(value) => VersionFlag::effective(*value),
        }
    }

    fn recovery(&self) -> DialectRecovery<'_> {
        match self {
            Self::Unreadable(declaration) => DialectRecovery::Unverified(
                UnverifiedDialectRecovery::UnreadableDeclaration(declaration),
            ),
            Self::Clamped(_) => DialectRecovery::Unverified(UnverifiedDialectRecovery::Clamped),
            Self::Exact(version) => match version.verified_version() {
                Some(_) => DialectRecovery::Verified,
                None => DialectRecovery::Unverified(UnverifiedDialectRecovery::UnverifiedVersion),
            },
        }
    }
}

#[derive(Debug)]
struct NumericDeclarations {
    integer_bits: Option<u32>,
    single_magnitude: Option<i64>,
    double_magnitude: Supplied<i64>,
    single_significance: Option<u32>,
    double_significance: Supplied<u32>,
}

/// The source of the numeric context of a Global section.
#[derive(Debug)]
enum NumericContext {
    /// Global fields 7 to 11 declare the sender numeric context.
    Declared(NumericDeclarations),
    /// The Binary Flag Section supersedes Global fields 7 to 11 (IGES 5.3
    /// Appendix H.2.3), and the decoder ignores them.
    BinaryFlagSection,
}

impl NumericContext {
    fn precision(&self) -> RealPrecision {
        match self {
            Self::Declared(declarations) => declarations.precision(),
            Self::BinaryFlagSection => RealPrecision {
                single_significance: FALLBACK_SIGNIFICANCE,
                double_significance: FALLBACK_SIGNIFICANCE,
            },
        }
    }

    fn limits(&self) -> NumericLimits {
        match self {
            Self::Declared(declarations) => declarations.limits(),
            Self::BinaryFlagSection => NumericLimits::default(),
        }
    }
}

impl NumericDeclarations {
    fn precision(&self) -> RealPrecision {
        RealPrecision {
            single_significance: self.single_significance.unwrap_or(FALLBACK_SIGNIFICANCE),
            double_significance: self
                .double_significance
                .value()
                .unwrap_or(FALLBACK_SIGNIFICANCE),
        }
    }

    fn limits(&self) -> NumericLimits {
        NumericLimits {
            integer_bits: self.integer_bits,
            single_magnitude: self.single_magnitude,
            double_magnitude: self.double_magnitude.value(),
        }
    }
}

enum SuppliedReal {
    Absent,
    Value(FiniteReal),
    Recovered(FiniteReal),
    Malformed,
}

/// Global field boundaries recovered from the card stream, with no values resolved.
#[derive(Debug)]
struct RawGlobal {
    parameter_delimiter: u8,
    record_delimiter: u8,
    values: Vec<Value>,
    field_count: usize,
    unframed: Option<UnframedField>,
}

/// A Global field whose end is unknown. No later field has a boundary, so
/// every later field is absent.
#[derive(Debug)]
struct UnframedField {
    /// The Global field index (0 is field 1).
    field: usize,
    /// The Global stream offset where the field starts.
    offset: usize,
    error: CodecError,
}

/// The effective specification family selected by Global field 23.
///
/// The older declarations remain grouped as `Legacy` until their own
/// specifications are verified. The 4.0 and 5.0 families are separate because
/// their Global tables stop at fields 24 and 25 respectively. Versions 5.1,
/// 5.2, and 5.3 share the same 26-field grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobalTable {
    Legacy,
    V4_0,
    V5_0,
    V5Later,
}

impl GlobalTable {
    const fn global_field_count(self) -> usize {
        match self {
            Self::Legacy | Self::V5Later => 26,
            Self::V4_0 => 24,
            Self::V5_0 => 25,
        }
    }

    const fn accepts_four_digit_date(self) -> bool {
        matches!(self, Self::Legacy | Self::V5Later)
    }

    const fn default_model_scale(self) -> Option<PositiveReal> {
        match self {
            Self::V4_0 => None,
            Self::Legacy | Self::V5_0 | Self::V5Later => Some(PositiveReal::ONE),
        }
    }

    const fn default_units_flag(self) -> Option<i64> {
        match self {
            Self::V4_0 | Self::V5_0 => None,
            Self::Legacy | Self::V5Later => Some(1),
        }
    }

    const fn has_model_date(self) -> bool {
        !matches!(self, Self::V4_0)
    }

    const fn has_application_protocol(self) -> bool {
        matches!(self, Self::Legacy | Self::V5Later)
    }

    const fn defaults_receiver_product_to_sender(self) -> bool {
        matches!(self, Self::V5_0 | Self::V5Later)
    }

    const fn defaults_units_name(self) -> bool {
        matches!(self, Self::V5_0 | Self::V5Later)
    }

    const fn string_byte_is_forbidden(self, byte: u8) -> bool {
        !byte.is_ascii() || (byte.is_ascii_control() && !matches!(self, Self::V4_0))
    }

    /// Whether an empty field has no specification default in this Global table.
    ///
    /// A required-no-default field must contain a supplied value. Its data
    /// type's implicit default does not override that category. V4.0 names
    /// only fields 1, 2, and 23 as defaulted. V5.0's Recommended Practices
    /// Guide marks its unconditional required fields explicitly; conditional
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
            Self::Legacy => false,
            Self::V5Later => matches!(
                index,
                FIELD_SENDER_PRODUCT..=FIELD_DOUBLE_SIGNIFICANCE
                    | FIELD_MAXIMUM_LINE_WIDTH..=FIELD_MINIMUM_RESOLUTION
            ),
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
    numeric: NumericContext,
    minimum_resolution: NonNegativeReal,
    length_factor_mm: Option<PositiveReal>,
    line_weight_scale: Option<LineWeightScale>,
    declaration: VersionDeclaration,
}

/// Length-valued Global view. It exists only when the millimetre factor resolved.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProjectedGlobal {
    length_factor_mm: PositiveReal,
    minimum_resolution_mm: f64,
    precision: RealPrecision,
    line_weight_scale: Option<LineWeightScale>,
    global_table: GlobalTable,
}

#[derive(Debug, Clone, Copy)]
struct LineWeightScale {
    gradations: i64,
    mode: LineWeightMode,
}

#[derive(Debug, Clone, Copy)]
enum LineWeightMode {
    Absolute { maximum_width: PositiveReal },
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

/// The parameter delimiter when Global field 1 is empty or given up: a comma
/// (IGES 5.3 §2.2.4.3.1).
pub(crate) const DEFAULT_PARAMETER_DELIMITER: u8 = b',';

/// The record delimiter when Global field 2 is empty or given up: a
/// semicolon (IGES 5.3 §2.2.4.3.2).
pub(crate) const DEFAULT_RECORD_DELIMITER: u8 = b';';

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
const METADATA_CONSEQUENCE: &str = "its value was not transferred";
const SIGNIFICANCE_CONSEQUENCE: &str =
    "the decoder substituted 17 significant decimal digits from its own specification";
const RESOLUTION_CONSEQUENCE: &str =
    "the decoder substituted 0.0 millimetres as the minimum user-intended resolution";
const REAL_SYNTAX_RECOVERY_CONSEQUENCE: &str =
    "the decoder recovered the finite numeric value and retained the source spelling";
const LENGTH_CONSEQUENCE: &str =
    "the decoder resolved no millimetre length factor, suppressed every geometry projection, and retained the native records";
const LINE_WEIGHT_CONSEQUENCE: &str =
    "the line-weight scale is unavailable, so no entity carries a display width";

fn global_loss_note(
    code: IgesLossCode,
    index: usize,
    defect: Defect,
    consequence: &str,
) -> LossNote {
    let message = format!(
        "IGES Global field {} ({}) is {}; {consequence}",
        index + 1,
        field_name(index),
        defect.as_str()
    );
    admitted_global_loss(code, message)
}

fn admitted_global_loss(code: IgesLossCode, message: String) -> LossNote {
    code.note(message)
}

fn recovered_real_loss_note(index: usize, source: &str, value: f64) -> LossNote {
    let message = format!(
        "IGES Global field {} ({}) uses noncanonical real syntax {source:?}; the \
         decoder recovered finite value {value}; \
         {REAL_SYNTAX_RECOVERY_CONSEQUENCE}",
        index + 1,
        field_name(index)
    );
    admitted_global_loss(IgesLossCode::GlobalNumericSyntaxRecovered, message)
}

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

    let mut fields: Vec<_> = std::iter::once(0..cursor).collect();
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
    let mut card = Vec::new();
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
            card.extend(std::iter::repeat_with(|| b' ').take(72 - card.len()));
            cards.push(std::mem::take(&mut card));
        }
        for byte in field.iter().copied() {
            if card.len() == 72 {
                cards.push(std::mem::take(&mut card));
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

fn source_span_crosses_card(start: usize, end: usize) -> bool {
    end > start && start / 72 != (end - 1) / 72
}

fn hollerith(bytes: &[u8], start: usize) -> Result<Option<(&[u8], usize)>, CodecError> {
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
    Ok(Some((payload, payload_end)))
}

fn first_delimiter(bytes: &[u8]) -> Result<(u8, usize), CodecError> {
    let start = bytes
        .iter()
        .position(|byte| *byte != b' ')
        .unwrap_or(bytes.len());
    if bytes.get(start) == Some(&b',') {
        return Ok((b',', start + 1));
    }
    let Some((payload, cursor)) = hollerith(bytes, start)? else {
        return Err(malformed("parameter delimiter is not a Hollerith string"));
    };
    if source_span_crosses_card(start, cursor - payload.len()) {
        return Err(malformed(
            "parameter delimiter is not a valid one-card Hollerith string",
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

#[derive(Clone, Copy)]
enum ValueKind {
    Omitted,
    String,
    Malformed,
    Atom,
}

struct ValueFrame {
    kind: ValueKind,
    payload: std::ops::Range<usize>,
    next: usize,
    ended: bool,
}

impl ValueFrame {
    fn value(&self, payload: &[u8]) -> Value {
        if matches!(self.kind, ValueKind::Omitted) {
            Value::Omitted
        } else {
            self.owned_value(payload.to_vec())
        }
    }

    fn owned_value(&self, payload: Vec<u8>) -> Value {
        match self.kind {
            ValueKind::Omitted => Value::Omitted,
            ValueKind::String => Value::String(payload),
            ValueKind::Malformed => Value::Malformed(payload),
            ValueKind::Atom => Value::Atom(payload),
        }
    }

    fn window_value(&self, bytes: &[u8]) -> Value {
        self.value(&bytes[self.payload.clone()])
    }
}

fn frame_value(
    bytes: &[u8],
    start: usize,
    parameter_delimiter: u8,
    record_delimiter: Option<u8>,
    retain: bool,
) -> Result<ValueFrame, CodecError> {
    let remaining = bytes
        .len()
        .checked_sub(start)
        .ok_or_else(|| CodecError::malformed("IGES Global value offset exceeds input"))?;
    let value_start = start
        + bytes[start..]
            .iter()
            .position(|byte| *byte != b' ')
            .unwrap_or(remaining);
    if bytes.get(value_start) == Some(&parameter_delimiter)
        || record_delimiter.is_some_and(|delimiter| bytes.get(value_start) == Some(&delimiter))
    {
        return Ok(ValueFrame {
            kind: ValueKind::Omitted,
            payload: value_start..value_start,
            next: value_start + 1,
            ended: bytes.get(value_start) != Some(&parameter_delimiter)
                && record_delimiter == bytes.get(value_start).copied(),
        });
    }
    let (kind, payload, end, allow_padding_after) =
        if let Some((payload, end)) = hollerith(bytes, value_start)? {
            let payload_start = end - payload.len();
            let kind = if !retain {
                ValueKind::Omitted
            } else if source_span_crosses_card(value_start, payload_start) {
                ValueKind::Malformed
            } else {
                ValueKind::String
            };
            (kind, payload_start..end, end, true)
        } else {
            let end = bytes[value_start..]
                .iter()
                .position(|byte| *byte == parameter_delimiter || record_delimiter == Some(*byte))
                .and_then(|relative| value_start.checked_add(relative))
                .ok_or_else(|| malformed("record delimiter is missing"))?;
            let atom = &bytes[value_start..end];
            let kind = if !retain || atom.iter().all(|byte| *byte == b' ') {
                ValueKind::Omitted
            } else if source_span_crosses_card(value_start, end + 1) {
                ValueKind::Malformed
            } else {
                ValueKind::Atom
            };
            (kind, value_start..end, end, false)
        };
    let separator_start = if allow_padding_after {
        let remaining = bytes
            .len()
            .checked_sub(end)
            .ok_or_else(|| CodecError::malformed("IGES Global separator exceeds input"))?;
        end + bytes[end..]
            .iter()
            .position(|byte| *byte != b' ')
            .unwrap_or(remaining)
    } else {
        end
    };
    let ended = match bytes.get(separator_start).copied() {
        Some(separator) if separator == parameter_delimiter => false,
        Some(separator) if record_delimiter == Some(separator) => true,
        _ => return Err(malformed("value is not followed by a delimiter")),
    };
    Ok(ValueFrame {
        kind,
        payload,
        next: separator_start + 1,
        ended,
    })
}

fn global_bytes(scan: &CardScan<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (_, line) in scan.section(Section::Global) {
        bytes.extend_from_slice(&line.payload[..72]);
    }
    bytes
}

fn parse_raw(scan: &CardScan) -> Result<RawGlobal, CodecError> {
    let bytes = global_bytes(scan);
    parse_raw_with(&bytes, |_, frame| Ok(frame.window_value(&bytes)))
}

fn parse_raw_with(
    bytes: &[u8],
    mut value: impl FnMut(usize, &ValueFrame) -> Result<Value, CodecError>,
) -> Result<RawGlobal, CodecError> {
    if bytes.is_empty() {
        return Err(malformed("section is missing"));
    }
    if bytes.iter().copied().filter(|byte| *byte != b' ').eq(*b";") {
        return Ok(RawGlobal {
            parameter_delimiter: DEFAULT_PARAMETER_DELIMITER,
            record_delimiter: DEFAULT_RECORD_DELIMITER,
            values: vec![Value::Omitted; 26],
            field_count: 1,
            unframed: None,
        });
    }
    let (parameter_delimiter, mut cursor) = first_delimiter(bytes)?;
    let record_frame = frame_value(bytes, cursor, parameter_delimiter, None, true)?;
    let record_value = record_frame.window_value(bytes);
    cursor = record_frame.next;
    let record_delimiter = match &record_value {
        Value::Omitted => DEFAULT_RECORD_DELIMITER,
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

    let mut values = vec![Value::Omitted; 26];
    values[0] = Value::String(vec![parameter_delimiter]);
    values[1] = if let Value::String(value) = record_value {
        Value::String(value)
    } else {
        Value::String(vec![record_delimiter])
    };
    let mut field_count = 2_usize;
    let mut unframed = None;
    loop {
        let retain = field_count < values.len();
        let framed = frame_value(
            bytes,
            cursor,
            parameter_delimiter,
            Some(record_delimiter),
            retain,
        );
        let frame = match framed {
            Ok(frame) => frame,
            // The delimiter declarations bound every field. A field whose end
            // is unknown leaves only the later fields without boundaries.
            Err(error @ CodecError::Malformed(_)) => {
                unframed = Some(UnframedField {
                    field: field_count,
                    offset: cursor,
                    error,
                });
                break;
            }
            Err(error) => return Err(error),
        };
        let parsed = value(cursor, &frame)?;
        if retain {
            values[field_count] = parsed;
        }
        field_count = field_count
            .checked_add(1)
            .ok_or_else(|| malformed("Global field count overflows"))?;
        cursor = frame.next;
        if frame.ended {
            break;
        }
    }
    Ok(RawGlobal {
        parameter_delimiter,
        record_delimiter,
        values,
        field_count,
        unframed,
    })
}

/// The Global loss of a field whose end is unknown, when no part holds it.
fn unframed_loss(raw: &RawGlobal) -> Option<LossNote> {
    let unframed = raw.unframed.as_ref()?;
    let message = format!(
        "IGES Global field {} ({}) at Global byte {} has an unreadable suffix: {}; \
         preceding interpretation fields retained, remaining metadata left source-only",
        unframed.field + 1,
        field_name(unframed.field),
        unframed.offset,
        unframed.error
    );
    Some(admitted_global_loss(
        IgesLossCode::GlobalNoncanonicalFraming,
        message,
    ))
}

/// The IGES form of the file that holds a Global section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobalForm {
    Ascii,
    Binary,
}

/// Resolve a raw Global record that no part holds.
fn resolve_unparted(raw: RawGlobal, form: GlobalForm) -> (ResolvedGlobal, Vec<LossNote>) {
    let suffix = unframed_loss(&raw);
    let (global, mut losses) = resolve_form(raw, form);
    if let Some(loss) = suffix {
        losses.insert(0, loss);
    }
    (global, losses)
}

/// The Global context of a given-up Global section: every field is absent,
/// and the delimiters are the specified defaults.
pub(crate) fn given_up() -> (ResolvedGlobal, Vec<LossNote>) {
    resolve(RawGlobal {
        parameter_delimiter: DEFAULT_PARAMETER_DELIMITER,
        record_delimiter: DEFAULT_RECORD_DELIMITER,
        values: vec![Value::Omitted; 26],
        field_count: 2,
        unframed: None,
    })
}

/// Recover the Global field boundaries, then resolve every field once.
pub(crate) fn parse(scan: &CardScan) -> Result<(ResolvedGlobal, Vec<LossNote>), CodecError> {
    Ok(resolve_unparted(parse_raw(scan)?, GlobalForm::Ascii))
}

#[cfg(test)]
pub(crate) fn parse_stream(bytes: &[u8]) -> Result<(ResolvedGlobal, Vec<LossNote>), CodecError> {
    Ok(resolve_unparted(
        parse_raw_with(bytes, |_, frame| Ok(frame.window_value(bytes)))?,
        GlobalForm::Ascii,
    ))
}

/// Resolve the normalized Global payload of a Binary file. Global fields 7
/// to 11 stay read values; the Binary Flag Section supersedes them as the
/// numeric context.
pub(crate) fn parse_binary_stream(
    bytes: &[u8],
) -> Result<(ResolvedGlobal, Vec<LossNote>), CodecError> {
    Ok(resolve_unparted(
        parse_raw_with(bytes, |_, frame| Ok(frame.window_value(bytes)))?,
        GlobalForm::Binary,
    ))
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

fn parse_real_text(text: &str) -> Option<FiniteReal> {
    if !text.bytes().any(|byte| matches!(byte, b'D' | b'd')) {
        return text.parse::<f64>().ok().and_then(FiniteReal::new);
    }
    let mut normalized = text.as_bytes().to_vec();
    for byte in &mut normalized {
        if matches!(byte, b'D' | b'd') {
            *byte = b'E';
        }
    }
    std::str::from_utf8(&normalized)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .and_then(FiniteReal::new)
}

fn recovered_real_text(text: &str) -> Option<FiniteReal> {
    let prefix = text.strip_suffix('.')?;
    if !prefix
        .bytes()
        .any(|byte| matches!(byte, b'E' | b'e' | b'D' | b'd'))
    {
        return None;
    }
    parse_real_text(prefix)
}

impl Resolution {
    fn apply_string_policy(&mut self, global_table: GlobalTable) {
        for value in &mut self.values {
            let Value::String(bytes) = value else {
                continue;
            };
            if bytes
                .iter()
                .copied()
                .any(|byte| global_table.string_byte_is_forbidden(byte))
            {
                *value = Value::ForbiddenString;
            }
        }
    }

    fn value(&self, index: usize) -> &Value {
        self.values.get(index).unwrap_or(&Value::Omitted)
    }

    fn charge(&mut self, code: IgesLossCode, index: usize, defect: Defect, consequence: &str) {
        let note = global_loss_note(code, index, defect, consequence);

        self.losses.push(note);
    }

    fn declaration_text(&self, index: usize) -> String {
        match self.value(index) {
            Value::Omitted => String::new(),
            Value::String(bytes) | Value::Malformed(bytes) | Value::Atom(bytes) => {
                String::from_utf8_lossy(bytes).into_owned()
            }
            Value::ForbiddenString => {
                "a string payload contains a byte forbidden by the effective specification family"
                    .to_owned()
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

    fn supplied_date(&self, index: usize, global_table: GlobalTable) -> Supplied<String> {
        match self.supplied_string(index) {
            Supplied::Value(text)
                if date_value_is_valid(text.as_bytes(), global_table.accepts_four_digit_date()) =>
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

    fn supplied_real(&self, index: usize) -> SuppliedReal {
        match self.value(index) {
            Value::Omitted => SuppliedReal::Absent,
            Value::Atom(bytes) => {
                let Some(text) = numeric_text(bytes) else {
                    return SuppliedReal::Malformed;
                };
                if let Some(value) = parse_real_text(text) {
                    return SuppliedReal::Value(value);
                }
                if let Some(value) = recovered_real_text(text) {
                    return SuppliedReal::Recovered(value);
                }
                SuppliedReal::Malformed
            }
            Value::String(_) | Value::Malformed(_) | Value::ForbiddenString => {
                SuppliedReal::Malformed
            }
        }
    }

    fn charge_recovered_real(&mut self, index: usize, value: f64) {
        let source = self.declaration_text(index);
        let note = recovered_real_loss_note(index, &source, value);

        self.losses.push(note);
    }

    /// Charges the field when the declaration states no value the model can use.
    ///
    /// This is the one place a metadata field states its defect. Every
    /// metadata method below resolves its declaration into a `Supplied` and
    /// then calls this; the value-returning methods hand the value on, and
    /// the charging methods state the charge alone.
    fn charge_metadata<T>(
        &mut self,
        index: usize,
        supplied: &Supplied<T>,
        global_table: GlobalTable,
    ) {
        let defect = match supplied {
            Supplied::Absent if global_table.field_requires_value(index) => Defect::Absent,
            Supplied::Malformed => Defect::Malformed,
            Supplied::Absent | Supplied::Value(_) => return,
        };
        self.charge(
            IgesLossCode::GlobalMetadataFieldUnusable,
            index,
            defect,
            METADATA_CONSEQUENCE,
        );
    }

    /// The supplied integer, with a value the field does not admit read as malformed.
    fn admitted_integer(&self, index: usize, admits: fn(i64) -> bool) -> Supplied<i64> {
        match self.supplied_integer(index) {
            Supplied::Value(value) if admits(value) => Supplied::Value(value),
            Supplied::Absent => Supplied::Absent,
            Supplied::Malformed | Supplied::Value(_) => Supplied::Malformed,
        }
    }

    fn metadata_string(&mut self, index: usize, global_table: GlobalTable) -> Option<String> {
        let supplied = self.supplied_string(index);
        self.charge_metadata(index, &supplied, global_table);
        match supplied {
            Supplied::Value(text) => Some(text),
            Supplied::Absent | Supplied::Malformed => None,
        }
    }

    /// Charges a metadata string field whose value the model does not carry.
    fn charge_metadata_string(&mut self, index: usize, global_table: GlobalTable) {
        let supplied = self.supplied_string(index);
        self.charge_metadata(index, &supplied, global_table);
    }

    fn metadata_date(&mut self, index: usize, global_table: GlobalTable) {
        let supplied = self.supplied_date(index, global_table);
        self.charge_metadata(index, &supplied, global_table);
    }

    fn metadata_integer_value(
        &mut self,
        index: usize,
        global_table: GlobalTable,
        admits: fn(i64) -> bool,
    ) -> Option<i64> {
        self.metadata_integer_declaration(index, global_table, admits)
            .value()
    }

    fn metadata_integer_declaration(
        &mut self,
        index: usize,
        global_table: GlobalTable,
        admits: fn(i64) -> bool,
    ) -> Supplied<i64> {
        let supplied = self.admitted_integer(index, admits);
        self.charge_metadata(index, &supplied, global_table);
        supplied
    }

    /// Charges a metadata integer field whose value the model does not carry.
    fn charge_metadata_integer(
        &mut self,
        index: usize,
        global_table: GlobalTable,
        admits: fn(i64) -> bool,
    ) {
        let supplied = self.admitted_integer(index, admits);
        self.charge_metadata(index, &supplied, global_table);
    }

    /// Charges the maximum-coordinate field.
    ///
    /// The coordinate is not carried into the model, so the charge the
    /// absent and malformed arms state is the whole effect and there is no
    /// value for a caller to take. The arms are the ones the reading kept;
    /// only the values they answered are gone.
    fn charge_maximum_coordinate(&mut self, global_table: GlobalTable) {
        match self.supplied_real(FIELD_MAXIMUM_COORDINATE) {
            SuppliedReal::Absent if global_table == GlobalTable::V5_0 => {}
            SuppliedReal::Absent if global_table == GlobalTable::V4_0 => {
                self.charge(
                    IgesLossCode::GlobalMetadataFieldUnusable,
                    FIELD_MAXIMUM_COORDINATE,
                    Defect::Absent,
                    METADATA_CONSEQUENCE,
                );
            }
            SuppliedReal::Absent => {}
            SuppliedReal::Value(value) if value.get() >= 0.0 => {}
            SuppliedReal::Recovered(value) if value.get() >= 0.0 => {
                self.charge_recovered_real(FIELD_MAXIMUM_COORDINATE, value.get());
            }
            SuppliedReal::Value(_) | SuppliedReal::Recovered(_) | SuppliedReal::Malformed => {
                self.charge(
                    IgesLossCode::GlobalMetadataFieldUnusable,
                    FIELD_MAXIMUM_COORDINATE,
                    Defect::Malformed,
                    METADATA_CONSEQUENCE,
                );
            }
        }
    }

    fn significance(&mut self, index: usize) -> Supplied<u32> {
        let defect = match self.supplied_integer(index) {
            Supplied::Absent => Defect::Absent,
            Supplied::Value(value) => match u32::try_from(value).ok().filter(|value| *value > 0) {
                Some(value) => return Supplied::Value(value),
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
        match defect {
            Defect::Absent => Supplied::Absent,
            Defect::Malformed => Supplied::Malformed,
        }
    }

    /// The sender numeric context that Global fields 7 to 11 declare.
    fn numeric_declarations(&mut self, global_table: GlobalTable) -> NumericDeclarations {
        let integer_bits = self
            .metadata_integer_value(FIELD_INTEGER_BITS, global_table, |value| {
                value > 0 && u32::try_from(value).is_ok()
            })
            .and_then(|value| u32::try_from(value).ok());
        let single_magnitude =
            self.metadata_integer_value(FIELD_SINGLE_MAGNITUDE, global_table, |_| true);
        let single_significance = self.significance(FIELD_SINGLE_SIGNIFICANCE).value();
        let double_magnitude =
            self.metadata_integer_declaration(FIELD_DOUBLE_MAGNITUDE, global_table, |_| true);
        let double_significance = if global_table == GlobalTable::V5_0
            && matches!(
                self.supplied_integer(FIELD_DOUBLE_SIGNIFICANCE),
                Supplied::Absent
            ) {
            Supplied::Absent
        } else {
            self.significance(FIELD_DOUBLE_SIGNIFICANCE)
        };
        NumericDeclarations {
            integer_bits,
            single_magnitude,
            double_magnitude,
            single_significance,
            double_significance,
        }
    }

    fn minimum_resolution(&mut self, global_table: GlobalTable) -> NonNegativeReal {
        match self.supplied_real(FIELD_MINIMUM_RESOLUTION) {
            SuppliedReal::Absent if global_table.field_requires_value(FIELD_MINIMUM_RESOLUTION) => {
                self.charge(
                    IgesLossCode::GlobalSemanticContextSubstituted,
                    FIELD_MINIMUM_RESOLUTION,
                    Defect::Absent,
                    RESOLUTION_CONSEQUENCE,
                );
                NonNegativeReal::ZERO
            }
            SuppliedReal::Absent => NonNegativeReal::ZERO,
            SuppliedReal::Value(value) => match NonNegativeReal::try_from(value) {
                Ok(value) => value,
                Err(_) => {
                    self.charge(
                        IgesLossCode::GlobalSemanticContextSubstituted,
                        FIELD_MINIMUM_RESOLUTION,
                        Defect::Malformed,
                        RESOLUTION_CONSEQUENCE,
                    );
                    NonNegativeReal::ZERO
                }
            },
            SuppliedReal::Recovered(value) => match NonNegativeReal::try_from(value) {
                Ok(admitted) => {
                    self.charge_recovered_real(FIELD_MINIMUM_RESOLUTION, value.get());
                    admitted
                }
                Err(_) => {
                    self.charge(
                        IgesLossCode::GlobalSemanticContextSubstituted,
                        FIELD_MINIMUM_RESOLUTION,
                        Defect::Malformed,
                        RESOLUTION_CONSEQUENCE,
                    );
                    NonNegativeReal::ZERO
                }
            },
            SuppliedReal::Malformed => {
                self.charge(
                    IgesLossCode::GlobalSemanticContextSubstituted,
                    FIELD_MINIMUM_RESOLUTION,
                    Defect::Malformed,
                    RESOLUTION_CONSEQUENCE,
                );
                NonNegativeReal::ZERO
            }
        }
    }

    fn line_weight_scale(&mut self, global_table: GlobalTable) -> Option<LineWeightScale> {
        let supplied_gradations = self.supplied_integer(FIELD_LINE_WEIGHT_GRADATIONS);
        let gradations_was_supplied = !matches!(&supplied_gradations, Supplied::Absent);
        let (gradations, gradations_defect) = match supplied_gradations {
            Supplied::Absent if matches!(global_table, GlobalTable::V4_0) => {
                (None, Some(Defect::Absent))
            }
            Supplied::Absent => (Some(1), None),
            Supplied::Value(value)
                if value > 0 && (global_table != GlobalTable::V4_0 || value <= 32_768) =>
            {
                (Some(value), None)
            }
            Supplied::Value(_) | Supplied::Malformed => (None, Some(Defect::Malformed)),
        };
        let (mode, width_defect) = match self.supplied_real(FIELD_MAXIMUM_LINE_WIDTH) {
            SuppliedReal::Absent
                if global_table == GlobalTable::V5_0 && !gradations_was_supplied =>
            {
                (None, None)
            }
            SuppliedReal::Value(value)
                if value.get() == 0.0 && global_table == GlobalTable::V5_0 =>
            {
                (Some(LineWeightMode::Relative), None)
            }
            SuppliedReal::Recovered(value)
                if value.get() == 0.0 && global_table == GlobalTable::V5_0 =>
            {
                self.charge_recovered_real(FIELD_MAXIMUM_LINE_WIDTH, 0.0);
                (Some(LineWeightMode::Relative), None)
            }
            SuppliedReal::Value(value) => match PositiveReal::try_from(value) {
                Ok(maximum_width) => (Some(LineWeightMode::Absolute { maximum_width }), None),
                Err(_) => (None, Some(Defect::Malformed)),
            },
            SuppliedReal::Recovered(value) => match PositiveReal::try_from(value) {
                Ok(maximum_width) => {
                    self.charge_recovered_real(FIELD_MAXIMUM_LINE_WIDTH, value.get());
                    (Some(LineWeightMode::Absolute { maximum_width }), None)
                }
                Err(_) => (None, Some(Defect::Malformed)),
            },
            SuppliedReal::Absent => (None, Some(Defect::Absent)),
            SuppliedReal::Malformed => (None, Some(Defect::Malformed)),
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
        let (Some(gradations), Some(mode)) = (gradations, mode) else {
            return None;
        };
        Some(LineWeightScale { gradations, mode })
    }

    fn length_unit(&mut self, global_table: GlobalTable) -> (Option<String>, Option<PositiveReal>) {
        let (scale, scale_defect) = match self.supplied_real(FIELD_MODEL_SCALE) {
            SuppliedReal::Absent => (global_table.default_model_scale(), None),
            SuppliedReal::Value(value) => match PositiveReal::try_from(value) {
                Ok(scale) => (Some(scale), None),
                Err(_) => (None, Some(Defect::Malformed)),
            },
            SuppliedReal::Recovered(value) => match PositiveReal::try_from(value) {
                Ok(scale) => {
                    self.charge_recovered_real(FIELD_MODEL_SCALE, value.get());
                    (Some(scale), None)
                }
                Err(_) => (None, Some(Defect::Malformed)),
            },
            SuppliedReal::Malformed => (None, Some(Defect::Malformed)),
        };
        let (units_flag, flag_defect) = match self.supplied_integer(FIELD_UNITS_FLAG) {
            Supplied::Absent => (global_table.default_units_flag(), None),
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
                Supplied::Absent if global_table.defaults_units_name() => units_flag
                    .and_then(enumerated_unit_name)
                    .map(std::borrow::ToOwned::to_owned),
                Supplied::Absent if global_table.field_requires_value(FIELD_UNITS_NAME) => {
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
            (Some(unit), Some(scale)) => PositiveReal::new(unit / scale.get()),
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
                None => {
                    let message = format!("IGES Global fields {} ({}) and {} ({}) produce no finite positive millimetre length factor; {LENGTH_CONSEQUENCE}",
                    FIELD_MODEL_SCALE + 1,
                    field_name(FIELD_MODEL_SCALE),
                    FIELD_UNITS_FLAG + 1,
                    field_name(FIELD_UNITS_FLAG));
                    let note =
                        admitted_global_loss(IgesLossCode::GlobalLengthUnitUnresolved, message);

                    self.losses.push(note);
                }
            }
        }
        (units_name, length_factor_mm)
    }
}

fn resolve(raw: RawGlobal) -> (ResolvedGlobal, Vec<LossNote>) {
    resolve_form(raw, GlobalForm::Ascii)
}

fn resolve_form(raw: RawGlobal, form: GlobalForm) -> (ResolvedGlobal, Vec<LossNote>) {
    let RawGlobal {
        parameter_delimiter,
        record_delimiter,
        values,
        field_count,
        unframed: _,
    } = raw;
    let mut resolution = Resolution {
        values,
        losses: Vec::new(),
    };
    if field_count < 2 {
        let message = "IGES Global record ends before the delimiter declarations; all fields \
                       are omitted and the decoder uses the default comma and semicolon grammar"
            .to_string();
        let note = admitted_global_loss(IgesLossCode::GlobalNoncanonicalFraming, message);
        resolution.losses.push(note);
    }
    let declaration = match resolution.supplied_integer(FIELD_VERSION_FLAG) {
        Supplied::Absent => VersionDeclaration::Exact(VersionFlag::V2_0),
        Supplied::Value(value) => match VersionFlag::exact(value) {
            Some(version) => VersionDeclaration::Exact(version),
            None => VersionDeclaration::Clamped(value),
        },
        Supplied::Malformed => {
            VersionDeclaration::Unreadable(resolution.declaration_text(FIELD_VERSION_FLAG))
        }
    };
    let effective_version = declaration.effective_version();
    let global_table = effective_version.global_table();
    resolution.apply_string_policy(global_table);
    let global_field_count = global_table.global_field_count();

    if field_count > global_field_count {
        let message = format!("IGES Global record has {field_count} fields; IGES {} Table 1 defines {global_field_count} and the decoder ignored the rest",
        effective_version.name());
        let note = admitted_global_loss(IgesLossCode::GlobalNoncanonicalFraming, message);

        resolution.losses.push(note);
    }

    let sender_product = resolution.metadata_string(FIELD_SENDER_PRODUCT, global_table);
    let native_file_name = resolution.metadata_string(FIELD_FILE_NAME, global_table);
    resolution.charge_metadata_string(FIELD_NATIVE_SYSTEM, global_table);
    resolution.charge_metadata_string(FIELD_PREPROCESSOR_VERSION, global_table);
    let numeric = match form {
        GlobalForm::Binary => NumericContext::BinaryFlagSection,
        GlobalForm::Ascii => {
            NumericContext::Declared(resolution.numeric_declarations(global_table))
        }
    };
    let receiver_product = match resolution.supplied_string(FIELD_RECEIVER_PRODUCT) {
        Supplied::Absent if global_table.defaults_receiver_product_to_sender() => {
            sender_product.as_deref().map(str::to_owned)
        }
        Supplied::Absent if global_table.field_requires_value(FIELD_RECEIVER_PRODUCT) => {
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
    let (units_name, length_factor_mm) = resolution.length_unit(global_table);
    let line_weight_scale = resolution.line_weight_scale(global_table);
    resolution.metadata_date(FIELD_GENERATION_DATE, global_table);
    let minimum_resolution = resolution.minimum_resolution(global_table);
    resolution.charge_maximum_coordinate(global_table);
    resolution.charge_metadata_string(FIELD_AUTHOR, global_table);
    resolution.charge_metadata_string(FIELD_ORGANIZATION, global_table);
    resolution.charge_metadata_integer(FIELD_DRAFTING_STANDARD, global_table, |value| {
        (0..=7).contains(&value)
    });
    if global_table.has_model_date() {
        resolution.metadata_date(FIELD_MODEL_DATE, global_table);
    }
    if global_table.has_application_protocol() {
        resolution.charge_metadata_string(FIELD_APPLICATION_PROTOCOL, global_table);
    }

    let resolved = ResolvedGlobal {
        parameter_delimiter,
        record_delimiter,
        sender_product,
        receiver_product,
        native_file_name,
        units_name,
        numeric,
        minimum_resolution,
        length_factor_mm,
        line_weight_scale,
        declaration,
    };
    (resolved, resolution.losses)
}

impl ResolvedGlobal {
    /// The projection view, present only when the millimetre length factor resolved.
    pub(crate) fn length_context(&self) -> Option<ProjectedGlobal> {
        let length_factor_mm = self.length_factor_mm?;
        Some(ProjectedGlobal {
            length_factor_mm,
            minimum_resolution_mm: self.minimum_resolution.get() * length_factor_mm.get(),
            precision: self.real_precision(),
            line_weight_scale: self.line_weight_scale,
            global_table: self.global_table(),
        })
    }

    pub(crate) fn real_precision(&self) -> RealPrecision {
        self.numeric.precision()
    }

    pub(crate) fn numeric_limits(&self) -> NumericLimits {
        self.numeric.limits()
    }

    pub(crate) fn sender_product(&self) -> Option<&str> {
        self.sender_product.as_deref()
    }

    fn receiver_product(&self) -> Option<&str> {
        self.receiver_product.as_deref()
    }

    pub(crate) fn native_file_name(&self) -> Option<&str> {
        self.native_file_name.as_deref()
    }

    pub(crate) fn units_name(&self) -> Option<&str> {
        self.units_name.as_deref()
    }

    /// The version flag as declared, with the specification default for an absent field.
    pub(crate) fn declared_version_flag(&self) -> i64 {
        self.declaration.declared_flag()
    }

    /// The exact field-23 table entry, before postprocessor recovery.
    pub(crate) fn declared_version(&self) -> Option<VersionFlag> {
        VersionFlag::exact(self.declared_version_flag())
    }

    /// The declaration text of a field 23 that does not read as an integer.
    pub(crate) fn unreadable_version_declaration(&self) -> Option<&str> {
        match &self.declaration {
            VersionDeclaration::Unreadable(text) => Some(text),
            _ => None,
        }
    }

    /// The declared version flag after the specification's postprocessor clamp.
    pub(crate) fn effective_version_flag(&self) -> i64 {
        self.declaration.effective_version().value()
    }

    /// Why this decode did not read the file with a Global table verified for
    /// the version its field 23 declares.
    ///
    /// The single predicate from which dialect classification derives its
    /// admission. Loss reporting consumes that admission as the authoritative
    /// proof and consults this recovery only for the explanatory message.
    pub(crate) fn dialect_recovery(&self) -> DialectRecovery<'_> {
        self.declaration.recovery()
    }

    pub(crate) fn version_name(&self) -> &'static str {
        self.declaration.effective_version().name()
    }

    pub(crate) fn global_table(&self) -> GlobalTable {
        self.declaration.effective_version().global_table()
    }

    pub(crate) fn conditional_double_precision_losses(
        &self,
        uses_double_precision: bool,
    ) -> Vec<LossNote> {
        let NumericContext::Declared(numeric) = &self.numeric else {
            return Vec::new();
        };
        if self.global_table() != GlobalTable::V5_0 || !uses_double_precision {
            return Vec::new();
        }
        let mut losses = Vec::new();
        if matches!(numeric.double_magnitude, Supplied::Absent) {
            let note = global_loss_note(
                IgesLossCode::GlobalMetadataFieldUnusable,
                FIELD_DOUBLE_MAGNITUDE,
                Defect::Absent,
                METADATA_CONSEQUENCE,
            );

            losses.push(note);
        }
        if matches!(numeric.double_significance, Supplied::Absent) {
            let note = global_loss_note(
                IgesLossCode::GlobalSemanticContextSubstituted,
                FIELD_DOUBLE_SIGNIFICANCE,
                Defect::Absent,
                SIGNIFICANCE_CONSEQUENCE,
            );

            losses.push(note);
        }
        losses
    }

    /// Inspection notes for this Global section.
    ///
    /// Any recovery other than [`DialectRecovery::Verified`] means the
    /// effective Global table was not verified for the source declaration. In
    /// that case, retain the declared flag and label the effective version as
    /// recovery rather than presenting it as the document's verified version.
    pub(crate) fn summary_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        notes.push(format!(
            "parameter_delimiter={}",
            char::from(self.parameter_delimiter)
        ));
        notes.push(format!(
            "record_delimiter={}",
            char::from(self.record_delimiter)
        ));
        if let Some(product) = self.sender_product() {
            notes.push(format!("sender_product={product}"));
        }
        if self.global_table() == GlobalTable::V5_0 {
            if let Some(product) = self.receiver_product() {
                notes.push(format!("receiver_product={product}"));
            }
        }
        if let Some(units) = self.units_name() {
            notes.push(format!("units={units}"));
        }
        if matches!(self.dialect_recovery(), DialectRecovery::Verified) {
            notes.push(format!("iges_version={}", self.version_name()));
        } else {
            notes.push("iges_version=unverified".to_string());
            notes.push(format!(
                "iges_declared_version_flag={}",
                self.declaration.declared_flag()
            ));
            notes.push(format!("iges_effective_version={}", self.version_name()));
        }
        notes
    }
}

impl ProjectedGlobal {
    pub(crate) fn global_table(&self) -> GlobalTable {
        self.global_table
    }

    pub(crate) fn length_factor_mm(&self) -> f64 {
        self.length_factor_mm.get()
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
        Some(
            cadmpeg_core::convert::f64_from_i64(number)?
                * maximum_width.get()
                * self.length_factor_mm.get()
                / cadmpeg_core::convert::f64_from_i64(scale.gradations)?,
        )
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
