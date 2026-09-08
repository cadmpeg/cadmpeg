// SPDX-License-Identifier: Apache-2.0
use super::{parse_value, SpannedValue, ValueKind};
use rmp::Marker;

fn dimension_field(payload: &[u8], offset: u64, field: &str) -> Result<SpannedValue, String> {
    let invalid = || format!("DimSemData {field} has an invalid patch field");
    let mut cursor = usize::try_from(offset).map_err(|_| invalid())?;
    let outer = parse_value(payload, &mut cursor, 0).ok_or_else(invalid)?;
    let ValueKind::Map(outer) = outer.kind else {
        return Err(invalid());
    };
    let ValueKind::Array(items) = &outer.get("dimItems").ok_or_else(invalid)?.kind else {
        return Err(invalid());
    };
    let ValueKind::Map(item) = &items.first().ok_or_else(invalid)?.kind else {
        return Err(invalid());
    };
    item.get(field).cloned().ok_or_else(invalid)
}

pub(super) struct FloatPatchSlot<'a>(&'a mut [u8; 8]);

impl<'a> FloatPatchSlot<'a> {
    pub(super) fn read(payload: &'a mut [u8], offset: u64) -> Result<Self, String> {
        let invalid = || "DimSemData value has an invalid f64 patch slot".to_string();
        let value = dimension_field(payload, offset, "value")?;
        if payload.get(value.start) != Some(&Marker::F64.to_u8()) {
            return Err(invalid());
        }
        let bytes = payload
            .get_mut(value.data_offset..)
            .and_then(|bytes| bytes.get_mut(..8))
            .ok_or_else(invalid)?;
        Ok(Self(bytes.try_into().map_err(|_| invalid())?))
    }

    pub(super) fn write(self, value: f64) {
        *self.0 = value.to_be_bytes();
    }
}

pub(super) struct BooleanPatchSlot<'a>(&'a mut u8);

impl<'a> BooleanPatchSlot<'a> {
    pub(super) fn read(payload: &'a mut [u8], offset: u64, field: &str) -> Result<Self, String> {
        let invalid = || format!("DimSemData {field} has an invalid boolean patch slot");
        let value = dimension_field(payload, offset, field)?;
        if !matches!(value.kind, ValueKind::Bool(_)) {
            return Err(invalid());
        }
        Ok(Self(
            payload.get_mut(value.data_offset).ok_or_else(invalid)?,
        ))
    }

    pub(super) fn write(self, value: bool) {
        *self.0 = if value { 0xc3 } else { 0xc2 };
    }
}

enum IntegerEncoding<'a> {
    Fix(&'a mut u8),
    U8(&'a mut [u8; 1]),
    U16(&'a mut [u8; 2]),
    U32(&'a mut [u8; 4]),
    U64(&'a mut [u8; 8]),
    I8(&'a mut [u8; 1]),
    I16(&'a mut [u8; 2]),
    I32(&'a mut [u8; 4]),
    I64(&'a mut [u8; 8]),
}

pub(super) struct IntegerPatchSlot<'a>(IntegerEncoding<'a>);

impl<'a> IntegerPatchSlot<'a> {
    pub(super) fn read(payload: &'a mut [u8], offset: u64, field: &str) -> Result<Self, String> {
        let invalid = || format!("DimSemData {field} has an invalid integer patch slot");
        let value = dimension_field(payload, offset, field)?;
        let marker = Marker::from_u8(*payload.get(value.start).ok_or_else(invalid)?);
        let bytes = payload.get_mut(value.data_offset..).ok_or_else(invalid)?;
        macro_rules! slot {
            ($variant:ident, $width:literal) => {
                IntegerEncoding::$variant(
                    bytes
                        .get_mut(..$width)
                        .ok_or_else(invalid)?
                        .try_into()
                        .map_err(|_| invalid())?,
                )
            };
        }
        Ok(Self(match marker {
            Marker::FixPos(_) | Marker::FixNeg(_) => {
                IntegerEncoding::Fix(bytes.first_mut().ok_or_else(invalid)?)
            }
            Marker::U8 => slot!(U8, 1),
            Marker::U16 => slot!(U16, 2),
            Marker::U32 => slot!(U32, 4),
            Marker::U64 => slot!(U64, 8),
            Marker::I8 => slot!(I8, 1),
            Marker::I16 => slot!(I16, 2),
            Marker::I32 => slot!(I32, 4),
            Marker::I64 => slot!(I64, 8),
            _ => return Err(invalid()),
        }))
    }

    pub(super) fn write(self, value: i64) -> Result<(), String> {
        let invalid = || "DimSemData valPrecision exceeds its integer encoding".to_string();
        macro_rules! write {
            ($bytes:ident, $ty:ty) => {
                *$bytes = <$ty>::try_from(value).map_err(|_| invalid())?.to_be_bytes()
            };
        }
        match self.0 {
            IntegerEncoding::Fix(byte) => {
                if !(-32..=127).contains(&value) {
                    return Err(invalid());
                }
                *byte = value as u8;
            }
            IntegerEncoding::U8(bytes) => write!(bytes, u8),
            IntegerEncoding::U16(bytes) => write!(bytes, u16),
            IntegerEncoding::U32(bytes) => write!(bytes, u32),
            IntegerEncoding::U64(bytes) => write!(bytes, u64),
            IntegerEncoding::I8(bytes) => write!(bytes, i8),
            IntegerEncoding::I16(bytes) => write!(bytes, i16),
            IntegerEncoding::I32(bytes) => write!(bytes, i32),
            IntegerEncoding::I64(bytes) => *bytes = value.to_be_bytes(),
        }
        Ok(())
    }
}
