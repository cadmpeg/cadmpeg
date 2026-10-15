// SPDX-License-Identifier: Apache-2.0
//! Primary V13 field arrays and bounded embedded-schema edit programs.

use crate::printable_string::PrintableString;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

mod base;
pub(crate) mod instance;

/// A field's physical scalar type; pointer classes remain separate constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DataType {
    #[serde(rename = "u")]
    Byte,
    #[serde(rename = "c")]
    Char,
    #[serde(rename = "l")]
    Logical,
    #[serde(rename = "n")]
    Short,
    #[serde(rename = "w")]
    Word,
    #[serde(rename = "d")]
    Integer,
    #[serde(rename = "p")]
    Pointer,
    #[serde(rename = "f")]
    Real,
    #[serde(rename = "i")]
    Interval,
    #[serde(rename = "v")]
    Vector,
    #[serde(rename = "b")]
    Box,
    #[serde(rename = "h")]
    Hvec,
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
            _ => return None,
        })
    }
}

/// One reconstructed effective field, independent of a node's field values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FieldWire")]
pub(crate) struct SchemaField {
    name: PrintableString<String>,
    data_type: DataType,
    pointer_class: u16,
    n_elements: u32,
    transmit: bool,
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

/// Complete schema metadata ending before variable length or node identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SchemaWire")]
pub(crate) struct EmbeddedSchema {
    kind: u16,
    fields: Vec<SchemaField>,
}

impl EmbeddedSchema {
    pub(super) fn matches_base_with_pointer_tail(&self, kind: u16, tail: &[(&str, u16)]) -> bool {
        if self.kind != kind {
            return false;
        }
        let Some(original) = base::fields(self.kind) else {
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

    pub(super) fn matches_fields<'a>(
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

#[derive(Deserialize)]
struct SchemaWire {
    kind: u16,
    fields: Vec<SchemaField>,
}

impl TryFrom<SchemaWire> for EmbeddedSchema {
    type Error = &'static str;
    fn try_from(wire: SchemaWire) -> Result<Self, Self::Error> {
        if base::fields(wire.kind).is_none() || wire.fields.is_empty() || wire.fields.len() >= 255 {
            return Err("definition: unsupported kind or invalid effective field count");
        }
        Ok(Self {
            kind: wire.kind,
            fields: wire.fields,
        })
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

pub(super) fn positive(view: &mut View<'_>) -> Option<u32> {
    let remainder = view.i16_be()?;
    if remainder > 0 {
        return u32::try_from(remainder).ok()?.checked_sub(1);
    }
    if remainder == 0 {
        return None;
    }
    let quotient = view.u16_be()?;
    if quotient == 0 {
        return None;
    }
    u32::from(quotient)
        .checked_mul(32_767)?
        .checked_add(u32::from(remainder.unsigned_abs()))?
        .checked_sub(1)
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

/// Replay C/D/I/A/Z against the declared V13 base. No following node bytes are read.
pub(crate) fn read(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<(EmbeddedSchema, usize)>, CodecError> {
    let mut view = View::over_retained(stream);
    let Some((kind, count, original)) = (|| {
        view.seek(offset)?;
        let kind = view.u16_be()?;
        let original = base::fields(kind)?;
        Some((kind, view.u8()?, original))
    })() else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "NX schema edit fields")?;
    let mut fields = Vec::new();
    if count == 255 {
        for field in original {
            ctx.push_scoped_vec(&mut scratch, &mut fields, *field, "NX schema edit fields")?;
        }
    } else {
        // An empty effective array cannot define any of the supported node types.
        if count == 0 {
            return Ok(None);
        }
        let mut cursor = 0;
        loop {
            ctx.charge_work(1, "read NX schema edit")?;
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
                    let Some(field) = descriptor(&mut view) else {
                        return Ok(None);
                    };
                    Some(field)
                }
                b'A' if cursor == original.len() => {
                    let Some(field) = descriptor(&mut view) else {
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
                ctx.push_scoped_vec(&mut scratch, &mut fields, field, "NX schema edit fields")?;
            }
        }
        if fields.len() != usize::from(count) {
            return Ok(None);
        }
    }
    let mut owned = Vec::new();
    for field in fields {
        let name = ctx.copy_retained_text(field.name, "NX schema field name")?;
        let Ok(name) = PrintableString::new(name) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut owned,
            SchemaField {
                name,
                data_type: field.data_type,
                pointer_class: field.pointer_class,
                n_elements: field.n_elements,
                transmit: field.transmit,
            },
            "NX effective schema fields",
        )?;
    }
    Ok(Some((
        EmbeddedSchema {
            kind,
            fields: owned,
        },
        view.position(),
    )))
}

#[cfg(test)]
mod tests;
