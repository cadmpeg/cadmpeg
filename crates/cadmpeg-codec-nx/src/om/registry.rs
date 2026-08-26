// SPDX-License-Identifier: Apache-2.0
//! NX OM registry-token framing.

use super::{FieldDefinition, IndexedDefinitionLayout, TypeDefinition};

/// Encoding family of a value in an OM registry declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryTokenForm {
    /// One direct byte in `00..7f`.
    Direct,
    /// `80..8f` followed by one low byte, with a one-based decoded value.
    Compact,
    /// `90`, `a0..af`, or `f1` followed by a big-endian `u16`, with a
    /// one-based decoded value.
    Wide,
}

/// One decoded token from a class or member registry declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegistryToken {
    /// Decoded registry value.
    pub value: u32,
    /// Encoding family selected by the leading byte.
    pub form: RegistryTokenForm,
    /// Serialized token width in bytes.
    pub width: usize,
}

/// Complete class-registry tail following one `UGS::` class name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassRegistryLayout {
    /// Registry storage code.
    pub storage_code: RegistryToken,
    /// One-based base-class ordinal, or zero for the root.
    pub base_class: u32,
    /// Eight-byte member-layout fingerprint.
    pub schema_fingerprint: [u8; 8],
    /// Registry reference-list ordinal.
    pub reference: u32,
}

/// Complete member-registry head following one member name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldRegistryLayout {
    /// Registry storage code.
    pub storage_code: RegistryToken,
    /// One-based declaring-class ordinal.
    pub owner_class: u32,
}

fn registry_byte(first: u8, suffix: &[u8], offset: usize) -> Option<u8> {
    if offset == 0 {
        Some(first)
    } else {
        suffix.get(offset - 1).copied()
    }
}

pub(crate) fn registry_token_at(first: u8, suffix: &[u8], offset: usize) -> Option<RegistryToken> {
    let prefix = registry_byte(first, suffix, offset)?;
    match prefix {
        0x00..=0x7f => Some(RegistryToken {
            value: u32::from(prefix),
            form: RegistryTokenForm::Direct,
            width: 1,
        }),
        0x80..=0x8f => {
            let low = registry_byte(first, suffix, offset + 1)?;
            Some(RegistryToken {
                value: u32::from(prefix - 0x80) * 256 + u32::from(low) + 1,
                form: RegistryTokenForm::Compact,
                width: 2,
            })
        }
        0x90 | 0xa0..=0xaf | 0xf1 => {
            let high = u32::from(registry_byte(first, suffix, offset + 1)?);
            let low = u32::from(registry_byte(first, suffix, offset + 2)?);
            Some(RegistryToken {
                value: ((u32::from(prefix & 0x0f) << 16) | (high << 8) | low) + 1,
                form: RegistryTokenForm::Wide,
                width: 3,
            })
        }
        _ => None,
    }
}

pub(crate) fn class_registry_layout(first: u8, suffix: &[u8]) -> Option<ClassRegistryLayout> {
    let total = suffix.len().checked_add(1)?;
    let storage_code = registry_token_at(first, suffix, 0)?;
    let base_offset = storage_code.width;
    let base = registry_token_at(first, suffix, base_offset)?;
    let fingerprint_offset = base_offset.checked_add(base.width)?;
    let fingerprint_end = fingerprint_offset.checked_add(8)?;
    if fingerprint_end > total {
        return None;
    }
    let mut schema_fingerprint = [0; 8];
    for (index, byte) in schema_fingerprint.iter_mut().enumerate() {
        *byte = registry_byte(first, suffix, fingerprint_offset + index)?;
    }
    let reference = registry_token_at(first, suffix, fingerprint_end)?;
    (fingerprint_end
        .checked_add(reference.width)
        .is_some_and(|end| end == total)
        && reference.value != 0)
        .then_some(ClassRegistryLayout {
            storage_code,
            base_class: base.value,
            schema_fingerprint,
            reference: reference.value,
        })
}

pub(crate) fn field_registry_layout(first: u8, suffix: &[u8]) -> Option<FieldRegistryLayout> {
    let storage_code = registry_token_at(first, suffix, 0)?;
    let owner = registry_token_at(first, suffix, storage_code.width)?;
    (owner.value != 0).then_some(FieldRegistryLayout {
        storage_code,
        owner_class: owner.value,
    })
}

pub(super) fn materialize_type_definition<'a>(
    bytes: &'a [u8],
    layout: &IndexedDefinitionLayout,
) -> TypeDefinition<'a> {
    let name_start = layout.offset + 1;
    let name_end = name_start + layout.name_len;
    let name = std::str::from_utf8(
        bytes
            .get(name_start..name_end)
            .expect("cached indexed declaration name remains in source"),
    )
    .expect("cached indexed declaration name remains UTF-8");
    TypeDefinition {
        offset: layout.offset,
        name,
        trailing_code: layout.trailing_code,
        registry_suffix: materialize_registry_suffix(bytes, layout.registry_suffix),
    }
}

pub(super) fn materialize_field_definition<'a>(
    bytes: &'a [u8],
    layout: &IndexedDefinitionLayout,
) -> FieldDefinition<'a> {
    let name_start = layout.offset + 1;
    let name_end = name_start + layout.name_len;
    let name = std::str::from_utf8(
        bytes
            .get(name_start..name_end)
            .expect("cached indexed declaration name remains in source"),
    )
    .expect("cached indexed declaration name remains UTF-8");
    FieldDefinition {
        offset: layout.offset,
        name,
        trailing_code: layout.trailing_code,
        registry_suffix: materialize_registry_suffix(bytes, layout.registry_suffix),
    }
}

fn materialize_registry_suffix(bytes: &[u8], range: Option<super::IndexedByteRange>) -> &[u8] {
    range.map_or(&bytes[..0], |range| {
        bytes
            .get(range.start..range.end)
            .expect("cached indexed registry suffix remains in source")
    })
}

pub(super) fn type_definition_layouts(
    definitions: &[TypeDefinition<'_>],
) -> Vec<IndexedDefinitionLayout> {
    definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| IndexedDefinitionLayout {
            offset: definition.offset,
            name_len: definition.name.len(),
            trailing_code: definition.trailing_code,
            registry_suffix: (index + 1 < definitions.len()).then(|| super::IndexedByteRange {
                start: definition.offset + definition.name.len() + 2,
                end: definitions[index + 1].offset,
            }),
        })
        .collect()
}

pub(super) fn field_definition_layouts(
    definitions: &[FieldDefinition<'_>],
) -> Vec<IndexedDefinitionLayout> {
    definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| IndexedDefinitionLayout {
            offset: definition.offset,
            name_len: definition.name.len(),
            trailing_code: definition.trailing_code,
            registry_suffix: (index + 1 < definitions.len()).then(|| super::IndexedByteRange {
                start: definition.offset + definition.name.len() + 2,
                end: definitions[index + 1].offset,
            }),
        })
        .collect()
}

pub(super) fn type_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<TypeDefinition<'_>> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        let declared = usize::from(bytes[at]);
        let Some(length) = declared.checked_sub(1) else {
            at += 1;
            continue;
        };
        let name_start = at + 1;
        let name_end = name_start.saturating_add(length);
        let Some(raw) = bytes.get(name_start..name_end) else {
            at += 1;
            continue;
        };
        let valid = raw.starts_with(b"UGS::")
            && raw.iter().all(|byte| (0x20..0x7f).contains(byte))
            && name_end < end;
        if valid {
            let name = std::str::from_utf8(raw)
                .expect("invariant: validated printable ASCII is valid UTF-8");
            out.push(TypeDefinition {
                offset: at,
                name,
                trailing_code: bytes[name_end],
                registry_suffix: &[],
            });
            at = name_end + 1;
        } else {
            at += 1;
        }
    }
    for index in 0..out.len().saturating_sub(1) {
        let suffix_start = out[index].offset + out[index].name.len() + 2;
        let suffix_end = out[index + 1].offset;
        out[index].registry_suffix = &bytes[suffix_start..suffix_end];
    }
    out
}

pub(super) fn field_definitions(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Vec<FieldDefinition<'_>> {
    let mut out = Vec::new();
    let mut search = start;
    let mut limit = start.saturating_add(256).min(end);
    while let Some((definition, at)) = (search..limit)
        .find_map(|at| field_definition_at(bytes, at, end).map(|definition| (definition, at)))
    {
        let next = at + definition.name.len() + 2;
        search = next;
        limit = search.saturating_add(256).min(end);
        out.push(definition);
    }
    bound_field_registry_suffixes(bytes, &mut out);
    out
}

pub(super) fn all_field_definitions(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Vec<FieldDefinition<'_>> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        if let Some(definition) = field_definition_at(bytes, at, end) {
            at += definition.name.len() + 2;
            out.push(definition);
        } else {
            at += 1;
        }
    }
    bound_field_registry_suffixes(bytes, &mut out);
    out
}

fn bound_field_registry_suffixes<'a>(bytes: &'a [u8], definitions: &mut [FieldDefinition<'a>]) {
    for index in 0..definitions.len().saturating_sub(1) {
        let suffix_start = definitions[index].offset + definitions[index].name.len() + 2;
        let suffix_end = definitions[index + 1].offset;
        definitions[index].registry_suffix = &bytes[suffix_start..suffix_end];
    }
}

fn field_definition_at(bytes: &[u8], at: usize, end: usize) -> Option<FieldDefinition<'_>> {
    let declared = usize::from(*bytes.get(at)?);
    let length = declared.checked_sub(1)?;
    let name_start = at.checked_add(1)?;
    let name_end = name_start.checked_add(length)?;
    (name_end < end).then_some(())?;
    let raw = bytes.get(name_start..name_end)?;
    (raw.starts_with(b"m_") && raw.iter().all(|byte| (0x20..0x7f).contains(byte))).then_some(())?;
    Some(FieldDefinition {
        offset: at,
        name: std::str::from_utf8(raw).ok()?,
        trailing_code: bytes[name_end],
        registry_suffix: &[],
    })
}
