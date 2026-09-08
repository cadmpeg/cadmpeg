// SPDX-License-Identifier: Apache-2.0
//! IGES Binary representation normalization.
//!
//! The Binary representation carries the same section and entity model as
//! Fixed ASCII, but replaces the free-form constants with a bit-packed,
//! control-byte stream.  This module decodes that physical envelope and
//! emits a bounded Fixed ASCII image for the existing section, Directory, and
//! Parameter Data owners.  The original Binary image remains the source image
//! passed to the reader, so normalization does not replace source fidelity.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const CARD_WIDTH: usize = 80;
const CARD_DATA_WIDTH: usize = 72;
const PARAMETER_DATA_WIDTH: usize = 64;
const SECTION_HEADER_WIDTH: usize = 5;
const BINARY_FLAG_WIDTH: usize = 80;
const BINARY_FLAG_COUNT: u32 = 75;
const BINARY_INTEGER_POINTER_BITS: u8 = 32;
const MAX_SEQUENCE: u32 = 9_999_999;

#[derive(Debug, Clone, Copy)]
struct PrimitiveLengths {
    single_integer: u8,
    double_integer: u8,
    single_exponent: u8,
    single_fraction: u8,
    double_exponent: u8,
    double_fraction: u8,
}

#[derive(Debug, Clone, Copy)]
struct SectionDisplacements {
    start: usize,
    global: usize,
    directory: usize,
    parameter: usize,
    terminate: usize,
    end: usize,
}

#[derive(Debug)]
struct BinarySections<'a> {
    start: &'a [u8],
    global: &'a [u8],
    directory: &'a [u8],
    parameter: &'a [u8],
    lengths: PrimitiveLengths,
}

#[derive(Debug, Clone, PartialEq)]
enum BinaryValue {
    Default,
    Integer(i64),
    Real(f64),
    Pointer(i64),
    String(Vec<u8>),
}

#[derive(Debug)]
struct BinaryDirectory {
    offset: u32,
    values: Vec<BinaryValue>,
    entity_type: i64,
    parameter_pointer: i64,
}

#[derive(Debug)]
struct BinaryParameter {
    offset: u32,
    entity_type: i64,
    directory_pointer: i64,
    values: Vec<BinaryValue>,
    text: Vec<u8>,
    lines: Vec<Vec<u8>>,
    first_sequence: u32,
}

#[derive(Debug)]
struct BitReader<'a> {
    bytes: &'a [u8],
    byte: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte: 0,
            bit: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.byte == self.bytes.len() && self.bit == 0
    }

    fn read_bits(&mut self, count: u8) -> Result<u64, CodecError> {
        if count > 64 {
            return Err(malformed("a Binary primitive requests more than 64 bits"));
        }
        let mut value = 0_u64;
        for _ in 0..count {
            let byte = *self
                .bytes
                .get(self.byte)
                .ok_or_else(|| malformed("a Binary primitive is truncated"))?;
            let shift = 7_u8.saturating_sub(self.bit);
            value = (value << 1) | u64::from((byte >> shift) & 1);
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.byte = self
                    .byte
                    .checked_add(1)
                    .ok_or_else(|| malformed("a Binary bit offset overflows"))?;
            }
        }
        Ok(value)
    }

    fn align_zero(&mut self) -> Result<(), CodecError> {
        if self.bit == 0 {
            return Ok(());
        }
        let padding = self.read_bits(8 - self.bit)?;
        if padding != 0 {
            return Err(malformed("Binary primitive padding contains a one bit"));
        }
        Ok(())
    }

    fn read_integer(&mut self, width: u8) -> Result<i64, CodecError> {
        if width == 0 {
            return Err(malformed("a Binary integer has zero width"));
        }
        let negative = self.read_bits(1)? == 1;
        let magnitude_bits = width - 1;
        let magnitude = self.read_bits(magnitude_bits)?;
        self.align_zero()?;
        let magnitude = i64::try_from(magnitude)
            .map_err(|_| malformed("a Binary integer exceeds the signed 64-bit model"))?;
        if negative {
            magnitude
                .checked_neg()
                .ok_or_else(|| malformed("a Binary integer is not representable"))
        } else {
            Ok(magnitude)
        }
    }

    fn read_pointer(&mut self) -> Result<i64, CodecError> {
        self.read_integer(BINARY_INTEGER_POINTER_BITS)
    }

    fn read_real(&mut self, exponent_bits: u8, fraction_bits: u8) -> Result<f64, CodecError> {
        if exponent_bits == 0 {
            return Err(malformed("a Binary real has zero exponent width"));
        }
        let total_bits = 1_u16
            .checked_add(u16::from(exponent_bits))
            .and_then(|bits| bits.checked_add(u16::from(fraction_bits)))
            .ok_or_else(|| malformed("a Binary real width overflows"))?;
        if total_bits > 64 {
            return Err(malformed(
                "a Binary real exceeds the supported 64-bit primitive width",
            ));
        }
        let negative = self.read_bits(1)? == 1;
        let biased_exponent = self.read_bits(exponent_bits)?;
        let fraction = self.read_bits(fraction_bits)?;
        self.align_zero()?;
        if biased_exponent == 0 {
            return Ok(0.0);
        }
        let bias = 2_f64.powi(i32::from(exponent_bits) - 1);
        let exponent = biased_exponent as f64 - bias;
        let fraction = 0.5 + (fraction as f64 / 2_f64.powi(i32::from(fraction_bits) + 1));
        let value = fraction * 2_f64.powf(exponent);
        if !value.is_finite() {
            return Err(malformed("a Binary real is not finite"));
        }
        Ok(if negative { -value } else { value })
    }

    fn read_string(&mut self, lengths: PrimitiveLengths) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::new();
        loop {
            let count = self.read_integer(lengths.single_integer)?;
            if count == 0 {
                return Err(malformed("a Binary string has a zero character count"));
            }
            let negative = count < 0;
            let count = usize::try_from(count.unsigned_abs())
                .map_err(|_| malformed("a Binary string count is out of range"))?;
            let remaining = self.bytes.len().saturating_sub(self.byte);
            if count > remaining {
                return Err(malformed("a Binary string payload is truncated"));
            }
            for _ in 0..count {
                let byte = self.read_bits(8)?;
                let byte =
                    u8::try_from(byte).map_err(|_| malformed("a Binary string byte overflows"))?;
                if !byte.is_ascii() {
                    return Err(malformed("a Binary string contains a non-ASCII byte"));
                }
                output.push(byte);
            }
            if !negative {
                return Ok(output);
            }
        }
    }
}

struct ValueStream<'a> {
    bits: BitReader<'a>,
    lengths: PrimitiveLengths,
    pending: VecDeque<BinaryValue>,
}

impl<'a> ValueStream<'a> {
    fn new(bytes: &'a [u8], lengths: PrimitiveLengths) -> Self {
        Self {
            bits: BitReader::new(bytes),
            lengths,
            pending: VecDeque::new(),
        }
    }

    fn one(&mut self, format: u8) -> Result<BinaryValue, CodecError> {
        match format {
            0 => Ok(BinaryValue::Default),
            1 => self
                .bits
                .read_integer(self.lengths.single_integer)
                .map(BinaryValue::Integer),
            2 => self
                .bits
                .read_integer(self.lengths.double_integer)
                .map(BinaryValue::Integer),
            3 => self
                .bits
                .read_real(self.lengths.single_exponent, self.lengths.single_fraction)
                .map(BinaryValue::Real),
            4 => self
                .bits
                .read_real(self.lengths.double_exponent, self.lengths.double_fraction)
                .map(BinaryValue::Real),
            5 => self.bits.read_pointer().map(BinaryValue::Pointer),
            6 => self.bits.read_string(self.lengths).map(BinaryValue::String),
            _ => Err(malformed("a Binary control byte has an invalid format")),
        }
    }

    fn next(&mut self) -> Result<Option<BinaryValue>, CodecError> {
        if let Some(value) = self.pending.pop_front() {
            return Ok(Some(value));
        }
        if self.bits.is_empty() {
            return Ok(None);
        }
        if self.bits.bit != 0 {
            return Err(malformed("a Binary control byte is not byte aligned"));
        }
        let control = u8::try_from(self.bits.read_bits(8)?)
            .map_err(|_| malformed("a Binary control byte overflows"))?;
        let physically_present = control >> 7 == 1;
        let repeat = usize::from((control >> 3) & 0x0f) + 1;
        let format = control & 0x07;
        let first = self.one(format)?;
        if physically_present {
            for _ in 1..repeat {
                let value = self.one(format)?;
                self.pending.push_back(value);
            }
        } else {
            for _ in 1..repeat {
                self.pending.push_back(first.clone());
            }
        }
        Ok(Some(first))
    }

    fn finish(&self) -> Result<(), CodecError> {
        if !self.pending.is_empty() || !self.bits.is_empty() {
            return Err(malformed(
                "a Binary section has values outside its declared fields",
            ));
        }
        Ok(())
    }
}

fn malformed(message: impl Into<String>) -> CodecError {
    crate::error::malformed(format!("IGES Binary: {}", message.into()))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, CodecError> {
    View::u32_be_at(bytes, offset).ok_or_else(|| malformed("a Binary u32 field is truncated"))
}

fn checked_offset(base: usize, displacement: u32) -> Result<usize, CodecError> {
    base.checked_add(
        usize::try_from(displacement)
            .map_err(|_| malformed("a Binary section displacement is out of range"))?,
    )
    .ok_or_else(|| malformed("a Binary section displacement overflows"))
}

fn padding_is_zero(bytes: &[u8], range: std::ops::Range<usize>) -> Result<(), CodecError> {
    if bytes
        .get(range.clone())
        .ok_or_else(|| malformed("Binary section padding is truncated"))?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(malformed("Binary section padding is not null"));
    }
    Ok(())
}

fn section_payload(
    source: &[u8],
    start: usize,
    expected: u8,
    next_start: usize,
) -> Result<&[u8], CodecError> {
    if source.get(start) != Some(&expected) {
        return Err(malformed(format!(
            "Binary section at byte {start} is not {:?}",
            char::from(expected)
        )));
    }
    let count = usize::try_from(u32_at(source, start + 1)?)
        .map_err(|_| malformed("a Binary section count is out of range"))?;
    let payload_start = start
        .checked_add(SECTION_HEADER_WIDTH)
        .ok_or_else(|| malformed("a Binary section header overflows"))?;
    let payload_end = payload_start
        .checked_add(count)
        .ok_or_else(|| malformed("a Binary section count overflows"))?;
    if payload_end > next_start {
        return Err(malformed("a Binary section overlaps its successor"));
    }
    source
        .get(payload_start..payload_end)
        .ok_or_else(|| malformed("a Binary section payload is truncated"))
}

fn parse_header(source: &[u8]) -> Result<(PrimitiveLengths, SectionDisplacements), CodecError> {
    if source.len() < BINARY_FLAG_WIDTH || source.first() != Some(&b'B') {
        return Err(malformed("Binary Flag Section is truncated"));
    }
    if u32_at(source, 1)? != BINARY_FLAG_COUNT {
        return Err(malformed("Binary Flag Section byte count is not 75"));
    }
    let lengths = PrimitiveLengths {
        single_integer: source[5],
        double_integer: source[6],
        single_exponent: source[7],
        single_fraction: source[8],
        double_exponent: source[9],
        double_fraction: source[10],
    };
    if lengths.single_integer == 0
        || lengths.double_integer == 0
        || lengths.single_integer > 64
        || lengths.double_integer > 64
        || lengths.single_exponent == 0
        || lengths.double_exponent == 0
    {
        return Err(malformed(
            "Binary primitive bit lengths are outside the supported range",
        ));
    }
    let identifiers = *b"BSGDPT";
    let mut displacements = [0_u32; 6];
    for (index, identifier) in identifiers.into_iter().enumerate() {
        let offset = 11 + index * 5;
        if source.get(offset) != Some(&identifier) {
            return Err(malformed("Binary Flag Section identifiers are not ordered"));
        }
        displacements[index] = u32_at(source, offset + 1)?;
    }
    let start = usize::try_from(displacements[0])
        .map_err(|_| malformed("a Binary Start displacement is out of range"))?;
    if start < BINARY_FLAG_WIDTH {
        return Err(malformed(
            "Binary Start displacement precedes the flag section",
        ));
    }
    padding_is_zero(source, BINARY_FLAG_WIDTH..start)?;
    if source.get(72) != Some(&b'B')
        || !source
            .get(73..79)
            .is_some_and(|padding| padding.iter().all(|byte| matches!(byte, b' ' | b'0')))
        || source.get(79) != Some(&b'1')
    {
        return Err(malformed("Binary Flag Section sequence marker is invalid"));
    }
    let global = checked_offset(start, displacements[1])?;
    let directory = checked_offset(global, displacements[2])?;
    let parameter = checked_offset(directory, displacements[3])?;
    let terminate = checked_offset(parameter, displacements[4])?;
    let end = checked_offset(terminate, displacements[5])?;
    let absolute = [start, global, directory, parameter, terminate, end];
    for pair in absolute.windows(2) {
        if pair[1] <= pair[0] {
            return Err(malformed("Binary section displacements are not increasing"));
        }
    }
    let section_displacements = SectionDisplacements {
        start,
        global,
        directory,
        parameter,
        terminate,
        end,
    };
    Ok((lengths, section_displacements))
}

fn parse_sections(source: &[u8]) -> Result<BinarySections<'_>, CodecError> {
    let (lengths, offsets) = parse_header(source)?;
    if offsets.end > source.len() {
        return Err(malformed(
            "Binary Terminate displacement exceeds the source",
        ));
    }
    let start = section_payload(source, offsets.start, b'S', offsets.global)?;
    let global = section_payload(source, offsets.global, b'G', offsets.directory)?;
    let directory = section_payload(source, offsets.directory, b'D', offsets.parameter)?;
    let parameter = section_payload(source, offsets.parameter, b'P', offsets.terminate)?;
    let terminate = section_payload(source, offsets.terminate, b'T', offsets.end)?;
    let start_end = offsets
        .start
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(start.len()))
        .ok_or_else(|| malformed("Binary Start section count overflows"))?;
    let global_end = offsets
        .global
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(global.len()))
        .ok_or_else(|| malformed("Binary Global section count overflows"))?;
    let directory_end = offsets
        .directory
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(directory.len()))
        .ok_or_else(|| malformed("Binary Directory section count overflows"))?;
    let parameter_end = offsets
        .parameter
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(parameter.len()))
        .ok_or_else(|| malformed("Binary Parameter section count overflows"))?;
    for (payload_end, next_start) in [
        (start_end, offsets.global),
        (global_end, offsets.directory),
        (directory_end, offsets.parameter),
        (parameter_end, offsets.terminate),
    ] {
        padding_is_zero(source, payload_end..next_start)?;
    }
    let terminate_end = offsets
        .terminate
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(terminate.len()))
        .ok_or_else(|| malformed("Binary Terminate section count overflows"))?;
    padding_is_zero(source, terminate_end..offsets.end)?;
    validate_terminate(
        terminate,
        [
            BINARY_FLAG_WIDTH,
            SECTION_HEADER_WIDTH + start.len(),
            SECTION_HEADER_WIDTH + global.len(),
            SECTION_HEADER_WIDTH + directory.len(),
            SECTION_HEADER_WIDTH + parameter.len(),
        ],
    )?;
    if source.get(offsets.end) != Some(&b'E') {
        return Err(malformed(
            "Binary Terminate displacement does not lead to the end marker",
        ));
    }
    Ok(BinarySections {
        start,
        global,
        directory,
        parameter,
        lengths,
    })
}

fn validate_terminate(payload: &[u8], expected: [usize; 5]) -> Result<(), CodecError> {
    if payload.len() != 25 {
        return Err(malformed(
            "Binary Terminate payload does not contain five section counts",
        ));
    }
    for (index, (identifier, size)) in b"BSGDP".iter().copied().zip(expected).enumerate() {
        let offset = index * 5;
        if payload[offset] != identifier
            || u32_at(payload, offset + 1)? != u32::try_from(size).unwrap_or(u32::MAX)
        {
            return Err(malformed("Binary Terminate section counts disagree"));
        }
    }
    Ok(())
}

fn one_value(stream: &mut ValueStream<'_>, what: &str) -> Result<BinaryValue, CodecError> {
    stream
        .next()?
        .ok_or_else(|| malformed(format!("Binary {what} section ends before its fields")))
}

fn integer_value(value: &BinaryValue, what: &str) -> Result<i64, CodecError> {
    match value {
        BinaryValue::Integer(value) | BinaryValue::Pointer(value) => Ok(*value),
        BinaryValue::Default => Err(malformed(format!("Binary {what} field is defaulted"))),
        BinaryValue::Real(_) | BinaryValue::String(_) => {
            Err(malformed(format!("Binary {what} field is not an integer")))
        }
    }
}

fn pointer_value(value: &BinaryValue, what: &str) -> Result<i64, CodecError> {
    match value {
        BinaryValue::Pointer(value) | BinaryValue::Integer(value) => Ok(*value),
        BinaryValue::Default => Ok(0),
        BinaryValue::Real(_) | BinaryValue::String(_) => {
            Err(malformed(format!("Binary {what} field is not a pointer")))
        }
    }
}

fn positive_pointer(value: i64, what: &str) -> Result<u32, CodecError> {
    if value <= 0 {
        return Err(malformed(format!("Binary {what} pointer is not positive")));
    }
    u32::try_from(value).map_err(|_| malformed(format!("Binary {what} pointer is out of range")))
}

fn read_global(payload: &[u8], lengths: PrimitiveLengths) -> Result<Vec<BinaryValue>, CodecError> {
    let mut stream = ValueStream::new(payload, lengths);
    let mut values = Vec::with_capacity(24);
    for _ in 0..24 {
        values.push(one_value(&mut stream, "Global")?);
    }
    stream.finish()?;
    Ok(values)
}

fn read_directory(
    payload: &[u8],
    lengths: PrimitiveLengths,
) -> Result<Vec<BinaryDirectory>, CodecError> {
    let mut records = Vec::new();
    let mut cursor = 0_usize;
    while cursor < payload.len() {
        let offset = u32::try_from(cursor + 1)
            .map_err(|_| malformed("Binary Directory pointer exceeds u32"))?;
        let byte_count = usize::try_from(u32_at(payload, cursor)?)
            .map_err(|_| malformed("Binary Directory entity count is out of range"))?;
        let body_start = cursor
            .checked_add(4)
            .ok_or_else(|| malformed("Binary Directory entity start overflows"))?;
        let body_end = body_start
            .checked_add(byte_count)
            .ok_or_else(|| malformed("Binary Directory entity count overflows"))?;
        if body_end > payload.len() {
            return Err(malformed("Binary Directory entity exceeds its section"));
        }
        let mut stream = ValueStream::new(&payload[body_start..body_end], lengths);
        let mut values = Vec::with_capacity(16);
        for _ in 0..16 {
            values.push(one_value(&mut stream, "Directory")?);
        }
        stream.finish()?;
        let entity_type = integer_value(&values[0], "Directory entity type")?;
        let parameter_pointer = pointer_value(&values[1], "Directory Parameter Data")?;
        records.push(BinaryDirectory {
            offset,
            values,
            entity_type,
            parameter_pointer,
        });
        cursor = body_end;
    }
    Ok(records)
}

fn render_real(value: f64) -> Vec<u8> {
    if value == 0.0 {
        b"0".to_vec()
    } else {
        format!("{value:.17E}").into_bytes()
    }
}

fn render_parameter_value(value: &BinaryValue, language: bool) -> Result<Vec<u8>, CodecError> {
    match value {
        BinaryValue::Default => Ok(Vec::new()),
        BinaryValue::Integer(value) | BinaryValue::Pointer(value) => {
            Ok(value.to_string().into_bytes())
        }
        BinaryValue::Real(value) => Ok(render_real(*value)),
        BinaryValue::String(value) if language => Ok(value.clone()),
        BinaryValue::String(value) => {
            if value.is_empty() {
                return Err(malformed("Binary string constant has no characters"));
            }
            let mut output = value.len().to_string().into_bytes();
            output.push(b'H');
            output.extend_from_slice(value);
            Ok(output)
        }
    }
}

fn parameter_text(entity_type: i64, values: &[BinaryValue]) -> Result<Vec<u8>, CodecError> {
    if entity_type == 306 {
        let mut output = b"306,".to_vec();
        if values.is_empty() {
            return Err(malformed(
                "Binary Macro Definition has no language statements",
            ));
        }
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                output.push(b';');
            }
            let rendered = render_parameter_value(value, true)?;
            if rendered.is_empty() {
                return Err(malformed("Binary Macro Definition has an empty statement"));
            }
            output.extend_from_slice(&rendered);
        }
        output.push(b';');
        return Ok(output);
    }
    let mut output = entity_type.to_string().into_bytes();
    for value in values {
        output.push(b',');
        output.extend_from_slice(&render_parameter_value(value, false)?);
    }
    output.push(b';');
    Ok(output)
}

fn render_field(value: &BinaryValue, label: bool, status: bool) -> Result<[u8; 8], CodecError> {
    let mut field = [b' '; 8];
    let rendered = if status {
        match value {
            BinaryValue::Default => Vec::new(),
            BinaryValue::Integer(value) | BinaryValue::Pointer(value) => {
                format!("{value:08}").into_bytes()
            }
            BinaryValue::Real(_) | BinaryValue::String(_) => {
                return Err(malformed("Binary Directory status is not an integer"));
            }
        }
    } else if label {
        match value {
            BinaryValue::Default => Vec::new(),
            BinaryValue::String(value) => value.clone(),
            BinaryValue::Integer(value) | BinaryValue::Pointer(value) => {
                value.to_string().into_bytes()
            }
            BinaryValue::Real(value) => render_real(*value),
        }
    } else {
        match value {
            BinaryValue::Default => Vec::new(),
            BinaryValue::Integer(value) | BinaryValue::Pointer(value) => {
                value.to_string().into_bytes()
            }
            BinaryValue::Real(value) => render_real(*value),
            BinaryValue::String(value) => value.clone(),
        }
    };
    if rendered.len() > field.len() {
        return Err(malformed(
            "Binary Directory field does not fit eight columns",
        ));
    }
    let start = field.len() - rendered.len();
    field[start..].copy_from_slice(&rendered);
    Ok(field)
}

fn render_card(
    output: &mut Vec<u8>,
    data: &[u8],
    section: u8,
    sequence: &mut u32,
) -> Result<(), CodecError> {
    if data.len() > CARD_DATA_WIDTH {
        return Err(malformed(
            "normalized Fixed ASCII card payload exceeds 72 bytes",
        ));
    }
    if *sequence == 0 || *sequence > MAX_SEQUENCE {
        return Err(malformed(
            "normalized Fixed ASCII sequence exceeds seven digits",
        ));
    }
    let mut card = [b' '; CARD_WIDTH];
    card[..data.len()].copy_from_slice(data);
    card[CARD_DATA_WIDTH] = section;
    let sequence_bytes = format!("{:>7}", *sequence);
    card[CARD_DATA_WIDTH + 1..].copy_from_slice(sequence_bytes.as_bytes());
    output.extend_from_slice(&card);
    output.push(b'\n');
    *sequence = sequence
        .checked_add(1)
        .ok_or_else(|| malformed("normalized Fixed ASCII sequence overflows"))?;
    Ok(())
}

fn render_cards(
    output: &mut Vec<u8>,
    data: &[u8],
    section: u8,
    sequence: &mut u32,
) -> Result<(), CodecError> {
    if data.is_empty() {
        return Ok(());
    }
    for chunk in data.chunks(CARD_DATA_WIDTH) {
        render_card(output, chunk, section, sequence)?;
    }
    Ok(())
}

fn render_terminate(
    output: &mut Vec<u8>,
    start_count: usize,
    global_count: usize,
    directory_count: usize,
    parameter_count: usize,
) -> Result<(), CodecError> {
    let counts = [
        (b'S', start_count),
        (b'G', global_count),
        (b'D', directory_count),
        (b'P', parameter_count),
    ];
    let mut data = [b' '; CARD_DATA_WIDTH];
    for (index, (marker, count)) in counts.into_iter().enumerate() {
        let count = u32::try_from(count).map_err(|_| malformed("section count is out of range"))?;
        if count > MAX_SEQUENCE {
            return Err(malformed("normalized section count exceeds seven digits"));
        }
        let field = format!("{}{:>7}", char::from(marker), count);
        data[index * 8..(index + 1) * 8].copy_from_slice(field.as_bytes());
    }
    let mut card = [b' '; CARD_WIDTH];
    card[..CARD_DATA_WIDTH].copy_from_slice(&data);
    card[CARD_DATA_WIDTH] = b'T';
    card[CARD_DATA_WIDTH + 1..].copy_from_slice(b"      1");
    output.extend_from_slice(&card);
    output.push(b'\n');
    Ok(())
}

fn normalize_start(payload: &[u8], lengths: PrimitiveLengths) -> Result<Vec<u8>, CodecError> {
    let mut stream = ValueStream::new(payload, lengths);
    let mut text = Vec::new();
    while let Some(value) = stream.next()? {
        let BinaryValue::String(mut value) = value else {
            return Err(malformed(
                "Binary Start section contains a non-text primitive",
            ));
        };
        text.append(&mut value);
    }
    stream.finish()?;
    if text.is_empty() {
        return Err(malformed("Binary Start section contains no text"));
    }
    Ok(text)
}

fn render_start_cards(
    output: &mut Vec<u8>,
    text: &[u8],
    sequence: &mut u32,
) -> Result<(), CodecError> {
    let mut line_start = 0;
    let mut index = 0;
    while index < text.len() {
        if text[index] == b'\n' {
            return Err(malformed(
                "Binary Start contains a line feed without a preceding carriage return",
            ));
        }
        if text[index] != b'\r' {
            index += 1;
            continue;
        }
        render_start_line(output, &text[line_start..index], sequence)?;
        if text[index] == b'\r' && text.get(index + 1) == Some(&b'\n') {
            index += 1;
        }
        index += 1;
        line_start = index;
    }
    render_start_line(output, &text[line_start..], sequence)
}

fn render_start_line(
    output: &mut Vec<u8>,
    line: &[u8],
    sequence: &mut u32,
) -> Result<(), CodecError> {
    if line.is_empty() {
        return render_card(output, line, b'S', sequence);
    }
    render_cards(output, line, b'S', sequence)
}

fn normalize_global(values: &[BinaryValue]) -> Result<Vec<u8>, CodecError> {
    if values.len() != 24 {
        return Err(malformed(
            "Binary Global section does not contain 24 fields",
        ));
    }
    let mut output = b"1H,,1H;,".to_vec();
    for (index, value) in values.iter().enumerate().skip(2) {
        if index > 2 {
            output.push(b',');
        }
        output.extend_from_slice(&render_parameter_value(value, false)?);
    }
    output.push(b';');
    Ok(output)
}

fn read_parameters(
    payload: &[u8],
    lengths: PrimitiveLengths,
    directory_by_offset: &BTreeMap<u32, usize>,
) -> Result<Vec<BinaryParameter>, CodecError> {
    let mut records = Vec::new();
    let mut cursor = 0_usize;
    while cursor < payload.len() {
        let offset = u32::try_from(cursor + 1)
            .map_err(|_| malformed("Binary Parameter pointer exceeds u32"))?;
        let byte_count = usize::try_from(u32_at(payload, cursor)?)
            .map_err(|_| malformed("Binary Parameter entity count is out of range"))?;
        let body_start = cursor
            .checked_add(4)
            .ok_or_else(|| malformed("Binary Parameter entity start overflows"))?;
        let body_end = body_start
            .checked_add(byte_count)
            .ok_or_else(|| malformed("Binary Parameter entity count overflows"))?;
        if body_end > payload.len() {
            return Err(malformed("Binary Parameter entity exceeds its section"));
        }
        let mut stream = ValueStream::new(&payload[body_start..body_end], lengths);
        let entity_type = integer_value(
            &one_value(&mut stream, "Parameter")?,
            "Parameter entity type",
        )?;
        let directory_pointer = pointer_value(
            &one_value(&mut stream, "Parameter Directory pointer")?,
            "Parameter Directory pointer",
        )?;
        let mut values = Vec::new();
        while let Some(value) = stream.next()? {
            values.push(value);
        }
        stream.finish()?;
        let directory_pointer = positive_pointer(directory_pointer, "Parameter Directory")?;
        if !directory_by_offset.contains_key(&directory_pointer) {
            return Err(malformed(
                "Binary Parameter Directory pointer does not resolve",
            ));
        }
        records.push(BinaryParameter {
            offset,
            entity_type,
            directory_pointer: i64::from(directory_pointer),
            values,
            text: Vec::new(),
            lines: Vec::new(),
            first_sequence: 0,
        });
        cursor = body_end;
    }
    Ok(records)
}

fn normalize_directory_and_parameters(
    output: &mut Vec<u8>,
    directory: &[BinaryDirectory],
    parameters: &mut [BinaryParameter],
    ctx: &DecodeContext<'_>,
) -> Result<(usize, usize), CodecError> {
    let directory_by_offset = directory
        .iter()
        .enumerate()
        .map(|(index, record)| (record.offset, index))
        .collect::<BTreeMap<_, _>>();
    let parameter_by_offset = parameters
        .iter()
        .enumerate()
        .map(|(index, record)| (record.offset, index))
        .collect::<BTreeMap<_, _>>();
    let mut referenced_parameters = BTreeSet::new();
    for parameter in parameters.iter_mut() {
        let directory_pointer =
            positive_pointer(parameter.directory_pointer, "Parameter Directory")?;
        let directory_index = *directory_by_offset
            .get(&directory_pointer)
            .ok_or_else(|| malformed("Binary Parameter Directory pointer does not resolve"))?;
        if directory[directory_index].entity_type != parameter.entity_type {
            return Err(malformed(
                "Binary Directory and Parameter entity types disagree",
            ));
        }
        parameter.text = parameter_text(parameter.entity_type, &parameter.values)?;
        parameter.lines = render_parameter_lines(&parameter.text, parameter.entity_type == 306)?;
    }
    let mut parameter_sequence = 1_u32;
    for parameter in parameters.iter_mut() {
        parameter.first_sequence = parameter_sequence;
        parameter_sequence = parameter_sequence
            .checked_add(
                u32::try_from(parameter.lines.len())
                    .map_err(|_| malformed("normalized Parameter Data line count exceeds u32"))?,
            )
            .ok_or_else(|| malformed("normalized Parameter Data sequence overflows"))?;
    }
    let mut parameter_starts =
        ctx.alloc_filled(directory.len(), 0_u32, "iges_binary_parameter_starts")?;
    let mut parameter_counts =
        ctx.alloc_filled(directory.len(), 0_usize, "iges_binary_parameter_counts")?;
    for (directory_index, directory_record) in directory.iter().enumerate() {
        let pointer = directory_record.parameter_pointer;
        if pointer < 0 {
            return Err(malformed(
                "Binary Directory Parameter Data pointer is negative",
            ));
        }
        if pointer == 0 {
            continue;
        }
        let parameter_offset = positive_pointer(pointer, "Directory Parameter Data")?;
        let parameter_index = *parameter_by_offset
            .get(&parameter_offset)
            .ok_or_else(|| malformed("Binary Directory Parameter Data pointer does not resolve"))?;
        if !referenced_parameters.insert(parameter_index) {
            return Err(malformed(
                "Binary Parameter Data entry is referenced by more than one Directory Entry",
            ));
        }
        parameter_starts[directory_index] = parameters[parameter_index].first_sequence;
        parameter_counts[directory_index] = parameters[parameter_index].lines.len();
    }
    if referenced_parameters.len() != parameters.len() {
        return Err(malformed(
            "Binary Parameter Data section contains an unreferenced entry",
        ));
    }
    let mut directory_sequence = 1_u32;
    for (index, directory_record) in directory.iter().enumerate() {
        let first_sequence = directory_sequence;
        let second_sequence = first_sequence
            .checked_add(1)
            .ok_or_else(|| malformed("normalized Directory sequence overflows"))?;
        let values = &directory_record.values;
        let first = [
            render_field(&values[0], false, false)?,
            render_field(
                &BinaryValue::Pointer(i64::from(parameter_starts[index])),
                false,
                false,
            )?,
            render_field(&values[2], false, false)?,
            render_field(&values[3], false, false)?,
            render_field(&values[4], false, false)?,
            render_field(&values[5], false, false)?,
            render_field(&values[6], false, false)?,
            render_field(&values[7], false, false)?,
            render_field(&values[8], false, true)?,
        ];
        let second = [
            render_field(&values[0], false, false)?,
            render_field(&values[9], false, false)?,
            render_field(&values[10], false, false)?,
            render_field(
                &BinaryValue::Integer(i64::try_from(parameter_counts[index]).map_err(|_| {
                    malformed("normalized Parameter Data line count exceeds signed range")
                })?),
                false,
                false,
            )?,
            render_field(&values[11], false, false)?,
            render_field(&values[12], false, false)?,
            render_field(&values[13], false, false)?,
            render_field(&values[14], true, false)?,
            render_field(&values[15], false, false)?,
        ];
        let mut first_data = [b' '; CARD_DATA_WIDTH];
        let mut second_data = [b' '; CARD_DATA_WIDTH];
        for (field, bytes) in first.iter().chain(second.iter()).enumerate() {
            let target = if field < 9 {
                &mut first_data[field * 8..(field + 1) * 8]
            } else {
                &mut second_data[(field - 9) * 8..(field - 8) * 8]
            };
            target.copy_from_slice(bytes);
        }
        render_directory_card(output, &first_data, first_sequence)?;
        render_directory_card(output, &second_data, second_sequence)?;
        directory_sequence = second_sequence
            .checked_add(1)
            .ok_or_else(|| malformed("normalized Directory sequence overflows"))?;
    }
    let mut parameter_sequence = 1_u32;
    for parameter in parameters.iter() {
        let directory_pointer =
            positive_pointer(parameter.directory_pointer, "Parameter Directory")?;
        let directory_index = *directory_by_offset
            .get(&directory_pointer)
            .ok_or_else(|| malformed("Binary Parameter Directory pointer does not resolve"))?;
        let directory_sequence = 1_u32
            .checked_add(
                u32::try_from(directory_index)
                    .map_err(|_| malformed("normalized Directory index exceeds u32"))?
                    .checked_mul(2)
                    .ok_or_else(|| malformed("normalized Directory sequence overflows"))?,
            )
            .ok_or_else(|| malformed("normalized Directory sequence overflows"))?;
        for line in &parameter.lines {
            render_parameter_line(output, line, directory_sequence, parameter_sequence)?;
            parameter_sequence = parameter_sequence
                .checked_add(1)
                .ok_or_else(|| malformed("normalized Parameter Data sequence overflows"))?;
        }
    }
    let directory_count = directory
        .len()
        .checked_mul(2)
        .ok_or_else(|| malformed("normalized Directory count overflows"))?;
    let parameter_count = parameter_sequence
        .checked_sub(1)
        .ok_or_else(|| malformed("normalized Parameter Data count underflows"))?;
    Ok((
        directory_count,
        usize::try_from(parameter_count)
            .map_err(|_| malformed("normalized Parameter Data count is out of range"))?,
    ))
}

fn render_parameter_lines(data: &[u8], language: bool) -> Result<Vec<Vec<u8>>, CodecError> {
    if language {
        return Ok(data
            .chunks(PARAMETER_DATA_WIDTH)
            .map(<[u8]>::to_vec)
            .collect());
    }
    crate::parameter::layout_parameter_cards(data)
}

fn render_directory_card(
    output: &mut Vec<u8>,
    data: &[u8; CARD_DATA_WIDTH],
    sequence: u32,
) -> Result<(), CodecError> {
    if sequence == 0 || sequence > MAX_SEQUENCE {
        return Err(malformed(
            "normalized Directory sequence exceeds seven digits",
        ));
    }
    let mut card = [b' '; CARD_WIDTH];
    card[..CARD_DATA_WIDTH].copy_from_slice(data);
    card[72] = b'D';
    card[73..].copy_from_slice(format!("{sequence:>7}").as_bytes());
    output.extend_from_slice(&card);
    output.push(b'\n');
    Ok(())
}

fn render_parameter_line(
    output: &mut Vec<u8>,
    data: &[u8],
    directory_sequence: u32,
    sequence: u32,
) -> Result<(), CodecError> {
    if data.len() > PARAMETER_DATA_WIDTH || sequence == 0 || sequence > MAX_SEQUENCE {
        return Err(malformed("normalized Parameter Data card is out of range"));
    }
    let mut card = [b' '; CARD_WIDTH];
    card[..data.len()].copy_from_slice(data);
    card[64..72].copy_from_slice(format!("{directory_sequence:>8}").as_bytes());
    card[72] = b'P';
    card[73..].copy_from_slice(format!("{sequence:>7}").as_bytes());
    output.extend_from_slice(&card);
    output.push(b'\n');
    Ok(())
}

fn charge_normalization(
    ctx: &DecodeContext<'_>,
    source_len: usize,
    normalized_len: usize,
) -> Result<(), CodecError> {
    ctx.charge_work(
        u64::try_from(source_len.saturating_add(normalized_len)).unwrap_or(u64::MAX),
        "iges_binary_normalization",
    )
}

/// Normalize one Binary IGES source into the Fixed ASCII image consumed by the
/// typed reader.
pub(crate) fn normalize(source: &[u8], ctx: &DecodeContext<'_>) -> Result<Vec<u8>, CodecError> {
    let sections = parse_sections(source)?;
    let start_text = normalize_start(sections.start, sections.lengths)?;
    let global_values = read_global(sections.global, sections.lengths)?;
    let global_text = normalize_global(&global_values)?;
    let directory = read_directory(sections.directory, sections.lengths)?;
    let directory_by_offset = directory
        .iter()
        .enumerate()
        .map(|(index, record)| (record.offset, index))
        .collect::<BTreeMap<_, _>>();
    let mut parameters =
        read_parameters(sections.parameter, sections.lengths, &directory_by_offset)?;
    let mut output = Vec::new();
    let mut start_sequence = 1_u32;
    render_start_cards(&mut output, &start_text, &mut start_sequence)?;
    let start_count = start_sequence.saturating_sub(1) as usize;
    let mut global_sequence = 1_u32;
    let global_cards = crate::global::layout_global_cards(&global_text)?;
    for card in &global_cards {
        render_cards(&mut output, card, b'G', &mut global_sequence)?;
    }
    let global_count = global_sequence.saturating_sub(1) as usize;
    let (directory_count, parameter_count) =
        normalize_directory_and_parameters(&mut output, &directory, &mut parameters, ctx)?;
    render_terminate(
        &mut output,
        start_count,
        global_count,
        directory_count,
        parameter_count,
    )?;
    charge_normalization(ctx, source.len(), output.len())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{normalize, BinaryValue, PrimitiveLengths, ValueStream};
    use cadmpeg_core::decode::InspectOptions;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    const EPS_POINT: f64 = 1.0e-12;

    fn normalize_for_test(source: &[u8]) -> Result<Vec<u8>, cadmpeg_core::CodecError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)?;
        normalize(source, &ctx)
    }

    #[derive(Debug, Default)]
    struct BitWriter {
        bytes: Vec<u8>,
        bit: u8,
    }

    impl BitWriter {
        fn push_bits(&mut self, value: u64, count: u8) {
            for index in (0..count).rev() {
                let bit = ((value >> index) & 1) as u8;
                if self.bit == 0 {
                    self.bytes.push(0);
                }
                if bit != 0 {
                    let last = self.bytes.len() - 1;
                    self.bytes[last] |= 1 << (7 - self.bit);
                }
                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                }
            }
        }

        fn align(&mut self) {
            if self.bit != 0 {
                self.push_bits(0, 8 - self.bit);
            }
        }

        fn control(&mut self, format: u8) {
            self.align();
            self.push_bits(u64::from(format), 8);
        }

        fn control_repeat(&mut self, physically_present: bool, repeat: u8, format: u8) {
            assert!((1..=16).contains(&repeat));
            assert!(format <= 6);
            self.align();
            let presence = u64::from(physically_present) << 7;
            let repetition = u64::from(repeat - 1) << 3;
            self.push_bits(presence | repetition | u64::from(format), 8);
        }

        fn integer(&mut self, value: i64, width: u8) {
            self.push_bits(u64::from(value < 0), 1);
            self.push_bits(value.unsigned_abs(), width - 1);
            self.align();
        }

        fn pointer(&mut self, value: i64) {
            self.integer(value, 32);
        }

        fn real(&mut self, value: f64, exponent_bits: u8, fraction_bits: u8) {
            if value == 0.0 {
                self.push_bits(0, 1);
                self.push_bits(0, exponent_bits);
                self.push_bits(0, fraction_bits);
                self.align();
                return;
            }
            let negative = value.is_sign_negative();
            let value = value.abs();
            let mut exponent = value.log2().floor() as i64;
            let mut fraction = value / 2_f64.powi(exponent as i32);
            if fraction >= 1.0 {
                exponent += 1;
                fraction /= 2.0;
            }
            let scale = 2_f64.powi(i32::from(fraction_bits) + 1);
            let raw_fraction = ((fraction - 0.5) * scale).round() as u64;
            let bias = 1_i64 << (exponent_bits - 1);
            let biased = u64::try_from(exponent + bias)
                .expect("Binary test real exponent fits the selected field");
            self.push_bits(u64::from(negative), 1);
            self.push_bits(biased, exponent_bits);
            self.push_bits(raw_fraction, fraction_bits);
            self.align();
        }

        fn string(&mut self, value: &[u8], lengths: PrimitiveLengths) {
            self.string_part(value, false, lengths);
        }

        fn string_part(&mut self, value: &[u8], continues: bool, lengths: PrimitiveLengths) {
            let count =
                i64::try_from(value.len()).expect("Binary test string length fits the integer");
            self.integer(
                if continues { -count } else { count },
                lengths.single_integer,
            );
            for byte in value {
                self.push_bits(u64::from(*byte), 8);
            }
            self.align();
        }

        fn bytes(self) -> Vec<u8> {
            self.bytes
        }
    }

    fn lengths() -> PrimitiveLengths {
        PrimitiveLengths {
            single_integer: 32,
            double_integer: 64,
            single_exponent: 8,
            single_fraction: 23,
            double_exponent: 11,
            double_fraction: 52,
        }
    }

    fn primitive_string(value: &[u8], lengths: PrimitiveLengths) -> Vec<u8> {
        let mut writer = BitWriter::default();
        writer.control(6);
        writer.string(value, lengths);
        writer.bytes()
    }

    fn integer_value(writer: &mut BitWriter, value: i64, lengths: PrimitiveLengths) {
        writer.control(1);
        writer.integer(value, lengths.single_integer);
    }

    fn global_payload(lengths: PrimitiveLengths) -> Vec<u8> {
        let mut writer = BitWriter::default();
        writer.control(0);
        writer.control(0);
        for (format, value) in [
            (6, b"product".as_slice()),
            (6, b"part.igs".as_slice()),
            (6, b"cadmpeg".as_slice()),
            (6, b"0.1".as_slice()),
        ] {
            writer.control(format);
            writer.string(value, lengths);
        }
        for value in [32, 38, 6, 308, 15] {
            writer.control(1);
            writer.integer(value, lengths.single_integer);
        }
        writer.control(0);
        writer.control(3);
        writer.real(1.0, lengths.single_exponent, lengths.single_fraction);
        writer.control(1);
        writer.integer(2, lengths.single_integer);
        writer.control(6);
        writer.string(b"MM", lengths);
        writer.control(1);
        writer.integer(1, lengths.single_integer);
        writer.control(3);
        writer.real(1.0, lengths.single_exponent, lengths.single_fraction);
        writer.control(6);
        writer.string(b"260714.000000", lengths);
        writer.control(3);
        writer.real(0.001, lengths.single_exponent, lengths.single_fraction);
        writer.control(3);
        writer.real(1000.0, lengths.single_exponent, lengths.single_fraction);
        writer.control(6);
        writer.string(b"author", lengths);
        writer.control(6);
        writer.string(b"org", lengths);
        writer.control(1);
        writer.integer(6, lengths.single_integer);
        writer.control(1);
        writer.integer(0, lengths.single_integer);
        writer.bytes()
    }

    fn directory_payload(lengths: PrimitiveLengths) -> Vec<u8> {
        let mut body = BitWriter::default();
        integer_value(&mut body, 116, lengths);
        body.control(5);
        body.pointer(1);
        for value in [0, 1, 0, 0, 0, 0, 0] {
            integer_value(&mut body, value, lengths);
        }
        integer_value(&mut body, 0, lengths);
        integer_value(&mut body, 0, lengths);
        integer_value(&mut body, 0, lengths);
        body.control(0);
        body.control(0);
        body.control(6);
        body.string(b"P", lengths);
        integer_value(&mut body, 0, lengths);
        let body = body.bytes();
        let mut payload = Vec::new();
        payload.extend_from_slice(
            &u32::try_from(body.len())
                .expect("Binary test Directory body fits a u32")
                .to_be_bytes(),
        );
        payload.extend_from_slice(&body);
        payload
    }

    fn parameter_payload(lengths: PrimitiveLengths) -> Vec<u8> {
        let mut body = BitWriter::default();
        body.control(1);
        body.integer(116, lengths.single_integer);
        body.control(5);
        body.pointer(1);
        for value in [1.0, 2.0, 3.0] {
            body.control(3);
            body.real(value, lengths.single_exponent, lengths.single_fraction);
        }
        body.control(5);
        body.pointer(0);
        let body = body.bytes();
        let mut payload = Vec::new();
        payload.extend_from_slice(
            &u32::try_from(body.len())
                .expect("Binary test Parameter body fits a u32")
                .to_be_bytes(),
        );
        payload.extend_from_slice(&body);
        payload
    }

    fn section(identifier: u8, payload: &[u8]) -> Vec<u8> {
        let mut section = vec![identifier];
        section.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("Binary test section payload fits a u32")
                .to_be_bytes(),
        );
        section.extend_from_slice(payload);
        section
    }

    fn binary_file_with_start(start_text: &[u8]) -> Vec<u8> {
        let lengths = lengths();
        let start = section(b'S', &primitive_string(start_text, lengths));
        let global = section(b'G', &global_payload(lengths));
        let directory = section(b'D', &directory_payload(lengths));
        let parameter = section(b'P', &parameter_payload(lengths));
        let terminate_payload = {
            let mut payload = Vec::new();
            for (marker, size) in [
                (b'B', 80_u32),
                (
                    b'S',
                    u32::try_from(start.len()).expect("Binary test Start section fits a u32"),
                ),
                (
                    b'G',
                    u32::try_from(global.len()).expect("Binary test Global section fits a u32"),
                ),
                (
                    b'D',
                    u32::try_from(directory.len())
                        .expect("Binary test Directory section fits a u32"),
                ),
                (
                    b'P',
                    u32::try_from(parameter.len())
                        .expect("Binary test Parameter section fits a u32"),
                ),
            ] {
                payload.push(marker);
                payload.extend_from_slice(&size.to_be_bytes());
            }
            payload
        };
        let terminate = section(b'T', &terminate_payload);
        let mut flag = vec![0_u8; 80];
        flag[0] = b'B';
        flag[1..5].copy_from_slice(&75_u32.to_be_bytes());
        flag[5] = lengths.single_integer;
        flag[6] = lengths.double_integer;
        flag[7] = lengths.single_exponent;
        flag[8] = lengths.single_fraction;
        flag[9] = lengths.double_exponent;
        flag[10] = lengths.double_fraction;
        let starts = [
            (11, b'B'),
            (16, b'S'),
            (21, b'G'),
            (26, b'D'),
            (31, b'P'),
            (36, b'T'),
        ];
        for (index, (marker_offset, marker)) in starts.into_iter().enumerate() {
            flag[marker_offset] = marker;
            let value = if index == 0 {
                80
            } else {
                match index {
                    1 => u32::try_from(start.len()).expect("Binary test Start section fits a u32"),
                    2 => {
                        u32::try_from(global.len()).expect("Binary test Global section fits a u32")
                    }
                    3 => u32::try_from(directory.len())
                        .expect("Binary test Directory section fits a u32"),
                    4 => u32::try_from(parameter.len())
                        .expect("Binary test Parameter section fits a u32"),
                    5 => u32::try_from(terminate.len())
                        .expect("Binary test Terminate section fits a u32"),
                    _ => unreachable!("Binary fixture has six sections"),
                }
            };
            flag[marker_offset + 1..marker_offset + 5].copy_from_slice(&value.to_be_bytes());
        }
        flag[72] = b'B';
        flag[73..79].fill(b'0');
        flag[79] = b'1';
        let mut source = flag;
        source.extend_from_slice(&start);
        source.extend_from_slice(&global);
        source.extend_from_slice(&directory);
        source.extend_from_slice(&parameter);
        source.extend_from_slice(&terminate);
        source.push(b'E');
        source
    }

    fn binary_point_file() -> Vec<u8> {
        binary_file_with_start(b"binary fixture")
    }

    #[test]
    fn binary_control_byte_expands_physical_and_implicit_repetitions() {
        let lengths = PrimitiveLengths {
            single_integer: 8,
            double_integer: 8,
            single_exponent: 8,
            single_fraction: 7,
            double_exponent: 8,
            double_fraction: 7,
        };
        let mut physical = BitWriter::default();
        physical.control_repeat(true, 3, 1);
        for value in [1, 2, 3] {
            physical.integer(value, lengths.single_integer);
        }
        let physical_bytes = physical.bytes();
        let mut stream = ValueStream::new(&physical_bytes, lengths);
        assert_eq!(
            stream.next().expect("physical value"),
            Some(BinaryValue::Integer(1))
        );
        assert_eq!(
            stream.next().expect("physical value"),
            Some(BinaryValue::Integer(2))
        );
        assert_eq!(
            stream.next().expect("physical value"),
            Some(BinaryValue::Integer(3))
        );
        assert_eq!(stream.next().expect("end of physical values"), None);
        stream.finish().expect("physical repetitions consumed");

        let mut implicit = BitWriter::default();
        implicit.control_repeat(false, 3, 1);
        implicit.integer(7, lengths.single_integer);
        let implicit_bytes = implicit.bytes();
        let mut stream = ValueStream::new(&implicit_bytes, lengths);
        for _ in 0..3 {
            assert_eq!(
                stream.next().expect("implicit value"),
                Some(BinaryValue::Integer(7))
            );
        }
        assert_eq!(stream.next().expect("end of implicit values"), None);
        stream.finish().expect("implicit repetitions consumed");
    }

    #[test]
    fn binary_primitive_formats_decode_with_padding_and_substrings() {
        let lengths = PrimitiveLengths {
            single_integer: 8,
            double_integer: 16,
            single_exponent: 4,
            single_fraction: 7,
            double_exponent: 5,
            double_fraction: 10,
        };
        let mut writer = BitWriter::default();
        writer.control(0);
        writer.control(2);
        writer.integer(-257, lengths.double_integer);
        writer.control(3);
        writer.real(-3.5, lengths.single_exponent, lengths.single_fraction);
        writer.control(4);
        writer.real(0.125, lengths.double_exponent, lengths.double_fraction);
        writer.control(5);
        writer.pointer(-7);
        writer.control(6);
        writer.string_part(b"abc", true, lengths);
        writer.string_part(b"de", false, lengths);
        let bytes = writer.bytes();

        let mut stream = ValueStream::new(&bytes, lengths);
        assert_eq!(
            stream.next().expect("default value"),
            Some(BinaryValue::Default)
        );
        assert_eq!(
            stream.next().expect("double integer"),
            Some(BinaryValue::Integer(-257))
        );
        match stream.next().expect("single real") {
            Some(BinaryValue::Real(value)) => assert!((value + 3.5).abs() < f64::EPSILON),
            other => panic!("expected single real, got {other:?}"),
        }
        match stream.next().expect("double real") {
            Some(BinaryValue::Real(value)) => assert!((value - 0.125).abs() < f64::EPSILON),
            other => panic!("expected double real, got {other:?}"),
        }
        assert_eq!(
            stream.next().expect("pointer"),
            Some(BinaryValue::Pointer(-7))
        );
        assert_eq!(
            stream.next().expect("substring string"),
            Some(BinaryValue::String(b"abcde".to_vec()))
        );
        assert_eq!(stream.next().expect("end of primitives"), None);
        stream.finish().expect("all primitive bytes consumed");
    }

    #[test]
    fn binary_point_normalizes_into_the_existing_typed_pipeline() {
        let source = binary_point_file();
        let normalized = normalize_for_test(&source).expect("valid Binary fixture");
        assert_eq!(normalized[72], b'S');
        assert!(normalized
            .chunks_exact(81)
            .any(|card| card.get(72) == Some(&b'G')));
        let result = crate::IgesCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("Binary point decodes");
        assert_eq!(
            result
                .ir()
                .source
                .as_ref()
                .expect("Binary decode retains source metadata")
                .attributes["representation"],
            "binary"
        );
        assert_eq!(result.ir().model.points.len(), 1);
        let point = &result.ir().model.points[0].position;
        assert!((point.x - 1.0).abs() < EPS_POINT);
        assert!((point.y - 2.0).abs() < EPS_POINT);
        assert!((point.z - 3.0).abs() < EPS_POINT);
    }

    #[test]
    fn binary_inspection_reports_the_normalized_container() {
        let result = crate::IgesCodec
            .inspect(
                &mut Cursor::new(binary_point_file()),
                &InspectOptions::default(),
            )
            .expect("Binary inspection");
        assert_eq!(result.container_kind, "binary");
        assert!(result
            .notes
            .iter()
            .any(|note| note == "normalized_representation=binary"));
    }

    #[test]
    fn binary_start_carriage_returns_become_fixed_start_records() {
        let normalized = normalize_for_test(&binary_file_with_start(b"first\rsecond\r\nthird"))
            .expect("valid Binary Start text");
        let cards = normalized
            .chunks_exact(81)
            .filter(|card| card.get(72) == Some(&b'S'))
            .collect::<Vec<_>>();
        assert_eq!(cards.len(), 3);
        assert_eq!(&cards[0][..5], b"first");
        assert_eq!(&cards[1][..6], b"second");
        assert_eq!(&cards[2][..5], b"third");
    }

    #[test]
    fn binary_start_rejects_a_lone_line_feed() {
        assert!(normalize_for_test(&binary_file_with_start(b"first\nsecond")).is_err());
    }
}
