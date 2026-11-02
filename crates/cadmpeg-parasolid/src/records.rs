// SPDX-License-Identifier: Apache-2.0
//! Neutral-binary node reading through base and embedded schema field lists.
//!
//! A node type's first occurrence in an embedded-schema stream carries its
//! effective field list as an edit program over the base schema. Later
//! occurrences reuse the established list.

use cadmpeg_core::decode::View;
use cadmpeg_core::printable::PrintableString;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

mod base;
pub mod deltas;
pub mod group;
pub mod primary;

pub use deltas::DeltasRecords;
pub use primary::PrimaryRecords;

/// A base schema that embedded edit programs refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseVersion {
    /// Base schema 11004.
    V11004,
    /// Base schema 13006.
    V13006,
}

/// Whether a transmit description names a partition stream.
pub fn is_partition_description(description: &[u8]) -> bool {
    description == b": TRANSMIT FILE (partition)"
        || description.starts_with(b": TRANSMIT FILE (partition) ")
}

pub mod instance;

/// Field type code of one schema field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    /// `u`: one unsigned byte.
    #[serde(rename = "u")]
    Byte,
    /// `c`: one character byte.
    #[serde(rename = "c")]
    Char,
    /// `l`: one logical byte, zero or one.
    #[serde(rename = "l")]
    Logical,
    /// `n`: one big-endian u16 short.
    #[serde(rename = "n")]
    Short,
    /// `w`: one big-endian u16 word.
    #[serde(rename = "w")]
    Word,
    /// `d`: one big-endian u32 integer.
    #[serde(rename = "d")]
    Integer,
    /// `p`: one biased-index node pointer.
    #[serde(rename = "p")]
    Pointer,
    /// `f`: one big-endian binary64 real.
    #[serde(rename = "f")]
    Real,
    /// `i`: two reals, an interval.
    #[serde(rename = "i")]
    Interval,
    /// `v`: three reals, a vector.
    #[serde(rename = "v")]
    Vector,
    /// `b`: six reals, a box.
    #[serde(rename = "b")]
    Box,
    /// `h`: one Hvec value.
    #[serde(rename = "h")]
    Hvec,
    /// A declared type code without an established instance width.
    #[serde(skip)]
    Unestablished(u8),
}

impl DataType {
    fn read(value: &str) -> Option<Self> {
        Some(match value {
            "u" => Self::Byte,
            "c" => Self::Char,
            "l" => Self::Logical,
            "n" => Self::Short,
            "w" => Self::Word,
            "d" => Self::Integer,
            "p" => Self::Pointer,
            "f" => Self::Real,
            "i" => Self::Interval,
            "v" => Self::Vector,
            "b" => Self::Box,
            "h" => Self::Hvec,
            "q" => Self::Unestablished(b'q'),
            "t" => Self::Unestablished(b't'),
            _ => return None,
        })
    }
}

/// One effective field of a node type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FieldWire")]
pub struct SchemaField {
    /// Field name.
    pub name: PrintableString<String>,
    /// Field type code.
    pub data_type: DataType,
    pub(crate) pointer_class: u16,
    /// Element extent: zero is scalar, one is variable, a greater value is a fixed array.
    pub n_elements: u32,
    /// Whether instances transmit the field.
    pub transmit: bool,
}

#[derive(Deserialize)]
struct FieldWire {
    name: PrintableString<String>,
    data_type: DataType,
    pointer_class: u16,
    n_elements: u32,
    transmit: bool,
}

impl TryFrom<FieldWire> for SchemaField {
    type Error = &'static str;
    fn try_from(wire: FieldWire) -> Result<Self, Self::Error> {
        if wire.pointer_class != 0 && wire.data_type != DataType::Pointer {
            return Err("pointer_class: requires pointer data_type");
        }
        if wire.n_elements != 1 && !wire.transmit {
            return Err("transmit: a fixed effective field must be transmitted");
        }
        Ok(Self {
            name: wire.name,
            data_type: wire.data_type,
            pointer_class: wire.pointer_class,
            n_elements: wire.n_elements,
            transmit: wire.transmit,
        })
    }
}

/// The effective field list of one node type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaPlan {
    /// Node type.
    pub kind: u16,
    /// Effective fields in instance order.
    pub fields: Vec<SchemaField>,
    /// Name declared by a node type absent from the base schema.
    #[serde(skip)]
    pub custom_name: Option<String>,
    /// Description declared by a node type absent from the base schema.
    #[serde(skip)]
    pub custom_description: Option<String>,
}

impl SchemaPlan {
    /// Whether the plan is the base-13006 field list of `kind` followed by the scalar pointer fields `tail`.
    pub fn matches_base_with_pointer_tail(&self, kind: u16, tail: &[(&str, u16)]) -> bool {
        if self.kind != kind {
            return false;
        }
        let Some(original) = base::fields(BaseVersion::V13006, self.kind) else {
            return false;
        };
        self.fields
            .iter()
            .map(|field| {
                (
                    field.name.as_str(),
                    field.data_type,
                    field.pointer_class,
                    field.n_elements,
                    field.transmit,
                )
            })
            .eq(original
                .iter()
                .map(|field| {
                    (
                        field.name,
                        field.data_type,
                        field.pointer_class,
                        field.n_elements,
                        field.transmit,
                    )
                })
                .chain(
                    tail.iter()
                        .map(|(name, class)| (*name, DataType::Pointer, *class, 0, true)),
                ))
    }

    pub(crate) fn matches_fields<'a>(
        &self,
        kind: u16,
        expected: impl Iterator<Item = (&'a str, DataType, u16)>,
    ) -> bool {
        self.kind == kind
            && self
                .fields
                .iter()
                .map(|field| {
                    (
                        field.name.as_str(),
                        field.data_type,
                        field.pointer_class,
                        field.n_elements,
                        field.transmit,
                    )
                })
                .eq(expected.map(|(name, kind, class)| (name, kind, class, 0, true)))
    }
}

#[derive(Clone, Copy)]
struct BorrowedField<'a> {
    name: &'a str,
    data_type: DataType,
    pointer_class: u16,
    n_elements: u32,
    transmit: bool,
}

impl<'a> BorrowedField<'a> {
    const fn new(
        name: &'a str,
        data_type: DataType,
        pointer_class: u16,
        n_elements: u32,
        transmit: bool,
    ) -> Self {
        Self {
            name,
            data_type,
            pointer_class,
            n_elements,
            transmit,
        }
    }
}

fn short_string<'a>(view: &mut View<'a>) -> Option<&'a str> {
    let count = usize::from(view.u8()?);
    let text = std::str::from_utf8(view.take(count)?).ok()?;
    PrintableString::new(text).ok()?;
    Some(text)
}

/// Read one positive integer in the biased index encoding and return its logical value.
///
/// A positive `i16` holds the value plus one. A negative `i16` remainder and a
/// `u16` quotient hold `quotient * 32767 + |remainder|` plus one.
pub fn positive(view: &mut View<'_>) -> Option<u32> {
    crate::pointer::read(view)?.checked_sub(1)
}

fn descriptor<'a>(view: &mut View<'a>) -> Option<BorrowedField<'a>> {
    let name = short_string(view)?;
    let pointer_class = view.u16_be()?;
    let n_elements = positive(view)?;
    let data_type = if pointer_class == 0 {
        DataType::read(short_string(view)?)?
    } else {
        DataType::Pointer
    };
    let transmit = if n_elements == 1 {
        match view.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        }
    } else {
        true
    };
    Some(BorrowedField {
        name,
        data_type,
        pointer_class,
        n_elements,
        transmit,
    })
}

fn read_definition(
    view: &mut View<'_>,
    version: BaseVersion,
    kind: u16,
    count: u8,
) -> Result<Option<SchemaPlan>, CodecError> {
    Ok(read_edit_program(view, version, kind, count)?.map(|program| program.plan))
}

/// One first-use schema declaration and the base fields its program consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditProgram {
    plan: SchemaPlan,
    base_consumed: usize,
    base_len: usize,
}

impl EditProgram {
    /// The effective field list the declaration establishes.
    #[must_use]
    pub const fn plan(&self) -> &SchemaPlan {
        &self.plan
    }

    /// Whether the program copied or deleted every base field before `Z`.
    #[must_use]
    pub const fn consumes_base(&self) -> bool {
        self.base_consumed == self.base_len
    }
}

/// Read the first-use schema declaration of node type `kind` after its
/// effective-field `count` byte.
///
/// A count of `ff` declares the base field list. Another count starts an
/// edit program over the base list, or a complete declaration for a type
/// absent from the base schema. The view ends after the declaration.
///
/// # Errors
///
/// Returns [`CodecError::Malformed`] when a field name fails the shared
/// printable-name check after it was read.
pub fn read_edit_program(
    view: &mut View<'_>,
    version: BaseVersion,
    kind: u16,
    count: u8,
) -> Result<Option<EditProgram>, CodecError> {
    let original = base::fields(version, kind);

    let mut fields = Vec::new();
    let mut custom = None;
    let mut base_consumed = original.map_or(0, <[_]>::len);
    if count == 255 {
        let Some(original) = original else {
            return Ok(None);
        };
        for field in original {
            fields.push(*field);
        }
    } else if let Some(original) = original {
        let mut cursor = 0;
        loop {
            let Some(opcode) = view.u8() else {
                return Ok(None);
            };
            if opcode == b'Z' {
                break;
            }
            let field = match opcode {
                b'C' | b'D' => {
                    let Some(field) = original.get(cursor) else {
                        return Ok(None);
                    };
                    cursor += 1;
                    (opcode == b'C').then_some(*field)
                }
                b'I' if cursor < original.len() => {
                    let Some(field) = descriptor(view) else {
                        return Ok(None);
                    };
                    Some(field)
                }
                b'A' if cursor == original.len() => {
                    let Some(field) = descriptor(view) else {
                        return Ok(None);
                    };
                    Some(field)
                }
                _ => return Ok(None),
            };
            if let Some(field) = field {
                if fields.len() == usize::from(count) {
                    return Ok(None);
                }
                fields.push(field);
            }
        }
        if fields.len() != usize::from(count) {
            return Ok(None);
        }
        base_consumed = cursor;
    } else {
        let Some(name) = short_string(view) else {
            return Ok(None);
        };
        let Some(description) = short_string(view) else {
            return Ok(None);
        };
        custom = Some((name, description));
        for _ in 0..count {
            let Some(field) = descriptor(view) else {
                return Ok(None);
            };
            fields.push(field);
        }
    }
    let mut plan = own_definition(kind, &fields)?;
    if let Some((name, description)) = custom {
        plan.custom_name = Some(name.to_owned());
        plan.custom_description = Some(description.to_owned());
    }
    Ok(Some(EditProgram {
        plan,
        base_consumed,
        base_len: original.map_or(0, <[_]>::len),
    }))
}

fn own_definition(kind: u16, fields: &[BorrowedField<'_>]) -> Result<SchemaPlan, CodecError> {
    let mut owned = Vec::new();
    for field in fields {
        let name = (field.name).to_owned();
        let name = PrintableString::new(name)
            .map_err(|_| CodecError::Malformed("invalid Parasolid schema field name".into()))?;
        owned.push(SchemaField {
            name,
            data_type: field.data_type,
            pointer_class: field.pointer_class,
            n_elements: field.n_elements,
            transmit: field.transmit,
        });
    }
    Ok(SchemaPlan {
        kind,
        fields: owned,
        custom_name: None,
        custom_description: None,
    })
}

#[cfg(test)]
mod tests;
