// SPDX-License-Identifier: Apache-2.0

use crate::printable_string::PrintableString;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

mod base;
pub(crate) mod group;
mod primary;

pub(crate) use primary::PrimaryRecords;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseVersion {
    V11004,
    V13006,
}

pub(crate) fn is_partition_description(description: &[u8]) -> bool {
    description == b": TRANSMIT FILE (partition)"
        || description.starts_with(b": TRANSMIT FILE (partition) ")
}

fn status_schema_kind(kind: u16) -> bool {
    matches!(kind, 12 | 19 | 38 | 41 | 70 | 74 | 100 | 101)
}
pub(crate) mod instance;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SchemaWire")]
pub(crate) struct EmbeddedSchema {
    #[serde(flatten)]
    plan: SchemaPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SchemaPlan {
    kind: u16,
    fields: Vec<SchemaField>,
}

impl EmbeddedSchema {
    pub(crate) fn matches_base_with_pointer_tail(&self, kind: u16, tail: &[(&str, u16)]) -> bool {
        if self.plan.kind != kind {
            return false;
        }
        let Some(original) = base::fields(BaseVersion::V13006, self.plan.kind) else {
            return false;
        };
        self.plan
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
        self.plan.kind == kind
            && self
                .plan
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
        if !status_schema_kind(wire.kind) || wire.fields.is_empty() || wire.fields.len() >= 255 {
            return Err("definition: unsupported kind or invalid effective field count");
        }
        Ok(Self {
            plan: SchemaPlan {
                kind: wire.kind,
                fields: wire.fields,
            },
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

pub(crate) fn positive(view: &mut View<'_>) -> Option<u32> {
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

/// Replay C/D/I/A/Z against the declared V13 base.
pub(crate) fn read(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<(EmbeddedSchema, usize)>, CodecError> {
    let mut view = View::over_retained(stream);
    let Some((kind, count)) = (|| {
        view.seek(offset)?;
        let kind = view.u16_be()?;
        if !status_schema_kind(kind) {
            return None;
        }
        Some((kind, view.u8()?))
    })() else {
        return Ok(None);
    };
    if count == 0 {
        return Ok(None);
    }
    let Some(plan) = read_definition(ctx, &mut view, BaseVersion::V13006, kind, count)? else {
        return Ok(None);
    };
    if plan
        .fields
        .iter()
        .any(|field| matches!(field.data_type, DataType::Unestablished(_)))
    {
        return Ok(None);
    }
    Ok(Some((EmbeddedSchema { plan }, view.position())))
}

fn read_definition(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    version: BaseVersion,
    kind: u16,
    count: u8,
) -> Result<Option<SchemaPlan>, CodecError> {
    let original = base::fields(version, kind);
    let mut scratch = ctx.reserve_scoped(0, "NX schema edit fields")?;
    let mut fields = Vec::new();
    if count == 255 {
        let Some(original) = original else {
            return Ok(None);
        };
        for field in original {
            ctx.push_scoped_vec(&mut scratch, &mut fields, *field, "NX schema edit fields")?;
        }
    } else if let Some(original) = original {
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
                ctx.push_scoped_vec(&mut scratch, &mut fields, field, "NX schema edit fields")?;
            }
        }
        if fields.len() != usize::from(count) {
            return Ok(None);
        }
    } else {
        if short_string(view).is_none() || short_string(view).is_none() {
            return Ok(None);
        }
        for _ in 0..count {
            let Some(field) = descriptor(view) else {
                return Ok(None);
            };
            ctx.push_scoped_vec(&mut scratch, &mut fields, field, "NX schema edit fields")?;
        }
    }
    own_definition(ctx, kind, &fields).map(Some)
}

fn own_definition(
    ctx: &DecodeContext<'_>,
    kind: u16,
    fields: &[BorrowedField<'_>],
) -> Result<SchemaPlan, CodecError> {
    let mut owned = Vec::new();
    for field in fields {
        let name = ctx.copy_retained_text(field.name, "NX schema field name")?;
        let name = PrintableString::new(name)
            .map_err(|_| CodecError::Malformed("invalid NX schema field name".into()))?;
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
    Ok(SchemaPlan {
        kind,
        fields: owned,
    })
}

#[cfg(test)]
mod tests;
