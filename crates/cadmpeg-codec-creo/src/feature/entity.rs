// SPDX-License-Identifier: Apache-2.0
//! `AllFeatur` generated-entity tables and walker-order entity graph.

use std::collections::BTreeSet;

use crate::psb;

use super::rows::row_spans;

/// One `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityTable {
    /// Owning feature of a bounded `AllFeatur` feature row.
    pub feature_id: u32,
    /// Entity-class identifier following the table's `f7` marker.
    pub table_class_id: u32,
    /// Structurally bounded records in their declared generated-entity order.
    pub entries: Vec<FeatureEntityTableEntry>,
    /// Byte offset of the `f8` table opener in the original stream.
    pub offset: usize,
}

impl FeatureEntityTable {
    pub fn entry_ids(&self) -> Vec<u32> {
        self.entries.iter().map(|entry| entry.entity_id).collect()
    }

    pub fn surface_ids(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|entry| entry.is_surface)
            .map(|entry| entry.entity_id)
            .collect()
    }

    pub fn non_surface_entity_ids(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|entry| !entry.is_surface)
            .map(|entry| entry.entity_id)
            .collect()
    }
}

#[cfg(test)]
impl FeatureEntityTable {
    pub(crate) fn mark_surface_ids(&mut self, surface_ids: impl IntoIterator<Item = u32>) {
        let set: BTreeSet<u32> = surface_ids.into_iter().collect();
        for entry in &mut self.entries {
            entry.is_surface = set.contains(&entry.entity_id);
        }
    }

    pub(crate) fn with_surface_ids(mut self, surface_ids: impl IntoIterator<Item = u32>) -> Self {
        self.mark_surface_ids(surface_ids);
        self
    }
}

#[cfg(test)]
pub(crate) fn dummy_table_entry(entity_id: u32, is_surface: bool) -> FeatureEntityTableEntry {
    FeatureEntityTableEntry {
        entity_id,
        class_id: 0,
        source_entity_id: None,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: 0,
        end_offset: 0,
        is_surface,
    }
}

/// One record in an `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityTableEntry {
    /// Entity identifier at the start of the record body.
    pub entity_id: u32,
    /// Positional entry class following the entity identifier.
    pub class_id: u32,
    /// Source section entity identifier carried by class `200` entries.
    pub source_entity_id: Option<u32>,
    /// Related entity identifier carried by class `210`, related-form class
    /// `214`, class `219`, and class `2017` entries.
    pub related_entity_id: Option<u32>,
    /// One-byte state following a related entity.
    pub related_entity_state: Option<u8>,
    /// Whether the record starts with the `f7 1e` entry prefix.
    pub prefixed: bool,
    /// Whether this entity identifier is a materialized `srf_array` identifier.
    pub is_surface: bool,
    /// Byte offset of the entity identifier in the original stream.
    pub offset: usize,
    /// Byte offset immediately after the entry body. This follows the
    /// structural `e3`, or points at the enclosing `f2 f7` table separator
    /// when the final entry uses that separator as its terminator.
    pub end_offset: usize,
}

/// One named record in the implicit `AllFeatur` walker-order entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntity {
    /// Zero-based walker-order identifier used by `f7` references.
    pub entity_id: u32,
    /// Named-record type byte.
    pub type_byte: u8,
    /// NUL-terminated named-record name.
    pub name: String,
    /// Byte offset of the `e0` header in the original stream.
    pub offset: usize,
}

/// One `f7 <id>` reference in `AllFeatur`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityReference {
    /// Walker-order entity containing this token, when one precedes it.
    pub source_entity_id: Option<u32>,
    /// Referenced walker-order entity identifier.
    pub target_entity_id: u32,
    /// Whether the target identifier exists in the decoded entity table.
    pub target_resolved: bool,
    /// Byte offset of the `f7` token in the original stream.
    pub offset: usize,
}

/// Source section identifiers carried by class-200 generated entries.
pub(crate) fn generated_class_200_source_entity_ids(table: &FeatureEntityTable) -> BTreeSet<u32> {
    table
        .entries
        .iter()
        .filter(|entry| entry.class_id == 200)
        .filter_map(|entry| entry.source_entity_id)
        .collect()
}

/// Decode the implicit named-record entity table and every canonical `f7`
/// reference, preserving both source context and unresolved target IDs.
pub fn entity_graph(payload: &[u8]) -> (Vec<FeatureEntity>, Vec<FeatureEntityReference>) {
    let tokens = psb::tokens(payload);
    let Some(root) = tokens.first() else {
        return (Vec::new(), Vec::new());
    };
    let root_name = payload.get(2..root.length.saturating_sub(1));
    if root.offset != 0
        || root.kind != psb::TokenKind::NamedRecord
        || payload.get(1) != Some(&0)
        || root_name != Some(b"Sld_Features".as_slice())
    {
        return (Vec::new(), Vec::new());
    }
    let mut entities = Vec::new();
    for token in &tokens {
        if token.kind != psb::TokenKind::NamedRecord || token.length < 3 {
            continue;
        }
        let name_start = token.offset + 2;
        let name_end = token.offset + token.length - 1;
        entities.push(FeatureEntity {
            entity_id: entities.len() as u32,
            type_byte: payload[token.offset + 1],
            name: String::from_utf8_lossy(&payload[name_start..name_end]).into_owned(),
            offset: token.offset,
        });
    }
    let entity_count = entities.len() as u32;
    let entity_by_offset = entities
        .iter()
        .map(|entity| (entity.offset, entity.entity_id))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut source = None;
    let mut references = Vec::new();
    for token in tokens {
        if token.kind == psb::TokenKind::NamedRecord {
            source = entity_by_offset.get(&token.offset).copied();
        } else if token.kind == psb::TokenKind::EntityReference {
            let Ok((target_entity_id, _)) = psb::reference_id(payload, token.offset + 1) else {
                continue;
            };
            references.push(FeatureEntityReference {
                source_entity_id: source,
                target_entity_id,
                target_resolved: target_entity_id < entity_count,
                offset: token.offset,
            });
        }
    }
    (entities, references)
}

pub(crate) fn read_entries(
    payload: &[u8],
    body_start: usize,
    count: u32,
) -> Option<Vec<FeatureEntityTableEntry>> {
    let count = usize::try_from(count).ok()?;
    let remaining = payload.len().checked_sub(body_start)?;
    (count <= remaining / 2).then_some(())?;
    let mut entries = Vec::with_capacity(count);
    let mut cursor = body_start;
    for index in 0..count {
        let prefixed_class = (payload.get(cursor) == Some(&psb::token::ENTITY_REF))
            .then(|| psb::reference_id(payload, cursor + 1).ok())
            .flatten();
        let prefixed = prefixed_class.is_some();
        if let Some((_, after_class)) = prefixed_class {
            cursor = after_class;
        }
        let offset = cursor;
        let (id, after) = psb::reference_id(payload, cursor).ok()?;
        let (class_id, after_class) = psb::reference_id(payload, after).ok().or_else(|| {
            (index == 0)
                .then_some(prefixed_class)
                .flatten()
                .map(|(class_id, _)| (class_id, after))
        })?;
        let (source_entity_id, related_entity_id, related_entity_state, body_start) =
            if class_id == 200 {
                match psb::reference_id(payload, after_class) {
                    Ok((order, after_order)) => (Some(order), None, None, after_order),
                    Err(_) => (None, None, None, after_class),
                }
            } else if matches!(class_id, 210 | 214 | 219 | 2017) {
                match psb::reference_id(payload, after_class) {
                    Ok((related, after_related))
                        if matches!(
                            (class_id, payload.get(after_related)),
                            (210 | 214 | 219 | 2017, Some(&0)) | (2017, Some(&1))
                        ) =>
                    {
                        (
                            None,
                            Some(related),
                            payload.get(after_related).copied(),
                            after_related,
                        )
                    }
                    Err(_) => (None, None, None, after_class),
                    Ok(_) => (None, None, None, after_class),
                }
            } else {
                (None, None, None, after_class)
            };
        let terminal_state = if class_id == 200 {
            payload
                .get(body_start)
                .copied()
                .filter(|state| matches!(state, 0 | 1))
        } else {
            related_entity_state
        };
        let terminal_table_separator = (index + 1 == count
            && terminal_state.is_some()
            && payload.get(body_start + 1..body_start + 3)
                == Some(&[0xf2, psb::token::ENTITY_REF]))
        .then_some(body_start + 1);
        let end_offset = if let Some(end_offset) = terminal_table_separator {
            end_offset
        } else {
            body_start
                + payload
                    .get(body_start..)?
                    .iter()
                    .position(|&byte| byte == 0xe3)?
                + 1
        };
        entries.push(FeatureEntityTableEntry {
            entity_id: id,
            class_id,
            source_entity_id,
            related_entity_id,
            related_entity_state,
            prefixed,
            is_surface: false,
            offset,
            end_offset,
        });
        cursor = end_offset;
    }
    Some(entries)
}

/// Decode valid `AllFeatur` mixed generated-entity tables.
///
/// `feature_ids` must come from byte-decoded geometry ownership; no owner is
/// inferred from a table's neighbouring bytes or entity contents.
pub fn entity_tables(
    payload: &[u8],
    feature_ids: &BTreeSet<u32>,
    surface_ids: &BTreeSet<u32>,
) -> Vec<FeatureEntityTable> {
    let spans = row_spans(payload, feature_ids);
    let mut tables = Vec::new();
    for offset in 0..payload.len() {
        if payload[offset] != psb::token::ARRAY_OPEN {
            continue;
        }
        let (count, after_count) = psb::compact_int(payload, offset + 1);
        if count == 0 || payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
            continue;
        }
        let Ok((table_class_id, after_table_class)) = psb::reference_id(payload, after_count + 1)
        else {
            continue;
        };
        if payload.get(after_table_class..after_table_class + 2) != Some(&[0xfb, 0xe3]) {
            continue;
        }
        let Some(&(_, row_end, feature_id)) = spans
            .iter()
            .find(|&&(start, end, _)| start <= offset && offset < end)
        else {
            continue;
        };
        let Some(mut entries) = read_entries(&payload[..row_end], after_table_class + 2, count)
        else {
            continue;
        };
        for entry in &mut entries {
            entry.is_surface = surface_ids.contains(&entry.entity_id);
        }
        tables.push(FeatureEntityTable {
            feature_id,
            table_class_id,
            entries,
            offset,
        });
    }
    tables
}
