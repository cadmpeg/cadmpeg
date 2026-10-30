// SPDX-License-Identifier: Apache-2.0
//! Value representations (spec §9.6, §9.7).
//!
//! A field's value is a 64-bit representation: flag bits, a type, and a
//! payload. The payload holds the value itself or the layer offset of its
//! bytes. [`Unpacker::unpack`] reads a representation into a [`Datum`],
//! whose scalar and array values use the value syntax of the text grammar.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::integers::{self, Width};
use super::sections::{absolute, Structure, Version};
use crate::text::syntax::{
    Dictionary, DictionaryEntry, DictionaryValue, ListOp, Number, Numbers, Packed, Reference, Value,
};

/// Representation bit: the value is an array.
const ARRAY_BIT: u64 = 1 << 63;
/// Representation bit: the payload holds the value.
const INLINED_BIT: u64 = 1 << 62;
/// Representation bit: the array is compressed.
const COMPRESSED_BIT: u64 = 1 << 61;
/// Representation bit: the value is an array edit.
const ARRAY_EDIT_BIT: u64 = 1 << 60;
/// Position of the type byte.
const TYPE_SHIFT: u32 = 48;
/// The payload bits.
const PAYLOAD_MASK: u64 = (1 << 48) - 1;
/// Representation bits 56 to 59, which no field uses.
const UNUSED_BITS: u64 = 0xF << 56;
/// The low 32 bits of a payload, which hold an inlined value.
const INLINE_MASK: u64 = 0xFFFF_FFFF;
/// Arrays shorter than this are stored uncompressed under the compressed bit.
const MIN_COMPRESSED_ARRAY: usize = 16;
/// Retained bytes charged for each materialized element.
const ELEMENT_CHARGE: u64 = 8;
/// The legacy `config` variability, read as `uniform`.
const LEGACY_CONFIG_VARIABILITY: u32 = 2;

const RANK_REMOVED: Version = [0, 5, 0];
const COMPRESSED_INTEGERS: Version = [0, 5, 0];
const COMPRESSED_REALS: Version = [0, 6, 0];
const WIDE_ARRAY_SIZE: Version = [0, 7, 0];
const PAYLOAD_OFFSET: Version = [0, 8, 0];

/// List-operation header bits (spec §9.7).
const LIST_EXPLICIT: u8 = 1;
const LIST_EXPLICIT_ITEMS: u8 = 1 << 1;
const LIST_ADDED: u8 = 1 << 2;
const LIST_DELETED: u8 = 1 << 3;
const LIST_ORDERED: u8 = 1 << 4;
const LIST_PREPENDED: u8 = 1 << 5;
const LIST_APPENDED: u8 = 1 << 6;

/// The binary form of one value component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Component {
    Bool,
    U8,
    I32,
    U32,
    I64,
    U64,
    F16,
    F32,
    F64,
    /// A string index; [`Value::String`].
    String,
    /// A token index; [`Value::String`].
    Token,
    /// An asset path: a token index when inlined, a string index in an
    /// array.
    Asset,
}

impl Component {
    pub(super) const fn size(self) -> usize {
        match self {
            Self::Bool | Self::U8 => 1,
            Self::F16 => 2,
            Self::I32 | Self::U32 | Self::F32 | Self::String | Self::Token | Self::Asset => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
        }
    }

    const fn width(self) -> Width {
        match self {
            Self::I64 | Self::U64 => Width::Bits64,
            _ => Width::Bits32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    Scalar,
    Vector(usize),
    /// Stored `(i, j, k, real)`; written `(real, i, j, k)`.
    Quaternion,
    Matrix(usize),
}

impl Shape {
    pub(super) const fn components(self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Vector(count) => count,
            Self::Quaternion => 4,
            Self::Matrix(rows) => rows * rows,
        }
    }
}

/// A value type of the type table that has the value syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Kind {
    pub(super) name: &'static str,
    pub(super) component: Component,
    pub(super) shape: Shape,
}

const fn kind(name: &'static str, component: Component, shape: Shape) -> Kind {
    Kind {
        name,
        component,
        shape,
    }
}

/// The value type of type number `ty`, when it has the value syntax.
pub(super) fn value_kind(ty: u8) -> Option<Kind> {
    use Component as C;
    use Shape::{Matrix, Quaternion, Scalar, Vector};
    Some(match ty {
        1 => kind("bool", C::Bool, Scalar),
        2 => kind("uchar", C::U8, Scalar),
        3 => kind("int", C::I32, Scalar),
        4 => kind("uint", C::U32, Scalar),
        5 => kind("int64", C::I64, Scalar),
        6 => kind("uint64", C::U64, Scalar),
        7 => kind("half", C::F16, Scalar),
        8 => kind("float", C::F32, Scalar),
        9 => kind("double", C::F64, Scalar),
        10 => kind("string", C::String, Scalar),
        11 => kind("token", C::Token, Scalar),
        12 => kind("asset", C::Asset, Scalar),
        13 => kind("matrix2d", C::F64, Matrix(2)),
        14 => kind("matrix3d", C::F64, Matrix(3)),
        15 => kind("matrix4d", C::F64, Matrix(4)),
        16 => kind("quatd", C::F64, Quaternion),
        17 => kind("quatf", C::F32, Quaternion),
        18 => kind("quath", C::F16, Quaternion),
        19 => kind("double2", C::F64, Vector(2)),
        20 => kind("float2", C::F32, Vector(2)),
        21 => kind("half2", C::F16, Vector(2)),
        22 => kind("int2", C::I32, Vector(2)),
        23 => kind("double3", C::F64, Vector(3)),
        24 => kind("float3", C::F32, Vector(3)),
        25 => kind("half3", C::F16, Vector(3)),
        26 => kind("int3", C::I32, Vector(3)),
        27 => kind("double4", C::F64, Vector(4)),
        28 => kind("float4", C::F32, Vector(4)),
        29 => kind("half4", C::F16, Vector(4)),
        30 => kind("int4", C::I32, Vector(4)),
        56 => kind("timecode", C::F64, Scalar),
        57 => kind("pathExpression", C::String, Scalar),
        _ => return None,
    })
}

/// The type number of crate value type `name` (spec §9.6).
pub(super) fn type_number(name: &str) -> Option<u8> {
    (1..=u8::MAX).find(|ty| value_kind(*ty).is_some_and(|kind| kind.name == name))
}

/// A list operation: the explicit items, or the items of each operation.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ListOpValue<T> {
    /// `None` is the explicit list; the rest are in the order the text
    /// form states them.
    pub(super) parts: Vec<(Option<ListOp>, Vec<T>)>,
}

/// One item of a list operation.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Item {
    Value(Value),
    Path(String),
    Reference(Reference),
}

/// A decoded field value.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Datum {
    /// A value of a value type, as the value syntax.
    Value {
        type_name: &'static str,
        array: bool,
        value: Value,
    },
    Dictionary(Dictionary),
    ListOp(ListOpValue<Item>),
    Paths(Vec<String>),
    Tokens(Vec<String>),
    Strings(Vec<String>),
    Doubles(Vec<f64>),
    /// Layer offsets: offset and scale.
    LayerOffsets(Vec<(f64, f64)>),
    Specifier(u32),
    Permission(u32),
    Variability(u32),
    Selections(Vec<(String, String)>),
    TimeSamples(Vec<(f64, Datum)>),
    Payload(Reference),
    Relocates(Vec<(String, String)>),
    /// `None`: a value block.
    Block,
    AnimationBlock,
    /// A value this reader does not decode, named for the loss note.
    Unread(&'static str),
}

fn malformed(what: &str) -> CodecError {
    CodecError::malformed(format_args!("corrupt USDC value: {what}"))
}

/// A number with no exact integer value.
pub(super) const fn real(float: f64) -> Value {
    Value::Number(Number {
        float,
        integer: None,
    })
}

fn integer(value: i128) -> Value {
    let float = i64::try_from(value)
        .ok()
        .and_then(cadmpeg_core::convert::f64_from_i64)
        .or_else(|| {
            u64::try_from(value)
                .ok()
                .and_then(cadmpeg_core::convert::f64_from_u64)
        })
        .unwrap_or(f64::NAN);
    Value::Number(Number {
        float,
        integer: Some(value),
    })
}

/// The binary16 value `bits` as binary64.
fn half(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = (bits >> 10) & 0x1F;
    let mantissa = f64::from(bits & 0x3FF);
    let magnitude = match exponent {
        0 => mantissa * 2.0_f64.powi(-24),
        0x1F if mantissa == 0.0 => f64::INFINITY,
        0x1F => f64::NAN,
        _ => (mantissa + 1024.0) * 2.0_f64.powi(i32::from(exponent) - 25),
    };
    sign * magnitude
}

fn bool_value(value: bool) -> Value {
    Value::Identifier(if value { "true" } else { "false" }.to_owned())
}

/// Builds one shaped value from its components in stored order.
fn shaped(shape: Shape, mut components: Vec<Value>) -> Value {
    match shape {
        Shape::Scalar => components.pop().unwrap_or(Value::List(Vec::new())),
        Shape::Vector(_) => Value::Tuple(components),
        Shape::Quaternion => {
            components.rotate_right(1);
            Value::Tuple(components)
        }
        Shape::Matrix(rows) => {
            let mut out = Vec::new();
            let mut rest = components.into_iter();
            for _ in 0..rows {
                out.push(Value::Tuple(rest.by_ref().take(rows).collect()));
            }
            Value::Tuple(out)
        }
    }
}

/// Reads representations of one layer.
pub(super) struct Unpacker<'s, 'a> {
    ctx: &'s DecodeContext<'a>,
    structure: &'s Structure<'a>,
    paths: &'s [Option<String>],
    /// The kinds of value that were not read.
    pub(super) unread: BTreeSet<&'static str>,
}

impl<'s, 'a> Unpacker<'s, 'a> {
    pub(super) fn new(
        ctx: &'s DecodeContext<'a>,
        structure: &'s Structure<'a>,
        paths: &'s [Option<String>],
    ) -> Self {
        Self {
            ctx,
            structure,
            paths,
            unread: BTreeSet::new(),
        }
    }

    fn version(&self) -> Version {
        self.structure.version
    }

    pub(super) fn token(&self, index: u64) -> Result<String, CodecError> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.structure.tokens.get(index))
            .cloned()
            .ok_or_else(|| malformed("a token index exceeds the token table"))
    }

    fn string(&self, index: u64) -> Result<String, CodecError> {
        let token = usize::try_from(index)
            .ok()
            .and_then(|index| self.structure.strings.get(index))
            .copied()
            .ok_or_else(|| malformed("a string index exceeds the string table"))?;
        self.token(cadmpeg_core::decode::u64_from_index(token))
    }

    /// The text of path `index`; an index the path tree does not place is
    /// the empty path.
    fn path(&self, index: u64) -> Result<String, CodecError> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.paths.get(index))
            .map(|path| path.clone().unwrap_or_default())
            .ok_or_else(|| malformed("a path index exceeds the path table"))
    }

    /// A view at the layer offset `offset`.
    fn at(&self, offset: u64) -> Result<View<'a>, CodecError> {
        let file = self.structure.file;
        let offset =
            usize::try_from(offset).map_err(|_| malformed("a value offset is out of range"))?;
        let mut view = file;
        view.seek(absolute(file, offset)?)
            .ok_or_else(|| malformed("a value offset is past the end"))?;
        Ok(view)
    }

    fn charge(&self, elements: usize) -> Result<(), CodecError> {
        let bytes = cadmpeg_core::decode::u64_from_index(elements)
            .checked_mul(ELEMENT_CHARGE)
            .ok_or_else(|| malformed("a value size overflows"))?;
        self.ctx.retain_bytes(bytes, "usdc_values")
    }

    /// Reads a `u64` count of elements of at least `size` bytes each.
    fn count(&self, view: &mut View<'_>, size: usize) -> Result<usize, CodecError> {
        let value = view.req_u64_le()?;
        let count = view
            .counted(value, size)
            .map(cadmpeg_core::decode::BoundedCount::get)
            .ok_or_else(|| malformed("a count exceeds the layer"))?;
        self.charge(count)?;
        Ok(count)
    }

    fn component(&self, view: &mut View<'_>, component: Component) -> Result<Value, CodecError> {
        Ok(match component {
            Component::Bool => bool_value(view.req_u8()? != 0),
            Component::U8 => integer(i128::from(view.req_u8()?)),
            Component::I32 => integer(i128::from(view.req_i32_le()?)),
            Component::U32 => integer(i128::from(view.req_u32_le()?)),
            Component::I64 => integer(i128::from(view.req_i64_le()?)),
            Component::U64 => integer(i128::from(view.req_u64_le()?)),
            Component::F16 => real(half(view.req_u16_le()?)),
            Component::F32 => real(f64::from(view.req_f32_le()?)),
            Component::F64 => real(view.req_f64_le()?),
            Component::String => Value::String(self.string(u64::from(view.req_u32_le()?))?),
            Component::Token => Value::String(self.token(u64::from(view.req_u32_le()?))?),
            Component::Asset => Value::Asset(self.string(u64::from(view.req_u32_le()?))?),
        })
    }

    /// One element of `kind` at the view's cursor.
    fn element(&self, view: &mut View<'_>, kind: Kind) -> Result<Value, CodecError> {
        let mut components = Vec::with_capacity(kind.shape.components());
        for _ in 0..kind.shape.components() {
            components.push(self.component(view, kind.component)?);
        }
        Ok(shaped(kind.shape, components))
    }

    /// A component held in the low 32 bits of a payload.
    fn inline_component(&self, bits: u32, component: Component) -> Result<Value, CodecError> {
        let bytes = bits.to_le_bytes();
        Ok(match component {
            Component::Bool => bool_value(bytes[0] != 0),
            Component::U8 => integer(i128::from(bytes[0])),
            Component::I32 => integer(i128::from(bits.cast_signed())),
            Component::U32 => integer(i128::from(bits)),
            Component::I64 => integer(i128::from(bits.cast_signed())),
            Component::U64 => integer(i128::from(bits)),
            Component::F16 => real(half(
                u16::try_from(bits & 0xFFFF).map_err(|_| malformed("an inline half"))?,
            )),
            Component::F32 | Component::F64 => real(f64::from(f32::from_bits(bits))),
            Component::String => Value::String(self.string(u64::from(bits))?),
            Component::Token => Value::String(self.token(u64::from(bits))?),
            Component::Asset => Value::Asset(self.token(u64::from(bits))?),
        })
    }

    /// A scalar of `kind`, inlined or at its offset.
    fn scalar(&self, rep: u64, kind: Kind) -> Result<Value, CodecError> {
        let payload = rep & PAYLOAD_MASK;
        let low = u32::try_from(payload & INLINE_MASK).map_err(|_| malformed("an inline value"))?;
        let always_inlined = kind.shape == Shape::Scalar
            && (kind.component.size() <= 4 || kind.component == Component::Asset);
        if always_inlined {
            return self.inline_component(low, kind.component);
        }
        if rep & INLINED_BIT != 0 {
            let small = low.to_le_bytes().map(u8::cast_signed);
            let small_value = |value: i8| match kind.component {
                Component::I32 | Component::I64 => integer(i128::from(value)),
                _ => real(f64::from(value)),
            };
            return Ok(match kind.shape {
                Shape::Scalar => self.inline_component(low, kind.component)?,
                Shape::Vector(count) => Value::Tuple(
                    small
                        .iter()
                        .take(count)
                        .map(|value| small_value(*value))
                        .collect(),
                ),
                Shape::Matrix(rows) => {
                    let mut components = Vec::with_capacity(rows * rows);
                    for row in 0..rows {
                        for column in 0..rows {
                            components.push(if row == column {
                                small
                                    .get(row)
                                    .map_or(real(1.0), |value| small_value(*value))
                            } else {
                                real(0.0)
                            });
                        }
                    }
                    shaped(kind.shape, components)
                }
                Shape::Quaternion => return Err(malformed("a quaternion is inlined")),
            });
        }
        let mut view = self.at(payload)?;
        self.element(&mut view, kind)
    }

    /// The element count of an array.
    fn array_size(&self, view: &mut View<'_>) -> Result<u64, CodecError> {
        Ok(if self.version() < WIDE_ARRAY_SIZE {
            u64::from(view.req_u32_le()?)
        } else {
            view.req_u64_le()?
        })
    }

    fn raw_elements(
        &self,
        view: &mut View<'_>,
        count: u64,
        kind: Kind,
    ) -> Result<Vec<Value>, CodecError> {
        let size = kind
            .component
            .size()
            .checked_mul(kind.shape.components())
            .ok_or_else(|| malformed("an element size overflows"))?;
        let count = view
            .counted(count, size)
            .map(cadmpeg_core::decode::BoundedCount::get)
            .ok_or_else(|| malformed("an array exceeds the layer"))?;
        self.charge(count)?;
        let mut out = self.ctx.alloc_filled(count, real(0.0), "usdc_array")?;
        for slot in &mut out {
            *slot = self.element(view, kind)?;
        }
        Ok(out)
    }

    /// `count` elements of numeric `kind` at the view's cursor, held flat.
    fn raw_numbers(
        &self,
        view: &mut View<'_>,
        count: u64,
        kind: Kind,
    ) -> Result<Packed, CodecError> {
        let width = kind.shape.components();
        let size = kind
            .component
            .size()
            .checked_mul(width)
            .ok_or_else(|| malformed("an element size overflows"))?;
        let count = view
            .counted(count, size)
            .map(cadmpeg_core::decode::BoundedCount::get)
            .ok_or_else(|| malformed("an array exceeds the layer"))?;
        self.charge(count)?;
        let total = count
            .checked_mul(width)
            .ok_or_else(|| malformed("an array size overflows"))?;
        let mut numbers = if matches!(
            kind.component,
            Component::F16 | Component::F32 | Component::F64
        ) {
            let mut out = self.ctx.alloc_filled(total, 0.0_f64, "usdc_array")?;
            for slot in &mut out {
                *slot = match kind.component {
                    Component::F16 => half(view.req_u16_le()?),
                    Component::F32 => f64::from(view.req_f32_le()?),
                    _ => view.req_f64_le()?,
                };
            }
            Numbers::Reals(out)
        } else {
            let mut out = self.ctx.alloc_filled(total, 0_i128, "usdc_array")?;
            for slot in &mut out {
                *slot = match kind.component {
                    Component::U8 => i128::from(view.req_u8()?),
                    Component::I32 => i128::from(view.req_i32_le()?),
                    Component::U32 => i128::from(view.req_u32_le()?),
                    Component::I64 => i128::from(view.req_i64_le()?),
                    _ => i128::from(view.req_u64_le()?),
                };
            }
            Numbers::Integers(out)
        };
        // A quaternion is stored imaginary part first.
        if kind.shape == Shape::Quaternion {
            match &mut numbers {
                Numbers::Reals(values) => {
                    values.chunks_mut(4).for_each(|chunk| chunk.rotate_right(1));
                }
                Numbers::Integers(values) => {
                    values.chunks_mut(4).for_each(|chunk| chunk.rotate_right(1));
                }
                Numbers::Literals { .. } => {}
            }
        }
        Ok(Packed { numbers, width })
    }

    /// `count` compressed integers of `component`.
    fn compressed_integers(
        &self,
        view: &mut View<'a>,
        count: usize,
        component: Component,
    ) -> Result<Vec<i128>, CodecError> {
        self.charge(count)?;
        let values = integers::read(self.ctx, view, count, component.width())?;
        Ok(values
            .into_iter()
            .map(|value| match component {
                Component::U32 => i128::from(value).rem_euclid(1 << 32),
                Component::U64 => i128::from(value).rem_euclid(1 << 64),
                _ => i128::from(value),
            })
            .collect())
    }

    fn array(&self, rep: u64, kind: Kind) -> Result<Value, CodecError> {
        let payload = rep & PAYLOAD_MASK;
        if payload == 0 {
            return Ok(Value::List(Vec::new()));
        }
        let numeric = !matches!(
            kind.component,
            Component::Bool | Component::String | Component::Token | Component::Asset
        );
        let mut view = self.at(payload)?;
        if self.version() < RANK_REMOVED {
            view.req_u32_le()?;
        }
        let count = self.array_size(&mut view)?;
        let compressed = rep & COMPRESSED_BIT != 0;
        let integers = matches!(
            kind.component,
            Component::I32 | Component::U32 | Component::I64 | Component::U64
        ) && kind.shape == Shape::Scalar;
        let reals = matches!(
            kind.component,
            Component::F16 | Component::F32 | Component::F64
        ) && kind.shape == Shape::Scalar;
        let compressed_integers = compressed && integers && self.version() >= COMPRESSED_INTEGERS;
        let compressed_reals = compressed && reals && self.version() >= COMPRESSED_REALS;
        if !(compressed_integers || compressed_reals)
            || usize::try_from(count).is_ok_and(|count| count < MIN_COMPRESSED_ARRAY)
        {
            return if numeric {
                self.raw_numbers(&mut view, count, kind).map(Value::Packed)
            } else {
                self.raw_elements(&mut view, count, kind).map(Value::List)
            };
        }
        // A compressed sequence codes each element in at least two bits,
        // which the LZ4 framing expands at most 255 times per input byte.
        let count = usize::try_from(count)
            .ok()
            .filter(|count| {
                view.remaining()
                    .checked_mul(255)
                    .is_none_or(|reach| *count / 4 <= reach)
            })
            .ok_or_else(|| malformed("a compressed array exceeds the layer"))?;
        let packed = |numbers: Numbers| Value::Packed(Packed { numbers, width: 1 });
        if compressed_integers {
            return Ok(packed(Numbers::Integers(self.compressed_integers(
                &mut view,
                count,
                kind.component,
            )?)));
        }
        match view.req_u8()? {
            b'i' => self
                .compressed_integers(&mut view, count, Component::I32)?
                .into_iter()
                .map(|value| {
                    i64::try_from(value)
                        .ok()
                        .and_then(cadmpeg_core::convert::f64_from_i64)
                        .ok_or_else(|| malformed("a compressed real exceeds binary64"))
                })
                .collect::<Result<Vec<f64>, _>>()
                .map(|values| packed(Numbers::Reals(values))),
            b't' => {
                let table_size = view.req_u32_le()?;
                let table = match self
                    .raw_numbers(&mut view, u64::from(table_size), kind)?
                    .numbers
                {
                    Numbers::Reals(values) => values,
                    Numbers::Integers(_) | Numbers::Literals { .. } => {
                        return Err(malformed("a real table holds integers"))
                    }
                };
                self.charge(count)?;
                let indices = integers::read(self.ctx, &mut view, count, Width::Bits32)?;
                indices
                    .into_iter()
                    .map(|index| {
                        usize::try_from(index)
                            .ok()
                            .and_then(|index| table.get(index))
                            .copied()
                            .ok_or_else(|| malformed("a table index exceeds the table"))
                    })
                    .collect::<Result<Vec<f64>, _>>()
                    .map(|values| packed(Numbers::Reals(values)))
            }
            _ => Err(malformed("a compressed real array has no known code")),
        }
    }

    fn u32_vector(&self, view: &mut View<'_>) -> Result<Vec<u32>, CodecError> {
        let count = self.count(view, 4)?;
        let mut out = self.ctx.alloc_filled(count, 0_u32, "usdc_vector")?;
        for slot in &mut out {
            *slot = view.req_u32_le()?;
        }
        Ok(out)
    }

    fn tokens(&self, view: &mut View<'_>) -> Result<Vec<String>, CodecError> {
        self.u32_vector(view)?
            .into_iter()
            .map(|index| self.token(u64::from(index)))
            .collect()
    }

    fn strings(&self, view: &mut View<'_>) -> Result<Vec<String>, CodecError> {
        self.u32_vector(view)?
            .into_iter()
            .map(|index| self.string(u64::from(index)))
            .collect()
    }

    fn paths(&self, view: &mut View<'_>) -> Result<Vec<String>, CodecError> {
        self.u32_vector(view)?
            .into_iter()
            .map(|index| self.path(u64::from(index)))
            .collect()
    }

    fn layer_offset(view: &mut View<'_>) -> Result<(f64, f64), CodecError> {
        Ok((view.req_f64_le()?, view.req_f64_le()?))
    }

    /// A dictionary: a count, then keys and values. Each value is a
    /// relative offset to a representation.
    fn dictionary(&mut self, view: &mut View<'a>) -> Result<Dictionary, CodecError> {
        let count = self.count(view, 16)?;
        let mut entries = Vec::new();
        for _ in 0..count {
            let key = self.string(u64::from(view.req_u32_le()?))?;
            let datum = self.boxed_value(view)?;
            match dictionary_value(datum) {
                Ok(value) => entries.push(DictionaryEntry { key, value }),
                Err(what) => {
                    self.unread.insert(what);
                }
            }
        }
        Ok(Dictionary { entries })
    }

    /// A value stated as a relative offset to its representation.
    fn boxed_value(&mut self, view: &mut View<'a>) -> Result<Datum, CodecError> {
        let start = view.position();
        let offset = view.req_i64_le()?;
        let target = i64::try_from(start)
            .ok()
            .and_then(|start| start.checked_add(offset))
            .and_then(|target| usize::try_from(target).ok())
            .ok_or_else(|| malformed("a value offset is out of range"))?;
        view.seek(target)
            .ok_or_else(|| malformed("a value offset is past the end"))?;
        let rep = view.req_u64_le()?;
        self.unpack(rep)
    }

    fn reference(&mut self, view: &mut View<'a>, payload: bool) -> Result<Reference, CodecError> {
        let asset = self.string(u64::from(view.req_u32_le()?))?;
        let prim_path = self.path(u64::from(view.req_u32_le()?))?;
        let (offset, scale) = if !payload || self.version() >= PAYLOAD_OFFSET {
            Self::layer_offset(view)?
        } else {
            (0.0, 1.0)
        };
        let custom_data = if payload {
            None
        } else {
            Some(self.dictionary(view)?).filter(|dictionary| !dictionary.entries.is_empty())
        };
        Ok(Reference {
            asset: (!asset.is_empty()).then_some(asset),
            prim_path: (!prim_path.is_empty()).then_some(prim_path),
            offset: (offset != 0.0).then_some(Number {
                float: offset,
                integer: None,
            }),
            scale: (scale != 1.0).then_some(Number {
                float: scale,
                integer: None,
            }),
            custom_data,
        })
    }

    fn list_item(&mut self, view: &mut View<'a>, ty: u8) -> Result<Item, CodecError> {
        Ok(match ty {
            32 => Item::Value(Value::String(self.token(u64::from(view.req_u32_le()?))?)),
            33 => Item::Value(Value::String(self.string(u64::from(view.req_u32_le()?))?)),
            34 => Item::Path(self.path(u64::from(view.req_u32_le()?))?),
            35 => Item::Reference(self.reference(view, false)?),
            55 => Item::Reference(self.reference(view, true)?),
            36 => Item::Value(integer(i128::from(view.req_i32_le()?))),
            37 => Item::Value(integer(i128::from(view.req_i64_le()?))),
            38 => Item::Value(integer(i128::from(view.req_u32_le()?))),
            _ => Item::Value(integer(i128::from(view.req_u64_le()?))),
        })
    }

    fn list_op(&mut self, view: &mut View<'a>, ty: u8) -> Result<ListOpValue<Item>, CodecError> {
        let header = view.req_u8()?;
        let read = |this: &mut Self, view: &mut View<'a>| -> Result<Vec<Item>, CodecError> {
            let count = this.count(view, 4)?;
            let mut items = Vec::new();
            for _ in 0..count {
                items.push(this.list_item(view, ty)?);
            }
            Ok(items)
        };
        let mut explicit = None;
        let mut added = None;
        let mut prepended = None;
        let mut appended = None;
        let mut deleted = None;
        let mut ordered = None;
        if header & LIST_EXPLICIT_ITEMS != 0 {
            explicit = Some(read(self, view)?);
        }
        if header & LIST_ADDED != 0 {
            added = Some(read(self, view)?);
        }
        if header & LIST_PREPENDED != 0 {
            prepended = Some(read(self, view)?);
        }
        if header & LIST_APPENDED != 0 {
            appended = Some(read(self, view)?);
        }
        if header & LIST_DELETED != 0 {
            deleted = Some(read(self, view)?);
        }
        if header & LIST_ORDERED != 0 {
            ordered = Some(read(self, view)?);
        }
        let mut parts = Vec::new();
        if header & LIST_EXPLICIT != 0 {
            parts.push((None, explicit.unwrap_or_default()));
            return Ok(ListOpValue { parts });
        }
        for (op, items) in [
            (ListOp::Delete, deleted),
            (ListOp::Add, added),
            (ListOp::Prepend, prepended),
            (ListOp::Append, appended),
            (ListOp::Reorder, ordered),
        ] {
            if let Some(items) = items {
                parts.push((Some(op), items));
            }
        }
        Ok(ListOpValue { parts })
    }

    fn time_samples(&mut self, view: &mut View<'a>) -> Result<Datum, CodecError> {
        let times = match self.boxed_value(view)? {
            Datum::Doubles(times) => times,
            Datum::Value {
                value: Value::List(values),
                ..
            } => values
                .into_iter()
                .map(|value| match value {
                    Value::Number(number) => Ok(number.float),
                    _ => Err(malformed("a sample time is not a number")),
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(malformed("the sample times are not doubles")),
        };
        let start = view.position();
        let offset = view.req_i64_le()?;
        let target = i64::try_from(start)
            .ok()
            .and_then(|start| start.checked_add(offset))
            .and_then(|target| usize::try_from(target).ok())
            .ok_or_else(|| malformed("a sample offset is out of range"))?;
        view.seek(target)
            .ok_or_else(|| malformed("a sample offset is past the end"))?;
        let count = self.count(view, 8)?;
        if count != times.len() {
            return Err(malformed("the sample times and values differ in count"));
        }
        let mut samples = Vec::new();
        for time in times {
            let rep = view.req_u64_le()?;
            samples.push((time, self.unpack(rep)?));
        }
        Ok(Datum::TimeSamples(samples))
    }

    /// Reads the value representation `rep`.
    pub(super) fn unpack(&mut self, rep: u64) -> Result<Datum, CodecError> {
        let _depth = self.ctx.enter_nested("usdc_value_nesting")?;
        if rep & UNUSED_BITS != 0 {
            return Err(malformed("a value representation sets bits 56 to 59"));
        }
        let ty = u8::try_from((rep >> TYPE_SHIFT) & 0xFF).map_err(|_| malformed("a type"))?;
        let payload = rep & PAYLOAD_MASK;
        let low = u32::try_from(payload & INLINE_MASK).map_err(|_| malformed("a payload"))?;
        if rep & ARRAY_EDIT_BIT != 0 {
            return Ok(Datum::Unread("array edit"));
        }
        if let Some(kind) = value_kind(ty) {
            let array = rep & ARRAY_BIT != 0;
            let value = if array {
                self.array(rep, kind)?
            } else {
                self.scalar(rep, kind)?
            };
            return Ok(Datum::Value {
                type_name: kind.name,
                array,
                value,
            });
        }
        let inlined = rep & INLINED_BIT != 0;
        Ok(match ty {
            31 if inlined => Datum::Dictionary(Dictionary::default()),
            31 => Datum::Dictionary(self.dictionary(&mut self.at(payload)?)?),
            32..=39 | 55 => Datum::ListOp(self.list_op(&mut self.at(payload)?, ty)?),
            40 => Datum::Paths(self.paths(&mut self.at(payload)?)?),
            41 => Datum::Tokens(self.tokens(&mut self.at(payload)?)?),
            42 => Datum::Specifier(low),
            43 => Datum::Permission(low),
            44 if low == LEGACY_CONFIG_VARIABILITY => Datum::Variability(1),
            44 => Datum::Variability(low),
            45 => {
                let mut view = self.at(payload)?;
                let count = self.count(&mut view, 8)?;
                let mut pairs = Vec::new();
                for _ in 0..count {
                    let set = self.string(u64::from(view.req_u32_le()?))?;
                    let variant = self.string(u64::from(view.req_u32_le()?))?;
                    pairs.push((set, variant));
                }
                Datum::Selections(pairs)
            }
            46 => self.time_samples(&mut self.at(payload)?)?,
            47 => Datum::Payload(self.reference(&mut self.at(payload)?, true)?),
            48 => {
                let mut view = self.at(payload)?;
                let count = self.count(&mut view, 8)?;
                let mut out = self.ctx.alloc_filled(count, 0.0, "usdc_vector")?;
                for slot in &mut out {
                    *slot = view.req_f64_le()?;
                }
                Datum::Doubles(out)
            }
            49 => {
                let mut view = self.at(payload)?;
                let count = self.count(&mut view, 16)?;
                let mut out = self.ctx.alloc_filled(count, (0.0, 1.0), "usdc_vector")?;
                for slot in &mut out {
                    *slot = Self::layer_offset(&mut view)?;
                }
                Datum::LayerOffsets(out)
            }
            50 => Datum::Strings(self.strings(&mut self.at(payload)?)?),
            51 => Datum::Block,
            52 | 53 => self.boxed_value(&mut self.at(payload)?)?,
            58 => {
                let mut view = self.at(payload)?;
                let count = self.count(&mut view, 8)?;
                let mut pairs = Vec::new();
                for _ in 0..count {
                    let source = self.path(u64::from(view.req_u32_le()?))?;
                    let target = self.path(u64::from(view.req_u32_le()?))?;
                    pairs.push((source, target));
                }
                Datum::Relocates(pairs)
            }
            60 => Datum::AnimationBlock,
            54 => Datum::Unread("unregistered list operation"),
            59 => Datum::Unread("spline"),
            61 => Datum::Unread("duration"),
            _ => Datum::Unread("unknown value type"),
        })
    }
}

/// `datum` as a dictionary entry value, or the kind of value that has no
/// entry form.
pub(super) fn dictionary_value(datum: Datum) -> Result<DictionaryValue, &'static str> {
    let list = |type_name: &str, values: Vec<Value>| DictionaryValue::Typed {
        type_name: type_name.to_owned(),
        array: true,
        value: Value::List(values),
    };
    Ok(match datum {
        Datum::Value {
            type_name,
            array,
            value,
        } => DictionaryValue::Typed {
            type_name: type_name.to_owned(),
            array,
            value,
        },
        Datum::Dictionary(dictionary) => DictionaryValue::Dictionary(dictionary),
        Datum::Tokens(values) => list("token", values.into_iter().map(Value::String).collect()),
        Datum::Strings(values) => list("string", values.into_iter().map(Value::String).collect()),
        Datum::Doubles(values) => list("double", values.into_iter().map(real).collect()),
        Datum::Unread(what) => return Err(what),
        _ => return Err("dictionary value"),
    })
}

#[cfg(test)]
mod tests;
