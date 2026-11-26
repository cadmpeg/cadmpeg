// SPDX-License-Identifier: Apache-2.0
//! Little-endian native binary transmits (`parasolid.md` §5).
//!
//! A native binary transmit has the node grammar of the neutral-binary
//! encoding with little-endian scalars and `u32` node indices. [`to_neutral`]
//! writes the same node sequence in the neutral-binary encoding of §1, so that
//! one node reader serves both encodings.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::records::{
    base_maximum, base_plan, in_base, read_edit_program, BaseVersion, DataType, SchemaPlan,
};

/// First byte of a native binary transmit.
pub const LEAD: u8 = 0x41;

/// Node type of the terminator.
const TERMINATOR: u16 = 1;

/// Edit-program operation that ends a first-use declaration.
const END_PROGRAM: u8 = b'Z';

/// Effective-field count byte that declares the base field list.
const BASE_FIELDS: u8 = 0xff;

fn malformed(at: usize, message: &str) -> CodecError {
    CodecError::malformed(format_args!(
        "Parasolid native transmit at byte {at}: {message}"
    ))
}

/// The neutral-binary transmit header fields of §1.
struct Header {
    version: BaseVersion,
    maximum: u16,
    embedded: bool,
}

/// Rewrites a native binary transmit as the neutral-binary encoding of §1.
///
/// # Errors
///
/// Returns [`CodecError::Malformed`] when the transmit header, a first-use
/// declaration, or a node does not follow the native grammar, when the
/// header states user fields, or when a node index exceeds the neutral
/// pointer range.
pub fn to_neutral(bytes: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut view = View::over_retained(bytes);
    let mut out = Vec::with_capacity(bytes.len());
    let header = header(&mut view, &mut out)?;
    let mut schemas = BTreeMap::<u16, SchemaPlan>::new();
    loop {
        let start = view.position();
        let kind = view
            .u16_le()
            .ok_or_else(|| malformed(start, "the node sequence has no terminator"))?;
        out.extend_from_slice(&kind.to_be_bytes());
        if kind == TERMINATOR {
            if view.u32_le() != Some(0) {
                return Err(malformed(start, "the terminator index is not null"));
            }
            pointer(&mut out, 0, start)?;
            out.extend_from_slice(view.unread());
            return Ok(out);
        }
        if kind == 0 || kind > header.maximum {
            return Err(malformed(start, "node type is outside the schema"));
        }
        let plan = match schemas.entry(kind) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let plan = if header.embedded {
                    let count = view
                        .u8()
                        .ok_or_else(|| malformed(start, "first-use declaration is truncated"))?;
                    out.push(count);
                    let mut declared = Vec::new();
                    declaration(&mut view, &mut declared, header.version, kind, count)?;
                    let plan = read_edit_program(
                        &mut View::over_retained(&declared),
                        header.version,
                        kind,
                        count,
                    )?
                    .map(|program| program.plan().clone());
                    out.extend_from_slice(&declared);
                    plan
                } else {
                    base_plan(header.version, kind)?
                };
                entry.insert(
                    plan.ok_or_else(|| malformed(start, "first-use declaration is invalid"))?,
                )
            }
        };
        let variable = if plan.fields.iter().any(|field| field.n_elements == 1) {
            let count = view
                .u32_le()
                .ok_or_else(|| malformed(start, "variable count is truncated"))?;
            out.extend_from_slice(&count.to_be_bytes());
            count
        } else {
            0
        };
        let index = view
            .u32_le()
            .ok_or_else(|| malformed(start, "node index is truncated"))?;
        if index == 0 {
            return Err(malformed(start, "node index is null"));
        }
        pointer(&mut out, index, start)?;
        for field in plan.fields.iter().filter(|field| field.transmit) {
            let count = if field.n_elements == 1 {
                variable
            } else {
                field.n_elements.max(1)
            };
            for _ in 0..count {
                element(&mut view, &mut out, field.data_type, start)?;
            }
        }
    }
}

/// Reads the native header and writes the neutral header.
fn header(view: &mut View<'_>, out: &mut Vec<u8>) -> Result<Header, CodecError> {
    let truncated = || malformed(0, "transmit header is truncated");
    if view.u8() != Some(LEAD) {
        return Err(malformed(0, "lead byte is not 0x41"));
    }
    let description = usize::from(view.u16_le().ok_or_else(truncated)?);
    let description = view.take(description).ok_or_else(truncated)?;
    let schema = usize::try_from(view.u32_le().ok_or_else(truncated)?).map_err(|_| truncated())?;
    let schema = view.take(schema).ok_or_else(truncated)?;
    let token = crate::find_schema_token(schema)
        .filter(|token| token.value().as_bytes() == schema)
        .ok_or_else(|| malformed(0, "schema declaration is invalid"))?;
    let embedded = match token.value().split('_').count() {
        3 => false,
        4 => true,
        _ => return Err(malformed(0, "schema token has no modeller and schema")),
    };
    let version = match token.value().rsplit('_').next() {
        Some("11004") => BaseVersion::V11004,
        Some("13006") => BaseVersion::V13006,
        _ => return Err(malformed(0, "schema token names an unknown base schema")),
    };
    out.extend_from_slice(b"PS");
    for text in [description, schema] {
        let length = u32::try_from(text.len()).map_err(|_| truncated())?;
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(text);
    }
    let maximum = if embedded {
        let maximum = view.u16_le().ok_or_else(truncated)?;
        out.extend_from_slice(&maximum.to_be_bytes());
        maximum
    } else {
        base_maximum(version)
    };
    if view.u32_le().ok_or_else(truncated)? != 0 {
        return Err(malformed(0, "user-field size is nonzero"));
    }
    out.extend_from_slice(&0_u32.to_be_bytes());
    Ok(Header {
        version,
        maximum,
        embedded,
    })
}

/// Writes node index `value` (`0` is null) as a neutral biased pointer.
fn pointer(out: &mut Vec<u8>, value: u32, at: usize) -> Result<(), CodecError> {
    value
        .checked_add(1)
        .and_then(|encoded| crate::pointer::write(out, encoded))
        .ok_or_else(|| malformed(at, "node index exceeds the neutral pointer range"))
}

/// Copies one short string: a byte count and that many bytes.
fn short_string(view: &mut View<'_>, out: &mut Vec<u8>, at: usize) -> Result<(), CodecError> {
    let count = view
        .u8()
        .ok_or_else(|| malformed(at, "short string is truncated"))?;
    let text = view
        .take(usize::from(count))
        .ok_or_else(|| malformed(at, "short string is truncated"))?;
    out.push(count);
    out.extend_from_slice(text);
    Ok(())
}

/// Rewrites one first-use declaration after its effective-field count byte.
fn declaration(
    view: &mut View<'_>,
    out: &mut Vec<u8>,
    version: BaseVersion,
    kind: u16,
    count: u8,
) -> Result<(), CodecError> {
    let at = view.position();
    if count == BASE_FIELDS {
        return Ok(());
    }
    if in_base(version, kind) {
        loop {
            let operation = view
                .u8()
                .ok_or_else(|| malformed(at, "edit program is truncated"))?;
            out.push(operation);
            match operation {
                END_PROGRAM => return Ok(()),
                b'C' | b'D' => {}
                b'I' | b'A' => descriptor(view, out, at)?,
                _ => return Err(malformed(at, "edit program holds an unknown operation")),
            }
        }
    }
    short_string(view, out, at)?;
    short_string(view, out, at)?;
    for _ in 0..count {
        descriptor(view, out, at)?;
    }
    Ok(())
}

/// Rewrites one field descriptor: name, pointer class, extent, type, and
/// transmit flag.
fn descriptor(view: &mut View<'_>, out: &mut Vec<u8>, at: usize) -> Result<(), CodecError> {
    short_string(view, out, at)?;
    let class = view
        .u16_le()
        .ok_or_else(|| malformed(at, "field descriptor is truncated"))?;
    out.extend_from_slice(&class.to_be_bytes());
    let extent = view
        .u32_le()
        .ok_or_else(|| malformed(at, "field descriptor is truncated"))?;
    pointer(out, extent, at)?;
    if class == 0 {
        short_string(view, out, at)?;
    }
    if extent == 1 {
        let transmit = view
            .u8()
            .ok_or_else(|| malformed(at, "field descriptor is truncated"))?;
        out.push(transmit);
    }
    Ok(())
}

/// Rewrites one field element of `data_type`.
fn element(
    view: &mut View<'_>,
    out: &mut Vec<u8>,
    data_type: DataType,
    at: usize,
) -> Result<(), CodecError> {
    let truncated = || malformed(at, "node field is truncated");
    let (words, width) = match data_type {
        DataType::Pointer => {
            let value = view.u32_le().ok_or_else(truncated)?;
            return pointer(out, value, at);
        }
        DataType::Byte | DataType::Char | DataType::Logical => (1, 1),
        DataType::Short | DataType::Word => (1, 2),
        DataType::Integer => (1, 4),
        DataType::Real => (1, 8),
        DataType::Interval => (2, 8),
        DataType::Vector | DataType::Hvec => (3, 8),
        DataType::Box => (6, 8),
        DataType::Unestablished(_) => {
            return Err(malformed(at, "node field has no established width"))
        }
    };
    for _ in 0..words {
        let word = view.take(width).ok_or_else(truncated)?;
        out.extend(word.iter().rev());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
