// SPDX-License-Identifier: Apache-2.0
//! NX OM registry-token framing.

use super::{FieldDefinition, IndexedDefinitionLayout, TypeDefinition};

const FIELD_START_PROBE_LIMIT: usize = 256;

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

/// Class declarations and the first byte of the following member registry.
///
/// The boundary is retained because the member registry follows the class
/// registry, while the class list itself does not contain the reference-list
/// declarations that precede it.
pub(super) struct TypeRegistry<'a> {
    pub definitions: Vec<TypeDefinition<'a>>,
    pub field_start: usize,
}

#[derive(Debug, Clone, Copy)]
struct RegistryDeclaration<'a> {
    offset: usize,
    name: &'a str,
    name_end: usize,
    trailing_code: u8,
    core_end: usize,
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

fn registry_declaration_at<'a>(
    bytes: &'a [u8],
    at: usize,
    end: usize,
    prefix: &[u8],
) -> Option<RegistryDeclaration<'a>> {
    let declared = usize::from(*bytes.get(at)?);
    let name_len = declared.checked_sub(1)?;
    let name_start = at.checked_add(1)?;
    let name_end = name_start.checked_add(name_len)?;
    let raw = bytes.get(name_start..name_end)?;
    if name_end >= end
        || !raw.starts_with(prefix)
        || !raw.iter().all(|byte| (0x20..0x7f).contains(byte))
    {
        return None;
    }
    Some(RegistryDeclaration {
        offset: at,
        name: std::str::from_utf8(raw).ok()?,
        name_end,
        trailing_code: bytes[name_end],
        core_end: name_end + 1,
    })
}

fn registry_token_in(bytes: &[u8], at: usize, end: usize) -> Option<RegistryToken> {
    let first = *bytes.get(at)?;
    let suffix = bytes.get(at.checked_add(1)?..end)?;
    registry_token_at(first, suffix, 0).filter(|token| {
        at.checked_add(token.width)
            .is_some_and(|token_end| token_end <= end)
    })
}

fn class_registry_layout_at(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<(ClassRegistryLayout, usize)> {
    let storage_code = registry_token_in(bytes, at, end)?;
    let base_at = at.checked_add(storage_code.width)?;
    let base = registry_token_in(bytes, base_at, end)?;
    let fingerprint_at = base_at.checked_add(base.width)?;
    let fingerprint_end = fingerprint_at.checked_add(8)?;
    let fingerprint = bytes
        .get(fingerprint_at..fingerprint_end)?
        .try_into()
        .ok()?;
    let reference_at = fingerprint_end;
    let reference = registry_token_in(bytes, reference_at, end)?;
    let tail_end = reference_at.checked_add(reference.width)?;
    (reference.value != 0).then_some((
        ClassRegistryLayout {
            storage_code,
            base_class: base.value,
            schema_fingerprint: fingerprint,
            reference: reference.value,
        },
        tail_end,
    ))
}

fn complete_type_registry_at(bytes: &[u8], first: usize, end: usize) -> Option<TypeRegistry<'_>> {
    let mut at = first;
    loop {
        let declaration = registry_declaration_at(bytes, at, end, b"UGS::")?;
        at = declaration.core_end;
        match bytes.get(at) {
            Some(0x01) => {
                at += 1;
                break;
            }
            Some(_) if registry_declaration_at(bytes, at, end, b"UGS::").is_some() => {}
            _ => return None,
        }
    }

    let mut definitions = Vec::new();
    loop {
        if let Some(field_start) = field_registry_start(bytes, at, end) {
            return Some(TypeRegistry {
                definitions,
                field_start,
            });
        }
        let declaration = registry_declaration_at(bytes, at, end, b"UGS::")?;
        let (_, tail_end) = class_registry_layout_at(bytes, declaration.name_end, end)?;
        let registry_suffix = bytes.get(declaration.core_end..tail_end)?;
        definitions.push(TypeDefinition {
            offset: declaration.offset,
            name: declaration.name,
            trailing_code: declaration.trailing_code,
            registry_suffix,
        });
        at = tail_end;
        if field_registry_start(bytes, at, end).is_none()
            && registry_declaration_at(bytes, at, end, b"UGS::").is_none()
        {
            return None;
        }
    }
}

fn field_registry_start(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    if bytes.get(at) == Some(&0x02) {
        return at.checked_add(1);
    }
    if registry_declaration_at(bytes, at, end, b"UGS::").is_some() {
        return None;
    }
    (0..=1).find_map(|gap| {
        let candidate = at.checked_add(gap)?;
        if registry_declaration_at(bytes, candidate, end, b"UGS::").is_some() {
            return None;
        }
        let probe_end = candidate.saturating_add(FIELD_START_PROBE_LIMIT).min(end);
        for probe in candidate..probe_end {
            if registry_declaration_at(bytes, probe, end, b"UGS::").is_some() {
                return None;
            }
            if field_definition_at(bytes, probe, end).is_some() {
                return Some(probe);
            }
        }
        None
    })
}

/// Parse the complete reference/class registry when its explicit terminators
/// and class tails are present. Older or partial layouts use the historical
/// scanner and retain its exact suffix bytes.
pub(super) fn type_registry(bytes: &[u8], start: usize, end: usize) -> TypeRegistry<'_> {
    let complete = (start..end).find_map(|at| {
        registry_declaration_at(bytes, at, end, b"UGS::")
            .and_then(|_| complete_type_registry_at(bytes, at, end))
    });
    if let Some(registry) = complete {
        return registry;
    }

    let definitions = legacy_type_definitions(bytes, start, end);
    let field_start = definitions.last().map_or(start, |definition| {
        definition.offset + definition.name.len() + 2
    });
    TypeRegistry {
        definitions,
        field_start,
    }
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
        .map(|definition| IndexedDefinitionLayout {
            offset: definition.offset,
            name_len: definition.name.len(),
            trailing_code: definition.trailing_code,
            registry_suffix: Some(super::IndexedByteRange {
                start: definition.offset + definition.name.len() + 2,
                end: definition.offset
                    + definition.name.len()
                    + 2
                    + definition.registry_suffix.len(),
            }),
        })
        .collect()
}

pub(super) fn field_definition_layouts(
    definitions: &[FieldDefinition<'_>],
) -> Vec<IndexedDefinitionLayout> {
    definitions
        .iter()
        .map(|definition| IndexedDefinitionLayout {
            offset: definition.offset,
            name_len: definition.name.len(),
            trailing_code: definition.trailing_code,
            registry_suffix: Some(super::IndexedByteRange {
                start: definition.offset + definition.name.len() + 2,
                end: definition.offset
                    + definition.name.len()
                    + 2
                    + definition.registry_suffix.len(),
            }),
        })
        .collect()
}

fn legacy_type_definitions(bytes: &[u8], start: usize, end: usize) -> Vec<TypeDefinition<'_>> {
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
