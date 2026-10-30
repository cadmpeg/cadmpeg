// SPDX-License-Identifier: Apache-2.0
//! Value representations and the value bytes they point to (spec §9.6,
//! §9.7).

use cadmpeg_core::CodecError;

use super::{encode, unsupported, Crate};
use crate::text::syntax::{Dictionary, DictionaryValue, ListOp, Reference, Value};
use crate::text::value::{typed, Components, Typed};
use crate::usdc::value::{
    half, type_number, value_kind, Component, Shape, COMPRESSED_BIT, MIN_COMPRESSED_ARRAY,
};

const ARRAY_BIT: u64 = 1 << 63;
const INLINED_BIT: u64 = 1 << 62;
const TYPE_SHIFT: u32 = 48;
/// The most distinct values of a real array stored as a table.
const MAX_REAL_TABLE: usize = 1024;
/// A real array is stored as a table when its element count exceeds its
/// distinct values this many times.
const REAL_TABLE_RATIO: usize = 4;

/// Value types of the representations this writer builds (spec §9.6).
pub(super) mod ty {
    pub(in crate::usdc) const BOOL: u8 = 1;
    pub(in crate::usdc) const STRING: u8 = 10;
    pub(in crate::usdc) const TOKEN: u8 = 11;
    pub(in crate::usdc) const DICTIONARY: u8 = 31;
    pub(in crate::usdc) const TOKEN_LIST_OP: u8 = 32;
    pub(in crate::usdc) const STRING_LIST_OP: u8 = 33;
    pub(in crate::usdc) const PATH_LIST_OP: u8 = 34;
    pub(in crate::usdc) const REFERENCE_LIST_OP: u8 = 35;
    pub(in crate::usdc) const TOKEN_VECTOR: u8 = 41;
    pub(in crate::usdc) const SPECIFIER: u8 = 42;
    pub(in crate::usdc) const PERMISSION: u8 = 43;
    pub(in crate::usdc) const VARIABILITY: u8 = 44;
    pub(in crate::usdc) const VARIANT_SELECTIONS: u8 = 45;
    pub(in crate::usdc) const TIME_SAMPLES: u8 = 46;
    pub(in crate::usdc) const DOUBLE_VECTOR: u8 = 48;
    pub(in crate::usdc) const LAYER_OFFSET_VECTOR: u8 = 49;
    pub(in crate::usdc) const STRING_VECTOR: u8 = 50;
    pub(in crate::usdc) const VALUE_BLOCK: u8 = 51;
    pub(in crate::usdc) const PAYLOAD_LIST_OP: u8 = 55;
}

/// The representation of type `ty` with `payload`.
pub(super) fn rep(ty: u8, payload: u64, array: bool, inlined: bool) -> u64 {
    let mut out = (u64::from(ty) << TYPE_SHIFT) | payload;
    if array {
        out |= ARRAY_BIT;
    }
    if inlined {
        out |= INLINED_BIT;
    }
    out
}

/// The crate value type of a text type name: role types store their
/// component type (spec §7.2).
fn crate_type(name: &str) -> Option<u8> {
    for (role, count) in [
        ("point3", 3),
        ("normal3", 3),
        ("vector3", 3),
        ("color3", 3),
        ("color4", 4),
        ("texCoord2", 2),
        ("texCoord3", 3),
    ] {
        if let Some(suffix) = name.strip_prefix(role) {
            let precision = match suffix {
                "h" => "half",
                "f" => "float",
                "d" => "double",
                _ => return None,
            };
            return type_number(&format!("{precision}{count}"));
        }
    }
    match name {
        "frame4d" => type_number("matrix4d"),
        // A time code is a double before 0.9.0.
        "timecode" => type_number("double"),
        _ => type_number(name),
    }
}

/// The binary16 bits nearest `value`, ties to even.
fn half_bits(value: f64) -> u16 {
    let single = crate::text::value::round_f32(value);
    let bits = single.to_bits();
    let sign = u16::try_from((bits >> 16) & 0x8000).unwrap_or(0);
    let exponent = i32::try_from((bits >> 23) & 0xFF).unwrap_or(0);
    let mantissa = bits & 0x007F_FFFF;
    if exponent == 0xFF {
        let nan = if mantissa == 0 { 0 } else { 0x0200 };
        return sign | 0x7C00 | nan;
    }
    let unbiased = exponent - 127;
    if unbiased > 15 {
        return sign | 0x7C00;
    }
    let (shift, implicit) = if unbiased >= -14 {
        (13, (1 << 23) | mantissa)
    } else {
        let extra = u32::try_from(-14 - unbiased).unwrap_or(32);
        if extra > 11 {
            return sign;
        }
        (13 + extra, (1 << 23) | mantissa)
    };
    let shifted = implicit >> shift;
    let remainder = implicit & ((1 << shift) - 1);
    let halfway = 1 << (shift - 1);
    let rounded = if remainder > halfway || (remainder == halfway && shifted & 1 == 1) {
        shifted + 1
    } else {
        shifted
    };
    let magnitude = if unbiased >= -14 {
        let biased = u32::try_from(unbiased + 15).unwrap_or(0);
        (biased << 10) + (rounded - (1 << 10))
    } else {
        rounded
    };
    sign | u16::try_from(magnitude).unwrap_or(0x7C00)
}

impl Crate {
    fn here(&self) -> u64 {
        cadmpeg_core::decode::u64_from_index(self.out.len())
    }

    /// A text value of the declared type `type_name`.
    pub(super) fn typed_value(
        &mut self,
        value: &Value,
        type_name: &str,
        array: bool,
    ) -> Result<u64, CodecError> {
        let typed = typed(value, type_name, array)
            .map_err(|error| CodecError::malformed(format_args!("{type_name} value: {error}")))?;
        self.typed(&typed, type_name)
    }

    fn typed(&mut self, typed: &Typed, type_name: &str) -> Result<u64, CodecError> {
        let ty = crate_type(type_name)
            .ok_or_else(|| unsupported(&format!("the value type {type_name}")))?;
        let kind = value_kind(ty).ok_or_else(|| unsupported(type_name))?;
        let quaternion = kind.shape == Shape::Quaternion;
        let components = match &typed.components {
            Components::Texts(texts) => {
                let indices = texts
                    .iter()
                    .map(|text| match kind.component {
                        Component::Token => self.token(text),
                        Component::Asset if !typed.array => self.token(text),
                        _ => self.string(text),
                    })
                    .collect();
                Lane::Indices(indices)
            }
            Components::Bools(flags) => {
                Lane::Integers(flags.iter().map(|flag| i128::from(*flag)).collect())
            }
            Components::Integers(values) => Lane::Integers(values.clone()),
            Components::Reals(values) => Lane::Reals(values.clone()),
        };
        let width = kind.shape.components();
        if !typed.array && kind.shape == Shape::Scalar && kind.component.size() <= 4 {
            let bits = match (&components, kind.component) {
                (Lane::Indices(indices), _) => indices.first().copied().unwrap_or(0),
                (Lane::Integers(values), _) => {
                    let value = values.first().copied().unwrap_or(0);
                    u32::try_from(value)
                        .or_else(|_| i32::try_from(value).map(i32::cast_unsigned))
                        .map_err(|_| unsupported("an integer outside its type"))?
                }
                (Lane::Reals(values), Component::F16) => {
                    u32::from(half_bits(values.first().copied().unwrap_or(0.0)))
                }
                (Lane::Reals(values), _) => {
                    crate::text::value::round_f32(values.first().copied().unwrap_or(0.0)).to_bits()
                }
            };
            return Ok(rep(ty, u64::from(bits), false, true));
        }
        let count = components.len() / width.max(1);
        if typed.array && count == 0 {
            return Ok(rep(ty, 0, true, false));
        }
        if typed.array && kind.shape == Shape::Scalar && count >= MIN_COMPRESSED_ARRAY {
            if let Some(body) = compressed(&components, kind.component)? {
                let start = self.here();
                self.out.extend_from_slice(&body);
                return Ok(rep(ty, start, true, false) | COMPRESSED_BIT);
            }
        }
        let start = self.here();
        if typed.array {
            self.out
                .extend_from_slice(&cadmpeg_core::decode::u64_from_index(count).to_le_bytes());
        }
        for element in 0..count {
            for position in 0..width {
                // A quaternion is stored (i, j, k, real); its text form is
                // (real, i, j, k).
                let source = if quaternion {
                    element * width + (position + 1) % 4
                } else {
                    element * width + position
                };
                self.component(&components, source, kind.component)?;
            }
        }
        Ok(rep(ty, start, typed.array, false))
    }

    fn component(
        &mut self,
        lane: &Lane,
        at: usize,
        component: Component,
    ) -> Result<(), CodecError> {
        match (lane, component) {
            (Lane::Indices(indices), _) => {
                self.out.extend_from_slice(&indices[at].to_le_bytes());
            }
            (Lane::Integers(values), _) => {
                let value = values[at];
                let range = || unsupported("an integer outside its type");
                match component {
                    Component::Bool | Component::U8 => {
                        self.out.push(u8::try_from(value).map_err(|_| range())?);
                    }
                    Component::I32 => self.out.extend_from_slice(
                        &i32::try_from(value).map_err(|_| range())?.to_le_bytes(),
                    ),
                    Component::U32 => self.out.extend_from_slice(
                        &u32::try_from(value).map_err(|_| range())?.to_le_bytes(),
                    ),
                    Component::I64 => self.out.extend_from_slice(
                        &i64::try_from(value).map_err(|_| range())?.to_le_bytes(),
                    ),
                    _ => self.out.extend_from_slice(
                        &u64::try_from(value).map_err(|_| range())?.to_le_bytes(),
                    ),
                }
            }
            (Lane::Reals(values), Component::F16) => {
                self.out
                    .extend_from_slice(&half_bits(values[at]).to_le_bytes());
            }
            (Lane::Reals(values), Component::F32) => self
                .out
                .extend_from_slice(&crate::text::value::round_f32(values[at]).to_le_bytes()),
            (Lane::Reals(values), _) => self.out.extend_from_slice(&values[at].to_le_bytes()),
        }
        Ok(())
    }

    /// A vector of `u32` items, at its offset: a `uint64` count, then the
    /// items.
    fn index_vector(&mut self, ty: u8, items: &[u32]) -> u64 {
        let start = self.here();
        self.out
            .extend_from_slice(&cadmpeg_core::decode::u64_from_index(items.len()).to_le_bytes());
        for item in items {
            self.out.extend_from_slice(&item.to_le_bytes());
        }
        rep(ty, start, false, false)
    }

    pub(super) fn token_vector(&mut self, names: &[String]) -> u64 {
        let items: Vec<u32> = names.iter().map(|name| self.token(name)).collect();
        self.index_vector(ty::TOKEN_VECTOR, &items)
    }

    pub(super) fn string_vector(&mut self, names: &[String]) -> u64 {
        let items: Vec<u32> = names.iter().map(|name| self.string(name)).collect();
        self.index_vector(ty::STRING_VECTOR, &items)
    }

    pub(super) fn double_vector(&mut self, values: &[f64]) -> u64 {
        let start = self.here();
        self.out
            .extend_from_slice(&cadmpeg_core::decode::u64_from_index(values.len()).to_le_bytes());
        for value in values {
            self.out.extend_from_slice(&value.to_le_bytes());
        }
        rep(ty::DOUBLE_VECTOR, start, false, false)
    }

    pub(super) fn layer_offsets(&mut self, offsets: &[(f64, f64)]) -> u64 {
        let start = self.here();
        self.out
            .extend_from_slice(&cadmpeg_core::decode::u64_from_index(offsets.len()).to_le_bytes());
        for (offset, scale) in offsets {
            self.out.extend_from_slice(&offset.to_le_bytes());
            self.out.extend_from_slice(&scale.to_le_bytes());
        }
        rep(ty::LAYER_OFFSET_VECTOR, start, false, false)
    }

    pub(super) fn selections(&mut self, pairs: &[(String, String)]) -> u64 {
        let indices: Vec<(u32, u32)> = pairs
            .iter()
            .map(|(set, variant)| (self.string(set), self.string(variant)))
            .collect();
        let start = self.here();
        self.out
            .extend_from_slice(&cadmpeg_core::decode::u64_from_index(indices.len()).to_le_bytes());
        for (set, variant) in indices {
            self.out.extend_from_slice(&set.to_le_bytes());
            self.out.extend_from_slice(&variant.to_le_bytes());
        }
        rep(ty::VARIANT_SELECTIONS, start, false, false)
    }

    /// The dictionary record of `dictionary`, after the values it boxes.
    fn dictionary_record(&mut self, dictionary: &Dictionary) -> Result<Vec<u8>, CodecError> {
        let mut entries = Vec::new();
        for entry in &dictionary.entries {
            let value = match &entry.value {
                DictionaryValue::Typed {
                    type_name,
                    array,
                    value,
                } => self.typed_value(value, type_name, *array)?,
                DictionaryValue::Dictionary(inner) => self.dictionary(inner)?,
            };
            entries.push((self.string(&entry.key), value));
        }
        let mut record = cadmpeg_core::decode::u64_from_index(entries.len())
            .to_le_bytes()
            .to_vec();
        for (key, value) in entries {
            record.extend_from_slice(&key.to_le_bytes());
            // The boxed representation follows its offset.
            record.extend_from_slice(&8_i64.to_le_bytes());
            record.extend_from_slice(&value.to_le_bytes());
        }
        Ok(record)
    }

    pub(super) fn dictionary(&mut self, dictionary: &Dictionary) -> Result<u64, CodecError> {
        if dictionary.entries.is_empty() {
            return Ok(rep(ty::DICTIONARY, 0, false, true));
        }
        let record = self.dictionary_record(dictionary)?;
        let start = self.here();
        self.out.extend_from_slice(&record);
        Ok(rep(ty::DICTIONARY, start, false, false))
    }

    /// A list operation of type `ty` whose parts are the items each list
    /// operation keyword states; `None` is the explicit list.
    pub(super) fn list_op(
        &mut self,
        ty: u8,
        parts: &[(Option<ListOp>, Vec<ListItem<'_>>)],
    ) -> Result<u64, CodecError> {
        const EXPLICIT: u8 = 1;
        const EXPLICIT_ITEMS: u8 = 1 << 1;
        const ADDED: u8 = 1 << 2;
        const DELETED: u8 = 1 << 3;
        const ORDERED: u8 = 1 << 4;
        const PREPENDED: u8 = 1 << 5;
        const APPENDED: u8 = 1 << 6;
        let mut slots: [Option<Vec<u8>>; 6] = Default::default();
        let mut header = 0;
        for (op, items) in parts {
            let (bit, slot) = match op {
                None => (EXPLICIT | EXPLICIT_ITEMS, 0),
                Some(ListOp::Add) => (ADDED, 1),
                Some(ListOp::Prepend) => (PREPENDED, 2),
                Some(ListOp::Append) => (APPENDED, 3),
                Some(ListOp::Delete) => (DELETED, 4),
                Some(ListOp::Reorder) => (ORDERED, 5),
            };
            header |= bit;
            let mut bytes = Vec::new();
            for item in items {
                self.list_item(item, &mut bytes)?;
            }
            let entry = slots[slot].get_or_insert_with(Vec::new);
            entry.extend_from_slice(&bytes);
        }
        let counts: Vec<usize> = parts
            .iter()
            .fold([0_usize; 6], |mut counts, (op, items)| {
                let slot = match op {
                    None => 0,
                    Some(ListOp::Add) => 1,
                    Some(ListOp::Prepend) => 2,
                    Some(ListOp::Append) => 3,
                    Some(ListOp::Delete) => 4,
                    Some(ListOp::Reorder) => 5,
                };
                counts[slot] += items.len();
                counts
            })
            .to_vec();
        let start = self.here();
        self.out.push(header);
        for (slot, bytes) in slots.iter().enumerate() {
            if let Some(bytes) = bytes {
                self.out.extend_from_slice(
                    &cadmpeg_core::decode::u64_from_index(counts[slot]).to_le_bytes(),
                );
                self.out.extend_from_slice(bytes);
            }
        }
        Ok(rep(ty, start, false, false))
    }

    fn list_item(&mut self, item: &ListItem<'_>, out: &mut Vec<u8>) -> Result<(), CodecError> {
        match item {
            ListItem::Token(name) => out.extend_from_slice(&self.token(name).to_le_bytes()),
            ListItem::String(name) => out.extend_from_slice(&self.string(name).to_le_bytes()),
            ListItem::Path(path) => out.extend_from_slice(&self.path(path)?.to_le_bytes()),
            ListItem::Reference(reference, payload) => {
                let custom = match (&reference.custom_data, payload) {
                    (Some(dictionary), false) => Some(self.dictionary_record(dictionary)?),
                    (None, false) => Some(0_u64.to_le_bytes().to_vec()),
                    _ => None,
                };
                out.extend_from_slice(
                    &self
                        .string(reference.asset.as_deref().unwrap_or(""))
                        .to_le_bytes(),
                );
                let path = match reference.prim_path.as_deref() {
                    Some(path) => self.path(path)?,
                    None => Crate::empty_path(),
                };
                out.extend_from_slice(&path.to_le_bytes());
                let offset = reference.offset.map_or(0.0, |number| number.float);
                let scale = reference.scale.map_or(1.0, |number| number.float);
                out.extend_from_slice(&offset.to_le_bytes());
                out.extend_from_slice(&scale.to_le_bytes());
                if let Some(custom) = custom {
                    out.extend_from_slice(&custom);
                }
            }
        }
        Ok(())
    }

    /// Time samples (spec §9.7): the boxed sample times, then the offset
    /// to the sample count and the sample representations.
    pub(super) fn time_samples(&mut self, times: &[f64], values: &[u64]) -> u64 {
        let times = self.double_vector(times);
        let start = self.here();
        self.out.extend_from_slice(&8_i64.to_le_bytes());
        self.out.extend_from_slice(&times.to_le_bytes());
        self.out.extend_from_slice(&8_i64.to_le_bytes());
        self.out
            .extend_from_slice(&cadmpeg_core::decode::u64_from_index(values.len()).to_le_bytes());
        for value in values {
            self.out.extend_from_slice(&value.to_le_bytes());
        }
        rep(ty::TIME_SAMPLES, start, false, false)
    }
}

/// The bytes of `value` as a `component` real.
fn real_bytes(value: f64, component: Component) -> Vec<u8> {
    match component {
        Component::F16 => half_bits(value).to_le_bytes().to_vec(),
        Component::F32 => crate::text::value::round_f32(value).to_le_bytes().to_vec(),
        _ => value.to_le_bytes().to_vec(),
    }
}

/// `value` as a reader of a `component` real reads it back.
fn stored_real(value: f64, component: Component) -> f64 {
    match component {
        Component::F16 => half(half_bits(value)),
        Component::F32 => f64::from(crate::text::value::round_f32(value)),
        _ => value,
    }
}

/// The 32-bit integer equal to the stored `value`.
fn exact_integer(value: f64) -> Option<i32> {
    cadmpeg_core::convert::truncate_f64_to_i32(value).filter(|integer| {
        f64::from(*integer) == value && (value != 0.0 || value.is_sign_positive())
    })
}

/// The compressed array data (spec §9.7) of a scalar array of `component`
/// with `lane` elements, or `None` when no compressed form holds it.
fn compressed(lane: &Lane, component: Component) -> Result<Option<Vec<u8>>, CodecError> {
    let range = || unsupported("an integer outside its type");
    let mut out = cadmpeg_core::decode::u64_from_index(lane.len())
        .to_le_bytes()
        .to_vec();
    match (lane, component) {
        (Lane::Integers(values), Component::I32) => {
            let values = values
                .iter()
                .map(|value| i32::try_from(*value).map_err(|_| range()))
                .collect::<Result<Vec<_>, _>>()?;
            encode::integers(&values, &mut out)?;
        }
        (Lane::Integers(values), Component::U32) => {
            let values = values
                .iter()
                .map(|value| {
                    u32::try_from(*value)
                        .map(u32::cast_signed)
                        .map_err(|_| range())
                })
                .collect::<Result<Vec<_>, _>>()?;
            encode::integers(&values, &mut out)?;
        }
        (Lane::Integers(values), Component::I64) => {
            let values = values
                .iter()
                .map(|value| i64::try_from(*value).map_err(|_| range()))
                .collect::<Result<Vec<_>, _>>()?;
            encode::wide_integers(&values, &mut out)?;
        }
        (Lane::Integers(values), Component::U64) => {
            let values = values
                .iter()
                .map(|value| {
                    u64::try_from(*value)
                        .map(u64::cast_signed)
                        .map_err(|_| range())
                })
                .collect::<Result<Vec<_>, _>>()?;
            encode::wide_integers(&values, &mut out)?;
        }
        (Lane::Reals(values), Component::F16 | Component::F32 | Component::F64) => {
            let stored: Vec<f64> = values
                .iter()
                .map(|value| stored_real(*value, component))
                .collect();
            if let Some(integers) = stored
                .iter()
                .map(|value| exact_integer(*value))
                .collect::<Option<Vec<i32>>>()
            {
                out.push(b'i');
                encode::integers(&integers, &mut out)?;
                return Ok(Some(out));
            }
            let mut table: Vec<f64> = Vec::new();
            let mut slots: std::collections::HashMap<u64, i32> = std::collections::HashMap::new();
            let mut indices = Vec::with_capacity(stored.len());
            for value in &stored {
                let next = i32::try_from(table.len()).map_err(|_| range())?;
                let index = *slots.entry(value.to_bits()).or_insert_with(|| {
                    table.push(*value);
                    next
                });
                if table.len() > MAX_REAL_TABLE || table.len() * REAL_TABLE_RATIO >= stored.len() {
                    return Ok(None);
                }
                indices.push(index);
            }
            out.push(b't');
            out.extend_from_slice(
                &u32::try_from(table.len())
                    .map_err(|_| range())?
                    .to_le_bytes(),
            );
            for value in &table {
                out.extend_from_slice(&real_bytes(*value, component));
            }
            encode::integers(&indices, &mut out)?;
        }
        _ => return Ok(None),
    }
    Ok(Some(out))
}

/// The flat components of a value, as the crate stores them.
enum Lane {
    /// String or token indices.
    Indices(Vec<u32>),
    Integers(Vec<i128>),
    Reals(Vec<f64>),
}

impl Lane {
    fn len(&self) -> usize {
        match self {
            Self::Indices(values) => values.len(),
            Self::Integers(values) => values.len(),
            Self::Reals(values) => values.len(),
        }
    }
}

/// One item of a list operation.
pub(super) enum ListItem<'a> {
    Token(&'a str),
    String(&'a str),
    Path(&'a str),
    /// A reference, or a payload when the flag is set.
    Reference(&'a Reference, bool),
}

#[cfg(test)]
mod tests;
