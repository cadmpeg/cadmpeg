// SPDX-License-Identifier: Apache-2.0
//! NX OM registry-token framing.

use super::{FieldDefinition, TypeDefinition};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::num::NonZeroU32;

const FIELD_START_PROBE_LIMIT: usize = 256;

/// Encoding family of a value in an OM registry declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryTokenForm {
    /// One direct byte in `00..7f`.
    Direct,
    /// `80..8f` followed by one low byte, with a one-based decoded value.
    Compact,
    /// `90`, `a0..af`, or `f1` followed by a big-endian `u16`, with a
    /// one-based decoded value.
    Wide,
}

/// A decoded registry value retains the encoding family that bounded it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegistryToken(RegistryValue);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryValue {
    Direct(u8),
    Compact(u16),
    Wide(u32),
}

impl RegistryToken {
    pub(crate) fn value(self) -> u32 {
        match self.0 {
            RegistryValue::Direct(value) => u32::from(value),
            RegistryValue::Compact(value) => u32::from(value),
            RegistryValue::Wide(value) => value,
        }
    }

    fn form(self) -> RegistryTokenForm {
        match self.0 {
            RegistryValue::Direct(_) => RegistryTokenForm::Direct,
            RegistryValue::Compact(_) => RegistryTokenForm::Compact,
            RegistryValue::Wide(_) => RegistryTokenForm::Wide,
        }
    }

    fn width(self) -> usize {
        match self.form() {
            RegistryTokenForm::Direct => 1,
            RegistryTokenForm::Compact => 2,
            RegistryTokenForm::Wide => 3,
        }
    }
}

/// Complete class-registry tail following one `UGS::` class name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassRegistryLayout {
    /// Registry storage code.
    pub(crate) storage_code: RegistryToken,
    /// One-based base-class ordinal; absent for the root.
    pub(crate) base_class: Option<NonZeroU32>,
    /// Eight-byte member-layout fingerprint.
    pub(crate) schema_fingerprint: [u8; 8],
    /// Registry reference-list ordinal.
    pub(crate) reference: NonZeroU32,
}

/// Complete member-registry head following one member name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldRegistryLayout {
    /// Registry storage code.
    pub(crate) storage_code: RegistryToken,
    /// One-based declaring-class ordinal.
    pub(crate) owner_class: NonZeroU32,
}

/// Class declarations and the first byte of the following member registry.
///
/// The boundary is retained because the member registry follows the class
/// registry, while the class list itself does not contain the reference-list
/// declarations that precede it.
pub(super) struct TypeRegistry<'a> {
    pub(super) definitions: Vec<TypeDefinition<'a>>,
    pub(super) field_start: usize,
}

#[derive(Debug, Clone, Copy)]
struct RegistryDeclaration<'a> {
    offset: usize,
    name: &'a str,
}

impl RegistryDeclaration<'_> {
    fn name_end(self) -> usize {
        self.offset + 1 + self.name.len()
    }
}

fn registry_token_at(tail: &[u8], offset: usize) -> Option<RegistryToken> {
    let prefix = *tail.get(offset)?;
    let value = match prefix {
        0x00..=0x7f => RegistryValue::Direct(prefix),
        0x80..=0x8f => {
            let low = *tail.get(offset + 1)?;
            RegistryValue::Compact(u16::from(prefix - 0x80) * 256 + u16::from(low) + 1)
        }
        0x90 | 0xa0..=0xaf | 0xf1 => {
            let high = u32::from(*tail.get(offset + 1)?);
            let low = u32::from(*tail.get(offset + 2)?);
            RegistryValue::Wide(((u32::from(prefix & 0x0f) << 16) | (high << 8) | low) + 1)
        }
        _ => return None,
    };
    Some(RegistryToken(value))
}

pub(crate) fn class_registry_layout(tail: &[u8]) -> Option<ClassRegistryLayout> {
    let (layout, end) = class_registry_layout_at(tail, 0, tail.len())?;
    (end == tail.len()).then_some(layout)
}

pub(crate) fn field_registry_layout(tail: &[u8]) -> Option<FieldRegistryLayout> {
    let storage_code = registry_token_at(tail, 0)?;
    let owner = registry_token_at(tail, storage_code.width())?;
    Some(FieldRegistryLayout {
        storage_code,
        owner_class: NonZeroU32::new(owner.value())?,
    })
}

fn registry_declaration_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    at: usize,
    end: usize,
    prefix: &[u8],
) -> Result<Option<RegistryDeclaration<'a>>, CodecError> {
    let Some(declared) = bytes.get(at).copied().map(usize::from) else {
        return Ok(None);
    };
    let Some(name_len) = declared.checked_sub(1) else {
        return Ok(None);
    };
    let Some(name_start) = at.checked_add(1) else {
        return Ok(None);
    };
    let Some(name_end) = name_start.checked_add(name_len) else {
        return Ok(None);
    };
    let Some(raw) = bytes.get(name_start..name_end) else {
        return Ok(None);
    };
    if name_end >= end
        || !raw.starts_with(prefix)
        || !ctx.all_by(
            raw,
            |byte| Ok((0x20..0x7f).contains(byte)),
            "NX registry declaration syntax",
        )?
    {
        return Ok(None);
    }
    let Ok(name) = ctx.validate_utf8(raw, "NX registry declaration UTF-8 validation")? else {
        return Ok(None);
    };
    Ok(Some(RegistryDeclaration { offset: at, name }))
}

fn class_registry_layout_at(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<(ClassRegistryLayout, usize)> {
    let storage_code = registry_token_at(bytes.get(at..end)?, 0)?;
    let base_at = at.checked_add(storage_code.width())?;
    let base = registry_token_at(bytes.get(base_at..end)?, 0)?;
    let fingerprint_at = base_at.checked_add(base.width())?;
    let fingerprint_end = fingerprint_at.checked_add(8)?;
    let fingerprint = bytes
        .get(fingerprint_at..fingerprint_end)?
        .try_into()
        .ok()?;
    let reference_at = fingerprint_end;
    let reference = registry_token_at(bytes.get(reference_at..end)?, 0)?;
    let tail_end = reference_at.checked_add(reference.width())?;
    Some((
        ClassRegistryLayout {
            storage_code,
            base_class: NonZeroU32::new(base.value()),
            schema_fingerprint: fingerprint,
            reference: NonZeroU32::new(reference.value())?,
        },
        tail_end,
    ))
}

fn complete_type_registry_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    first: usize,
    end: usize,
) -> Result<Option<TypeRegistry<'a>>, CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "nx complete type registry")?;
    let mut at = first;
    loop {
        ctx.charge_work(1, "NX reference registry traversal")?;
        let Some(declaration) = registry_declaration_at(ctx, bytes, at, end, b"UGS::")? else {
            return Ok(None);
        };
        at = declaration.name_end() + 1;
        match bytes.get(at) {
            Some(0x01) => {
                at += 1;
                break;
            }
            Some(_) if registry_declaration_at(ctx, bytes, at, end, b"UGS::")?.is_some() => {}
            _ => return Ok(None),
        }
    }

    let mut definitions = Vec::new();
    loop {
        ctx.charge_work(1, "NX class registry traversal")?;
        if let Some(field_start) = field_registry_start(ctx, bytes, at, end)? {
            reservation.commit()?;
            return Ok(Some(TypeRegistry {
                definitions,
                field_start,
            }));
        }
        let Some(declaration) = registry_declaration_at(ctx, bytes, at, end, b"UGS::")? else {
            return Ok(None);
        };
        let Some((_, tail_end)) = class_registry_layout_at(bytes, declaration.name_end(), end)
        else {
            return Ok(None);
        };
        let Some(registry_tail) = bytes.get(declaration.name_end()..tail_end) else {
            return Ok(None);
        };
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut definitions,
            1,
            "nx complete type registry",
        )?;
        definitions.push(TypeDefinition {
            offset: declaration.offset,
            name: declaration.name,
            registry_tail,
        });
        at = tail_end;
        if field_registry_start(ctx, bytes, at, end)?.is_none()
            && registry_declaration_at(ctx, bytes, at, end, b"UGS::")?.is_none()
        {
            return Ok(None);
        }
    }
}

fn field_registry_start(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    if bytes.get(at) == Some(&0x02) {
        return Ok(at.checked_add(1));
    }
    if registry_declaration_at(ctx, bytes, at, end, b"UGS::")?.is_some() {
        return Ok(None);
    }
    for gap in 0..=1 {
        let Some(candidate) = at.checked_add(gap) else {
            continue;
        };
        if registry_declaration_at(ctx, bytes, candidate, end, b"UGS::")?.is_some() {
            continue;
        }
        let Some(probe_end) = candidate
            .checked_add(FIELD_START_PROBE_LIMIT)
            .map(|probe| probe.min(end))
        else {
            continue;
        };
        for probe in candidate..probe_end {
            if registry_declaration_at(ctx, bytes, probe, end, b"UGS::")?.is_some() {
                break;
            }
            if field_definition_at(ctx, bytes, probe, end)?.is_some() {
                return Ok(Some(probe));
            }
        }
    }
    Ok(None)
}

/// Parse the complete reference/class registry when its explicit terminators
/// and class tails are present. Older or partial layouts use the historical
/// scanner and retain its exact suffix bytes.
pub(super) fn type_registry<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
    end: usize,
) -> Result<TypeRegistry<'a>, CodecError> {
    end.checked_sub(start)
        .ok_or_else(|| ctx.refuse_codec_limit("nx type registry range", 0, 1))?;
    if let Some(registry) = ctx.find_map(
        start..end,
        |at| {
            if registry_declaration_at(ctx, bytes, at, end, b"UGS::")?.is_none() {
                return Ok(None);
            }
            complete_type_registry_at(ctx, bytes, at, end)
        },
        "nx type registry scan",
    )? {
        return Ok(registry);
    }

    let definitions = legacy_type_definitions(ctx, bytes, start, end)?;
    let field_start = definitions.last().map_or(start, |definition| {
        definition.offset + definition.name.len() + 2
    });
    Ok(TypeRegistry {
        definitions,
        field_start,
    })
}

fn legacy_type_definitions<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
    end: usize,
) -> Result<Vec<TypeDefinition<'a>>, CodecError> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        ctx.charge_work(1, "NX legacy type registry traversal")?;
        if let Some(declaration) = registry_declaration_at(ctx, bytes, at, end, b"UGS::")? {
            let name_end = declaration.name_end();
            ctx.reserve_vec(&mut out, 1, "nx legacy type definitions")?;
            out.push(TypeDefinition {
                offset: declaration.offset,
                name: declaration.name,
                registry_tail: &bytes[name_end..=name_end],
            });
            at = name_end + 1;
        } else {
            at += 1;
        }
    }
    if let Some(range_end) = out.len().checked_sub(1) {
        for index in ctx.admit_iter(
            &(0..range_end),
            "NX legacy type definitions range traversal",
        )? {
            let tail_start = out[index].offset + out[index].name.len() + 1;
            let tail_end = out[index + 1].offset;
            out[index].registry_tail = &bytes[tail_start..tail_end];
        }
    }
    Ok(out)
}

pub(super) fn field_definitions<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
    end: usize,
) -> Result<Vec<FieldDefinition<'a>>, CodecError> {
    let mut out = Vec::new();
    let mut search = start;
    let mut limit = start
        .checked_add(FIELD_START_PROBE_LIMIT)
        .ok_or_else(|| CodecError::Malformed("NX field search offset overflow".into()))?
        .min(end);
    while search < limit {
        ctx.charge_work(1, "NX field registry window traversal")?;
        let mut found = None;
        for at in search..limit {
            if let Some(definition) = field_definition_at(ctx, bytes, at, end)? {
                found = Some((definition, at));
                break;
            }
        }
        let Some((definition, at)) = found else {
            break;
        };
        let next = at + definition.name.len() + 2;
        search = next;
        limit = search
            .checked_add(FIELD_START_PROBE_LIMIT)
            .ok_or_else(|| CodecError::Malformed("NX field search offset overflow".into()))?
            .min(end);
        ctx.reserve_vec(&mut out, 1, "nx field definitions")?;
        out.push(definition);
    }
    bound_field_registry_tails(ctx, bytes, &mut out)?;
    Ok(out)
}

pub(super) fn all_field_definitions<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
    end: usize,
) -> Result<Vec<FieldDefinition<'a>>, CodecError> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        ctx.charge_work(1, "NX complete field registry traversal")?;
        if let Some(definition) = field_definition_at(ctx, bytes, at, end)? {
            at += definition.name.len() + 2;
            ctx.reserve_vec(&mut out, 1, "nx all field definitions")?;
            out.push(definition);
        } else {
            at += 1;
        }
    }
    bound_field_registry_tails(ctx, bytes, &mut out)?;
    Ok(out)
}

fn bound_field_registry_tails<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    definitions: &mut [FieldDefinition<'a>],
) -> Result<(), CodecError> {
    if let Some(last) = definitions.len().checked_sub(1) {
        for index in ctx.admit_iter(&(0..last), "NX field registry tail traversal")? {
            let tail_start = definitions[index].offset + definitions[index].name.len() + 1;
            let tail_end = definitions[index + 1].offset;
            definitions[index].registry_tail = &bytes[tail_start..tail_end];
        }
    }
    Ok(())
}

fn field_definition_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    at: usize,
    end: usize,
) -> Result<Option<FieldDefinition<'a>>, CodecError> {
    let Some(declaration) = registry_declaration_at(ctx, bytes, at, end, b"m_")? else {
        return Ok(None);
    };
    let name_end = declaration.name_end();
    Ok(Some(FieldDefinition {
        offset: declaration.offset,
        name: declaration.name,
        registry_tail: &bytes[name_end..=name_end],
    }))
}

#[cfg(test)]
mod tests;
