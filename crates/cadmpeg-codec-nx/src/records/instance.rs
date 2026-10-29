// SPDX-License-Identifier: Apache-2.0

use super::{positive, DataType, SchemaPlan};
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RealBits(u64);

impl Serialize for RealBits {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{:016x}", self.0))
    }
}

impl<'de> Deserialize<'de> for RealBits {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() != 16
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom(
                "real bits require 16 lowercase hexadecimal digits",
            ));
        }
        u64::from_str_radix(&value, 16)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(crate) enum FieldValue {
    Byte(u8),
    Char(u8),
    Logical(bool),
    Short(u16),
    Word(u16),
    Integer(u32),
    Pointer { target: NonZeroU32, status: bool },
    RealBits(RealBits),
    IntervalBits([RealBits; 2]),
    VectorBits([RealBits; 3]),
    BoxBits([RealBits; 6]),
    HvecBits([RealBits; 11]),
}

impl FieldValue {
    pub(crate) fn data_type(&self) -> DataType {
        match self {
            Self::Byte(_) => DataType::Byte,
            Self::Char(_) => DataType::Char,
            Self::Logical(_) => DataType::Logical,
            Self::Short(_) => DataType::Short,
            Self::Word(_) => DataType::Word,
            Self::Integer(_) => DataType::Integer,
            Self::Pointer { .. } => DataType::Pointer,
            Self::RealBits(_) => DataType::Real,
            Self::IntervalBits(_) => DataType::Interval,
            Self::VectorBits(_) => DataType::Vector,
            Self::BoxBits(_) => DataType::Box,
            Self::HvecBits(_) => DataType::Hvec,
        }
    }

    pub(crate) fn encoded_len(&self) -> u64 {
        match self {
            Self::Byte(_) | Self::Char(_) | Self::Logical(_) => 1,
            Self::Short(_) | Self::Word(_) => 2,
            Self::Integer(_) => 4,
            Self::Pointer { target, .. } => {
                if target.get() <= 32_767 {
                    3
                } else {
                    5
                }
            }
            Self::RealBits(_) => 8,
            Self::IntervalBits(_) => 16,
            Self::VectorBits(_) => 24,
            Self::BoxBits(_) => 48,
            Self::HvecBits(_) => 88,
        }
    }

    pub(crate) fn read(kind: DataType, view: &mut View<'_>) -> Option<Self> {
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
                let target = NonZeroU32::new(positive(view)?.checked_add(1)?)?;
                let status = match view.u8()? {
                    0 => false,
                    1 => true,
                    _ => return None,
                };
                Self::Pointer { target, status }
            }
            DataType::Real => Self::RealBits(RealBits(view.u64_be()?)),
            DataType::Interval => Self::IntervalBits(real_bits(view)?),
            DataType::Vector => Self::VectorBits(real_bits(view)?),
            DataType::Box => Self::BoxBits(real_bits(view)?),
            DataType::Hvec => Self::HvecBits(real_bits(view)?),
            DataType::Unestablished(_) => return None,
        })
    }
}

fn real_bits<const N: usize>(view: &mut View<'_>) -> Option<[RealBits; N]> {
    let mut values = [RealBits(0); N];
    for value in &mut values {
        *value = RealBits(view.u64_be()?);
    }
    Some(values)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StatusInstance {
    pub(super) identity_at: usize,
    pub(super) xmt: NonNullXmt,
    pub(super) variable_length: Option<u32>,
    pub(super) fields: Vec<Range<usize>>,
}

pub(super) fn read_status_instance(
    view: &mut View<'_>,
    plan: &SchemaPlan,
) -> Option<StatusInstance> {
    let variable_length = if plan.fields.iter().any(|field| field.n_elements == 1) {
        Some(view.u32_be()?)
    } else {
        None
    };
    let identity_at = view.position();
    let xmt = NonNullXmt::try_from(positive(view)?.checked_add(1)?).ok()?;
    let mut fields = Vec::new();
    for field in &plan.fields {
        let start = view.position();
        let count = if !field.transmit {
            0
        } else if field.n_elements == 1 {
            variable_length.unwrap_or(0)
        } else {
            field.n_elements.max(1)
        };
        let count = usize::try_from(count)
            .ok()
            .filter(|count| *count <= view.remaining())?;
        for _ in 0..count {
            FieldValue::read(field.data_type, view)?;
        }
        fields.push(start..view.position());
    }
    Some(StatusInstance {
        identity_at,
        xmt,
        variable_length,
        fields,
    })
}

#[cfg(test)]
mod tests;
