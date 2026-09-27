// SPDX-License-Identifier: Apache-2.0
//! Frame SAT/SMT (ASM text) record streams into the typed [`Token`] model.
//!
//! The text encoding carries the same entity model as the binary SAB encoding
//! ([`crate::sab`]) in a line-oriented ASCII form: three header lines, one
//! record per `#` terminator, and a final `End-of-ASM-data` or
//! `End-of-ACIS-data` line. [`parse`] returns the header and an indexed
//! [`Record`] table whose token stream matches the binary framer's output, so
//! the shared decoders in [`crate::brep`] and [`crate::nurbs`] consume both
//! encodings through one path.
//!
//! The text fields are untyped, so a per-head shape grammar assigns each field
//! its binary token type: `POSITION`/`VECTOR_3D` slots coalesce three bare
//! numbers into one token, boolean words map onto `TRUE`/`FALSE`, enumeration
//! words map onto `ENUM_VALUE`, and integral-looking numbers become `DOUBLE`
//! where the slot is a double. Length-bearing slots are converted from the
//! stream's own unit (the header `scale`, in millimetres per unit) into the
//! centimetre convention of the binary encoding, so the decode path applies
//! its uniform cm→mm rule unchanged. A record whose shape no grammar accepts
//! is typed by lexical form alone: every field stays one token, so record
//! indexing and reference resolution hold for every record.

use crate::kernel_header::KernelHeader;
use crate::sab::{Record, Token};
use crate::stream_error::{StreamError, StreamFailure, StreamFormat};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{NonNegativeReal, PositiveReal};

/// The stream branch, from the terminator line ([`asm.md` §7]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// `End-of-ASM-data`.
    Asm,
    /// `End-of-ACIS-data`.
    Acis,
}

/// The three header lines of a text stream: the four binary header words, the
/// three product strings, and the three kernel doubles.
#[derive(Debug, Clone, PartialEq)]
pub struct TextHeader {
    /// ACIS save-format version word, `major * 100 + minor`.
    pub save_format_version: u32,
    /// Entity-count word: the `RecordTable` index of the first referenced record.
    pub entity_count: u64,
    /// Flags word: bit 0 marks a history partition, bits 1..=7 the revision.
    pub flags: u64,
    /// Product family string.
    pub product_family: String,
    /// Product version string.
    pub product_version: String,
    /// Save date string.
    pub save_date: String,
    /// Length unit of the stream, in millimetres per unit.
    scale: PositiveReal,
    /// Absolute distance tolerance in stream length units.
    resabs: NonNegativeReal,
    /// Normal tolerance.
    resnor: NonNegativeReal,
    normalized_resabs_cm: NonNegativeReal,
}

impl TextHeader {
    /// Length unit of the stream, in millimetres per unit.
    pub const fn scale(&self) -> PositiveReal {
        self.scale
    }

    /// Absolute distance tolerance in stream length units.
    pub const fn resabs(&self) -> NonNegativeReal {
        self.resabs
    }

    /// Normal tolerance.
    pub const fn resnor(&self) -> NonNegativeReal {
        self.resnor
    }

    /// The header as a [`KernelHeader`] for the shared decode path.
    ///
    /// `scale` is reported as `10.0`: [`parse`] converts length-bearing values
    /// into the centimetre convention, including `resabs`, so the decoders see
    /// the same unit a binary stream carries.
    pub fn as_kernel_header(&self, ctx: &DecodeContext<'_>) -> Result<KernelHeader, CodecError> {
        let copy = |value: &str| -> Result<String, CodecError> {
            let requested = u64::try_from(value.len()).map_err(|_| {
                ctx.refuse_codec_limit("retain SAT kernel header string", u64::MAX, u64::MAX)
            })?;
            ctx.charge_retained(requested, "retain SAT kernel header string")?;
            copy_sat_string(ctx, value, "SAT kernel header string")
        };
        Ok(KernelHeader {
            save_format_version: Some(self.save_format_version),
            entity_count: Some(self.entity_count),
            flags: Some(self.flags),
            product_family: Some(copy(&self.product_family)?),
            product_version: Some(copy(&self.product_version)?),
            save_date: Some(copy(&self.save_date)?),
            scale: Some(10.0),
            linear: Some(self.normalized_resabs_cm.get()),
            angular: Some(self.resnor.get()),
        })
    }
}

/// Convert a text length tolerance to the binary stream's centimetre unit.
/// The second product keeps a positive result when `scale / 10` rounds to zero.
fn resabs_cm(scale: PositiveReal, resabs: NonNegativeReal) -> f64 {
    let converted = resabs.get() * (scale.get() / 10.0);
    if converted == 0.0 && resabs.get() > 0.0 {
        (resabs.get() / 10.0) * scale.get()
    } else {
        converted
    }
}

/// A parsed text stream: header, indexed records, and terminator branch.
#[derive(Debug, Clone)]
pub struct TextStream {
    /// The three header lines.
    pub header: TextHeader,
    /// Records in file order. Index 0 is the first record after the header
    /// lines; the stream does not always begin with `asmheader`.
    pub records: Vec<Record>,
    /// Which terminator line closed the stream.
    pub terminator: Terminator,
}

/// Whether `bytes` begins like a text ASM stream: an ASCII digit run (the
/// save-format word) followed by a space.
pub fn has_text_magic(bytes: &[u8]) -> bool {
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    digits >= 3 && bytes.get(digits) == Some(&b' ')
}

// ---------------------------------------------------------------------------
// Primitive fields
// ---------------------------------------------------------------------------

/// One whitespace-delimited field, before typing.
#[derive(Debug, Clone, PartialEq)]
enum Prim {
    /// An exact signed decimal integer field.
    Integer(i64),
    /// A real field whose lexical shape includes a decimal point or exponent.
    Real(f64),
    /// `$N` entity reference.
    Ref(i64),
    /// `@N` length-prefixed raw-byte string.
    Str(String),
    /// `{` subtype open.
    Open,
    /// `}` subtype close.
    Close,
    /// Any other bare word.
    Word(String),
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

fn copy_sat_string(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let amount = u64::try_from(value.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    let mut copy = String::new();
    copy.try_reserve(value.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, amount))?;
    copy.push_str(value);
    Ok(copy)
}

struct FieldReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl FieldReader<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && is_ws(self.bytes[self.pos]) {
            self.pos += 1;
        }
    }

    /// Read one raw whitespace-delimited field. Returns `None` at end of
    /// input. An `@N` field consumes one separator byte and exactly `N` raw
    /// bytes, which may include whitespace and newlines.
    fn next_field(
        &mut self,
        ctx: &DecodeContext<'_>,
        scratch: &mut ScopedReservation<'_>,
        retained: bool,
    ) -> Result<Option<(usize, String)>, StreamFailure> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let start = self.pos;
        while self.pos < self.bytes.len() && !is_ws(self.bytes[self.pos]) {
            self.pos += 1;
        }
        let word =
            std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|error| StreamError {
                format: StreamFormat::Text,
                offset: start + error.valid_up_to(),
                reason: "field is not valid UTF-8".to_string(),
            })?;
        if retained {
            ctx.charge_retained(word.len() as u64, "retain SAT record name")?;
        } else {
            scratch.grow(word.len() as u64)?;
        }
        let word = copy_sat_string(ctx, word, "SAT field")?;
        Ok(Some((start, word)))
    }

    /// Consume the `@N` payload after its length field: one separator byte,
    /// then `N` raw bytes.
    fn read_str_payload(
        &mut self,
        ctx: &DecodeContext<'_>,
        len: usize,
        at: usize,
        scratch: &mut ScopedReservation<'_>,
    ) -> Result<String, StreamFailure> {
        self.pos += 1; // one separator byte after the length field
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len());
        let Some(end) = end else {
            return Err(StreamError {
                format: StreamFormat::Text,
                offset: at,
                reason: format!("truncated @{len} string"),
            }
            .into());
        };
        let payload =
            std::str::from_utf8(&self.bytes[self.pos..end]).map_err(|error| StreamError {
                format: StreamFormat::Text,
                offset: self.pos + error.valid_up_to(),
                reason: format!("@{len} string is not valid UTF-8"),
            })?;
        scratch.grow(payload.len() as u64)?;
        let payload = copy_sat_string(ctx, payload, "SAT string payload")?;
        self.pos = end;
        Ok(payload)
    }
}

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

/// Split one header line into whitespace-separated fields.
fn header_line<'a>(bytes: &'a [u8], pos: &mut usize, what: &str) -> Result<&'a [u8], StreamError> {
    let start = *pos;
    let end = bytes[start..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|off| start + off)
        .ok_or_else(|| StreamError {
            format: StreamFormat::Text,
            offset: start,
            reason: format!("missing {what} line"),
        })?;
    *pos = end + 1;
    Ok(&bytes[start..end])
}

fn header_int<T: std::str::FromStr>(
    field: Option<&[u8]>,
    at: usize,
    what: &str,
) -> Result<T, StreamError> {
    field
        .and_then(|field| std::str::from_utf8(field).ok())
        .and_then(|field| field.parse().ok())
        .ok_or_else(|| StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: format!("header line has no {what} field"),
        })
}

/// Read one `N <bytes>` counted string from a header line's raw byte slice.
/// Header strings use a bare count without the record encoding's `@` prefix.
fn counted_string(
    ctx: &DecodeContext<'_>,
    line: &[u8],
    pos: &mut usize,
    at: usize,
    what: &str,
) -> Result<String, StreamFailure> {
    while *pos < line.len() && is_ws(line[*pos]) {
        *pos += 1;
    }
    let start = *pos;
    while *pos < line.len() && line[*pos].is_ascii_digit() {
        *pos += 1;
    }
    let len: usize = std::str::from_utf8(&line[start..*pos])
        .ok()
        .and_then(|digits| digits.parse().ok())
        .ok_or_else(|| StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: format!("header line has no {what} count"),
        })?;
    if line.get(*pos).is_none_or(|byte| !is_ws(*byte)) {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: format!("header {what} count has no separator"),
        }
        .into());
    }
    *pos += 1;
    let end = pos
        .checked_add(len)
        .filter(|end| *end <= line.len())
        .ok_or_else(|| StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: format!("truncated {what} string"),
        })?;
    let value = std::str::from_utf8(&line[*pos..end]).map_err(|error| StreamError {
        format: StreamFormat::Text,
        offset: at + *pos + error.valid_up_to(),
        reason: format!("header {what} string is not valid UTF-8"),
    })?;
    ctx.charge_retained(value.len() as u64, "retain SAT header string")?;
    let value = copy_sat_string(ctx, value, "SAT header string")?;
    *pos = end;
    Ok(value)
}

fn parse_header(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    pos: &mut usize,
) -> Result<TextHeader, StreamFailure> {
    let at = *pos;
    let line1 = header_line(bytes, pos, "save-format")?;
    let mut fields = line1.split(|b| is_ws(*b)).filter(|field| !field.is_empty());
    let line1: [Option<&[u8]>; 4] = std::array::from_fn(|_| fields.next());
    if line1.iter().any(Option::is_none) || fields.next().is_some() {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "save-format header line must contain four fields".to_string(),
        }
        .into());
    }
    let save_format_version = header_int(line1[0], at, "save format")?;
    header_int::<u32>(line1[1], at, "record count")?;
    let entity_count = header_int(line1[2], at, "entity count")?;
    let flags = header_int(line1[3], at, "flags")?;

    let at = *pos;
    let line2_start = *pos;
    let line2_end = bytes[line2_start..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|off| line2_start + off)
        .ok_or_else(|| StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "missing product line".to_string(),
        })?;
    let line2 = &bytes[line2_start..line2_end];
    *pos = line2_end + 1;
    let mut cursor = 0usize;
    let product_family = counted_string(ctx, line2, &mut cursor, at, "product family")?;
    let product_version = counted_string(ctx, line2, &mut cursor, at, "product version")?;
    let save_date = counted_string(ctx, line2, &mut cursor, at, "save date")?;
    if line2[cursor..].iter().any(|byte| !is_ws(*byte)) {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "product header line must contain three counted strings".to_string(),
        }
        .into());
    }

    let at = *pos;
    let line3 = header_line(bytes, pos, "tolerance")?;
    let mut fields = line3.split(|b| is_ws(*b)).filter(|field| !field.is_empty());
    let line3: [Option<&[u8]>; 3] = std::array::from_fn(|_| fields.next());
    if line3.iter().any(Option::is_none) || fields.next().is_some() {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "tolerance header line must contain three fields".to_string(),
        }
        .into());
    }
    let float = |field: Option<&[u8]>, what: &str| -> Result<f64, StreamError> {
        field
            .and_then(|field| std::str::from_utf8(field).ok())
            .and_then(|field| field.parse().ok())
            .ok_or_else(|| StreamError {
                format: StreamFormat::Text,
                offset: at,
                reason: format!("header line has no {what} field"),
            })
    };
    let scale = PositiveReal::new(float(line3[0], "scale")?).ok_or_else(|| StreamError {
        format: StreamFormat::Text,
        offset: at,
        reason: "header scale must be finite and positive".to_string(),
    })?;
    let raw_resabs = float(line3[1], "resabs")?;
    let raw_resnor = float(line3[2], "resnor")?;
    let (Some(resabs), Some(resnor)) = (
        NonNegativeReal::new(raw_resabs),
        NonNegativeReal::new(raw_resnor),
    ) else {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "header tolerances must be finite and nonnegative".to_string(),
        }
        .into());
    };
    let normalized_resabs = resabs_cm(scale, resabs);
    if resabs.get() > 0.0 && (!normalized_resabs.is_finite() || normalized_resabs == 0.0) {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "header resabs cannot be represented in centimetres".to_string(),
        }
        .into());
    }
    let normalized_resabs_cm =
        NonNegativeReal::new(normalized_resabs).ok_or_else(|| StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "header resabs cannot be represented in centimetres".to_string(),
        })?;
    Ok(TextHeader {
        save_format_version,
        entity_count,
        flags,
        product_family,
        product_version,
        save_date,
        scale,
        resabs,
        resnor,
        normalized_resabs_cm,
    })
}

// ---------------------------------------------------------------------------
// Record framing
// ---------------------------------------------------------------------------

fn record_error_reason(
    ctx: &DecodeContext<'_>,
    name: &str,
    description: &'static str,
) -> Result<String, StreamFailure> {
    let length = "record `"
        .len()
        .checked_add(name.len())
        .and_then(|length| length.checked_add("` ".len()))
        .and_then(|length| length.checked_add(description.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("SAT record error text", u64::MAX, u64::MAX))?;
    let requested = u64::try_from(length)
        .map_err(|_| ctx.refuse_codec_limit("SAT record error text", u64::MAX, u64::MAX))?;
    ctx.charge_retained(requested, "SAT record error text")?;
    let mut reason = String::new();
    reason
        .try_reserve(length)
        .map_err(|_| ctx.refuse_codec_limit("SAT record error text", 0, requested))?;
    reason.push_str("record `");
    reason.push_str(name);
    reason.push_str("` ");
    reason.push_str(description);
    Ok(reason)
}

/// Parse a complete text stream into its header and typed record table.
pub fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<TextStream, StreamFailure> {
    let mut pos = 0usize;
    let header = parse_header(ctx, bytes, &mut pos)?;
    // Length conversion into the binary centimetre convention: the stream
    // stores lengths in `scale` millimetres per unit.
    let scale = header.scale().get();

    let mut reader = FieldReader { bytes, pos };
    let mut records = Vec::new();
    let mut terminator = None;
    let mut admitted_entities = 0_u64;
    ctx.admit_entities(
        header.entity_count,
        &mut admitted_entities,
        "preflight SAT header entities",
    )?;
    // Record name field, then payload fields until the terminator.
    'stream: loop {
        let mut scratch = ctx.reserve_scoped(0, "frame SAT record")?;
        let Some((rec_start, name)) = reader.next_field(ctx, &mut scratch, true)? else {
            break;
        };
        match name.as_str() {
            "End-of-ASM-data" => {
                terminator = Some(Terminator::Asm);
                break 'stream;
            }
            "End-of-ACIS-data" => {
                terminator = Some(Terminator::Acis);
                break 'stream;
            }
            _ => {}
        }
        // Payload fields until the `#` terminator.
        let mut prims = Vec::new();
        let mut subtype_depth = 0usize;
        loop {
            let Some((at, field)) = reader.next_field(ctx, &mut scratch, false)? else {
                return Err(StreamError {
                    format: StreamFormat::Text,
                    offset: rec_start,
                    reason: record_error_reason(ctx, &name, "has no `#` terminator")?,
                }
                .into());
            };
            if field == "#" {
                if subtype_depth != 0 {
                    return Err(StreamError {
                        format: StreamFormat::Text,
                        offset: at,
                        reason: record_error_reason(
                            ctx,
                            &name,
                            "terminates inside a subtype scope",
                        )?,
                    }
                    .into());
                }
                break;
            }
            let prim = lex_prim(ctx, &mut reader, at, field, &mut scratch)?;
            match prim {
                Prim::Open => subtype_depth += 1,
                Prim::Close if subtype_depth == 0 => {
                    return Err(StreamError {
                        format: StreamFormat::Text,
                        offset: at,
                        reason: record_error_reason(
                            ctx,
                            &name,
                            "closes an unopened subtype scope",
                        )?,
                    }
                    .into());
                }
                Prim::Close => subtype_depth -= 1,
                _ => {}
            }
            ctx.charge_collection_items(1, "frame SAT primitive")?;
            scratch.grow(std::mem::size_of::<Prim>() as u64)?;
            ctx.charge_work(1, "lex SAT primitive")?;
            prims.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("frame SAT primitive", 0, 1)
            })?;
            prims.push(prim);
        }
        let head = name.split_once('-').map_or(name.as_str(), |(head, _)| head);
        let candidates = head_shapes(head).len() + 1;
        let possible_tokens = prims
            .len()
            .checked_mul(candidates)
            .ok_or_else(|| ctx.refuse_codec_limit("SAT typed token count", u64::MAX, u64::MAX))?;
        ctx.charge_work(possible_tokens as u64, "type SAT tokens")?;
        let token_bytes = possible_tokens
            .checked_mul(std::mem::size_of::<Token>())
            .ok_or_else(|| ctx.refuse_codec_limit("SAT token bytes", u64::MAX, u64::MAX))?;
        scratch.grow(token_bytes as u64)?;
        let string_bytes = prims
            .iter()
            .try_fold(0usize, |used, prim| {
                let extra = match prim {
                    Prim::Str(value) | Prim::Word(value) => value.len(),
                    _ => 0,
                };
                used.checked_add(extra)
            })
            .ok_or_else(|| ctx.refuse_codec_limit("SAT token strings", u64::MAX, u64::MAX))?;
        ctx.charge_retained(string_bytes as u64, "retain SAT typed strings")?;
        let tokens = type_record(ctx, head, &prims, scale).map_err(|failure| match failure {
            TypedRecordFailure::Resource(error) => StreamFailure::Resource(error),
            TypedRecordFailure::Type(failure) => {
                let error = StreamError {
                    format: StreamFormat::Text,
                    offset: rec_start,
                    reason: failure.reason().to_string(),
                };
                match failure {
                    TypeFailure::UnrepresentableLength => StreamFailure::NotImplemented(error),
                    TypeFailure::InvalidSplineCount => StreamFailure::Malformed(error),
                }
            }
        })?;
        ctx.charge_retained(
            (tokens.len() * std::mem::size_of::<Token>()) as u64,
            "retain SAT typed tokens",
        )?;
        ctx.charge_collection_items(1, "frame SAT record")?;
        ctx.admit_entities(
            (records.len() + 1) as u64,
            &mut admitted_entities,
            "admit SAT native records",
        )?;
        records.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("frame SAT record", 0, 1)
        })?;
        records.push(Record {
            index: records.len(),
            name,

            tokens: tokens.into(),
            offset: rec_start,
            len: reader.pos - rec_start,
        });
    }
    let Some(terminator) = terminator else {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: reader.pos,
            reason: "stream has no End-of-ASM-data or End-of-ACIS-data line".to_string(),
        }
        .into());
    };
    reader.skip_ws();
    if reader.pos != bytes.len() {
        return Err(StreamError {
            format: StreamFormat::Text,
            offset: reader.pos,
            reason: "non-whitespace data follows the stream terminator".to_string(),
        }
        .into());
    }
    Ok(TextStream {
        header,
        records,
        terminator,
    })
}

fn lex_prim(
    ctx: &DecodeContext<'_>,
    reader: &mut FieldReader<'_>,
    at: usize,
    field: String,
    scratch: &mut ScopedReservation<'_>,
) -> Result<Prim, StreamFailure> {
    if let Some(rest) = field.strip_prefix('$') {
        let index = rest.parse::<i64>().map_err(|_| StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "reference field has no valid decimal index".to_string(),
        })?;
        return Ok(Prim::Ref(index));
    }
    if let Some(rest) = field.strip_prefix('@') {
        let len = rest.parse::<usize>().map_err(|_| StreamError {
            format: StreamFormat::Text,
            offset: at,
            reason: "string field has no valid decimal byte count".to_string(),
        })?;
        return Ok(Prim::Str(reader.read_str_payload(ctx, len, at, scratch)?));
    }
    if field == "{" {
        return Ok(Prim::Open);
    }
    if field == "}" {
        return Ok(Prim::Close);
    }
    let digits = field
        .strip_prefix('+')
        .or_else(|| field.strip_prefix('-'))
        .unwrap_or(&field);
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return field.parse::<i64>().map(Prim::Integer).map_err(|_| {
            StreamError {
                format: StreamFormat::Text,
                offset: at,
                reason: "integer field is outside the signed 64-bit range".to_string(),
            }
            .into()
        });
    }
    if let Ok(value) = field.parse::<f64>() {
        return Ok(Prim::Real(value));
    }
    Ok(Prim::Word(field))
}

// ---------------------------------------------------------------------------
// Typing: per-head shape grammars
// ---------------------------------------------------------------------------

/// One typed field slot of a fixed-shape record ([`asm.md` §5, §6]).
#[derive(Clone, Copy)]
enum Slot {
    /// Entity reference.
    R,
    /// `LONG` integer.
    L,
    /// `DOUBLE`, dimensionless.
    D,
    /// `DOUBLE`, a model-space length in the stream unit.
    DLen,
    /// `DOUBLE` length whose `-1` value is an unset sentinel and does not
    /// convert (tolerant-vertex tolerance slots).
    DLenSentinel,
    /// String.
    S,
    /// `POSITION`: three bare numbers, length-converted.
    P,
    /// `VECTOR_3D` carrying lengths (direction magnitude is a model length).
    VLen,
    /// `VECTOR_3D` carrying a unit direction; not converted.
    VUnit,
    /// Sense word: `forward` = `FALSE`, `reversed` = `TRUE`.
    Sense,
    /// Face sides word: `single` = `FALSE`, `double` = `TRUE`.
    Sides,
    /// Surface v-sense word: `forward_v` = `FALSE`, `reverse_v` = `TRUE`.
    UvSense,
    /// Plain logical word: `T`/`in`/property words = `TRUE`, `F`/`out`/`no_*`
    /// words = `FALSE`.
    B,
    /// Optional range bound: `I` = `FALSE` (unbounded); `F` = `TRUE` followed
    /// by one dimensionless `DOUBLE` (the bound value).
    OptB,
    /// One balanced subtype scope typed by the construction grammars.
    Sub,
}

/// Boolean word aliases for plain logical slots.
fn logical_word(word: &str) -> Option<Token> {
    match word {
        "T" | "in" | "rotate" | "reflect" | "shear" => Some(Token::True),
        "F" | "out" | "no_rotate" | "no_reflect" | "no_shear" => Some(Token::False),
        _ => None,
    }
}

/// Closure enumeration words (`nubs`/`nurbs` block headers).
const CLOSURE: &[(&str, i64)] = &[
    ("open", 0),
    ("closed", 1),
    ("periodic", 2),
    ("OPEN", 0),
    ("CLOSED", 1),
    ("PERIODIC", 2),
];
/// Singularity enumeration words (surface block headers).
const SINGULARITY: &[(&str, i64)] = &[("none", 0), ("NON_SINGULAR", 0)];
/// Approximation-cache form words (the `law_spl_sur` selector naming).
const CACHE_FORM: &[(&str, i64)] = &[
    ("full", 0),
    ("summary", 1),
    ("none", 2),
    ("historical", 3),
    ("optimal", 4),
];
/// Curve extension words.
const EXTENSION: &[(&str, i64)] = &[("UNEXTENDED", 0), ("EXTEND_G1", 1), ("EXTEND_213_G2", -1)];
/// Spring curve-direction words.
const CURV_DIR: &[(&str, i64)] = &[("left", 0), ("right", 2)];

macro_rules! push_token {
    ($cur:ident, $out:expr, $token:expr) => {{
        let token = $token;
        $cur.push_token($out, token);
    }};
}

/// A backtracking cursor over one record's primitive fields.
struct Cur<'a, 'c, 'p> {
    prims: &'a [Prim],
    pos: usize,
    /// Millimetres per stream length unit.
    scale: f64,
    failure: Option<TypeFailure>,
    resource: Option<cadmpeg_core::CodecError>,
    ctx: Option<&'c DecodeContext<'p>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeFailure {
    UnrepresentableLength,
    InvalidSplineCount,
}

enum TypedRecordFailure {
    Type(TypeFailure),
    Resource(cadmpeg_core::CodecError),
}

impl From<TypeFailure> for TypedRecordFailure {
    fn from(failure: TypeFailure) -> Self {
        Self::Type(failure)
    }
}

impl TypeFailure {
    fn reason(self) -> &'static str {
        match self {
            Self::UnrepresentableLength => "source length cannot be represented in centimetres",
            Self::InvalidSplineCount => "spline multiplicity or degree is invalid",
        }
    }
}

/// Keep the division after multiplication when a subnormal scale loses its
/// value in `scale / 10`; otherwise the first product avoids an intermediate
/// overflow for large source coordinates.
fn length_cm(value: f64, scale: f64) -> Option<f64> {
    let direct = value * (scale / 10.0);
    let converted = if value != 0.0 && direct == 0.0 {
        (value / 10.0) * scale
    } else {
        direct
    };
    (converted.is_finite() && (value == 0.0 || converted != 0.0)).then_some(converted)
}

impl<'a, 'c, 'p> Cur<'a, 'c, 'p> {
    fn push_token(&mut self, out: &mut Vec<Token>, token: Token) {
        if self.resource.is_some() {
            return;
        }
        if let Some(ctx) = self.ctx {
            if let Err(error) = ctx.charge_collection_items(1, "type SAT tokens") {
                self.resource = Some(error);
                return;
            }
            if out.try_reserve(1).is_err() {
                self.resource = Some(ctx.refuse_codec_limit("type SAT tokens", 0, 1));
                return;
            }
        }
        out.push(token);
    }

    fn push_text_token(&mut self, out: &mut Vec<Token>, value: &str, string: bool) {
        if self.resource.is_some() {
            return;
        }
        let copy = if let Some(ctx) = self.ctx {
            let requested = match u64::try_from(value.len()) {
                Ok(requested) => requested,
                Err(_) => {
                    self.resource = Some(ctx.refuse_codec_limit(
                        "retain SAT typed string",
                        u64::MAX,
                        u64::MAX,
                    ));
                    return;
                }
            };
            if let Err(error) = ctx.charge_retained(requested, "retain SAT typed string") {
                self.resource = Some(error);
                return;
            }
            match copy_sat_string(ctx, value, "SAT typed string") {
                Ok(copy) => copy,
                Err(error) => {
                    self.resource = Some(error);
                    return;
                }
            }
        } else {
            value.to_owned()
        };
        self.push_token(out, if string { Token::Str(copy) } else { Token::Ident(copy) });
    }

    /// Type a field by its written shape when no record grammar matches.
    fn push_lexical_token(&mut self, out: &mut Vec<Token>, prim: &Prim) {
        match prim {
            Prim::Integer(value) => self.push_token(out, Token::Long(*value)),
            Prim::Real(value) => self.push_token(out, Token::Double(*value)),
            Prim::Ref(index) => self.push_token(out, Token::Ref(*index)),
            Prim::Str(value) => self.push_text_token(out, value, true),
            Prim::Open => self.push_token(out, Token::SubtypeOpen),
            Prim::Close => self.push_token(out, Token::SubtypeClose),
            Prim::Word(word) => match word.as_str() {
                "forward" | "single" | "forward_v" | "I" | "F" | "out" => {
                    self.push_token(out, Token::False);
                }
                "reversed" | "double" | "reverse_v" | "T" | "in" => {
                    self.push_token(out, Token::True);
                }
                _ => self.push_text_token(out, word, false),
            },
        }
    }

    fn length(&mut self, value: f64) -> Option<f64> {
        match length_cm(value, self.scale) {
            Some(converted) => Some(converted),
            None => {
                self.failure = Some(TypeFailure::UnrepresentableLength);
                None
            }
        }
    }

    fn invalid_spline_count<T>(&mut self) -> Option<T> {
        self.failure = Some(TypeFailure::InvalidSplineCount);
        None
    }
    fn peek(&self) -> Option<&'a Prim> {
        self.prims.get(self.pos)
    }

    fn bump(&mut self) -> Option<&'a Prim> {
        let prim = self.prims.get(self.pos)?;
        self.pos += 1;
        Some(prim)
    }

    fn done(&self) -> bool {
        self.pos == self.prims.len()
    }

    fn num(&mut self) -> Option<f64> {
        match self.peek()? {
            Prim::Real(value) => {
                self.pos += 1;
                Some(*value)
            }
            Prim::Integer(value) => {
                self.pos += 1;
                Some(*value as f64)
            }
            _ => None,
        }
    }

    fn long(&mut self) -> Option<i64> {
        match self.peek()? {
            Prim::Integer(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    fn word(&mut self) -> Option<&'a str> {
        match self.peek()? {
            Prim::Word(word) => {
                self.pos += 1;
                Some(word)
            }
            _ => None,
        }
    }

    fn word_is(&mut self, expected: &str) -> Option<()> {
        match self.peek()? {
            Prim::Word(word) if word == expected => {
                self.pos += 1;
                Some(())
            }
            _ => None,
        }
    }

    fn enum_word(&mut self, vocab: &[(&str, i64)], out: &mut Vec<Token>) -> Option<()> {
        let word = self.word()?;
        let (_, value) = vocab.iter().find(|(name, _)| *name == word)?;
        push_token!(self, out, Token::Enum(*value));
        Some(())
    }

    /// Optional range bound: `I` is an absent bound; `F` is a present bound
    /// followed by one dimensionless value.
    fn opt_bound(&mut self, out: &mut Vec<Token>) -> Option<()> {
        match self.word()? {
            "I" => {
                push_token!(self, out, Token::False);
                Some(())
            }
            "F" => {
                let value = self.num()?;
                push_token!(self, out, Token::True);
                push_token!(self, out, Token::Double(value));
                Some(())
            }
            _ => None,
        }
    }

    fn triple(&mut self, scale: bool) -> Option<[f64; 3]> {
        let mark = self.pos;
        let mut values = [0.0; 3];
        for value in &mut values {
            let Some(number) = self.num() else {
                self.pos = mark;
                return None;
            };
            *value = if scale { self.length(number)? } else { number };
        }
        Some(values)
    }

    /// `LONG` count followed by that many dimensionless doubles.
    fn float_array(&mut self, out: &mut Vec<Token>) -> Option<()> {
        let mark = self.pos;
        let count = self.long()?;
        let count = usize::try_from(count).ok()?;
        if count > self.prims.len().checked_sub(self.pos)? {
            self.pos = mark;
            return None;
        }
        if let Some(ctx) = self.ctx {
            if let Err(error) = ctx.charge_collection_items(count as u64, "SAT float array values") {
                self.resource = Some(error);
                return None;
            }
        }
        let mut values = Vec::new();
        if values.try_reserve_exact(count).is_err() {
            self.resource = self.ctx.map(|ctx| ctx.refuse_codec_limit("SAT float array values", 0, count as u64));
            return None;
        }
        for _ in 0..count {
            let Some(value) = self.num() else {
                self.pos = mark;
                return None;
            };
            values.push(value);
        }
        let Some(output_count) = count.checked_add(1) else {
            self.resource = self.ctx.map(|ctx| ctx.refuse_codec_limit("SAT float array tokens", u64::MAX, u64::MAX));
            return None;
        };
        if let Some(ctx) = self.ctx {
            if let Err(error) = ctx.charge_collection_items(count as u64, "type SAT float array tokens") {
                self.resource = Some(error);
                return None;
            }
        }
        if out.try_reserve(output_count).is_err() {
            self.resource = self.ctx.map(|ctx| ctx.refuse_codec_limit("SAT float array tokens", 0, output_count as u64));
            return None;
        }
        push_token!(self, out, Token::Long(count as i64));
        out.extend(values.into_iter().map(Token::Double));
        Some(())
    }
}

/// Run one fixed slot against the cursor.
fn take_slot(cur: &mut Cur<'_, '_, '_>, slot: Slot, out: &mut Vec<Token>) -> Option<()> {
    match slot {
        Slot::R => match cur.bump()? {
            Prim::Ref(index) => {
                push_token!(cur, out, Token::Ref(*index));
                Some(())
            }
            _ => None,
        },
        Slot::L => {
            let value = cur.long()?;
            push_token!(cur, out, Token::Long(value));
            Some(())
        }
        Slot::D => {
            let value = cur.num()?;
            push_token!(cur, out, Token::Double(value));
            Some(())
        }
        Slot::DLen => {
            let value = cur.num()?;
            push_token!(cur, out, Token::Double(cur.length(value)?));
            Some(())
        }
        Slot::DLenSentinel => {
            let value = cur.num()?;
            let converted = if value == -1.0 {
                value
            } else {
                cur.length(value)?
            };
            push_token!(cur, out, Token::Double(converted));
            Some(())
        }
        Slot::S => match cur.bump()? {
            Prim::Str(value) => {
                cur.push_text_token(out, value, true);
                Some(())
            }
            _ => None,
        },
        Slot::P => {
            let triple = cur.triple(true)?;
            push_token!(cur, out, Token::Position(triple));
            Some(())
        }
        Slot::VLen => {
            let triple = cur.triple(true)?;
            push_token!(cur, out, Token::Vector3(triple));
            Some(())
        }
        Slot::VUnit => {
            let triple = cur.triple(false)?;
            push_token!(cur, out, Token::Vector3(triple));
            Some(())
        }
        Slot::Sense => match cur.word()? {
            "forward" => {
                push_token!(cur, out, Token::False);
                Some(())
            }
            "reversed" => {
                push_token!(cur, out, Token::True);
                Some(())
            }
            _ => None,
        },
        Slot::Sides => match cur.word()? {
            "single" => {
                push_token!(cur, out, Token::False);
                Some(())
            }
            "double" => {
                push_token!(cur, out, Token::True);
                Some(())
            }
            _ => None,
        },
        Slot::UvSense => match cur.word()? {
            "forward_v" => {
                push_token!(cur, out, Token::False);
                Some(())
            }
            "reverse_v" => {
                push_token!(cur, out, Token::True);
                Some(())
            }
            _ => None,
        },
        Slot::B => {
            let token = logical_word(cur.word()?)?;
            push_token!(cur, out, token);
            Some(())
        }
        Slot::OptB => cur.opt_bound(out),
        Slot::Sub => type_subtype(cur, out),
    }
}

fn run_shape(cur: &mut Cur<'_, '_, '_>, slots: &[Slot], out: &mut Vec<Token>) -> Option<()> {
    for slot in slots {
        take_slot(cur, *slot, out)?;
    }
    Some(())
}

/// Try one candidate shape against the complete field list.
fn try_shape(
    ctx: &DecodeContext<'_>,
    prims: &[Prim],
    scale: f64,
    slots: &[Slot],
) -> Result<Option<Vec<Token>>, TypedRecordFailure> {
    let mut cur = Cur {
        prims,
        pos: 0,
        scale,
        failure: None,
        resource: None,
        ctx: Some(ctx),
    };
    let mut out = Vec::new();
    let matched = run_shape(&mut cur, slots, &mut out).is_some() && cur.done();
    match (cur.resource, cur.failure) {
        (Some(error), _) => Err(TypedRecordFailure::Resource(error)),
        (None, Some(failure)) => Err(TypedRecordFailure::Type(failure)),
        (None, None) => Ok(matched.then_some(out)),
    }
}

// ---------------------------------------------------------------------------
// Construction grammars (subtype scopes)
// ---------------------------------------------------------------------------

/// Coordinate domain of one B-spline curve block.
#[derive(Clone, Copy)]
enum BsKind {
    /// Two unscaled surface parameters per pole.
    Parameter,
    /// Three model-space lengths per pole.
    Model,
}

/// Read one spline knot vector and derive its pole count without signed
/// overflow or an unchecked loop count.
fn spline_poles(cur: &mut Cur<'_, '_, '_>, knots: i64, degree: i64, out: &mut Vec<Token>) -> Option<usize> {
    let Some(knot_count) = usize::try_from(knots).ok() else {
        return cur.invalid_spline_count();
    };
    if degree < 1 || knot_count < 2 || knot_count > (cur.prims.len() - cur.pos) / 2 {
        return cur.invalid_spline_count();
    }
    let mut sum = 0i64;
    for _ in 0..knot_count {
        let knot = cur.num()?;
        let mult = cur.long()?;
        if mult < 1 {
            return cur.invalid_spline_count();
        }
        sum = match sum.checked_add(mult) {
            Some(value) => value,
            None => return cur.invalid_spline_count(),
        };
        push_token!(cur, out, Token::Double(knot));
        push_token!(cur, out, Token::Long(mult));
    }
    let Some(poles) = degree
        .checked_sub(1)
        .and_then(|endpoint_adjustment| sum.checked_sub(endpoint_adjustment))
        .and_then(|count| usize::try_from(count).ok())
    else {
        return cur.invalid_spline_count();
    };
    if poles < 2 {
        return cur.invalid_spline_count();
    }
    Some(poles)
}

/// A `nubs`/`nurbs` curve block ([`asm.md` §6.5]): marker, degree, closure,
/// unique-knot count, `(knot, multiplicity)` pairs, poles, and for `nurbs`
/// one weight per pole.
fn bs_curve_block(cur: &mut Cur<'_, '_, '_>, kind: BsKind, out: &mut Vec<Token>) -> Option<()> {
    let marker = cur.word()?;
    let rational = match marker {
        "nubs" => false,
        "nurbs" => true,
        _ => return None,
    };
    push_token!(cur, out, Token::Ident(marker.to_string()));
    let degree = cur.long()?;
    push_token!(cur, out, Token::Long(degree));
    cur.enum_word(CLOSURE, out)?;
    let knots = cur.long()?;
    push_token!(cur, out, Token::Long(knots));
    let poles = spline_poles(cur, knots, degree, out)?;
    let coords = match kind {
        BsKind::Parameter => 2,
        BsKind::Model => 3,
    };
    let per_pole = coords + usize::from(rational);
    if poles > (cur.prims.len() - cur.pos) / per_pole {
        return cur.invalid_spline_count();
    }
    for _ in 0..poles {
        for coordinate in 0..per_pole {
            let value = cur.num()?;
            let scaled = matches!(kind, BsKind::Model) && coordinate < coords;
            push_token!(cur, out, Token::Double(if scaled {
                cur.length(value)?
            } else {
                value
            }));
        }
    }
    Some(())
}

/// A `nubs`/`nurbs` surface block ([`asm.md` §6.5]): marker, U/V degrees,
/// closures, singularities, unique-knot counts, both knot vectors, and the
/// row-major control grid.
fn bs_surface_block(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    let marker = cur.word()?;
    let rational = match marker {
        "nubs" => false,
        "nurbs" => true,
        _ => return None,
    };
    push_token!(cur, out, Token::Ident(marker.to_string()));
    let degree_u = cur.long()?;
    let degree_v = cur.long()?;
    push_token!(cur, out, Token::Long(degree_u));
    push_token!(cur, out, Token::Long(degree_v));
    cur.enum_word(CLOSURE, out)?;
    cur.enum_word(CLOSURE, out)?;
    cur.enum_word(SINGULARITY, out)?;
    cur.enum_word(SINGULARITY, out)?;
    let knots_u = cur.long()?;
    let knots_v = cur.long()?;
    push_token!(cur, out, Token::Long(knots_u));
    push_token!(cur, out, Token::Long(knots_v));
    let mut poles = [0usize; 2];
    for (direction, (knots, degree)) in [(knots_u, degree_u), (knots_v, degree_v)]
        .into_iter()
        .enumerate()
    {
        poles[direction] = spline_poles(cur, knots, degree, out)?;
    }
    let per_pole = 3 + usize::from(rational);
    let Some(total_poles) = poles[0].checked_mul(poles[1]) else {
        return cur.invalid_spline_count();
    };
    if total_poles > (cur.prims.len() - cur.pos) / per_pole {
        return cur.invalid_spline_count();
    }
    for _ in 0..total_poles {
        for coordinate in 0..per_pole {
            let value = cur.num()?;
            push_token!(cur, out, Token::Double(if coordinate < 3 {
                cur.length(value)?
            } else {
                value
            }));
        }
    }
    Some(())
}

/// `exact_int_cur` / `exactcur` payload after the subtype name
/// ([`asm.md` §6.3]): optional serializer stamp, cache-form enum, the solved
/// curve cache and fit tolerance, two null supports, two null pcurves, two
/// optional interval endpoints, three discontinuity arrays, the extension
/// integer, the unextended-range pair, and two extension enums.
fn exact_int_cur_tail(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    if let Some(stamp) = match cur.peek() { Some(Prim::Integer(value)) => Some(*value), _ => None } {
        // A serializer stamp is a release x100 word; smaller integers open
        // the legacy layout without a stamp.
        if stamp >= 100 {
            cur.long();
            push_token!(cur, out, Token::Long(stamp));
        }
    }
    cur.enum_word(CACHE_FORM, out)?;
    bs_curve_block(cur, BsKind::Model, out)?;
    let tolerance = cur.num()?;
    push_token!(cur, out, Token::Double(cur.length(tolerance)?));
    for _ in 0..2 {
        cur.word_is("null_surface")?;
        push_token!(cur, out, Token::Ident("null_surface".to_string()));
    }
    for _ in 0..2 {
        cur.word_is("nullbs")?;
        push_token!(cur, out, Token::Ident("nullbs".to_string()));
    }
    cur.opt_bound(out)?;
    cur.opt_bound(out)?;
    for _ in 0..3 {
        cur.float_array(out)?;
    }
    let extension = cur.long()?;
    push_token!(cur, out, Token::Long(extension));
    cur.opt_bound(out)?;
    cur.opt_bound(out)?;
    // Two extension enums close the modern tail; the legacy tail omits them.
    if matches!(cur.peek(), Some(Prim::Word(word)) if word == "UNEXTENDED") {
        cur.enum_word(EXTENSION, out)?;
        cur.enum_word(EXTENSION, out)?;
    }
    Some(())
}

/// `exp_par_cur` / `exppc` payload after the subtype name ([`asm.md` §6.4]):
/// the inline BS2 block, its parameter-space fit tolerance, the support
/// surface scope, and four trailing booleans.
fn exp_par_cur_tail(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    bs_curve_block(cur, BsKind::Parameter, out)?;
    let tolerance = cur.num()?;
    push_token!(cur, out, Token::Double(tolerance));
    cur.word_is("spline")?;
    push_token!(cur, out, Token::Ident("spline".to_string()));
    let sense = cur.word()?;
    push_token!(cur, out, match sense {
        "forward" => Token::False,
        "reversed" => Token::True,
        _ => return None,
    });
    type_subtype(cur, out)?;
    for _ in 0..4 {
        cur.opt_bound(out)?;
    }
    Some(())
}

/// `exact_spl_sur` / `exactsur` payload after the subtype name
/// ([`asm.md` §6.3]): optional stamp, cache-form enum, the solved surface,
/// fit tolerance, the U and V intervals, and the extension integer.
fn exact_spl_sur_tail(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    if let Some(stamp) = match cur.peek() { Some(Prim::Integer(value)) => Some(*value), _ => None } {
        if stamp >= 100 {
            cur.long();
            push_token!(cur, out, Token::Long(stamp));
        }
    }
    cur.enum_word(CACHE_FORM, out)?;
    bs_surface_block(cur, out)?;
    let tolerance = cur.num()?;
    push_token!(cur, out, Token::Double(cur.length(tolerance)?));
    for _ in 0..4 {
        let value = cur.num()?;
        push_token!(cur, out, Token::Double(value));
    }
    let extension = cur.long()?;
    push_token!(cur, out, Token::Long(extension));
    Some(())
}

/// A sense word: `forward` is `FALSE`, `reversed` is `TRUE`.
fn sense_word(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    match cur.word()? {
        "forward" => push_token!(cur, out, Token::False),
        "reversed" => push_token!(cur, out, Token::True),
        _ => return None,
    }
    Some(())
}

/// One nullable support-surface slot: the `null_surface` sentinel, a `spline`
/// reference or inline construction with its boolean and four optional
/// bounds, or an embedded analytic surface ([`asm.md` §6.3]).
fn nullable_surface(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    let word = match cur.peek()? {
        Prim::Word(word) => word.as_str(),
        _ => return None,
    };
    match word {
        "null_surface" => {
            cur.bump();
            push_token!(cur, out, Token::Ident("null_surface".to_string()));
            Some(())
        }
        "spline" => {
            cur.bump();
            push_token!(cur, out, Token::Ident("spline".to_string()));
            sense_word(cur, out)?;
            type_subtype_tabled(cur, out)?;
            for _ in 0..4 {
                cur.opt_bound(out)?;
            }
            Some(())
        }
        "plane" => {
            cur.bump();
            push_token!(cur, out, Token::Ident("plane".to_string()));
            for slot in [Slot::P, Slot::VUnit, Slot::VLen, Slot::UvSense] {
                take_slot(cur, slot, out)?;
            }
            for _ in 0..4 {
                cur.opt_bound(out)?;
            }
            Some(())
        }
        "cone" => {
            cur.bump();
            push_token!(cur, out, Token::Ident("cone".to_string()));
            for slot in [Slot::P, Slot::VUnit, Slot::VLen, Slot::D] {
                take_slot(cur, slot, out)?;
            }
            cur.opt_bound(out)?;
            cur.opt_bound(out)?;
            for slot in [Slot::D, Slot::D, Slot::DLen, Slot::Sense] {
                take_slot(cur, slot, out)?;
            }
            for _ in 0..4 {
                cur.opt_bound(out)?;
            }
            Some(())
        }
        "sphere" => {
            cur.bump();
            push_token!(cur, out, Token::Ident("sphere".to_string()));
            for slot in [Slot::P, Slot::DLen, Slot::VUnit, Slot::VUnit, Slot::UvSense] {
                take_slot(cur, slot, out)?;
            }
            for _ in 0..4 {
                cur.opt_bound(out)?;
            }
            Some(())
        }
        "torus" => {
            cur.bump();
            push_token!(cur, out, Token::Ident("torus".to_string()));
            for slot in [
                Slot::P,
                Slot::VUnit,
                Slot::DLen,
                Slot::DLen,
                Slot::VUnit,
                Slot::UvSense,
            ] {
                take_slot(cur, slot, out)?;
            }
            for _ in 0..4 {
                cur.opt_bound(out)?;
            }
            Some(())
        }
        _ => None,
    }
}

/// One nullable BS2 parameter-curve slot: the `nullbs` sentinel or an inline
/// 2D block without a fit-tolerance field.
fn nullable_bs2(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    if matches!(cur.peek(), Some(Prim::Word(word)) if word == "nullbs") {
        cur.bump();
        push_token!(cur, out, Token::Ident("nullbs".to_string()));
        return Some(());
    }
    bs_curve_block(cur, BsKind::Parameter, out)
}

/// The shared cache-first intcurve context ([`asm.md` §6.3]): serializer
/// stamp, the `full` approximation form with the solved curve cache and fit
/// tolerance, two supports, two parameter curves, two optional interval
/// endpoints, three discontinuity arrays, and the extension integer. The
/// cacheless form selects a different payload and is not typed here.
fn cache_first_curve_context(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    let stamp = cur.long()?;
    push_token!(cur, out, Token::Long(stamp));
    cur.word_is("full")?;
    push_token!(cur, out, Token::Enum(0));
    bs_curve_block(cur, BsKind::Model, out)?;
    let tolerance = cur.num()?;
    push_token!(cur, out, Token::Double(cur.length(tolerance)?));
    nullable_surface(cur, out)?;
    nullable_surface(cur, out)?;
    nullable_bs2(cur, out)?;
    nullable_bs2(cur, out)?;
    cur.opt_bound(out)?;
    cur.opt_bound(out)?;
    for _ in 0..3 {
        cur.float_array(out)?;
    }
    let extension = cur.long()?;
    push_token!(cur, out, Token::Long(extension));
    Some(())
}

/// The shared revision-gated surface tail ([`asm.md` §6.3]): the
/// approximation form, its payload, six discontinuity arrays, and one
/// boolean. Form `full` stores the solved surface and fit tolerance; form
/// `none` stores the U and V intervals and four closure/singularity enums.
fn revision_surface_tail(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    match cur.word()? {
        "full" => {
            push_token!(cur, out, Token::Enum(0));
            bs_surface_block(cur, out)?;
            let tolerance = cur.num()?;
            push_token!(cur, out, Token::Double(cur.length(tolerance)?));
        }
        "none" => {
            push_token!(cur, out, Token::Enum(2));
            for _ in 0..4 {
                cur.opt_bound(out)?;
            }
            cur.enum_word(CLOSURE, out)?;
            cur.enum_word(CLOSURE, out)?;
            cur.enum_word(SINGULARITY, out)?;
            cur.enum_word(SINGULARITY, out)?;
        }
        _ => return None,
    }
    for _ in 0..6 {
        cur.float_array(out)?;
    }
    let flag = logical_word(cur.word()?)?;
    push_token!(cur, out, flag);
    Some(())
}

/// `cyl_spl_sur` revision-gated payload after the subtype name: serializer
/// stamp, the embedded directrix intcurve with two optional parameter
/// endpoints, the extrusion direction, the native position, and the shared
/// revision-gated surface tail.
fn cyl_spl_sur_tail(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    let stamp = cur.long()?;
    push_token!(cur, out, Token::Long(stamp));
    cur.word_is("intcurve")?;
    push_token!(cur, out, Token::Ident("intcurve".to_string()));
    sense_word(cur, out)?;
    type_subtype_tabled(cur, out)?;
    cur.opt_bound(out)?;
    cur.opt_bound(out)?;
    take_slot(cur, Slot::VLen, out)?;
    take_slot(cur, Slot::P, out)?;
    revision_surface_tail(cur, out)
}

/// `exact_spl_sur` revision-gated payload: serializer stamp, the shared
/// surface tail, the U and V unextended intervals as optional bounds, and
/// the extension enum.
fn exact_spl_sur_revision_tail(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    let stamp = cur.long()?;
    push_token!(cur, out, Token::Long(stamp));
    revision_surface_tail(cur, out)?;
    for _ in 0..4 {
        cur.opt_bound(out)?;
    }
    cur.enum_word(EXTENSION, out)
}

/// Type one balanced subtype scope. The scope opens with `{` and a
/// construction name; tabled constructions get their grammar, and any other
/// construction falls back to lexical typing of the balanced scope.
fn type_subtype(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    let scope_start = cur.pos;
    let out_mark = out.len();
    if type_subtype_tabled(cur, out).is_some() {
        return Some(());
    }
    cur.pos = scope_start;
    out.truncate(out_mark);
    fallback_scope(cur, out)
}

/// Type one balanced subtype scope through a tabled construction grammar,
/// with no lexical rescue. A grammar whose interior must decode — a support
/// or directrix the shared decoders resolve — requires this form, so a
/// record with an untypable interior falls back as a whole instead of
/// decoding around a degraded nested construction.
fn type_subtype_tabled(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    if !matches!(cur.peek(), Some(Prim::Open)) {
        return None;
    }
    let scope_start = cur.pos;
    let out_mark = out.len();
    cur.bump();
    push_token!(cur, out, Token::SubtypeOpen);
    let Some(name) = cur.word() else {
        cur.pos = scope_start;
        out.truncate(out_mark);
        return None;
    };
    cur.push_text_token(out, name, false);
    let matched = match name {
        "ref" => cur.long().map(|index| push_token!(cur, out, Token::Long(index))),
        "exp_par_cur" | "exppc" => exp_par_cur_tail(cur, out),
        "exact_int_cur" | "exactcur" => exact_int_cur_tail(cur, out),
        "exact_spl_sur" | "exactsur" => {
            let mark = (cur.pos, out.len());
            exact_spl_sur_tail(cur, out).or_else(|| {
                cur.pos = mark.0;
                out.truncate(mark.1);
                exact_spl_sur_revision_tail(cur, out)
            })
        }
        "int_int_cur" => cache_first_curve_context(cur, out),
        "par_int_cur" => cache_first_curve_context(cur, out).and_then(|()| {
            for _ in 0..2 {
                let flag = logical_word(cur.word()?)?;
                push_token!(cur, out, flag);
            }
            Some(())
        }),
        "blend_int_cur" => cache_first_curve_context(cur, out).and_then(|()| {
            let flag = logical_word(cur.word()?)?;
            push_token!(cur, out, flag);
            Some(())
        }),
        "spring_int_cur" => {
            cache_first_curve_context(cur, out).and_then(|()| cur.enum_word(CURV_DIR, out))
        }
        "cyl_spl_sur" => cyl_spl_sur_tail(cur, out),
        _ => None,
    };
    let closed = matched.and_then(|()| match cur.peek() {
        Some(Prim::Close) => {
            cur.bump();
            push_token!(cur, out, Token::SubtypeClose);
            Some(())
        }
        _ => None,
    });
    if closed.is_some() {
        return Some(());
    }
    cur.pos = scope_start;
    out.truncate(out_mark);
    None
}

/// Lexically type one balanced subtype scope, `{` through its matching `}`.
fn fallback_scope(cur: &mut Cur<'_, '_, '_>, out: &mut Vec<Token>) -> Option<()> {
    if !matches!(cur.peek(), Some(Prim::Open)) {
        return None;
    }
    let mut depth = 0usize;
    loop {
        let prim = cur.bump()?;
        cur.push_lexical_token(out, prim);
        match prim {
            Prim::Open => depth += 1,
            Prim::Close => {
                depth -= 1;
                if depth == 0 {
                    return Some(());
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Head shape tables
// ---------------------------------------------------------------------------

use Slot::{DLen, DLenSentinel, OptB, Sense, Sides, Sub, UvSense, VLen, VUnit, B, D, L, P, R, S};

// Every entity record opens with the base fields the `shape!` macro
// prepends: the attribute-chain head reference, one integer, and one
// reference ([`asm.md` §5.2]).
macro_rules! shape {
    ($($slot:expr),* $(,)?) => {
        &[R, L, R, $($slot),*]
    };
}

/// Candidate shapes per record head, most specific first ([`asm.md` §5, §6]).
/// A head absent from this table, or a record no candidate matches exactly,
/// is typed lexically.
fn head_shapes(head: &str) -> &'static [&'static [Slot]] {
    match head {
        "asmheader" => &[&[R, L, S]],
        "body" => &[shape![R, R, R]],
        "lump" => &[shape![R, R, R]],
        "shell" | "subshell" => &[shape![R, R, R, R, R]],
        "wire" => &[shape![R, R, R, R, B]],
        // `face` sides selects the trailing containment chunk: single-sided
        // faces end at the sides word, double-sided faces carry one more
        // boolean ([`asm.md` §5.2]).
        "face" => &[
            shape![R, R, R, R, R, Sense, Sides],
            shape![R, R, R, R, R, Sense, Sides, B],
        ],
        "loop" => &[shape![R, R, R]],
        "coedge" => &[shape![R, R, R, R, Sense, R, L, R]],
        "tcoedge" => &[
            shape![R, R, R, R, Sense, R, L, R, D, D, R, L, L],
            shape![R, R, R, R, Sense, R, L, R, D, D, R],
            shape![R, R, R, R, Sense, R, L, R, D, D],
            // Save format 700 stores the tolerant coedge without the base
            // reserved integer; the two parameters stay doubles.
            shape![R, R, R, R, Sense, R, R, D, D],
        ],
        "edge" => &[shape![R, D, R, D, R, R, Sense, S]],
        "tedge" => &[
            shape![R, D, R, D, R, R, Sense, S, DLen, L, L],
            shape![R, D, R, D, R, R, Sense, S, DLen, L],
            shape![R, D, R, D, R, R, Sense, S, DLen],
        ],
        // Save format 700 stores the vertex without the endpoint-index
        // integer: owning edge, then point.
        "vertex" => &[shape![R, L, R], shape![R, R]],
        "tvertex" => &[
            shape![R, L, R, DLenSentinel, DLenSentinel, DLenSentinel, L],
            shape![R, L, R, DLenSentinel, DLenSentinel, DLenSentinel],
            // Save format 700 stores one tolerance and no endpoint index.
            shape![R, R, DLen],
        ],
        "point" => &[shape![P]],
        // A transform has a two-field base: the attribute head and one
        // integer, then the three rotation columns, the translation, the
        // overall scale, and the three classification booleans
        // ([`asm.md` §5.2]).
        "transform" => &[&[R, L, VUnit, VUnit, VUnit, VLen, D, B, B, B]],
        "straight" => &[shape![P, VLen, OptB, OptB]],
        "ellipse" => &[shape![P, VUnit, VLen, D, OptB, OptB]],
        "degenerate_curve" => &[shape![P, OptB, OptB]],
        "intcurve" => &[shape![Sense, Sub, OptB, OptB]],
        "plane" => &[shape![P, VUnit, VLen, UvSense, OptB, OptB, OptB, OptB]],
        "cone" => &[shape![
            P, VUnit, VLen, D, OptB, OptB, D, D, DLen, Sense, OptB, OptB, OptB, OptB
        ]],
        "sphere" => &[shape![
            P, DLen, VUnit, VUnit, UvSense, OptB, OptB, OptB, OptB
        ]],
        "torus" => &[shape![
            P, VUnit, DLen, DLen, VUnit, UvSense, OptB, OptB, OptB, OptB
        ]],
        "spline" => &[shape![Sense, Sub, OptB, OptB, OptB, OptB]],
        "pcurve" => &[
            // Ref form: nonzero discriminator, intcurve reference, interval.
            shape![L, R, D, D],
            // Wrapped form: zero discriminator, wrapper boolean, one balanced
            // subtype, interval.
            shape![L, Sense, Sub, D, D],
        ],
        "rgb_color" => &[shape![R, R, D, D, D, D], shape![R, R, D, D, D]],
        "ATTRIB_CUSTOM" => &[shape![R, R, S, L, D], shape![R, R, S, L, L, L, S]],
        "DXID" => &[shape![R, R, S, S]],
        _ => &[],
    }
}

/// Type one record's payload. Every tabled candidate must consume the
/// complete field list; otherwise the record is typed lexically, one token
/// per field.
fn type_record(
    ctx: &DecodeContext<'_>,
    head: &str,
    prims: &[Prim],
    scale: f64,
) -> Result<Vec<Token>, TypedRecordFailure> {
    for slots in head_shapes(head) {
        if let Some(tokens) = try_shape(ctx, prims, scale, slots)? {
            return Ok(tokens);
        }
    }
    let mut cur = Cur {
        prims,
        pos: 0,
        scale,
        failure: None,
        resource: None,
        ctx: Some(ctx),
    };
    let mut tokens = Vec::new();
    for prim in prims {
        cur.push_lexical_token(&mut tokens, prim);
    }
    if let Some(error) = cur.resource {
        return Err(TypedRecordFailure::Resource(error));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sat_float_array_values_refuse_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let prims = [Prim::Integer(1), Prim::Real(2.0)];
        let mut cur = Cur {
            prims: &prims,
            pos: 0,
            scale: 10.0,
            failure: None,
            resource: None,
            ctx: Some(&ctx),
        };
        let mut output = Vec::new();
        assert_eq!(cur.float_array(&mut output), None);
        let error = cur.resource.expect("resource refusal");
        let CodecError::ResourceLimit(limit) = error else { panic!("expected resource refusal: {error:?}") };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "SAT float array values");
    }

    #[test]
    fn sat_fallback_typed_tokens_refuse_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let prims = [Prim::Integer(4)];
        let error = super::type_record(&ctx, "unknown", &prims, 10.0).err().expect("resource refusal");
        let super::TypedRecordFailure::Resource(CodecError::ResourceLimit(limit)) = error else {
            panic!("expected resource refusal")
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "type SAT tokens");
    }

    fn assert_sat_frame_collection_limit(max_items: u64, operation: &str) {
        let source = asm_stream("audit 1 #\n");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
        let error = super::parse(&ctx, &source).err().expect("collection refusal");
        let StreamFailure::Resource(CodecError::ResourceLimit(limit)) = error else {
            panic!("expected resource refusal: {error:?}")
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, operation);
    }

    #[test]
    fn sat_primitives_refuse_collection_limit() {
        assert_sat_frame_collection_limit(0, "frame SAT primitive");
    }

    #[test]
    fn sat_records_refuse_collection_limit() {
        assert_sat_frame_collection_limit(2, "frame SAT record");
    }

    #[test]
    fn counted_float_array_refuses_an_unavailable_huge_count_without_allocating() {
        let prims = [Prim::Integer(i64::MAX)];
        let mut cur = Cur {
            prims: &prims,
            pos: 0,
            scale: 10.0,
            failure: None,
            resource: None,
            ctx: None,
        };
        let mut output = Vec::new();
        assert_eq!(cur.float_array(&mut output), None);
        assert_eq!(cur.pos, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn numerical_audit_sat_integer_fields_preserve_exact_values_and_reject_overflow() {
        let stream = parse(&asm_stream(
            "audit-integer 9007199254740993 -9223372036854775808 9223372036854775807 #\n",
        ))
        .unwrap();
        let tokens = &stream.records[0].tokens;
        assert!(tokens.contains(&Token::Long(9_007_199_254_740_993)));
        assert!(tokens.contains(&Token::Long(i64::MIN)));
        assert!(tokens.contains(&Token::Long(i64::MAX)));
        for value in ["9223372036854775808", "-9223372036854775809"] {
            assert!(parse(&asm_stream(&format!("audit-integer {value} #\n"))).is_err());
        }
    }

    use super::{
        bs_curve_block, bs_surface_block, BsKind, Cur, Prim, Terminator, TextStream, TypeFailure,
    };
    use crate::sab::Token;
    use crate::stream_error::{StreamError, StreamFailure};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::scalar::{NonNegativeReal, PositiveReal};

    fn kernel_header(header: &super::TextHeader) -> crate::kernel_header::KernelHeader {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test input fits input limit");
        header
            .as_kernel_header(&ctx)
            .expect("service profile admits header copy")
    }

    #[test]
    fn sat_framing_refuses_each_resource_before_record_materialization() {
        type LimitCase = (ResourceDimension, fn(&mut DecodePolicy));
        let source = asm_stream("point $-1 -1 $-1 10 0 0 #\n");
        let cases: [LimitCase; 5] = [
            (ResourceDimension::RetainedBytes, |policy| {
                policy.limits.max_retained_bytes = 0;
            }),
            (ResourceDimension::Entities, |policy| {
                policy.limits.max_entities = 1;
            }),
            (ResourceDimension::CollectionItems, |policy| {
                policy.limits.max_collection_items = 0;
            }),
            (ResourceDimension::MaterializedBytes, |policy| {
                policy.limits.max_materialized_bytes = 0;
            }),
            (ResourceDimension::WorkUnits, |policy| {
                policy.limits.max_work_units = 0;
            }),
        ];
        for (expected, set_limit) in cases {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            set_limit(&mut policy);
            let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
                .expect("source fits input limit");
            let error = super::parse(&ctx, &source).expect_err("resource limit must refuse");
            let StreamFailure::Resource(CodecError::ResourceLimit(limit)) = error else {
                panic!("expected resource refusal, got {error:?}");
            };
            assert_eq!(limit.dimension, expected);
        }
        assert_eq!(
            parse(&source)
                .expect("service profile admits point")
                .records
                .len(),
            1
        );
    }

    #[test]
    fn sat_string_copies_refuse_at_header_field_and_payload_limits() {
        let source = asm_stream("mystery @3 abc #\n");
        let cases = [
            (ResourceDimension::RetainedBytes, 15, "retain SAT header string"),
            (ResourceDimension::RetainedBytes, 67, "retain SAT record name"),
            (ResourceDimension::MaterializedBytes, 4, "frame SAT record"),
        ];
        for (dimension, limit, operation) in cases {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
                ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = limit;
                }
                _ => panic!("unexpected test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
                .expect("source fits input limit");
            let error = super::parse(&ctx, &source).expect_err("string limit must refuse");
            let StreamFailure::Resource(CodecError::ResourceLimit(refusal)) = error else {
                panic!("expected resource refusal, got {error:?}");
            };
            assert_eq!(refusal.dimension, dimension);
            assert_eq!(refusal.operation, operation);
        }
    }

    #[test]
    fn sat_typed_string_copies_refuse_retained_limit() {
        for (body, limit) in [
            ("mystery @3 abc #\n", 73),
            ("asmheader $-1 -1 @3 abc #\n", 75),
        ] {
            let source = asm_stream(body);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
                .expect("source fits input limit");
            let error = super::parse(&ctx, &source).expect_err("typed string limit must refuse");
            let StreamFailure::Resource(CodecError::ResourceLimit(refusal)) = error else {
                panic!("expected resource refusal, got {error:?}");
            };
            assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(refusal.operation, "retain SAT typed string");
        }
    }

    #[test]
    fn sat_kernel_header_copy_refuses_retained_limit() {
        let source = asm_stream("");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 91;
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
            .expect("source fits input limit");
        let stream = super::parse(&ctx, &source).expect("header and terminator fit retained limit");
        let error = stream
            .header
            .as_kernel_header(&ctx)
            .expect_err("kernel header copy exceeds retained limit");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("expected resource refusal, got {error:?}");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "retain SAT kernel header string");
    }

    #[test]
    fn sat_record_error_text_refuses_each_input_name_limit() {
        for (body, description) in [
            ("mystery 1\n", "has no `#` terminator"),
            ("mystery { #\n", "terminates inside a subtype scope"),
            ("mystery } #\n", "closes an unopened subtype scope"),
        ] {
            let source = asm_stream(body);
            let reason = format!("record `mystery` {description}");
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 16 + 21 + 24 + 7 + reason.len() as u64 - 1;
            let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
                .expect("source fits input limit");
            let error = super::parse(&ctx, &source).expect_err("error text exceeds retained limit");
            let StreamFailure::Resource(CodecError::ResourceLimit(refusal)) = error else {
                panic!("expected resource refusal, got {error:?}");
            };
            assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(refusal.operation, "SAT record error text");
        }
    }

    fn parse(bytes: &[u8]) -> Result<TextStream, StreamError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("test stream fits the service input limit");
        match super::parse(&ctx, bytes) {
            Ok(stream) => Ok(stream),
            Err(
                StreamFailure::Parse(error)
                | StreamFailure::Malformed(error)
                | StreamFailure::NotImplemented(error),
            ) => Err(error),
            Err(StreamFailure::Resource(error)) => {
                panic!("test stream exhausted a resource: {error}")
            }
        }
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1.0e-12 * a.abs().max(b.abs()).max(1.0)
    }

    fn asm_stream(body: &str) -> Vec<u8> {
        let mut text = String::from("23200 0 2 2 \n");
        text.push_str(
            "16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 24 Fri Jul 17 14:46:47 2026 \n",
        );
        text.push_str("1 1e-06 1.0e-10 \n");
        text.push_str(body);
        text.push_str("End-of-ASM-data \n");
        text.into_bytes()
    }

    #[test]
    fn both_dialect_headers_parse_and_record_their_terminator() {
        let asm = parse(&asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n")).expect("asm stream");
        assert_eq!(asm.terminator, Terminator::Asm);
        assert_eq!(asm.header.save_format_version, 23200);
        assert_eq!(asm.header.entity_count, 2);
        assert_eq!(asm.header.flags, 2);
        assert_eq!(asm.header.product_family, "Autodesk Neutron");
        assert_eq!(asm.header.product_version, "ASM 232.4.0.65535 OSX");
        assert_eq!(asm.header.save_date, "Fri Jul 17 14:46:47 2026");
        assert!(approx(asm.header.scale().get(), 1.0));
        assert!(approx(asm.header.resabs().get(), 1.0e-6));
        assert!(approx(asm.header.resnor().get(), 1.0e-10));
        assert_eq!(asm.records.len(), 1);
        assert_eq!(asm.records[0].name, "asmheader");
        assert_eq!(
            &*asm.records[0].tokens,
            [
                Token::Ref(-1),
                Token::Long(-1),
                Token::Str("232.4.0.65535".to_string())
            ]
        );

        let text = "700 0 1 0 \n30 Autodesk Translation Framework 21 ASM 232.4.0.65535 OSX 24 Fri \
                    Jul 17 14:48:06 2026 \n25.4 1e-06 1.0e-10 \nbody $-1 -1 $-1 $-1 $-1 $-1 \
                    #\nEnd-of-ACIS-data \n";
        let acis = parse(text.as_bytes()).expect("acis stream");
        assert_eq!(acis.terminator, Terminator::Acis);
        assert_eq!(acis.header.save_format_version, 700);
        assert!(approx(acis.header.scale().get(), 25.4));
        // No asmheader record: the first record is `body` at index 0.
        assert_eq!(acis.records[0].head(), "body");
        assert_eq!(acis.records[0].index, 0);
    }

    #[test]
    fn text_reference_tokens_do_not_declare_a_binary_width() {
        let stream = parse(&asm_stream(
            "asmheader $4294967296 -1 @13 232.4.0.65535 #\n",
        ))
        .unwrap();
        assert_eq!(stream.records[0].ref_at(0), Some(4_294_967_296));
        let header = kernel_header(&stream.header);
        for family in [
            crate::dialect::KernelHeaderRef::TextAsm(&header),
            crate::dialect::KernelHeaderRef::TextAcis(&header),
        ] {
            let classified = crate::dialect::classify(family);
            assert!(!classified
                .declared()
                .contains_key(crate::dialect::DECLARED_REFERENCE_WIDTH));
        }
    }

    #[test]
    fn header_conversion_reports_the_centimetre_convention() {
        let stream = parse(&asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n")).expect("stream");
        let header = kernel_header(&stream.header);
        assert_eq!(header.save_format_version, Some(23200));
        assert_eq!(header.entity_count, Some(2));
        assert_eq!(header.flags, Some(2));
        // The token values were converted; the reported unit is centimetres.
        assert_eq!(header.scale, Some(10.0));
        let expected_resabs_cm = stream.header.resabs().get() / 10.0;
        assert_eq!(header.linear, Some(expected_resabs_cm));
    }

    #[test]
    fn text_header_resabs_uses_declared_length_scale() {
        let source = String::from_utf8(asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n"))
            .expect("ASCII stream");
        let source = source.replacen("1 1e-06 1.0e-10", "25.4 1e-06 1.0e-10", 1);
        let stream = parse(source.as_bytes()).expect("inch-scale stream");
        let actual = kernel_header(&stream.header).linear.expect("resabs");
        let expected_resabs_cm = stream.header.resabs().get() * 2.54;
        assert!((actual / expected_resabs_cm - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn text_header_retains_admitted_tolerances_and_conversion_bits() {
        let source = String::from_utf8(asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n"))
            .expect("ASCII stream");
        let source = source.replacen("1 1e-06 1.0e-10", "25.4 1e-06 1.0e-10", 1);
        let stream = parse(source.as_bytes()).expect("positive scale and nonnegative tolerances");
        let scale: PositiveReal = stream.header.scale();
        let resabs: NonNegativeReal = stream.header.resabs();
        let resnor: NonNegativeReal = stream.header.resnor();
        assert_eq!(scale.get(), 25.4);
        assert_eq!(resabs.get(), 1.0e-6);
        assert_eq!(resnor.get(), 1.0e-10);
        let expected = resabs.get() * (scale.get() / 10.0);
        assert_eq!(kernel_header(&stream.header).linear, Some(expected));
    }

    #[test]
    fn text_header_refuses_unrepresentable_resabs_conversion() {
        let source = String::from_utf8(asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n"))
            .expect("ASCII stream");
        for replacement in ["20 1.7976931348623157e308 1.0e-10", "5e-324 1 1.0e-10"] {
            let source = source.replacen("1 1e-06 1.0e-10", replacement, 1);
            let error = parse(source.as_bytes()).expect_err("resabs cannot be normalized");
            assert_eq!(
                error.reason,
                "header resabs cannot be represented in centimetres"
            );
        }
    }

    #[test]
    fn invalid_header_scales_are_rejected() {
        for scale in ["0", "-1", "NaN", "inf"] {
            let mut stream = asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n");
            let scale_start = stream
                .windows(b"1 1e-06 1.0e-10".len())
                .position(|window| window == b"1 1e-06 1.0e-10")
                .expect("tolerance line");
            stream.splice(scale_start..=scale_start, scale.bytes());

            let error = parse(&stream).expect_err("invalid scale must fail");
            assert_eq!(error.offset, scale_start);
            assert_eq!(error.reason, "header scale must be finite and positive");
        }
    }

    #[test]
    fn invalid_header_tolerances_are_rejected() {
        for (resabs, resnor) in [
            ("-1", "1.0e-10"),
            ("NaN", "1.0e-10"),
            ("inf", "1.0e-10"),
            ("1.0e-6", "-1"),
            ("1.0e-6", "NaN"),
            ("1.0e-6", "inf"),
        ] {
            let mut stream = asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n");
            let tolerance_start = stream
                .windows(b"1 1e-06 1.0e-10".len())
                .position(|window| window == b"1 1e-06 1.0e-10")
                .expect("tolerance line");
            let replacement = format!("1 {resabs} {resnor}");
            stream.splice(
                tolerance_start..tolerance_start + b"1 1e-06 1.0e-10".len(),
                replacement.bytes(),
            );

            let error = parse(&stream).expect_err("invalid tolerance must fail");
            assert_eq!(error.offset, tolerance_start);
            assert_eq!(
                error.reason,
                "header tolerances must be finite and nonnegative"
            );
        }
    }

    #[test]
    fn header_lines_reject_extra_fields() {
        let valid = asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n");
        let lines = valid
            .split_inclusive(|byte| *byte == b'\n')
            .collect::<Vec<_>>();
        assert!(lines.len() >= 3);

        for (line, extra) in [(0, b" 9".as_slice()), (1, b" extra"), (2, b" 9")] {
            let mut malformed = Vec::new();
            for (ordinal, bytes) in lines.iter().enumerate() {
                if ordinal == line {
                    malformed.extend_from_slice(&bytes[..bytes.len() - 1]);
                    malformed.extend_from_slice(extra);
                    malformed.push(b'\n');
                } else {
                    malformed.extend_from_slice(bytes);
                }
            }
            assert!(parse(&malformed).is_err(), "header line {line}");
        }
    }

    #[test]
    fn counted_header_strings_require_a_separator_after_the_count() {
        let valid = asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n");
        let mut malformed = valid.clone();
        let separator = malformed
            .windows(b"16 Autodesk".len())
            .position(|window| window == b"16 Autodesk")
            .map(|start| start + 2)
            .expect("product-family separator");
        malformed.remove(separator);

        let error = parse(&malformed).expect_err("missing counted-string separator must fail");
        assert_eq!(
            error.offset,
            valid.iter().position(|byte| *byte == b'\n').unwrap() + 1
        );
    }

    #[test]
    fn counted_string_bytes_may_contain_whitespace_and_newlines() {
        let stream = parse(&asm_stream(
            "ATTRIB_CUSTOM-attrib $-1 -1 $-1 $-1 $0 @9 a b\nc d e 1 7 #\n",
        ))
        .expect("string with newline");
        assert_eq!(
            stream.records[0].tokens[5],
            Token::Str("a b\nc d e".to_string())
        );
        // The tabled ATTRIB_CUSTOM shape types the trailing value as DOUBLE.
        assert_eq!(stream.records[0].tokens[6], Token::Long(1));
        assert!(matches!(stream.records[0].tokens[7], Token::Double(v) if approx(v, 7.0)));

        let stream = parse(&asm_stream("mystery @2 é #\n")).expect("multibyte UTF-8 string");
        assert_eq!(stream.records[0].tokens[0], Token::Str("é".to_string()));
    }

    #[test]
    fn text_fields_reject_invalid_utf8_at_the_source_byte() {
        let mut header = asm_stream("mystery 1 #\n");
        let header_offset = header
            .windows(b"Autodesk".len())
            .position(|window| window == b"Autodesk")
            .expect("header string offset");
        header[header_offset] = 0xff;

        let mut bare = asm_stream("mystery invalid #\n");
        let bare_offset = bare
            .windows(b"invalid".len())
            .position(|window| window == b"invalid")
            .expect("bare field offset");
        bare[bare_offset] = 0xff;

        let mut counted = asm_stream("mystery @1 x #\n");
        let counted_offset = counted
            .windows(b"@1 x".len())
            .position(|window| window == b"@1 x")
            .map(|offset| offset + 3)
            .expect("counted string offset");
        counted[counted_offset] = 0xff;

        for (bytes, expected_offset) in [
            (header, header_offset),
            (bare, bare_offset),
            (counted, counted_offset),
        ] {
            let error = parse(&bytes).expect_err("invalid UTF-8 must fail");
            assert_eq!(error.offset, expected_offset);
        }
    }

    #[test]
    fn prefixed_fields_require_decimal_operands() {
        for malformed_field in [
            "$record",
            "@length",
            "$99999999999999999999999999999999999999999999999999",
            "@99999999999999999999999999999999999999999999999999",
        ] {
            let stream = asm_stream(&format!("mystery {malformed_field} #\n"));
            let field_offset = stream
                .windows(malformed_field.len())
                .position(|window| window == malformed_field.as_bytes())
                .expect("malformed field offset");

            let error = parse(&stream).expect_err("malformed prefixed field must fail");
            assert_eq!(error.offset, field_offset);
        }
    }

    #[test]
    fn subtype_scope_delimiters_must_balance() {
        for (body, error_field) in [("mystery { scope #\n", "#"), ("mystery } #\n", "}")] {
            let stream = asm_stream(body);
            let error_offset = stream
                .iter()
                .position(|byte| *byte == error_field.as_bytes()[0])
                .expect("delimiter offset");

            let error = parse(&stream).expect_err("unbalanced subtype scope must fail");
            assert_eq!(error.offset, error_offset);
        }
    }

    #[test]
    fn records_span_lines_until_their_terminator() {
        let stream = parse(&asm_stream("point $-1 -1 $-1 1 \n\t2 \n\t3 #\n")).expect("wrapped");
        assert_eq!(stream.records.len(), 1);
        assert_eq!(stream.records[0].tokens.len(), 4);
    }

    #[test]
    fn stream_terminator_rejects_trailing_data() {
        let mut stream = asm_stream("point $-1 -1 $-1 1 2 3 #\n");
        let trailing_offset = stream.len();
        stream.extend_from_slice(b"point $-1 -1 $-1 4 5 6 #\n");

        let error = parse(&stream).expect_err("record after stream terminator must fail");
        assert_eq!(error.offset, trailing_offset);
    }

    #[test]
    fn position_slots_coalesce_three_numbers_with_unit_conversion() {
        // Header scale 1 (millimetres): centimetre conversion divides by 10.
        let stream = parse(&asm_stream("point $-1 -1 $-1 180 30 20 #\n")).expect("point");
        let Token::Position(p) = &stream.records[0].tokens[3] else {
            panic!("position token expected");
        };
        assert!(approx(p[0], 18.0) && approx(p[1], 3.0) && approx(p[2], 2.0));
    }

    #[test]
    fn range_bound_words_and_logical_words_type_by_slot_class() {
        // `F` in a straight-curve range slot is a present bound: TRUE + value.
        let stream = parse(&asm_stream(
            "straight-curve $-1 -1 $-1 0 0 0 10 0 0 F 0 F 1 #\n",
        ))
        .expect("straight");
        let tokens = &stream.records[0].tokens;
        assert_eq!(tokens[5], Token::True);
        assert!(matches!(tokens[6], Token::Double(v) if approx(v, 0.0)));
        assert_eq!(tokens[7], Token::True);
        assert!(matches!(tokens[8], Token::Double(v) if approx(v, 1.0)));
        // `I` is an absent bound with no value.
        let stream = parse(&asm_stream(
            "straight-curve $-1 -1 $-1 0 0 0 10 0 0 I I #\n",
        ))
        .expect("straight unbounded");
        let tokens = &stream.records[0].tokens;
        assert_eq!(&tokens[5..], [Token::False, Token::False]);
        // In an untabled head, `F` is the plain logical FALSE.
        let stream = parse(&asm_stream("mystery $-1 -1 F T #\n")).expect("mystery");
        assert_eq!(
            &*stream.records[0].tokens,
            [Token::Ref(-1), Token::Long(-1), Token::False, Token::True]
        );
    }

    #[test]
    fn sense_and_sidedness_words_map_onto_booleans() {
        let stream = parse(&asm_stream(
            "face $1 -1 $-1 $-1 $0 $0 $-1 $0 reversed single #\n",
        ))
        .expect("face");
        let tokens = &stream.records[0].tokens;
        assert_eq!(tokens[8], Token::True);
        assert_eq!(tokens[9], Token::False);
    }

    #[test]
    fn subtype_reference_scope_types_as_ident_and_long() {
        let stream = parse(&asm_stream(
            "spline-surface $-1 -1 $-1 forward { ref 6 } I I I I #\n",
        ))
        .expect("spline ref");
        assert_eq!(
            &*stream.records[0].tokens,
            [
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::False,
                Token::SubtypeOpen,
                Token::Ident("ref".to_string()),
                Token::Long(6),
                Token::SubtypeClose,
                Token::False,
                Token::False,
                Token::False,
                Token::False
            ]
        );
    }

    #[test]
    fn legacy_and_modern_pcurve_spellings_share_the_wrapped_grammar() {
        for name in ["exp_par_cur", "exppc"] {
            let body = format!(
                "pcurve $-1 -1 $-1 0 forward {{ {name} nubs 1 open 2 0 1 1 1 0 0 1 0 0 spline \
                 forward {{ ref 1 }} I I I I }} 0 0 #\n"
            );
            let stream = parse(&asm_stream(&body)).expect("wrapped pcurve");
            let tokens = &stream.records[0].tokens;
            assert_eq!(tokens[3], Token::Long(0));
            assert_eq!(tokens[4], Token::False);
            assert_eq!(tokens[5], Token::SubtypeOpen);
            assert_eq!(tokens[6], Token::Ident(name.to_string()));
            assert_eq!(tokens[7], Token::Ident("nubs".to_string()));
            assert_eq!(tokens[8], Token::Long(1));
            // The closure word is an enumeration token, not an identifier.
            assert_eq!(tokens[9], Token::Enum(0));
            assert_eq!(tokens[10], Token::Long(2));
            // Knot values are doubles even when written as bare integers.
            assert!(matches!(tokens[11], Token::Double(v) if approx(v, 0.0)));
            assert_eq!(tokens[12], Token::Long(1));
            // The record ends with the two interval doubles.
            assert!(matches!(tokens[tokens.len() - 1], Token::Double(_)));
            assert!(matches!(tokens[tokens.len() - 2], Token::Double(_)));
        }
    }

    #[test]
    fn exact_int_cur_grammar_types_the_cache_and_scales_control_points() {
        // Header scale 25.4 (inches): control points convert x2.54, knots and
        // parameters stay unscaled.
        let text = "700 0 1 0 \n30 Autodesk Translation Framework 21 ASM 232.4.0.65535 OSX 24 \
                    Fri Jul 17 14:48:06 2026 \n25.4 1e-06 1.0e-10 \nintcurve-curve $-1 -1 $-1 \
                    forward { exact_int_cur 23100 full nubs 1 open 2 0 1 1 1 1 2 3 4 5 6 0 \
                    null_surface null_surface nullbs nullbs I I 0 0 0 0 F 1 F 0 UNEXTENDED \
                    UNEXTENDED } I I #\nEnd-of-ACIS-data \n";
        let stream = parse(text.as_bytes()).expect("exact intcurve");
        let tokens = &stream.records[0].tokens;
        assert_eq!(tokens[5], Token::Ident("exact_int_cur".to_string()));
        assert_eq!(tokens[6], Token::Long(23100));
        assert_eq!(tokens[7], Token::Enum(0));
        assert_eq!(tokens[8], Token::Ident("nubs".to_string()));
        // Knot value stays unscaled; the first control point scales.
        assert!(matches!(tokens[12], Token::Double(v) if approx(v, 0.0)));
        let cp0 = tokens
            .iter()
            .filter_map(|token| match token {
                Token::Double(v) => Some(*v),
                _ => None,
            })
            .nth(2)
            .expect("first control coordinate");
        assert!(approx(cp0, 2.54));
        // The extension words are enumeration tokens.
        assert_eq!(
            tokens.iter().filter(|t| **t == Token::Enum(0)).count(),
            4 // full + open + UNEXTENDED x2
        );
    }

    #[test]
    fn positive_subnormal_scale_preserves_or_refuses_each_point_coordinate() {
        let source = String::from_utf8(asm_stream("point $-1 -1 $-1 10 0 0 #\n"))
            .expect("ASCII stream")
            .replacen("1 1e-06 1.0e-10", "5e-324 0 0", 1);
        let stream = parse(source.as_bytes()).expect("representable subnormal coordinate");
        assert_eq!(stream.header.scale().get(), f64::from_bits(1));
        assert_eq!(
            stream.records[0].tokens[3],
            Token::Position([f64::from_bits(1), 0.0, 0.0])
        );

        let collapsed = source.replacen(" 10 0 0 #", " 1 0 0 #", 1);
        let error = parse(collapsed.as_bytes()).expect_err("unrepresentable coordinate");
        assert_eq!(
            error.reason,
            "source length cannot be represented in centimetres"
        );
    }

    #[test]
    fn curve_multiplicity_overflow_is_a_malformed_count() {
        let prims = [
            Prim::Word("nubs".into()),
            Prim::Integer(1),
            Prim::Word("open".into()),
            Prim::Integer(2),
            Prim::Real(0.0),
            Prim::Integer(i64::MAX),
            Prim::Real(1.0),
            Prim::Integer(1),
        ];
        let mut cur = Cur {
            prims: &prims,
            pos: 0,
            scale: 10.0,
            failure: None,
            resource: None,
            ctx: None,
        };
        assert_eq!(
            bs_curve_block(&mut cur, BsKind::Model, &mut Vec::new()),
            None
        );
        assert_eq!(cur.failure, Some(TypeFailure::InvalidSplineCount));

        let text = asm_stream(
            "intcurve-curve $-1 -1 $-1 forward { exact_int_cur 23100 full nubs 1 open 2 0 9223372036854775807 1 1 1 2 3 4 5 6 0 null_surface null_surface nullbs nullbs I I 0 0 0 0 F 1 F 0 UNEXTENDED UNEXTENDED } I I #\n",
        );
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&text, &arena, &DecodePolicy::service())
            .expect("source fits input limit");
        assert!(matches!(
            super::parse(&ctx, &text),
            Err(StreamFailure::Malformed(error))
                if error.reason == "spline multiplicity or degree is invalid"
        ));
    }

    #[test]
    fn surface_multiplicity_overflow_is_a_malformed_count() {
        let prims = [
            Prim::Word("nubs".into()),
            Prim::Integer(1),
            Prim::Integer(1),
            Prim::Word("open".into()),
            Prim::Word("open".into()),
            Prim::Word("none".into()),
            Prim::Word("none".into()),
            Prim::Integer(2),
            Prim::Integer(2),
            Prim::Real(0.0),
            Prim::Integer(i64::MAX),
            Prim::Real(1.0),
            Prim::Integer(1),
        ];
        let mut cur = Cur {
            prims: &prims,
            pos: 0,
            scale: 10.0,
            failure: None,
            resource: None,
            ctx: None,
        };
        assert_eq!(bs_surface_block(&mut cur, &mut Vec::new()), None);
        assert_eq!(cur.failure, Some(TypeFailure::InvalidSplineCount));
    }

    #[test]
    fn untabled_heads_keep_one_token_per_field() {
        let stream = parse(&asm_stream(
            "oddity-attrib $-1 -1 $-1 keep copy @3 abc 1.5 2 { weird 3 } #\n",
        ))
        .expect("untabled");
        let tokens = &stream.records[0].tokens;
        // Field-for-token fidelity: 12 fields, 12 tokens.
        assert_eq!(tokens.len(), 12);
        assert_eq!(tokens[3], Token::Ident("keep".to_string()));
        assert_eq!(tokens[5], Token::Str("abc".to_string()));
        assert!(matches!(tokens[6], Token::Double(v) if approx(v, 1.5)));
        assert_eq!(tokens[7], Token::Long(2));
        assert_eq!(tokens[8], Token::SubtypeOpen);
    }

    #[test]
    fn record_indices_count_file_order_and_resolve_references() {
        let text = "700 0 2 0 \n30 Autodesk Translation Framework 21 ASM 232.4.0.65535 OSX 24 \
                    Fri Jul 17 14:48:06 2026 \n1 1e-06 1.0e-10 \nbody $-1 -1 $-1 $2 $-1 $-1 #\nbody \
                    $-1 -1 $-1 $3 $-1 $-1 #\nlump $-1 -1 $-1 $-1 $-1 $0 #\nlump $-1 -1 $-1 $-1 \
                    $-1 $1 #\nEnd-of-ACIS-data \n";
        let stream = parse(text.as_bytes()).expect("bodies");
        assert_eq!(stream.records.len(), 4);
        let lump = stream.records[0].ref_at(3).expect("first lump ref");
        assert_eq!(
            stream.records[usize::try_from(lump).expect("index")].head(),
            "lump"
        );
        let owner = stream.records[2].ref_at(5).expect("lump owner");
        assert_eq!(owner, 0);
    }

    #[test]
    fn a_stream_without_a_terminator_line_is_an_error() {
        let text = "700 0 1 0 \n30 Autodesk Translation Framework 21 ASM 232.4.0.65535 OSX 24 \
                    Fri Jul 17 14:48:06 2026 \n1 1e-06 1.0e-10 \nbody $-1 -1 $-1 $-1 $-1 $-1 #\n";
        assert!(parse(text.as_bytes()).is_err());
    }
}
