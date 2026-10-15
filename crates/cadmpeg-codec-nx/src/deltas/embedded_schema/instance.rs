// SPDX-License-Identifier: Apache-2.0
//! Status-framed first-node values selected by the effective field array.

use super::{DataType, EmbeddedSchema};
use crate::framing::read_and_advance;
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// One transmitted scalar or aggregate. Integer words preserve all source bits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(crate) enum FieldValue {
    Byte(u8),
    Char(u8),
    Logical(bool),
    Short(u16),
    Word(u16),
    Integer(u32),
    Pointer { target: NonZeroU32, status: bool },
    Real(FiniteReal),
    Interval(FiniteVector<2>),
    Vector(FiniteVector<3>),
    Box(FiniteVector<6>),
    Hvec(FiniteVector<11>),
}

impl FieldValue {
    fn data_type(&self) -> DataType {
        match self {
            Self::Byte(_) => DataType::Byte,
            Self::Char(_) => DataType::Char,
            Self::Logical(_) => DataType::Logical,
            Self::Short(_) => DataType::Short,
            Self::Word(_) => DataType::Word,
            Self::Integer(_) => DataType::Integer,
            Self::Pointer { .. } => DataType::Pointer,
            Self::Real(_) => DataType::Real,
            Self::Interval(_) => DataType::Interval,
            Self::Vector(_) => DataType::Vector,
            Self::Box(_) => DataType::Box,
            Self::Hvec(_) => DataType::Hvec,
        }
    }

    fn read(kind: DataType, stream: &[u8], view: &mut View<'_>) -> Option<Self> {
        Some(match kind {
            DataType::Byte => Self::Byte(view.u8()?),
            DataType::Char => Self::Char(view.u8()?),
            DataType::Logical => Self::Logical(match view.u8()? {
                0 => false,
                1 => true,
                _ => return None,
            }),
            DataType::Short => Self::Short(view.u16_be()?),
            DataType::Word => Self::Word(view.u16_be()?),
            DataType::Integer => Self::Integer(view.u32_be()?),
            DataType::Pointer => {
                let mut at = view.position();
                let target = NonZeroU32::new(read_and_advance(stream, &mut at)?)?;
                view.seek(at)?;
                let status = match view.u8()? {
                    0 => false,
                    1 => true,
                    _ => return None,
                };
                Self::Pointer { target, status }
            }
            DataType::Real => Self::Real(FiniteReal::new(view.f64_be()?)?),
            DataType::Interval => Self::Interval(vector(view)?),
            DataType::Vector => Self::Vector(vector(view)?),
            DataType::Box => Self::Box(vector(view)?),
            // Deltas hvec carriers retain the complete eleven-double state.
            DataType::Hvec => Self::Hvec(vector(view)?),
        })
    }
}

fn vector<const N: usize>(view: &mut View<'_>) -> Option<FiniteVector<N>> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = view.f64_be()?;
    }
    FiniteVector::new(values)
}

/// A schema edit and its first ordinary instance, with separate byte boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "NodeWire")]
pub(crate) struct SchemaNode {
    definition: EmbeddedSchema,
    schema_byte_len: usize,
    xmt: NonNullXmt,
    variable_length: Option<u32>,
    values: Vec<Vec<FieldValue>>,
}

#[derive(Deserialize)]
struct NodeWire {
    definition: EmbeddedSchema,
    schema_byte_len: usize,
    xmt: NonNullXmt,
    variable_length: Option<u32>,
    values: Vec<Vec<FieldValue>>,
}

impl TryFrom<NodeWire> for SchemaNode {
    type Error = &'static str;
    fn try_from(wire: NodeWire) -> Result<Self, Self::Error> {
        if wire.schema_byte_len < 3 || wire.definition.fields.is_empty() {
            return Err("definition: requires bounded nonempty schema metadata");
        }
        let variable = wire
            .definition
            .fields
            .iter()
            .any(|field| field.n_elements == 1);
        if variable != wire.variable_length.is_some()
            || wire.values.len() != wire.definition.fields.len()
        {
            return Err("values: effective field cardinality differs from definition");
        }
        for (field, values) in wire.definition.fields.iter().zip(&wire.values) {
            let count = if !field.transmit {
                0
            } else if field.n_elements == 1 {
                wire.variable_length.ok_or("variable_length: missing")?
            } else {
                field.n_elements.max(1)
            };
            if usize::try_from(count).ok() != Some(values.len())
                || values.iter().any(|v| v.data_type() != field.data_type)
            {
                return Err("values: field type or element count differs from definition");
            }
        }
        Ok(Self {
            definition: wire.definition,
            schema_byte_len: wire.schema_byte_len,
            xmt: wire.xmt,
            variable_length: wire.variable_length,
            values: wire.values,
        })
    }
}

impl SchemaNode {
    pub(crate) fn kind(&self) -> u16 {
        self.definition.kind
    }

    pub(crate) fn definition(&self) -> &EmbeddedSchema {
        &self.definition
    }

    /// Read metadata and exactly one node. Following records remain unread.
    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        stream: &[u8],
        offset: usize,
    ) -> Result<Option<(Self, usize)>, CodecError> {
        let Some((definition, schema_end)) = super::read(ctx, stream, offset)? else {
            return Ok(None);
        };
        let mut view = View::over_retained(stream);
        let Some((variable_length, xmt)) = (|| {
            view.seek(schema_end)?;
            let variable_length = if definition.fields.iter().any(|field| field.n_elements == 1) {
                Some(view.u32_be()?)
            } else {
                None
            };
            let mut at = view.position();
            let xmt = NonNullXmt::try_from(read_and_advance(stream, &mut at)?).ok()?;
            view.seek(at)?;
            Some((variable_length, xmt))
        })() else {
            return Ok(None);
        };
        let mut values = Vec::new();
        for field in &definition.fields {
            let count = if !field.transmit {
                0
            } else if field.n_elements == 1 {
                variable_length.unwrap_or(0)
            } else {
                field.n_elements.max(1)
            };
            // Each element consumes at least one byte, even before its type is read.
            if usize::try_from(count)
                .ok()
                .is_none_or(|n| n > stream.len() - view.position())
            {
                return Ok(None);
            }
            let mut elements = Vec::new();
            for _ in 0..count {
                ctx.charge_work(1, "read NX schema node field")?;
                let Some(value) = FieldValue::read(field.data_type, stream, &mut view) else {
                    return Ok(None);
                };
                ctx.push_vec(&mut elements, value, "NX schema node field elements")?;
            }
            ctx.push_vec(&mut values, elements, "NX schema node values")?;
        }
        Ok(Some((
            Self {
                definition,
                schema_byte_len: schema_end - offset,
                xmt,
                variable_length,
                values,
            },
            view.position(),
        )))
    }
}

#[cfg(test)]
mod tests;
