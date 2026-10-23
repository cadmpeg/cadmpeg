// SPDX-License-Identifier: Apache-2.0
//! IGES Binary representation normalization.
//!
//! The Binary representation carries the same section and entity model as
//! Fixed ASCII, but replaces the free-form constants with a bit-packed,
//! control-byte stream.  This module decodes that physical envelope and
//! emits a bounded Fixed ASCII image for the existing section, Directory, and
//! Parameter Data owners.  The original Binary image remains the source image
//! passed to the reader, so normalization does not replace source fidelity.

use crate::directory::DirectoryFieldSlot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) mod parts;

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
    start: View<'a>,
    global: View<'a>,
    directory: View<'a>,
    parameter: View<'a>,
    terminate: View<'a>,
    lengths: PrimitiveLengths,
}

#[derive(Debug, Clone, PartialEq)]
enum BinaryValue {
    Default,
    Integer(i64),
    Real(FiniteReal),
    Pointer(i64),
    String(Vec<u8>),
}

#[derive(Debug)]
struct BinaryDirectory {
    offset: u32,
    source_sequence: u32,
    entity_type: i64,
    parameter_pointer: i64,
    /// Retains the wire fields in shared slot order for rendering beside the checked entity discriminator.
    values: [BinaryValue; 15],
    /// Body-relative byte range of each wire value, in shared slot order.
    value_extents: [std::ops::Range<usize>; 15],
}

/// The optional Directory fields (4, 5, 6, 8, 12, 13 and 19): shared slot and
/// field name.
const OPTIONAL_DIRECTORY_FIELDS: [(DirectoryFieldSlot, &str); 7] = [
    (DirectoryFieldSlot::LineFont, "line font"),
    (DirectoryFieldSlot::Level, "level"),
    (DirectoryFieldSlot::View, "view"),
    (DirectoryFieldSlot::LabelDisplay, "label display"),
    (DirectoryFieldSlot::LineWeight, "line weight"),
    (DirectoryFieldSlot::Color, "color"),
    (DirectoryFieldSlot::Subscript, "entity subscript"),
];

/// Rendered text of an optional value whose part is given up. It is not a
/// decimal integer, so the Directory admission states no value for the field.
const UNREADABLE_OPTIONAL_FIELD: &[u8] = b"*";

impl BinaryDirectory {
    fn new(
        offset: u32,
        source_sequence: u32,
        values: [BinaryValue; 16],
        extents: [std::ops::Range<usize>; 16],
    ) -> Result<Self, CodecError> {
        #[rustfmt::skip]
        let [
            entity_type, parameter_pointer, structure, line_font, level, view, transform,
            label_display, status, line_weight, color, form, reserved_first, reserved_second,
            label, subscript,
        ] = values;
        #[rustfmt::skip]
        let [
            entity_type_at, _, structure_at, line_font_at, level_at, view_at, transform_at,
            label_display_at, status_at, line_weight_at, color_at, form_at, reserved_first_at,
            reserved_second_at, label_at, subscript_at,
        ] = extents;
        Ok(Self {
            offset,
            source_sequence,
            entity_type: integer_value(&entity_type, "Directory entity type")?,
            parameter_pointer: pointer_value(&parameter_pointer, "Directory Parameter Data")?,
            values: [
                entity_type,
                structure,
                line_font,
                level,
                view,
                transform,
                label_display,
                status,
                line_weight,
                color,
                form,
                reserved_first,
                reserved_second,
                label,
                subscript,
            ],
            value_extents: [
                entity_type_at,
                structure_at,
                line_font_at,
                level_at,
                view_at,
                transform_at,
                label_display_at,
                status_at,
                line_weight_at,
                color_at,
                form_at,
                reserved_first_at,
                reserved_second_at,
                label_at,
                subscript_at,
            ],
        })
    }

    /// Admit one optional field: its rendered text is a decimal integer.
    fn admit_optional_field(
        &self,
        slot: DirectoryFieldSlot,
        name: &'static str,
    ) -> Result<(), CodecError> {
        let field = render_field(&self.values[slot.slot()], FieldRendering::Plain)?;
        crate::directory::integer(field, name).map_err(|error| malformed(error.to_string()))?;
        Ok(())
    }

    /// State no value for an optional field whose part is given up.
    fn give_up_optional_field(&mut self, slot: DirectoryFieldSlot) {
        self.values[slot.slot()] = BinaryValue::String(UNREADABLE_OPTIONAL_FIELD.to_vec());
    }

    fn entity_type(&self) -> i64 {
        self.entity_type
    }

    fn parameter_pointer(&self) -> i64 {
        self.parameter_pointer
    }

    /// State no Parameter Data for a record whose Parameter Data record is
    /// given up or absent.
    fn detach_parameter_data(&mut self) {
        self.parameter_pointer = 0;
    }

    fn admit_directory_fields(&self) -> Result<(), CodecError> {
        for (slot, name) in [
            (DirectoryFieldSlot::Structure, "structure"),
            (DirectoryFieldSlot::Form, "form"),
        ] {
            let value = &self.values[slot.slot()];
            if !matches!(value, BinaryValue::Pointer(_)) {
                let field = render_field(value, FieldRendering::Plain)?;
                crate::directory::integer(field, name)
                    .map_err(|error| malformed(error.to_string()))?;
            }
        }
        let value = &self.values[DirectoryFieldSlot::Status.slot()];
        if !matches!(value, BinaryValue::Pointer(_)) {
            let field = render_field(value, FieldRendering::Status)?;
            crate::directory::status(field).map_err(|error| malformed(error.to_string()))?;
        }
        Ok(())
    }

    fn has_point_parameters(&self) -> bool {
        self.entity_type == 116
            && matches!(
                self.values[DirectoryFieldSlot::Form.slot()],
                BinaryValue::Default | BinaryValue::Integer(0)
            )
    }
}

#[derive(Debug)]
struct BinaryParameter {
    offset: u32,
    entity_type: i64,
    directory_pointer: i64,
    values: Vec<BinaryValue>,
    admitted: Option<AdmittedBinaryParameter>,
}

#[derive(Debug)]
struct AdmittedBinaryParameter {
    lines: Vec<Vec<u8>>,
    content: crate::parameter::AdmittedParameter,
}

#[derive(Debug)]
struct NormalizedParameter {
    offset: u32,
    directory_sequence: u32,
    lines: Vec<Vec<u8>>,
    first_sequence: u32,
    admitted: Option<crate::parameter::AdmittedParameter>,
}

#[derive(Debug)]
enum BitInput<'a> {
    Detached(&'a [u8]),
    Original(View<'a>),
}

impl BitInput<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Detached(bytes) => bytes,
            Self::Original(source) => source.window(),
        }
    }

    fn byte(&self, offset: usize, semantic: bool) -> Option<u8> {
        match self {
            Self::Original(source) if semantic => source
                .read_at(source.start().checked_add(offset)?, 1)?
                .first()
                .copied(),
            Self::Original(_) | Self::Detached(_) => self.bytes().get(offset).copied(),
        }
    }
}

#[derive(Debug)]
struct BitReader<'a> {
    input: BitInput<'a>,
    semantic: bool,
    byte: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            input: BitInput::Detached(bytes),
            semantic: false,
            byte: 0,
            bit: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.byte == self.input.bytes().len() && self.bit == 0
    }

    fn read_bits(&mut self, count: u8) -> Result<u64, CodecError> {
        self.read_bits_as(count, self.semantic)
    }

    fn read_bits_as(&mut self, count: u8, semantic: bool) -> Result<u64, CodecError> {
        if count > 64 {
            return Err(malformed("a Binary primitive requests more than 64 bits"));
        }
        let mut value = 0_u64;
        for _ in 0..count {
            let byte = self
                .input
                .byte(self.byte, semantic)
                .ok_or_else(|| malformed("a Binary primitive is truncated"))?;
            let shift = 7_u8
                .checked_sub(self.bit)
                .ok_or_else(|| malformed("a Binary bit offset is out of range"))?;
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
        let padding = self.read_bits_as(8 - self.bit, false)?;
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

    fn read_real(
        &mut self,
        exponent_bits: u8,
        fraction_bits: u8,
    ) -> Result<FiniteReal, CodecError> {
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
            return Ok(FiniteReal::ZERO);
        }
        let bias = 1_u64 << (exponent_bits - 1);
        let exponent = i128::from(biased_exponent) - i128::from(bias);
        if exponent > 1024 {
            return Err(malformed("a Binary real is not finite"));
        }
        if exponent < -1074 {
            return FiniteReal::new(if negative { -0.0 } else { 0.0 })
                .ok_or_else(|| malformed("a Binary real is not finite"));
        }
        // The range checks prove this conversion and keep the power finite.
        let exponent = i32::try_from(exponent)
            .map_err(|_| malformed("a Binary real exponent is out of range"))?;
        let fraction = cadmpeg_core::convert::f64_from_u64(fraction)
            .ok_or_else(|| malformed("a Binary real fraction is not exactly representable"))?;
        let fraction = 0.5 + fraction / 2_f64.powi(i32::from(fraction_bits) + 1);
        let value = if exponent >= -1021 {
            (fraction * 2.0) * 2_f64.powi(exponent - 1)
        } else {
            (fraction * 2_f64.powi(exponent + 1074)) * f64::from_bits(1)
        };
        FiniteReal::new(if negative { -value } else { value })
            .ok_or_else(|| malformed("a Binary real is not finite"))
    }

    fn read_string(&mut self, lengths: PrimitiveLengths) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::new();
        loop {
            let semantic = self.semantic;
            self.semantic = false;
            let count = self.read_integer(lengths.single_integer)?;
            self.semantic = semantic;
            if count == 0 {
                return Err(malformed("a Binary string has a zero character count"));
            }
            let negative = count < 0;
            let count = usize::try_from(count.unsigned_abs())
                .map_err(|_| malformed("a Binary string count is out of range"))?;
            let remaining = self
                .input
                .bytes()
                .len()
                .checked_sub(self.byte)
                .ok_or_else(|| malformed("a Binary string offset is out of range"))?;
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

enum RepeatValue {
    Physical(u8),
    Shared(BinaryValue, std::ops::Range<usize>),
}

struct Repeat {
    value: RepeatValue,
    remaining: usize,
}

struct ValueStream<'a> {
    bits: BitReader<'a>,
    lengths: PrimitiveLengths,
    repeat: Option<Repeat>,
    value_extent: std::ops::Range<usize>,
    field: usize,
    semantic_end: usize,
}

impl<'a> ValueStream<'a> {
    fn new(bytes: &'a [u8], lengths: PrimitiveLengths) -> Self {
        Self {
            bits: BitReader::new(bytes),
            lengths,
            repeat: None,
            value_extent: 0..0,
            field: 0,
            semantic_end: 0,
        }
    }

    fn source_line(source: View<'a>, lengths: PrimitiveLengths) -> Self {
        Self {
            bits: BitReader {
                input: BitInput::Original(source),
                semantic: false,
                byte: 0,
                bit: 0,
            },
            lengths,
            repeat: None,
            value_extent: 0..0,
            field: 0,
            semantic_end: 8,
        }
    }

    fn source_point(source: View<'a>, lengths: PrimitiveLengths) -> Self {
        let mut stream = Self::source_line(source, lengths);
        stream.semantic_end = 5;
        stream
    }

    fn semantic(&self, count: usize) -> bool {
        matches!(self.bits.input, BitInput::Original(_))
            && self.field < self.semantic_end
            && self.field.checked_add(count).is_some_and(|end| end > 2)
    }

    fn one(&mut self, format: u8, semantic: bool) -> Result<BinaryValue, CodecError> {
        self.bits.semantic = semantic;
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
        if let Some(mut repeat) = self.repeat.take() {
            let value = match &repeat.value {
                RepeatValue::Physical(format) => {
                    let start = self.bits.byte;
                    let value = self.one(*format, self.semantic(1))?;
                    self.value_extent = start..self.bits.byte;
                    value
                }
                RepeatValue::Shared(value, extent) => {
                    self.value_extent = extent.clone();
                    value.clone()
                }
            };
            repeat.remaining -= 1;
            if repeat.remaining > 0 {
                self.repeat = Some(repeat);
            }
            self.field = self
                .field
                .checked_add(1)
                .ok_or_else(|| malformed("Binary value ordinal overflows"))?;
            return Ok(Some(value));
        }
        if self.bits.is_empty() {
            return Ok(None);
        }
        if self.bits.bit != 0 {
            return Err(malformed("a Binary control byte is not byte aligned"));
        }
        let control = u8::try_from(self.bits.read_bits_as(8, false)?)
            .map_err(|_| malformed("a Binary control byte overflows"))?;
        let physically_present = control >> 7 == 1;
        let repeat = usize::from((control >> 3) & 0x0f) + 1;
        let format = control & 0x07;
        let start = self.bits.byte;
        let first = self.one(
            format,
            self.semantic(if physically_present { 1 } else { repeat }),
        )?;
        self.value_extent = start..self.bits.byte;
        if repeat > 1 {
            self.repeat = Some(Repeat {
                value: if physically_present {
                    RepeatValue::Physical(format)
                } else {
                    RepeatValue::Shared(first.clone(), self.value_extent.clone())
                },
                remaining: repeat - 1,
            });
        }
        self.field = self
            .field
            .checked_add(1)
            .ok_or_else(|| malformed("Binary value ordinal overflows"))?;
        Ok(Some(first))
    }

    fn finish(&self) -> Result<(), CodecError> {
        if self.repeat.is_some() || !self.bits.is_empty() {
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
    source: View<'_>,
    start: usize,
    expected: u8,
    next_start: usize,
) -> Result<View<'_>, CodecError> {
    let bytes = source.window();
    if bytes.get(start) != Some(&expected) {
        return Err(malformed(format!(
            "Binary section at byte {start} is not {:?}",
            char::from(expected)
        )));
    }
    let count = usize::try_from(u32_at(bytes, start + 1)?)
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
    let absolute_start = source
        .start()
        .checked_add(payload_start)
        .ok_or_else(|| malformed("a Binary section start overflows"))?;
    let absolute_end = source
        .start()
        .checked_add(payload_end)
        .ok_or_else(|| malformed("a Binary section end overflows"))?;
    source
        .child(absolute_start, absolute_end)
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

fn parse_sections(source: View<'_>) -> Result<BinarySections<'_>, CodecError> {
    let bytes = source.window();
    let (lengths, offsets) = parse_header(bytes)?;
    if offsets.end > bytes.len() {
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
        .and_then(|offset| offset.checked_add(start.window().len()))
        .ok_or_else(|| malformed("Binary Start section count overflows"))?;
    let global_end = offsets
        .global
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(global.window().len()))
        .ok_or_else(|| malformed("Binary Global section count overflows"))?;
    let directory_end = offsets
        .directory
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(directory.window().len()))
        .ok_or_else(|| malformed("Binary Directory section count overflows"))?;
    let parameter_end = offsets
        .parameter
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(parameter.window().len()))
        .ok_or_else(|| malformed("Binary Parameter section count overflows"))?;
    for (payload_end, next_start) in [
        (start_end, offsets.global),
        (global_end, offsets.directory),
        (directory_end, offsets.parameter),
        (parameter_end, offsets.terminate),
    ] {
        padding_is_zero(bytes, payload_end..next_start)?;
    }
    let terminate_end = offsets
        .terminate
        .checked_add(SECTION_HEADER_WIDTH)
        .and_then(|offset| offset.checked_add(terminate.window().len()))
        .ok_or_else(|| malformed("Binary Terminate section count overflows"))?;
    padding_is_zero(bytes, terminate_end..offsets.end)?;
    validate_terminate(
        terminate.window(),
        [
            BINARY_FLAG_WIDTH,
            SECTION_HEADER_WIDTH + start.window().len(),
            SECTION_HEADER_WIDTH + global.window().len(),
            SECTION_HEADER_WIDTH + directory.window().len(),
            SECTION_HEADER_WIDTH + parameter.window().len(),
        ],
    )?;
    if bytes.get(offsets.end) != Some(&b'E') {
        return Err(malformed(
            "Binary Terminate displacement does not lead to the end marker",
        ));
    }
    Ok(BinarySections {
        start,
        global,
        directory,
        parameter,
        terminate,
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
            || u64::from(u32_at(payload, offset + 1)?) != u64_from_index(size)
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
    let mut values = Vec::new();
    for _ in 0..24 {
        values.push(one_value(&mut stream, "Global")?);
    }
    stream.finish()?;
    Ok(values)
}

fn counted_record<'a>(
    payload: View<'a>,
    cursor: usize,
    section: &str,
) -> Result<(u32, View<'a>, View<'a>), CodecError> {
    let offset = cursor
        .checked_add(1)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or_else(|| malformed(format!("Binary {section} pointer exceeds u32")))?;
    let byte_count = usize::try_from(u32_at(payload.window(), cursor)?)
        .map_err(|_| malformed(format!("Binary {section} entity count is out of range")))?;
    let body_start = cursor
        .checked_add(4)
        .ok_or_else(|| malformed(format!("Binary {section} entity start overflows")))?;
    let body_end = body_start
        .checked_add(byte_count)
        .ok_or_else(|| malformed(format!("Binary {section} entity count overflows")))?;
    if body_end > payload.window().len() {
        return Err(malformed(format!(
            "Binary {section} entity exceeds its section"
        )));
    }
    let start = payload
        .start()
        .checked_add(cursor)
        .ok_or_else(|| malformed(format!("Binary {section} record start overflows")))?;
    let body_start = payload
        .start()
        .checked_add(body_start)
        .ok_or_else(|| malformed(format!("Binary {section} body start overflows")))?;
    let end = payload
        .start()
        .checked_add(body_end)
        .ok_or_else(|| malformed(format!("Binary {section} record end overflows")))?;
    let extent = payload
        .child(start, end)
        .ok_or_else(|| malformed(format!("Binary {section} entity exceeds its section")))?;
    let body = extent
        .child(body_start, end)
        .ok_or_else(|| malformed(format!("Binary {section} entity exceeds its section")))?;
    Ok((offset, extent, body))
}

fn parse_directory_record(
    offset: u32,
    source_sequence: u32,
    body: View<'_>,
    lengths: PrimitiveLengths,
) -> Result<BinaryDirectory, CodecError> {
    let mut stream = ValueStream::new(body.window(), lengths);
    let mut values = std::array::from_fn(|_| BinaryValue::Default);
    let mut extents = std::array::from_fn(|_| 0..0);
    for (value, extent) in values.iter_mut().zip(extents.iter_mut()) {
        *value = one_value(&mut stream, "Directory")?;
        *extent = stream.value_extent.clone();
    }
    stream.finish()?;
    BinaryDirectory::new(offset, source_sequence, values, extents)
}

struct StackText {
    bytes: [u8; 64],
    len: usize,
}

impl StackText {
    fn new() -> Self {
        Self {
            bytes: [0; 64],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl std::fmt::Write for StackText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(std::fmt::Error)?;
        let target = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        target.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn stack_text(args: std::fmt::Arguments<'_>) -> Result<StackText, CodecError> {
    let mut text = StackText::new();
    std::fmt::write(&mut text, args)
        .map_err(|_| malformed("Binary numeric text exceeds the bounded render buffer"))?;
    Ok(text)
}

fn render_real(value: FiniteReal) -> Result<Vec<u8>, CodecError> {
    let value = value.get();
    if value == 0.0 {
        Ok(b"0".to_vec())
    } else {
        let text = stack_text(format_args!("{value:.17E}"))?;
        Ok(text.as_bytes().to_vec())
    }
}

fn render_parameter_value(value: &BinaryValue, language: bool) -> Result<Vec<u8>, CodecError> {
    match value {
        BinaryValue::Default => Ok(Vec::new()),
        BinaryValue::Integer(value) | BinaryValue::Pointer(value) => {
            let text = stack_text(format_args!("{value}"))?;
            Ok(text.as_bytes().to_vec())
        }
        BinaryValue::Real(value) => render_real(*value),
        BinaryValue::String(value) if language => Ok(value.clone()),
        BinaryValue::String(value) => {
            if value.is_empty() {
                return Err(malformed("Binary string constant has no characters"));
            }
            let header = stack_text(format_args!("{}H", value.len()))?;
            let mut output = header.as_bytes().to_vec();
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
                output.extend_from_slice(b";");
            }
            let rendered = render_parameter_value(value, true)?;
            if rendered.is_empty() {
                return Err(malformed("Binary Macro Definition has an empty statement"));
            }
            output.extend_from_slice(&rendered);
        }
        output.extend_from_slice(b";");
        return Ok(output);
    }
    let entity_text = stack_text(format_args!("{entity_type}"))?;
    let mut output = entity_text.as_bytes().to_vec();
    for value in values {
        output.extend_from_slice(b",");
        output.extend_from_slice(&render_parameter_value(value, false)?);
    }
    output.extend_from_slice(b";");
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
enum FieldRendering {
    Plain,
    Status,
}

fn render_field(value: &BinaryValue, rendering: FieldRendering) -> Result<[u8; 8], CodecError> {
    match rendering {
        FieldRendering::Status => match value {
            BinaryValue::Default => crate::directory::render_field(&[]),
            BinaryValue::Integer(value) | BinaryValue::Pointer(value) => {
                let text = stack_text(format_args!("{value:08}"))?;
                crate::directory::render_field(text.as_bytes())
            }
            BinaryValue::Real(_) | BinaryValue::String(_) => {
                Err(malformed("Binary Directory status is not an integer"))
            }
        },
        FieldRendering::Plain => match value {
            BinaryValue::Default => crate::directory::render_field(&[]),
            BinaryValue::Integer(value) | BinaryValue::Pointer(value) => {
                let text = stack_text(format_args!("{value}"))?;
                crate::directory::render_field(text.as_bytes())
            }
            BinaryValue::Real(value) => {
                let text = if value.get() == 0.0 {
                    stack_text(format_args!("0"))?
                } else {
                    stack_text(format_args!("{:.17E}", value.get()))?
                };
                crate::directory::render_field(text.as_bytes())
            }
            BinaryValue::String(value) => crate::directory::render_field(value),
        },
    }
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
    let sequence_bytes = stack_text(format_args!("{:>7}", *sequence))?;
    card[CARD_DATA_WIDTH + 1..].copy_from_slice(sequence_bytes.as_bytes());
    append_output_card(output, &card);
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
        let field = stack_text(format_args!("{}{:>7}", char::from(marker), count))?;
        data[index * 8..(index + 1) * 8].copy_from_slice(field.as_bytes());
    }
    let mut card = [b' '; CARD_WIDTH];
    card[..CARD_DATA_WIDTH].copy_from_slice(&data);
    card[CARD_DATA_WIDTH] = b'T';
    card[CARD_DATA_WIDTH + 1..].copy_from_slice(b"      1");
    append_output_card(output, &card);
    Ok(())
}

fn normalize_start(payload: &[u8], lengths: PrimitiveLengths) -> Result<Vec<u8>, CodecError> {
    let mut stream = ValueStream::new(payload, lengths);
    let mut text = Vec::new();
    while let Some(value) = stream.next()? {
        let BinaryValue::String(value) = value else {
            return Err(malformed(
                "Binary Start section contains a non-text primitive",
            ));
        };
        text.extend_from_slice(&value);
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
            output.extend_from_slice(b",");
        }
        output.extend_from_slice(&render_parameter_value(value, false)?);
    }
    output.extend_from_slice(b";");
    Ok(output)
}

struct ParameterPrefix<'a> {
    stream: ValueStream<'a>,
    entity_type: i64,
    directory_pointer: i64,
    pointer_extent: std::ops::Range<usize>,
}

fn parameter_prefix(
    body: View<'_>,
    lengths: PrimitiveLengths,
) -> Result<ParameterPrefix<'_>, CodecError> {
    prefix_from_stream(ValueStream::new(body.window(), lengths))
}

fn source_line_prefix(
    body: View<'_>,
    lengths: PrimitiveLengths,
) -> Result<ParameterPrefix<'_>, CodecError> {
    prefix_from_stream(ValueStream::source_line(body, lengths))
}

fn source_point_prefix(
    body: View<'_>,
    lengths: PrimitiveLengths,
) -> Result<ParameterPrefix<'_>, CodecError> {
    prefix_from_stream(ValueStream::source_point(body, lengths))
}

fn prefix_from_stream(mut stream: ValueStream<'_>) -> Result<ParameterPrefix<'_>, CodecError> {
    let entity_type = integer_value(
        &one_value(&mut stream, "Parameter")?,
        "Parameter entity type",
    )?;
    let directory_pointer = pointer_value(
        &one_value(&mut stream, "Parameter Directory pointer")?,
        "Parameter Directory pointer",
    )?;
    let pointer_extent = stream.value_extent.clone();
    Ok(ParameterPrefix {
        stream,
        entity_type,
        directory_pointer,
        pointer_extent,
    })
}

fn finish_parameter_record(
    offset: u32,
    prefix: ParameterPrefix<'_>,
    directory_by_offset: &BTreeMap<u32, usize>,
) -> Result<BinaryParameter, CodecError> {
    let ParameterPrefix {
        mut stream,
        entity_type,
        directory_pointer,
        ..
    } = prefix;
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
    Ok(BinaryParameter {
        offset,
        entity_type,
        directory_pointer: i64::from(directory_pointer),
        values,
        admitted: None,
    })
}

fn parameter_owner<'a>(
    parameter: &BinaryParameter,
    directory: &'a [BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
) -> Result<&'a BinaryDirectory, CodecError> {
    let directory_pointer = positive_pointer(parameter.directory_pointer, "Parameter Directory")?;
    directory_by_offset
        .get(&directory_pointer)
        .and_then(|index| directory.get(*index))
        .ok_or_else(|| malformed("Binary Parameter Directory pointer does not resolve"))
}

/// The resolved Directory reference held at one Parameter value position.
fn parameter_reference(
    value: &BinaryValue,
    index: usize,
    point_owner: bool,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
) -> Result<Option<BinaryValue>, CodecError> {
    match value {
        BinaryValue::Pointer(_) => resolve_value(value, directory, directory_by_offset).map(Some),
        BinaryValue::Integer(pointer) if point_owner && index == 3 && *pointer != 0 => {
            resolve_value(
                &BinaryValue::Pointer(*pointer),
                directory,
                directory_by_offset,
            )
            .map(Some)
        }
        _ => Ok(None),
    }
}

/// Check that every Directory reference of one Parameter record resolves to
/// a surviving Directory record.
fn check_parameter_references(
    parameter: &BinaryParameter,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
) -> Result<(), CodecError> {
    let point_owner =
        parameter_owner(parameter, directory, directory_by_offset)?.has_point_parameters();
    for (index, value) in parameter.values.iter().enumerate() {
        parameter_reference(value, index, point_owner, directory, directory_by_offset)?;
    }
    Ok(())
}

fn parameter_lines(
    parameter: &mut BinaryParameter,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
) -> Result<Vec<Vec<u8>>, CodecError> {
    let point_owner =
        parameter_owner(parameter, directory, directory_by_offset)?.has_point_parameters();
    for (index, value) in parameter.values.iter_mut().enumerate() {
        if let Some(resolved) =
            parameter_reference(value, index, point_owner, directory, directory_by_offset)?
        {
            *value = resolved;
        }
    }
    let text = parameter_text(parameter.entity_type, &parameter.values)?;
    render_parameter_lines(&text, parameter.entity_type == 306)
}

fn normalize_directory_and_parameters(
    output: &mut Vec<u8>,
    directory: &[BinaryDirectory],
    parameters: Vec<BinaryParameter>,
    ctx: &DecodeContext<'_>,
) -> Result<(usize, usize, Admitted), CodecError> {
    let mut directory_by_offset = BTreeMap::new();
    for (index, record) in directory.iter().enumerate() {
        directory_by_offset.insert(record.offset, index);
    }
    let mut referenced_parameters = BTreeSet::new();
    let mut normalized = Vec::new();
    let mut parameter_sequence = 1_u32;
    for mut parameter in parameters {
        let directory_pointer =
            positive_pointer(parameter.directory_pointer, "Parameter Directory")?;
        let directory_index = *directory_by_offset
            .get(&directory_pointer)
            .ok_or_else(|| malformed("Binary Parameter Directory pointer does not resolve"))?;
        if directory[directory_index].entity_type() != parameter.entity_type {
            return Err(malformed(
                "Binary Directory and Parameter entity types disagree",
            ));
        }
        let directory_sequence = directory[directory_index].source_sequence;
        let (lines, admitted) = match parameter.admitted.take() {
            Some(admitted) => (admitted.lines, Some(admitted.content)),
            None => (
                parameter_lines(&mut parameter, directory, &directory_by_offset)?,
                None,
            ),
        };
        let first_sequence = parameter_sequence;
        parameter_sequence = parameter_sequence
            .checked_add(
                u32::try_from(lines.len())
                    .map_err(|_| malformed("normalized Parameter Data line count exceeds u32"))?,
            )
            .ok_or_else(|| malformed("normalized Parameter Data sequence overflows"))?;
        normalized.push(NormalizedParameter {
            offset: parameter.offset,
            directory_sequence,
            lines,
            first_sequence,
            admitted,
        });
    }
    let mut parameter_by_offset = BTreeMap::new();
    for (index, record) in normalized.iter().enumerate() {
        parameter_by_offset.insert(record.offset, index);
    }
    let mut parameter_starts = vec![0_u32; directory.len()];
    let mut parameter_counts = vec![0_usize; directory.len()];
    for (directory_index, directory_record) in directory.iter().enumerate() {
        let pointer = directory_record.parameter_pointer();
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
        parameter_starts[directory_index] = normalized[parameter_index].first_sequence;
        parameter_counts[directory_index] = normalized[parameter_index].lines.len();
    }
    if referenced_parameters.len() != normalized.len() {
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
        let render_source_field = |value: &BinaryValue, rendering| {
            if matches!(value, BinaryValue::Pointer(_)) {
                render_field(
                    &resolve_value(value, directory, &directory_by_offset)?,
                    rendering,
                )
            } else {
                render_field(value, rendering)
            }
        };
        let first = [
            render_source_field(
                &values[DirectoryFieldSlot::EntityType.slot()],
                FieldRendering::Plain,
            )?,
            render_field(
                &BinaryValue::Pointer(i64::from(parameter_starts[index])),
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::Structure.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::LineFont.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::Level.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::View.slot()],
                FieldRendering::Plain,
            )?,
            match &values[DirectoryFieldSlot::Transform.slot()] {
                BinaryValue::Integer(pointer) if *pointer != 0 => {
                    render_source_field(&BinaryValue::Pointer(*pointer), FieldRendering::Plain)?
                }
                value => render_source_field(value, FieldRendering::Plain)?,
            },
            render_source_field(
                &values[DirectoryFieldSlot::LabelDisplay.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::Status.slot()],
                FieldRendering::Status,
            )?,
        ];
        let second = [
            render_source_field(
                &values[DirectoryFieldSlot::EntityType.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::LineWeight.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::Color.slot()],
                FieldRendering::Plain,
            )?,
            render_field(
                &BinaryValue::Integer(i64::try_from(parameter_counts[index]).map_err(|_| {
                    malformed("normalized Parameter Data line count exceeds signed range")
                })?),
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::Form.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::ReservedFirst.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::ReservedSecond.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::Label.slot()],
                FieldRendering::Plain,
            )?,
            render_source_field(
                &values[DirectoryFieldSlot::Subscript.slot()],
                FieldRendering::Plain,
            )?,
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
    let mut admitted = Vec::new();
    let mut unadmitted = BTreeSet::new();
    let storage = normalized
        .len()
        .checked_mul(size_of::<crate::parameter::ParameterRecord>())
        .ok_or_else(|| malformed("Binary admitted Parameter storage overflows"))?;
    admitted.try_reserve_exact(normalized.len()).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            u64_from_index(storage),
            "IGES Binary admitted Parameter storage",
        )
    })?;
    for parameter in normalized {
        let first = parameter_sequence;
        for line in &parameter.lines {
            render_parameter_line(
                output,
                line,
                parameter.directory_sequence,
                parameter_sequence,
            )?;
            parameter_sequence = parameter_sequence
                .checked_add(1)
                .ok_or_else(|| malformed("normalized Parameter Data sequence overflows"))?;
        }
        match parameter.admitted {
            Some(content) => admitted.push(content.bind_range(first..parameter_sequence)),
            None => {
                unadmitted.insert(parameter.directory_sequence);
            }
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
        Admitted {
            records: admitted,
            unadmitted,
        },
    ))
}

/// The normalized Parameter records, admitted one by one.
struct Admitted {
    records: Vec<crate::parameter::ParameterRecord>,
    unadmitted: BTreeSet<u32>,
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
    let sequence_text = stack_text(format_args!("{sequence:>7}"))?;
    card[73..].copy_from_slice(sequence_text.as_bytes());
    append_output_card(output, &card);
    Ok(())
}

fn render_parameter_line(
    output: &mut Vec<u8>,
    data: &[u8],
    directory_sequence: u32,
    sequence: u32,
) -> Result<(), CodecError> {
    if data.len() > PARAMETER_DATA_WIDTH
        || directory_sequence == 0
        || directory_sequence > MAX_SEQUENCE
        || sequence == 0
        || sequence > MAX_SEQUENCE
    {
        return Err(malformed("normalized Parameter Data card is out of range"));
    }
    let mut card = [b' '; CARD_WIDTH];
    card[..data.len()].copy_from_slice(data);
    let directory_text = stack_text(format_args!("{directory_sequence:>8}"))?;
    card[64..72].copy_from_slice(directory_text.as_bytes());
    card[72] = b'P';
    let sequence_text = stack_text(format_args!("{sequence:>7}"))?;
    card[73..].copy_from_slice(sequence_text.as_bytes());
    append_output_card(output, &card);
    Ok(())
}

fn append_output_card(output: &mut Vec<u8>, card: &[u8; CARD_WIDTH]) {
    output.extend_from_slice(card);
    output.push(b'\n');
}

pub(crate) struct NormalizedBinary {
    pub(crate) bytes: Vec<u8>,
    pub(crate) identities: BinaryIdentities,
    pub(crate) admission: BinaryAdmission,
}

pub(crate) struct BinaryAdmission {
    pub(crate) global: crate::global::ResolvedGlobal,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// The admitted Parameter records.
    pub(crate) parameters: Vec<crate::parameter::ParameterRecord>,
    /// Directory sequences of the Parameter records that are not admitted:
    /// each is assembled from its normalized cards.
    pub(crate) unadmitted: BTreeSet<u32>,
}

pub(crate) struct BinaryIdentities(Vec<u32>);

impl BinaryIdentities {
    pub(crate) fn semantic_sequence(&self, grammar_sequence: u32) -> Result<u32, CodecError> {
        if grammar_sequence == 0 || grammar_sequence % 2 == 0 {
            return Err(malformed(
                "normalized Directory key is not an odd record position",
            ));
        }
        let index = usize::try_from((grammar_sequence - 1) / 2)
            .map_err(|_| malformed("normalized Directory index exceeds usize"))?;
        self.0
            .get(index)
            .copied()
            .ok_or_else(|| malformed("normalized Directory key has no Binary source identity"))
    }
}

fn source_sequence(ordinal: usize) -> Result<u32, CodecError> {
    u32::try_from(ordinal)
        .ok()
        .and_then(|value| value.checked_mul(2))
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| malformed("Binary Directory ordinal exceeds u32"))
}

fn resolve_value(
    value: &BinaryValue,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
) -> Result<BinaryValue, CodecError> {
    let BinaryValue::Pointer(pointer) = value else {
        return Ok(value.clone());
    };
    if *pointer == 0 {
        return Ok(value.clone());
    }
    let offset = u32::try_from(pointer.unsigned_abs())
        .map_err(|_| malformed("Binary Directory reference offset exceeds u32"))?;
    let target = directory_by_offset
        .get(&offset)
        .and_then(|index| directory.get(*index))
        .ok_or_else(|| malformed("Binary Directory reference has no surviving source target"))?;
    let sequence = i64::from(target.source_sequence);
    Ok(BinaryValue::Pointer(if *pointer < 0 {
        -sequence
    } else {
        sequence
    }))
}

/// Normalize one Binary IGES source into the Fixed ASCII image consumed by the
/// typed reader.
fn normalize_with_records<T>(
    source: View<'_>,
    ctx: &DecodeContext<'_>,
    parse: impl FnOnce(
        &BinarySections<'_>,
        &crate::global::ResolvedGlobal,
    ) -> Result<(Vec<BinaryDirectory>, Vec<BinaryParameter>), CodecError>,
    finish: impl FnOnce(Vec<u8>, Vec<BinaryDirectory>, BinaryAdmission) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let sections = parse_sections(source)?;
    let start_text = normalize_start(sections.start.window(), sections.lengths)?;
    let global_values = read_global(sections.global.window(), sections.lengths)?;
    let global_text = normalize_global(&global_values)?;
    let global_cards = crate::global::layout_global_cards(&global_text)?;
    let global_byte_len = global_cards
        .len()
        .checked_mul(CARD_DATA_WIDTH)
        .ok_or_else(|| malformed("Binary normalized Global payload size overflows"))?;
    let mut global_payload = ctx.alloc_filled(
        global_byte_len,
        b' ',
        "IGES Binary normalized Global payload",
    )?;
    for (card, payload) in global_cards
        .iter()
        .zip(global_payload.chunks_exact_mut(CARD_DATA_WIDTH))
    {
        payload
            .get_mut(..card.len())
            .ok_or_else(|| malformed("Binary normalized Global card exceeds its payload"))?
            .copy_from_slice(card);
    }
    let (global, losses) = crate::global::parse_stream(&global_payload)?;
    let (directory, parameters) = parse(&sections, &global)?;
    let mut output = Vec::new();
    let mut start_sequence = 1_u32;
    render_start_cards(&mut output, &start_text, &mut start_sequence)?;
    let start_count = usize::try_from(
        start_sequence
            .checked_sub(1)
            .ok_or_else(|| malformed("a Binary Start sequence is zero"))?,
    )
    .map_err(|_| malformed("a Binary Start count does not fit memory"))?;
    let mut global_sequence = 1_u32;
    for card in &global_cards {
        render_cards(&mut output, card, b'G', &mut global_sequence)?;
    }
    let global_count = usize::try_from(
        global_sequence
            .checked_sub(1)
            .ok_or_else(|| malformed("a Binary Global sequence is zero"))?,
    )
    .map_err(|_| malformed("a Binary Global count does not fit memory"))?;
    let (
        directory_count,
        parameter_count,
        Admitted {
            records: parameters,
            unadmitted,
        },
    ) = normalize_directory_and_parameters(&mut output, &directory, parameters, ctx)?;
    render_terminate(
        &mut output,
        start_count,
        global_count,
        directory_count,
        parameter_count,
    )?;
    finish(
        output,
        directory,
        BinaryAdmission {
            global,
            losses,
            parameters,
            unadmitted,
        },
    )
}

#[cfg(test)]
mod tests {
    mod normalized_global;
    mod source_values;
    #[test]
    fn parameter_card_refuses_an_unrenderable_directory_sequence() {
        let mut output = Vec::new();
        assert!(super::render_parameter_line(&mut output, b"116;", 100_000_000, 1).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn binary_text_helpers_render_fixed_ascii_fields() {
        let payload = primitive_string(b"ab", lengths());
        assert_eq!(
            super::normalize_start(&payload, lengths()).expect("valid test fixture"),
            b"ab"
        );
        let values: [super::BinaryValue; 24] = std::array::from_fn(|_| super::BinaryValue::Default);
        assert!(super::normalize_global(&values).is_ok());
        assert_eq!(
            super::parameter_text(116, &[super::BinaryValue::Integer(1)])
                .expect("valid test fixture"),
            b"116,1;"
        );
        let value = super::BinaryValue::String(b"ab".to_vec());
        assert_eq!(
            super::render_parameter_value(&value, false).expect("valid test fixture"),
            b"2Hab"
        );
        let mut output = Vec::new();
        super::render_parameter_line(&mut output, b"116;", 1, 1).expect("valid test fixture");
        assert_eq!(output.len(), 81);
    }

    #[test]
    fn binary_implicit_repetition_repeats_a_string_value() {
        let lengths = PrimitiveLengths {
            single_integer: 8,
            double_integer: 8,
            single_exponent: 8,
            single_fraction: 7,
            double_exponent: 8,
            double_fraction: 7,
        };
        let mut writer = BitWriter::default();
        writer.control_repeat(false, 2, 6);
        writer.string_part(b"ab", false, lengths);
        let bytes = writer.bytes();
        let mut stream = ValueStream::new(&bytes, lengths);
        for _ in 0..2 {
            assert_eq!(
                stream.next().expect("valid test fixture"),
                Some(BinaryValue::String(b"ab".to_vec()))
            );
        }
    }

    #[test]
    fn numerical_audit_binary_real_keeps_representable_exponent_boundary() {
        for negative in [0, 1] {
            let mut writer = BitWriter::default();
            writer.push_bits(negative, 1);
            writer.push_bits(3072, 12);
            writer.push_bits(0, 51);
            let value = super::BitReader::new(&writer.bytes)
                .read_real(12, 51)
                .expect("the written exponent and fraction are representable");
            assert_eq!(
                value.get(),
                if negative == 0 {
                    2.0_f64.powi(1023)
                } else {
                    -2.0_f64.powi(1023)
                }
            );
        }
        // Integer exponent subtraction also preserves low bits with a wide field.
        let mut writer = BitWriter::default();
        writer.push_bits(0, 1);
        writer.push_bits((1_u64 << 62) + 1, 63);
        assert_eq!(
            super::BitReader::new(&writer.bytes)
                .read_real(63, 0)
                .expect("the written integer exponent is representable")
                .get(),
            1.0
        );
    }

    #[test]
    fn binary_real_refuses_inexact_fraction_integer() {
        let mut writer = BitWriter::default();
        writer.push_bits(0, 1);
        writer.push_bits(2048, 12);
        writer.push_bits((1_u64 << 53) + 1, 54);
        assert!(super::BitReader::new(&writer.bytes)
            .read_real(12, 54)
            .is_err());
    }

    #[test]
    fn binary_real_admission_preserves_the_underflow_zero_sign() {
        for (negative, expected) in [(0, 0.0_f64), (1, -0.0_f64)] {
            let mut writer = BitWriter::default();
            writer.push_bits(negative, 1);
            writer.push_bits(1, 12);
            writer.push_bits(0, 51);
            let value = super::BitReader::new(&writer.bytes)
                .read_real(12, 51)
                .expect("the underflowed real is finite");
            assert_eq!(value.get().to_bits(), expected.to_bits());
        }
    }

    use super::{BinaryValue, PrimitiveLengths, ValueStream};
    use cadmpeg_core::decode::InspectOptions;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    const EPS_POINT: f64 = 1.0e-12;

    /// Every counted Directory record of `payload`, read without parts.
    pub(super) fn read_directory(
        payload: cadmpeg_core::decode::View<'_>,
        lengths: PrimitiveLengths,
    ) -> Result<Vec<super::BinaryDirectory>, cadmpeg_core::CodecError> {
        let mut records = Vec::new();
        let mut cursor = 0_usize;
        while cursor < payload.window().len() {
            let (offset, extent, body) = super::counted_record(payload, cursor, "Directory")?;
            let sequence = super::source_sequence(records.len())?;
            records.push(super::parse_directory_record(
                offset, sequence, body, lengths,
            )?);
            cursor = extent.end() - payload.start();
        }
        Ok(records)
    }

    /// One counted Parameter record, read without parts.
    pub(super) fn parse_parameter_record(
        offset: u32,
        body: cadmpeg_core::decode::View<'_>,
        lengths: PrimitiveLengths,
        directory_by_offset: &std::collections::BTreeMap<u32, usize>,
    ) -> Result<super::BinaryParameter, cadmpeg_core::CodecError> {
        super::finish_parameter_record(
            offset,
            super::parameter_prefix(body, lengths)?,
            directory_by_offset,
        )
    }

    fn normalize_for_test(source: &[u8]) -> Result<Vec<u8>, cadmpeg_core::CodecError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )?;
        normalize(root, &ctx)
    }

    /// Normalize through the decode path; the normalized text only.
    fn normalize(
        source: cadmpeg_core::decode::View<'_>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<u8>, cadmpeg_core::CodecError> {
        super::parts::normalize(source, ctx).map(|(normalized, _)| normalized.bytes)
    }

    #[derive(Debug, Default)]
    struct BitWriter {
        bytes: Vec<u8>,
        bit: u8,
    }

    impl BitWriter {
        fn push_bits(&mut self, value: u64, count: u8) {
            for index in (0..count).rev() {
                let bit = u8::try_from((value >> index) & 1).expect("test bit fits u8");
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
            let mut exponent = cadmpeg_core::convert::truncate_f64_to_i64(value.log2().floor())
                .expect("test exponent fits i64");
            let mut fraction =
                value / 2_f64.powi(i32::try_from(exponent).expect("test exponent fits i32"));
            if fraction >= 1.0 {
                exponent += 1;
                fraction /= 2.0;
            }
            let scale = 2_f64.powi(i32::from(fraction_bits) + 1);
            let raw_fraction =
                cadmpeg_core::convert::truncate_f64_to_u64(((fraction - 0.5) * scale).round())
                    .expect("test fraction fits u64");
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
            Some(BinaryValue::Real(value)) => assert!((value.get() + 3.5).abs() < f64::EPSILON),
            other => panic!("expected single real, got {other:?}"),
        }
        match stream.next().expect("double real") {
            Some(BinaryValue::Real(value)) => assert!((value.get() - 0.125).abs() < f64::EPSILON),
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
                .expect("IGES source metadata")
                .attributes["representation"],
            "binary"
        );
        assert_eq!(result.ir().model.points.len(), 1);
        let point = &result.ir().model.points[0].position().get();
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

    #[test]
    fn counted_binary_records_keep_the_actual_prefixed_source_and_no_framing_read_credit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let original = binary_point_file();
        let mut bytes = b"pre".to_vec();
        bytes.extend_from_slice(&original);
        bytes.extend_from_slice(b"post");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root.child(3, bytes.len() - 4).unwrap();
        let sections = super::parse_sections(source).unwrap();
        // The decode-path normalizer opens its own record scopes, so it runs
        // in a separate session over the same bytes.
        let normalized_arena = DecodeArena::new();
        let (normalized_ctx, normalized_root) =
            DecodeContext::from_root_bytes(&bytes, &normalized_arena, &DecodePolicy::service())
                .unwrap();
        assert_eq!(
            normalize(
                normalized_root.child(3, bytes.len() - 4).unwrap(),
                &normalized_ctx
            )
            .unwrap(),
            normalize_for_test(&original).unwrap()
        );
        let foreign_arena = DecodeArena::new();
        let (foreign, _) =
            DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service())
                .unwrap();
        for (name, payload) in [
            ("Directory", sections.directory),
            ("Parameter", sections.parameter),
        ] {
            let (offset, extent, body) = ctx
                .record(name, payload, || super::counted_record(payload, 0, name))
                .unwrap();
            assert_eq!(offset, 1);
            assert_eq!(extent.start(), payload.start());
            assert_eq!(extent.end(), payload.end());
            assert_eq!(body.start(), extent.start() + 4);
            assert_eq!(body.end(), extent.end());
            let count = u32::from_be_bytes(extent.window()[..4].try_into().unwrap());
            assert_eq!(usize::try_from(count).unwrap(), body.window().len());
            assert_eq!(extent.window(), &bytes[extent.start()..extent.end()]);
            assert_eq!(body.window(), &bytes[body.start()..body.end()]);
            let capture = ctx.capture_part_extent(extent).unwrap();
            assert_eq!(capture.stream(), "file");
            assert_eq!(capture.offset(), u64::try_from(extent.start()).unwrap());
            assert_eq!(
                capture.byte_len(),
                u64::try_from(extent.window().len()).unwrap()
            );
            assert!(foreign.capture_part_extent(extent).is_err());
            let cut = payload.child(payload.start(), payload.end() - 1).unwrap();
            let error = super::counted_record(cut, 0, name).unwrap_err();
            assert!(error.to_string().contains("entity exceeds its section"));
        }
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 2);
        assert!(captures.iter().all(|capture| capture.reads().is_empty()));
    }

    #[test]
    fn counted_binary_record_bounds_never_borrow_bytes_from_an_adjacent_source_record() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let original = binary_point_file();
        let arena = DecodeArena::new();
        let (_ctx, root) =
            DecodeContext::from_root_bytes(&original, &arena, &DecodePolicy::service()).unwrap();
        let sections = super::parse_sections(root).unwrap();
        for (name, payload) in [
            ("Directory", sections.directory),
            ("Parameter", sections.parameter),
        ] {
            let mut bytes = payload.window().to_vec();
            bytes.extend_from_slice(payload.window());
            let paired_arena = DecodeArena::new();
            let (_ctx, paired) =
                DecodeContext::from_root_bytes(&bytes, &paired_arena, &DecodePolicy::service())
                    .unwrap();
            let (first_offset, first, first_body) = super::counted_record(paired, 0, name).unwrap();
            let cursor = first.end() - paired.start();
            let (second_offset, second, second_body) =
                super::counted_record(paired, cursor, name).unwrap();
            assert_eq!(first_offset, 1);
            assert_eq!(second_offset, u32::try_from(cursor + 1).unwrap());
            assert_eq!(first.end(), second.start());
            assert_eq!(second.end(), paired.end());
            assert_eq!(first.window(), payload.window());
            assert_eq!(second.window(), payload.window());
            assert_eq!(first_body.window(), second_body.window());
            let cut = paired.child(paired.start(), second.end() - 1).unwrap();
            assert!(super::counted_record(cut, 0, name).is_ok());
            assert!(super::counted_record(cut, cursor, name).is_err());
            let short_count = paired.child(paired.start(), paired.start() + 3).unwrap();
            assert!(super::counted_record(short_count, 0, name)
                .unwrap_err()
                .to_string()
                .contains("u32 field is truncated"));
        }
    }
}
