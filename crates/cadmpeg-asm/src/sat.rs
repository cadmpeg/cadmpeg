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

/// The stream branch, from the terminator line ([`asm.md` §7]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
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
    /// Record-count word; `0` when unwritten.
    pub record_count: u32,
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
    pub scale: f64,
    /// Absolute distance tolerance.
    pub resabs: f64,
    /// Normal tolerance.
    pub resnor: f64,
}

impl TextHeader {
    /// The header as a [`KernelHeader`] for the shared decode path.
    ///
    /// `scale` is reported as `10.0`: [`parse`] converts length-bearing values
    /// into the centimetre convention, so the decoders see the same unit a
    /// binary stream carries.
    pub fn as_kernel_header(&self) -> KernelHeader {
        KernelHeader {
            width: 8,
            save_format_version: Some(self.save_format_version),
            record_count: Some(self.record_count),
            entity_count: Some(self.entity_count),
            flags: Some(self.flags),
            product_family: Some(self.product_family.clone()),
            product_version: Some(self.product_version.clone()),
            save_date: Some(self.save_date.clone()),
            scale: Some(10.0),
            linear: Some(self.resabs),
            angular: Some(self.resnor),
        }
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
    pub dialect: Dialect,
}

/// A parse error with the byte offset where parsing could not continue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SatError {
    /// Byte offset in the stream.
    pub offset: usize,
    /// What went wrong.
    pub reason: String,
}

impl std::fmt::Display for SatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SAT parse failed at byte {}: {}",
            self.offset, self.reason
        )
    }
}

impl std::error::Error for SatError {}

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
    /// A field parsing as a number; the integral flag records its lexical
    /// shape (no `.`, `e`, or `E`).
    Num { value: f64, integral: bool },
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
    fn next_field(&mut self) -> Result<Option<(usize, String)>, SatError> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let start = self.pos;
        while self.pos < self.bytes.len() && !is_ws(self.bytes[self.pos]) {
            self.pos += 1;
        }
        let word = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|error| SatError {
                offset: start + error.valid_up_to(),
                reason: "field is not valid UTF-8".to_string(),
            })?
            .to_owned();
        Ok(Some((start, word)))
    }

    /// Consume the `@N` payload after its length field: one separator byte,
    /// then `N` raw bytes.
    fn read_str_payload(&mut self, len: usize, at: usize) -> Result<String, SatError> {
        self.pos += 1; // one separator byte after the length field
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len());
        let Some(end) = end else {
            return Err(SatError {
                offset: at,
                reason: format!("truncated @{len} string"),
            });
        };
        let payload = std::str::from_utf8(&self.bytes[self.pos..end])
            .map_err(|error| SatError {
                offset: self.pos + error.valid_up_to(),
                reason: format!("@{len} string is not valid UTF-8"),
            })?
            .to_owned();
        self.pos = end;
        Ok(payload)
    }
}

fn parse_number(word: &str) -> Option<(f64, bool)> {
    let value: f64 = word.parse().ok()?;
    let integral = !word.contains(['.', 'e', 'E']);
    Some((value, integral))
}

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

/// Split one header line into whitespace-separated fields.
fn header_line<'a>(
    bytes: &'a [u8],
    pos: &mut usize,
    what: &str,
) -> Result<Vec<&'a [u8]>, SatError> {
    let start = *pos;
    let end = bytes[start..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|off| start + off)
        .ok_or_else(|| SatError {
            offset: start,
            reason: format!("missing {what} line"),
        })?;
    *pos = end + 1;
    Ok(bytes[start..end]
        .split(|b| is_ws(*b))
        .filter(|field| !field.is_empty())
        .collect())
}

fn header_int<T: std::str::FromStr>(
    field: Option<&&[u8]>,
    at: usize,
    what: &str,
) -> Result<T, SatError> {
    field
        .and_then(|field| std::str::from_utf8(field).ok())
        .and_then(|field| field.parse().ok())
        .ok_or_else(|| SatError {
            offset: at,
            reason: format!("header line has no {what} field"),
        })
}

/// Read one `N <bytes>` counted string from a header line's raw byte slice.
/// Header strings use a bare count without the record encoding's `@` prefix.
fn counted_string(line: &[u8], pos: &mut usize, at: usize, what: &str) -> Result<String, SatError> {
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
        .ok_or_else(|| SatError {
            offset: at,
            reason: format!("header line has no {what} count"),
        })?;
    if line.get(*pos).is_none_or(|byte| !is_ws(*byte)) {
        return Err(SatError {
            offset: at,
            reason: format!("header {what} count has no separator"),
        });
    }
    *pos += 1;
    let end = pos
        .checked_add(len)
        .filter(|end| *end <= line.len())
        .ok_or_else(|| SatError {
            offset: at,
            reason: format!("truncated {what} string"),
        })?;
    let value = std::str::from_utf8(&line[*pos..end])
        .map_err(|error| SatError {
            offset: at + *pos + error.valid_up_to(),
            reason: format!("header {what} string is not valid UTF-8"),
        })?
        .to_owned();
    *pos = end;
    Ok(value)
}

fn parse_header(bytes: &[u8], pos: &mut usize) -> Result<TextHeader, SatError> {
    let at = *pos;
    let line1 = header_line(bytes, pos, "save-format")?;
    if line1.len() != 4 {
        return Err(SatError {
            offset: at,
            reason: "save-format header line must contain four fields".to_string(),
        });
    }
    let save_format_version = header_int(line1.first(), at, "save format")?;
    let record_count = header_int(line1.get(1), at, "record count")?;
    let entity_count = header_int(line1.get(2), at, "entity count")?;
    let flags = header_int(line1.get(3), at, "flags")?;

    let at = *pos;
    let line2_start = *pos;
    let line2_end = bytes[line2_start..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|off| line2_start + off)
        .ok_or_else(|| SatError {
            offset: at,
            reason: "missing product line".to_string(),
        })?;
    let line2 = &bytes[line2_start..line2_end];
    *pos = line2_end + 1;
    let mut cursor = 0usize;
    let product_family = counted_string(line2, &mut cursor, at, "product family")?;
    let product_version = counted_string(line2, &mut cursor, at, "product version")?;
    let save_date = counted_string(line2, &mut cursor, at, "save date")?;
    if line2[cursor..].iter().any(|byte| !is_ws(*byte)) {
        return Err(SatError {
            offset: at,
            reason: "product header line must contain three counted strings".to_string(),
        });
    }

    let at = *pos;
    let line3 = header_line(bytes, pos, "tolerance")?;
    if line3.len() != 3 {
        return Err(SatError {
            offset: at,
            reason: "tolerance header line must contain three fields".to_string(),
        });
    }
    let float = |field: Option<&&[u8]>, what: &str| -> Result<f64, SatError> {
        field
            .and_then(|field| std::str::from_utf8(field).ok())
            .and_then(|field| field.parse().ok())
            .ok_or_else(|| SatError {
                offset: at,
                reason: format!("header line has no {what} field"),
            })
    };
    let scale = float(line3.first(), "scale")?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(SatError {
            offset: at,
            reason: "header scale must be finite and positive".to_string(),
        });
    }
    let resabs = float(line3.get(1), "resabs")?;
    let resnor = float(line3.get(2), "resnor")?;
    if !resabs.is_finite() || resabs < 0.0 || !resnor.is_finite() || resnor < 0.0 {
        return Err(SatError {
            offset: at,
            reason: "header tolerances must be finite and nonnegative".to_string(),
        });
    }
    Ok(TextHeader {
        save_format_version,
        record_count,
        entity_count,
        flags,
        product_family,
        product_version,
        save_date,
        scale,
        resabs,
        resnor,
    })
}

// ---------------------------------------------------------------------------
// Record framing
// ---------------------------------------------------------------------------

/// Parse a complete text stream into its header and typed record table.
pub fn parse(bytes: &[u8]) -> Result<TextStream, SatError> {
    let mut pos = 0usize;
    let header = parse_header(bytes, &mut pos)?;
    // Length conversion into the binary centimetre convention: the stream
    // stores lengths in `scale` millimetres per unit.
    let len_factor = header.scale / 10.0;

    let mut reader = FieldReader { bytes, pos };
    let mut records = Vec::new();
    let mut dialect = None;
    // Record name field, then payload fields until the terminator.
    'stream: while let Some((rec_start, name)) = reader.next_field()? {
        match name.as_str() {
            "End-of-ASM-data" => {
                dialect = Some(Dialect::Asm);
                break 'stream;
            }
            "End-of-ACIS-data" => {
                dialect = Some(Dialect::Acis);
                break 'stream;
            }
            _ => {}
        }
        // Payload fields until the `#` terminator.
        let mut prims = Vec::new();
        let mut subtype_depth = 0usize;
        loop {
            let Some((at, field)) = reader.next_field()? else {
                return Err(SatError {
                    offset: rec_start,
                    reason: format!("record `{name}` has no `#` terminator"),
                });
            };
            if field == "#" {
                if subtype_depth != 0 {
                    return Err(SatError {
                        offset: at,
                        reason: format!("record `{name}` terminates inside a subtype scope"),
                    });
                }
                break;
            }
            let prim = lex_prim(&mut reader, at, field)?;
            match prim {
                Prim::Open => subtype_depth += 1,
                Prim::Close if subtype_depth == 0 => {
                    return Err(SatError {
                        offset: at,
                        reason: format!("record `{name}` closes an unopened subtype scope"),
                    });
                }
                Prim::Close => subtype_depth -= 1,
                _ => {}
            }
            prims.push(prim);
        }
        let head = name.split('-').next().unwrap_or_default().to_owned();
        let tokens = type_record(&head, &prims, len_factor);
        records.push(Record {
            index: records.len(),
            name,
            head,
            tokens: tokens.into(),
            offset: rec_start,
            len: reader.pos - rec_start,
        });
    }
    let Some(dialect) = dialect else {
        return Err(SatError {
            offset: reader.pos,
            reason: "stream has no End-of-ASM-data or End-of-ACIS-data line".to_string(),
        });
    };
    reader.skip_ws();
    if reader.pos != bytes.len() {
        return Err(SatError {
            offset: reader.pos,
            reason: "non-whitespace data follows the stream terminator".to_string(),
        });
    }
    Ok(TextStream {
        header,
        records,
        dialect,
    })
}

fn lex_prim(reader: &mut FieldReader<'_>, at: usize, field: String) -> Result<Prim, SatError> {
    if let Some(rest) = field.strip_prefix('$') {
        let index = rest.parse::<i64>().map_err(|_| SatError {
            offset: at,
            reason: "reference field has no valid decimal index".to_string(),
        })?;
        return Ok(Prim::Ref(index));
    }
    if let Some(rest) = field.strip_prefix('@') {
        let len = rest.parse::<usize>().map_err(|_| SatError {
            offset: at,
            reason: "string field has no valid decimal byte count".to_string(),
        })?;
        return Ok(Prim::Str(reader.read_str_payload(len, at)?));
    }
    if field == "{" {
        return Ok(Prim::Open);
    }
    if field == "}" {
        return Ok(Prim::Close);
    }
    if let Some((value, integral)) = parse_number(&field) {
        return Ok(Prim::Num { value, integral });
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

/// A backtracking cursor over one record's primitive fields.
#[derive(Clone)]
struct Cur<'a> {
    prims: &'a [Prim],
    pos: usize,
    /// Multiplier converting stream-unit lengths into centimetres.
    k: f64,
}

impl<'a> Cur<'a> {
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
            Prim::Num { value, .. } => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    fn long(&mut self) -> Option<i64> {
        match self.peek()? {
            Prim::Num { value, integral } if *integral => {
                self.pos += 1;
                #[allow(clippy::cast_possible_truncation)] // integral by lexical shape
                Some(*value as i64)
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
        out.push(Token::Enum(*value));
        Some(())
    }

    /// Optional range bound: `I` is an absent bound; `F` is a present bound
    /// followed by one dimensionless value.
    fn opt_bound(&mut self, out: &mut Vec<Token>) -> Option<()> {
        match self.word()? {
            "I" => {
                out.push(Token::False);
                Some(())
            }
            "F" => {
                let value = self.num()?;
                out.push(Token::True);
                out.push(Token::Double(value));
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
            *value = if scale { number * self.k } else { number };
        }
        Some(values)
    }

    /// `LONG` count followed by that many dimensionless doubles.
    fn float_array(&mut self, out: &mut Vec<Token>) -> Option<()> {
        let mark = self.pos;
        let count = self.long()?;
        let count = usize::try_from(count).ok()?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let Some(value) = self.num() else {
                self.pos = mark;
                return None;
            };
            values.push(value);
        }
        out.push(Token::Long(count as i64));
        out.extend(values.into_iter().map(Token::Double));
        Some(())
    }
}

/// Run one fixed slot against the cursor.
fn take_slot(cur: &mut Cur<'_>, slot: Slot, out: &mut Vec<Token>) -> Option<()> {
    match slot {
        Slot::R => match cur.bump()? {
            Prim::Ref(index) => {
                out.push(Token::Ref(*index));
                Some(())
            }
            _ => None,
        },
        Slot::L => {
            let value = cur.long()?;
            out.push(Token::Long(value));
            Some(())
        }
        Slot::D => {
            let value = cur.num()?;
            out.push(Token::Double(value));
            Some(())
        }
        Slot::DLen => {
            let value = cur.num()?;
            out.push(Token::Double(value * cur.k));
            Some(())
        }
        Slot::DLenSentinel => {
            let value = cur.num()?;
            let converted = if value == -1.0 { value } else { value * cur.k };
            out.push(Token::Double(converted));
            Some(())
        }
        Slot::S => match cur.bump()? {
            Prim::Str(value) => {
                out.push(Token::Str(value.clone()));
                Some(())
            }
            _ => None,
        },
        Slot::P => {
            let triple = cur.triple(true)?;
            out.push(Token::Position(triple));
            Some(())
        }
        Slot::VLen => {
            let triple = cur.triple(true)?;
            out.push(Token::Vector3(triple));
            Some(())
        }
        Slot::VUnit => {
            let triple = cur.triple(false)?;
            out.push(Token::Vector3(triple));
            Some(())
        }
        Slot::Sense => match cur.word()? {
            "forward" => {
                out.push(Token::False);
                Some(())
            }
            "reversed" => {
                out.push(Token::True);
                Some(())
            }
            _ => None,
        },
        Slot::Sides => match cur.word()? {
            "single" => {
                out.push(Token::False);
                Some(())
            }
            "double" => {
                out.push(Token::True);
                Some(())
            }
            _ => None,
        },
        Slot::UvSense => match cur.word()? {
            "forward_v" => {
                out.push(Token::False);
                Some(())
            }
            "reverse_v" => {
                out.push(Token::True);
                Some(())
            }
            _ => None,
        },
        Slot::B => {
            let token = logical_word(cur.word()?)?;
            out.push(token);
            Some(())
        }
        Slot::OptB => cur.opt_bound(out),
        Slot::Sub => type_subtype(cur, out),
    }
}

fn run_shape(cur: &mut Cur<'_>, slots: &[Slot], out: &mut Vec<Token>) -> Option<()> {
    for slot in slots {
        take_slot(cur, *slot, out)?;
    }
    Some(())
}

/// Try one candidate shape against the complete field list.
fn try_shape(prims: &[Prim], k: f64, slots: &[Slot]) -> Option<Vec<Token>> {
    let mut cur = Cur { prims, pos: 0, k };
    let mut out = Vec::new();
    run_shape(&mut cur, slots, &mut out)?;
    cur.done().then_some(out)
}

// ---------------------------------------------------------------------------
// Construction grammars (subtype scopes)
// ---------------------------------------------------------------------------

/// B-spline block dimensionality per pole.
#[derive(Clone, Copy)]
struct BsKind {
    /// Model-space coordinates per pole (converted); a BS2 pole's UV
    /// coordinates are parameters and are not converted.
    coords: usize,
    /// Whether pole coordinates are model-space lengths.
    scaled: bool,
}

/// A `nubs`/`nurbs` curve block ([`asm.md` §6.5]): marker, degree, closure,
/// unique-knot count, `(knot, multiplicity)` pairs, poles, and for `nurbs`
/// one weight per pole.
fn bs_curve_block(cur: &mut Cur<'_>, kind: BsKind, out: &mut Vec<Token>) -> Option<()> {
    let marker = cur.word()?;
    let rational = match marker {
        "nubs" => false,
        "nurbs" => true,
        _ => return None,
    };
    out.push(Token::Ident(marker.to_string()));
    let degree = cur.long()?;
    out.push(Token::Long(degree));
    cur.enum_word(CLOSURE, out)?;
    let knots = cur.long()?;
    out.push(Token::Long(knots));
    let mut mult_sum = 0i64;
    for _ in 0..usize::try_from(knots).ok()? {
        let knot = cur.num()?;
        let mult = cur.long()?;
        mult_sum += mult;
        out.push(Token::Double(knot));
        out.push(Token::Long(mult));
    }
    // Endpoint multiplicities are stored as `degree`, so the pole count is
    // `sum(mult) - (degree - 1)` ([`asm.md` §6.5]).
    let poles = usize::try_from(mult_sum - (degree - 1))
        .ok()
        .filter(|count| *count >= 2)?;
    let per_pole = kind.coords + usize::from(rational);
    for _ in 0..poles {
        for coordinate in 0..per_pole {
            let value = cur.num()?;
            let scaled = kind.scaled && coordinate < kind.coords;
            out.push(Token::Double(if scaled { value * cur.k } else { value }));
        }
    }
    Some(())
}

/// A `nubs`/`nurbs` surface block ([`asm.md` §6.5]): marker, U/V degrees,
/// closures, singularities, unique-knot counts, both knot vectors, and the
/// row-major control grid.
fn bs_surface_block(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    let marker = cur.word()?;
    let rational = match marker {
        "nubs" => false,
        "nurbs" => true,
        _ => return None,
    };
    out.push(Token::Ident(marker.to_string()));
    let degree_u = cur.long()?;
    let degree_v = cur.long()?;
    out.push(Token::Long(degree_u));
    out.push(Token::Long(degree_v));
    cur.enum_word(CLOSURE, out)?;
    cur.enum_word(CLOSURE, out)?;
    cur.enum_word(SINGULARITY, out)?;
    cur.enum_word(SINGULARITY, out)?;
    let knots_u = cur.long()?;
    let knots_v = cur.long()?;
    out.push(Token::Long(knots_u));
    out.push(Token::Long(knots_v));
    let mut poles = [0usize; 2];
    for (direction, (knots, degree)) in [(knots_u, degree_u), (knots_v, degree_v)]
        .into_iter()
        .enumerate()
    {
        let mut mult_sum = 0i64;
        for _ in 0..usize::try_from(knots).ok()? {
            let knot = cur.num()?;
            let mult = cur.long()?;
            mult_sum += mult;
            out.push(Token::Double(knot));
            out.push(Token::Long(mult));
        }
        poles[direction] = usize::try_from(mult_sum - (degree - 1))
            .ok()
            .filter(|count| *count >= 2)?;
    }
    let per_pole = 3 + usize::from(rational);
    for _ in 0..poles[0].checked_mul(poles[1])? {
        for coordinate in 0..per_pole {
            let value = cur.num()?;
            out.push(Token::Double(if coordinate < 3 {
                value * cur.k
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
fn exact_int_cur_tail(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    if let Some(stamp) = cur.clone().long() {
        // A serializer stamp is a release x100 word; smaller integers open
        // the legacy layout without a stamp.
        if stamp >= 100 {
            cur.long();
            out.push(Token::Long(stamp));
        }
    }
    cur.enum_word(CACHE_FORM, out)?;
    bs_curve_block(
        cur,
        BsKind {
            coords: 3,
            scaled: true,
        },
        out,
    )?;
    let tolerance = cur.num()?;
    out.push(Token::Double(tolerance * cur.k));
    for _ in 0..2 {
        cur.word_is("null_surface")?;
        out.push(Token::Ident("null_surface".to_string()));
    }
    for _ in 0..2 {
        cur.word_is("nullbs")?;
        out.push(Token::Ident("nullbs".to_string()));
    }
    cur.opt_bound(out)?;
    cur.opt_bound(out)?;
    for _ in 0..3 {
        cur.float_array(out)?;
    }
    let extension = cur.long()?;
    out.push(Token::Long(extension));
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
fn exp_par_cur_tail(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    bs_curve_block(
        cur,
        BsKind {
            coords: 2,
            scaled: false,
        },
        out,
    )?;
    let tolerance = cur.num()?;
    out.push(Token::Double(tolerance));
    cur.word_is("spline")?;
    out.push(Token::Ident("spline".to_string()));
    let sense = cur.word()?;
    out.push(match sense {
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
fn exact_spl_sur_tail(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    if let Some(stamp) = cur.clone().long() {
        if stamp >= 100 {
            cur.long();
            out.push(Token::Long(stamp));
        }
    }
    cur.enum_word(CACHE_FORM, out)?;
    bs_surface_block(cur, out)?;
    let tolerance = cur.num()?;
    out.push(Token::Double(tolerance * cur.k));
    for _ in 0..4 {
        let value = cur.num()?;
        out.push(Token::Double(value));
    }
    let extension = cur.long()?;
    out.push(Token::Long(extension));
    Some(())
}

/// A sense word: `forward` is `FALSE`, `reversed` is `TRUE`.
fn sense_word(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    match cur.word()? {
        "forward" => out.push(Token::False),
        "reversed" => out.push(Token::True),
        _ => return None,
    }
    Some(())
}

/// One nullable support-surface slot: the `null_surface` sentinel, a `spline`
/// reference or inline construction with its boolean and four optional
/// bounds, or an embedded analytic surface ([`asm.md` §6.3]).
fn nullable_surface(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    let word = match cur.peek()? {
        Prim::Word(word) => word.as_str(),
        _ => return None,
    };
    match word {
        "null_surface" => {
            cur.bump();
            out.push(Token::Ident("null_surface".to_string()));
            Some(())
        }
        "spline" => {
            cur.bump();
            out.push(Token::Ident("spline".to_string()));
            sense_word(cur, out)?;
            type_subtype_tabled(cur, out)?;
            for _ in 0..4 {
                cur.opt_bound(out)?;
            }
            Some(())
        }
        "plane" => {
            cur.bump();
            out.push(Token::Ident("plane".to_string()));
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
            out.push(Token::Ident("cone".to_string()));
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
            out.push(Token::Ident("sphere".to_string()));
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
            out.push(Token::Ident("torus".to_string()));
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
fn nullable_bs2(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    if matches!(cur.peek(), Some(Prim::Word(word)) if word == "nullbs") {
        cur.bump();
        out.push(Token::Ident("nullbs".to_string()));
        return Some(());
    }
    bs_curve_block(
        cur,
        BsKind {
            coords: 2,
            scaled: false,
        },
        out,
    )
}

/// The shared cache-first intcurve context ([`asm.md` §6.3]): serializer
/// stamp, the `full` approximation form with the solved curve cache and fit
/// tolerance, two supports, two parameter curves, two optional interval
/// endpoints, three discontinuity arrays, and the extension integer. The
/// cacheless form selects a different payload and is not typed here.
fn cache_first_curve_context(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    let stamp = cur.long()?;
    out.push(Token::Long(stamp));
    cur.word_is("full")?;
    out.push(Token::Enum(0));
    bs_curve_block(
        cur,
        BsKind {
            coords: 3,
            scaled: true,
        },
        out,
    )?;
    let tolerance = cur.num()?;
    out.push(Token::Double(tolerance * cur.k));
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
    out.push(Token::Long(extension));
    Some(())
}

/// The shared revision-gated surface tail ([`asm.md` §6.3]): the
/// approximation form, its payload, six discontinuity arrays, and one
/// boolean. Form `full` stores the solved surface and fit tolerance; form
/// `none` stores the U and V intervals and four closure/singularity enums.
fn revision_surface_tail(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    match cur.word()? {
        "full" => {
            out.push(Token::Enum(0));
            bs_surface_block(cur, out)?;
            let tolerance = cur.num()?;
            out.push(Token::Double(tolerance * cur.k));
        }
        "none" => {
            out.push(Token::Enum(2));
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
    out.push(flag);
    Some(())
}

/// `cyl_spl_sur` revision-gated payload after the subtype name: serializer
/// stamp, the embedded directrix intcurve with two optional parameter
/// endpoints, the extrusion direction, the native position, and the shared
/// revision-gated surface tail.
fn cyl_spl_sur_tail(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    let stamp = cur.long()?;
    out.push(Token::Long(stamp));
    cur.word_is("intcurve")?;
    out.push(Token::Ident("intcurve".to_string()));
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
fn exact_spl_sur_revision_tail(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    let stamp = cur.long()?;
    out.push(Token::Long(stamp));
    revision_surface_tail(cur, out)?;
    for _ in 0..4 {
        cur.opt_bound(out)?;
    }
    cur.enum_word(EXTENSION, out)
}

/// Type one balanced subtype scope. The scope opens with `{` and a
/// construction name; tabled constructions get their grammar, and any other
/// construction falls back to lexical typing of the balanced scope.
fn type_subtype(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
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
fn type_subtype_tabled(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    if !matches!(cur.peek(), Some(Prim::Open)) {
        return None;
    }
    let scope_start = cur.pos;
    let out_mark = out.len();
    cur.bump();
    out.push(Token::SubtypeOpen);
    let Some(name) = cur.word() else {
        cur.pos = scope_start;
        out.truncate(out_mark);
        return None;
    };
    out.push(Token::Ident(name.to_string()));
    let matched = match name {
        "ref" => cur.long().map(|index| out.push(Token::Long(index))),
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
                out.push(flag);
            }
            Some(())
        }),
        "blend_int_cur" => cache_first_curve_context(cur, out).and_then(|()| {
            let flag = logical_word(cur.word()?)?;
            out.push(flag);
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
            out.push(Token::SubtypeClose);
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
fn fallback_scope(cur: &mut Cur<'_>, out: &mut Vec<Token>) -> Option<()> {
    if !matches!(cur.peek(), Some(Prim::Open)) {
        return None;
    }
    let mut depth = 0usize;
    loop {
        let prim = cur.bump()?;
        out.push(lexical_token(prim));
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

/// Type one field by lexical form alone. Numbers follow their written shape
/// (`.`/`e`/`E` selects `DOUBLE`); the unambiguous boolean words map onto
/// `TRUE`/`FALSE`; every other word is a payload identifier. The bound word
/// `F` is `TRUE` in a range slot and `FALSE` in a logical slot; without a
/// grammar the logical reading is used.
fn lexical_token(prim: &Prim) -> Token {
    match prim {
        Prim::Num { value, integral } => {
            if *integral {
                #[allow(clippy::cast_possible_truncation)] // integral by lexical shape
                Token::Long(*value as i64)
            } else {
                Token::Double(*value)
            }
        }
        Prim::Ref(index) => Token::Ref(*index),
        Prim::Str(value) => Token::Str(value.clone()),
        Prim::Open => Token::SubtypeOpen,
        Prim::Close => Token::SubtypeClose,
        Prim::Word(word) => match word.as_str() {
            "forward" | "single" | "forward_v" | "I" | "F" | "out" => Token::False,
            "reversed" | "double" | "reverse_v" | "T" | "in" => Token::True,
            _ => Token::Ident(word.clone()),
        },
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
fn type_record(head: &str, prims: &[Prim], k: f64) -> Vec<Token> {
    for slots in head_shapes(head) {
        if let Some(tokens) = try_shape(prims, k, slots) {
            return tokens;
        }
    }
    prims.iter().map(lexical_token).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
    }

    fn asm_stream(body: &str) -> Vec<u8> {
        let mut text = String::from("23200 0 2 2 \n");
        text.push_str(
            "16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 24 Fri Jul 17 14:46:47 2026 \n",
        );
        text.push_str("1 1e-06 1e-10 \n");
        text.push_str(body);
        text.push_str("End-of-ASM-data \n");
        text.into_bytes()
    }

    #[test]
    fn both_dialect_headers_parse_and_record_their_terminator() {
        let asm = parse(&asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n")).expect("asm stream");
        assert_eq!(asm.dialect, Dialect::Asm);
        assert_eq!(asm.header.save_format_version, 23200);
        assert_eq!(asm.header.record_count, 0);
        assert_eq!(asm.header.entity_count, 2);
        assert_eq!(asm.header.flags, 2);
        assert_eq!(asm.header.product_family, "Autodesk Neutron");
        assert_eq!(asm.header.product_version, "ASM 232.4.0.65535 OSX");
        assert_eq!(asm.header.save_date, "Fri Jul 17 14:46:47 2026");
        assert!(approx(asm.header.scale, 1.0));
        assert!(approx(asm.header.resabs, 1e-6));
        assert!(approx(asm.header.resnor, 1e-10));
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
                    Jul 17 14:48:06 2026 \n25.4 1e-06 1e-10 \nbody $-1 -1 $-1 $-1 $-1 $-1 \
                    #\nEnd-of-ACIS-data \n";
        let acis = parse(text.as_bytes()).expect("acis stream");
        assert_eq!(acis.dialect, Dialect::Acis);
        assert_eq!(acis.header.save_format_version, 700);
        assert!(approx(acis.header.scale, 25.4));
        // No asmheader record: the first record is `body` at index 0.
        assert_eq!(acis.records[0].head, "body");
        assert_eq!(acis.records[0].index, 0);
    }

    #[test]
    fn header_conversion_reports_the_centimetre_convention() {
        let stream = parse(&asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n")).expect("stream");
        let header = stream.header.as_kernel_header();
        assert_eq!(header.save_format_version, Some(23200));
        assert_eq!(header.entity_count, Some(2));
        assert_eq!(header.flags, Some(2));
        // The token values were converted; the reported unit is centimetres.
        assert_eq!(header.scale, Some(10.0));
    }

    #[test]
    fn invalid_header_scales_are_rejected() {
        for scale in ["0", "-1", "NaN", "inf"] {
            let mut stream = asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n");
            let scale_start = stream
                .windows(b"1 1e-06 1e-10".len())
                .position(|window| window == b"1 1e-06 1e-10")
                .expect("tolerance line");
            stream.splice(scale_start..scale_start + 1, scale.bytes());

            let error = parse(&stream).expect_err("invalid scale must fail");
            assert_eq!(error.offset, scale_start);
            assert_eq!(error.reason, "header scale must be finite and positive");
        }
    }

    #[test]
    fn invalid_header_tolerances_are_rejected() {
        for (resabs, resnor) in [
            ("-1", "1e-10"),
            ("NaN", "1e-10"),
            ("inf", "1e-10"),
            ("1e-6", "-1"),
            ("1e-6", "NaN"),
            ("1e-6", "inf"),
        ] {
            let mut stream = asm_stream("asmheader $-1 -1 @13 232.4.0.65535 #\n");
            let tolerance_start = stream
                .windows(b"1 1e-06 1e-10".len())
                .position(|window| window == b"1 1e-06 1e-10")
                .expect("tolerance line");
            let replacement = format!("1 {resabs} {resnor}");
            stream.splice(
                tolerance_start..tolerance_start + b"1 1e-06 1e-10".len(),
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
                    Fri Jul 17 14:48:06 2026 \n25.4 1e-06 1e-10 \nintcurve-curve $-1 -1 $-1 \
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
                    Fri Jul 17 14:48:06 2026 \n1 1e-06 1e-10 \nbody $-1 -1 $-1 $2 $-1 $-1 #\nbody \
                    $-1 -1 $-1 $3 $-1 $-1 #\nlump $-1 -1 $-1 $-1 $-1 $0 #\nlump $-1 -1 $-1 $-1 \
                    $-1 $1 #\nEnd-of-ACIS-data \n";
        let stream = parse(text.as_bytes()).expect("bodies");
        assert_eq!(stream.records.len(), 4);
        let lump = stream.records[0].ref_at(3).expect("first lump ref");
        assert_eq!(
            stream.records[usize::try_from(lump).expect("index")].head,
            "lump"
        );
        let owner = stream.records[2].ref_at(5).expect("lump owner");
        assert_eq!(owner, 0);
    }

    #[test]
    fn a_stream_without_a_terminator_line_is_an_error() {
        let text = "700 0 1 0 \n30 Autodesk Translation Framework 21 ASM 232.4.0.65535 OSX 24 \
                    Fri Jul 17 14:48:06 2026 \n1 1e-06 1e-10 \nbody $-1 -1 $-1 $-1 $-1 $-1 #\n";
        assert!(parse(text.as_bytes()).is_err());
    }
}
