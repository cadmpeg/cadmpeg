// SPDX-License-Identifier: Apache-2.0
//! NX OM registry-token framing.

use super::{FieldDefinition, TypeDefinition};

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
}

pub(crate) fn registry_token_at(tail: &[u8], offset: usize) -> Option<RegistryToken> {
    let prefix = *tail.get(offset)?;
    match prefix {
        0x00..=0x7f => Some(RegistryToken {
            value: u32::from(prefix),
            form: RegistryTokenForm::Direct,
            width: 1,
        }),
        0x80..=0x8f => {
            let low = *tail.get(offset + 1)?;
            Some(RegistryToken {
                value: u32::from(prefix - 0x80) * 256 + u32::from(low) + 1,
                form: RegistryTokenForm::Compact,
                width: 2,
            })
        }
        0x90 | 0xa0..=0xaf | 0xf1 => {
            let high = u32::from(*tail.get(offset + 1)?);
            let low = u32::from(*tail.get(offset + 2)?);
            Some(RegistryToken {
                value: ((u32::from(prefix & 0x0f) << 16) | (high << 8) | low) + 1,
                form: RegistryTokenForm::Wide,
                width: 3,
            })
        }
        _ => None,
    }
}

pub(crate) fn class_registry_layout(tail: &[u8]) -> Option<ClassRegistryLayout> {
    let total = tail.len();
    let storage_code = registry_token_at(tail, 0)?;
    let base_offset = storage_code.width;
    let base = registry_token_at(tail, base_offset)?;
    let fingerprint_offset = base_offset.checked_add(base.width)?;
    let fingerprint_end = fingerprint_offset.checked_add(8)?;
    let schema_fingerprint = tail
        .get(fingerprint_offset..fingerprint_end)?
        .try_into()
        .ok()?;
    let reference = registry_token_at(tail, fingerprint_end)?;
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

pub(crate) fn field_registry_layout(tail: &[u8]) -> Option<FieldRegistryLayout> {
    let storage_code = registry_token_at(tail, 0)?;
    let owner = registry_token_at(tail, storage_code.width)?;
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
    })
}

fn class_registry_layout_at(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<(ClassRegistryLayout, usize)> {
    let storage_code = registry_token_at(bytes.get(at..end)?, 0)?;
    let base_at = at.checked_add(storage_code.width)?;
    let base = registry_token_at(bytes.get(base_at..end)?, 0)?;
    let fingerprint_at = base_at.checked_add(base.width)?;
    let fingerprint_end = fingerprint_at.checked_add(8)?;
    let fingerprint = bytes
        .get(fingerprint_at..fingerprint_end)?
        .try_into()
        .ok()?;
    let reference_at = fingerprint_end;
    let reference = registry_token_at(bytes.get(reference_at..end)?, 0)?;
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
        at = declaration.name_end + 1;
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
        let registry_tail = bytes.get(declaration.name_end..tail_end)?;
        definitions.push(TypeDefinition {
            offset: declaration.offset,
            name: declaration.name,
            registry_tail,
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
                registry_tail: &bytes[name_end..name_end + 1],
            });
            at = name_end + 1;
        } else {
            at += 1;
        }
    }
    for index in 0..out.len().saturating_sub(1) {
        let tail_start = out[index].offset + out[index].name.len() + 1;
        let tail_end = out[index + 1].offset;
        out[index].registry_tail = &bytes[tail_start..tail_end];
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
    bound_field_registry_tails(bytes, &mut out);
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
    bound_field_registry_tails(bytes, &mut out);
    out
}

fn bound_field_registry_tails<'a>(bytes: &'a [u8], definitions: &mut [FieldDefinition<'a>]) {
    for index in 0..definitions.len().saturating_sub(1) {
        let tail_start = definitions[index].offset + definitions[index].name.len() + 1;
        let tail_end = definitions[index + 1].offset;
        definitions[index].registry_tail = &bytes[tail_start..tail_end];
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
        registry_tail: &bytes[name_end..name_end + 1],
    })
}
