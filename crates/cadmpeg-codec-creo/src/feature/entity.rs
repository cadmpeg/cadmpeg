// SPDX-License-Identifier: Apache-2.0
//! `AllFeatur` generated-entity tables and walker-order entity graph.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::psb;

use super::rows::row_spans;

/// One `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureEntityTable {
    /// Owning feature of a bounded `AllFeatur` feature row.
    pub(crate) feature_id: u32,
    /// Entity-class identifier following the table's `f7` marker.
    pub(crate) table_class_id: u32,
    /// Structurally bounded records in their declared generated-entity order.
    pub(crate) entries: Vec<FeatureEntityTableEntry>,
    /// Materialized `srf_array` identifiers among this table's entity ids.
    surface_ids: BTreeSet<u32>,
    /// Byte offset of the `f8` table opener in the original stream.
    pub(crate) offset: usize,
}

impl FeatureEntityTable {
    /// Admits a table whose materialized identifiers are exactly the entries
    /// the model decoded as `srf_array` identifiers.
    #[cfg(test)]
    pub(crate) fn new(
        feature_id: u32,
        table_class_id: u32,
        entries: Vec<FeatureEntityTableEntry>,
        model_surface_ids: &BTreeSet<u32>,
        offset: usize,
    ) -> Self {
        let surface_ids = entries
            .iter()
            .map(|entry| entry.entity_id)
            .filter(|id| model_surface_ids.contains(id))
            .collect();
        Self {
            feature_id,
            table_class_id,
            entries,
            surface_ids,
            offset,
        }
    }

    pub(crate) fn entry_ids(&self) -> Vec<u32> {
        self.entries.iter().map(|entry| entry.entity_id).collect()
    }

    pub(crate) fn surface_ids(&self) -> Vec<u32> {
        self.entries
            .iter()
            .map(|entry| entry.entity_id)
            .filter(|id| self.surface_ids.contains(id))
            .collect()
    }

    pub(crate) fn non_surface_entity_ids(&self) -> Vec<u32> {
        self.entries
            .iter()
            .map(|entry| entry.entity_id)
            .filter(|id| !self.surface_ids.contains(id))
            .collect()
    }
}

#[cfg(test)]
impl FeatureEntityTable {
    pub(crate) fn mark_surface_ids(&mut self, surface_ids: impl IntoIterator<Item = u32>) {
        let model_surface_ids = surface_ids.into_iter().collect();
        *self = Self::new(
            self.feature_id,
            self.table_class_id,
            std::mem::take(&mut self.entries),
            &model_surface_ids,
            self.offset,
        );
    }

    pub(crate) fn with_surface_ids(mut self, surface_ids: impl IntoIterator<Item = u32>) -> Self {
        self.mark_surface_ids(surface_ids);
        self
    }

    pub(crate) fn mark_surface_id(&mut self, entity_id: u32) {
        let mut model_surface_ids = std::mem::take(&mut self.surface_ids);
        model_surface_ids.insert(entity_id);
        self.mark_surface_ids(model_surface_ids);
    }

    pub(crate) fn unmark_surface_id(&mut self, entity_id: u32) {
        let mut model_surface_ids = std::mem::take(&mut self.surface_ids);
        model_surface_ids.remove(&entity_id);
        self.mark_surface_ids(model_surface_ids);
    }
}

#[cfg(test)]
pub(crate) fn dummy_table_entry(entity_id: u32) -> FeatureEntityTableEntry {
    FeatureEntityTableEntry {
        entity_id,
        payload: EntryPayload::Plain {
            class: PlainClass::new(0).expect("0 is not the source class"),
        },
        prefixed: false,
        offset: 0,
        end_offset: 0,
    }
}

/// Class-specific generated-entity payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryPayload {
    /// Class `200` source-section identifier, present when the compact id parsed.
    Source { entity: Option<u32> },
    /// Related entity carried by class `210`, related-form `214`, `219`, or `2017`.
    Related {
        /// The related class that owns the pair.
        class: RelatedClass,
        entity: u32,
        state: RelatedState,
    },
    /// Any other class, or a related class whose pair did not parse.
    Plain {
        /// The positional entry class.
        class: PlainClass,
    },
}

/// A positional entry class that owns no payload of its own. Class `200`
/// always carries a source identifier, so it is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlainClass(u32);

impl PlainClass {
    /// Admits every entry class but `200`.
    pub(crate) const fn new(class: u32) -> Option<Self> {
        match class {
            200 => None,
            class => Some(Self(class)),
        }
    }

    /// The positional entry class.
    const fn get(self) -> u32 {
        self.0
    }
}

/// A generated-entity class that carries a related entity and its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelatedClass {
    Class210,
    Class214,
    Class219,
    Class2017,
}

impl RelatedClass {
    /// The related class for a positional entry class, when it is one.
    fn from_class_id(class_id: u32) -> Option<Self> {
        match class_id {
            210 => Some(Self::Class210),
            214 => Some(Self::Class214),
            219 => Some(Self::Class219),
            2017 => Some(Self::Class2017),
            _ => None,
        }
    }

    /// The positional entry class.
    fn class_id(self) -> u32 {
        match self {
            Self::Class210 => 210,
            Self::Class214 => 214,
            Self::Class219 => 219,
            Self::Class2017 => 2017,
        }
    }
}

/// One-byte state following a related entity identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelatedState {
    Zero,
    One,
}

impl RelatedState {
    #[cfg(test)]
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Zero),
            1 => Some(Self::One),
            _ => None,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
        }
    }
}

#[cfg(test)]
fn plain_payload(class_id: u32) -> EntryPayload {
    EntryPayload::Plain {
        class: PlainClass::new(class_id).expect("a plain class is not the source class"),
    }
}

#[cfg(test)]
pub(crate) fn entry_payload(
    class_id: u32,
    source_entity_id: Option<u32>,
    related_entity_id: Option<u32>,
    related_entity_state: Option<u8>,
) -> EntryPayload {
    match (class_id, RelatedClass::from_class_id(class_id)) {
        (200, _) => EntryPayload::Source {
            entity: source_entity_id,
        },
        (_, Some(class)) => match (
            related_entity_id,
            related_entity_state.and_then(RelatedState::from_byte),
        ) {
            (Some(entity), Some(state)) => EntryPayload::Related {
                class,
                entity,
                state,
            },
            _ => plain_payload(class_id),
        },
        _ => plain_payload(class_id),
    }
}

/// One record in an `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureEntityTableEntry {
    /// Entity identifier at the start of the record body.
    pub(crate) entity_id: u32,
    /// Payload of the positional entry class following the entity identifier.
    pub(crate) payload: EntryPayload,
    /// Whether the record starts with the `f7 1e` entry prefix.
    pub(crate) prefixed: bool,
    /// Byte offset of the entity identifier in the original stream.
    pub(crate) offset: usize,
    /// Byte offset immediately after the entry body. This follows the
    /// structural `e3`, or points at the enclosing `f2 f7` table separator
    /// when the final entry uses that separator as its terminator.
    pub(crate) end_offset: usize,
}

impl FeatureEntityTableEntry {
    /// The positional entry class following the entity identifier.
    pub(crate) fn class_id(&self) -> u32 {
        match self.payload {
            EntryPayload::Source { .. } => 200,
            EntryPayload::Related { class, .. } => class.class_id(),
            EntryPayload::Plain { class } => class.get(),
        }
    }

    pub(crate) fn source_entity_id(&self) -> Option<u32> {
        match self.payload {
            EntryPayload::Source { entity } => entity,
            _ => None,
        }
    }

    pub(crate) fn related_entity_id(&self) -> Option<u32> {
        match self.payload {
            EntryPayload::Related { entity, .. } => Some(entity),
            _ => None,
        }
    }

    pub(crate) fn related_entity_state(&self) -> Option<u8> {
        match self.payload {
            EntryPayload::Related { state, .. } => Some(state.as_u8()),
            _ => None,
        }
    }
}

/// One named record in the implicit `AllFeatur` walker-order entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureEntity {
    /// Zero-based walker-order identifier used by `f7` references.
    pub(crate) entity_id: u32,
    /// Named-record type byte.
    pub(crate) type_byte: u8,
    /// NUL-terminated named-record name.
    pub(crate) name: String,
    /// Byte offset of the `e0` header in the original stream.
    pub(crate) offset: usize,
}

/// One `f7 <id>` reference in `AllFeatur`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureEntityReference {
    /// Walker-order entity containing this token, when one precedes it.
    pub(crate) source_entity_id: Option<u32>,
    /// Referenced walker-order entity identifier.
    pub(crate) target_entity_id: u32,
    /// Byte offset of the `f7` token in the original stream.
    pub(crate) offset: usize,
}

/// Source section identifiers carried by class-200 generated entries.
pub(super) fn generated_class_200_source_entity_ids(table: &FeatureEntityTable) -> BTreeSet<u32> {
    table
        .entries
        .iter()
        .filter_map(FeatureEntityTableEntry::source_entity_id)
        .collect()
}

/// Decode the implicit named-record entity table and every canonical `f7`
/// reference, preserving both source context and unresolved target IDs.
pub(crate) fn entity_graph(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<(Vec<FeatureEntity>, Vec<FeatureEntityReference>), CodecError> {
    let Some(root) = psb::token_at(payload, 0) else {
        return Ok((Vec::new(), Vec::new()));
    };
    // The root name ends one byte before the record ends, at its NUL.
    let root_name = root
        .length
        .checked_sub(1)
        .and_then(|name_end| payload.get(2..name_end));
    if root.offset != 0
        || root.kind != psb::TokenKind::NamedRecord
        || payload.get(1) != Some(&0)
        || root_name != Some(b"Sld_Features".as_slice())
    {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut entities = Vec::new();
    let mut offset = 0;
    while let Some(token) = psb::token_at(payload, offset) {
        offset += token.length;
        if token.kind != psb::TokenKind::NamedRecord || token.length < 3 {
            continue;
        }
        let name_start = token.offset + 2;
        let name_end = token.offset + token.length - 1;
        let entity_id = u32::try_from(entities.len())
            .map_err(|_| CodecError::malformed("creo feature entity id exceeds u32"))?;
        ctx.try_reserve_items(&mut entities, 1, "creo feature entity graph nodes")?;
        entities.push(FeatureEntity {
            entity_id,
            type_byte: payload[token.offset + 1],
            name: copy_lossy_entity_name(ctx, &payload[name_start..name_end])?,
            offset: token.offset,
        });
    }
    let mut source = None;
    let mut references = Vec::new();
    let mut next_entity = 0;
    let mut offset = 0;
    while let Some(token) = psb::token_at(payload, offset) {
        offset += token.length;
        if token.kind == psb::TokenKind::NamedRecord {
            source = entities.get(next_entity).map(|entity| entity.entity_id);
            next_entity += 1;
        } else if token.kind == psb::TokenKind::EntityReference {
            let Ok((target_entity_id, _)) = psb::reference_id(payload, token.offset + 1) else {
                continue;
            };
            ctx.try_reserve_items(&mut references, 1, "creo feature entity graph references")?;
            references.push(FeatureEntityReference {
                source_entity_id: source,
                target_entity_id,
                offset: token.offset,
            });
        }
    }
    Ok((entities, references))
}

fn copy_lossy_entity_name(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<String, CodecError> {
    copy_lossy_text(ctx, bytes, "creo feature entity name")
}

pub(super) fn copy_lossy_text(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut text = String::new();
    let mut remaining = bytes;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                ctx.try_reserve_retained_text(&mut text, valid.len(), operation)?;
                text.push_str(valid);
                return Ok(text);
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                let valid = std::str::from_utf8(&remaining[..valid_len])
                    .map_err(|_| CodecError::malformed("creo feature entity UTF-8 prefix"))?;
                let growth = valid_len
                    .checked_add('\u{fffd}'.len_utf8())
                    .ok_or_else(|| CodecError::malformed("creo feature entity name length"))?;
                ctx.try_reserve_retained_text(&mut text, growth, operation)?;
                text.push_str(valid);
                text.push('\u{fffd}');
                let invalid_len = error.error_len().unwrap_or(remaining.len() - valid_len);
                remaining = &remaining[valid_len + invalid_len..];
            }
        }
    }
}

pub(super) fn read_entries(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body_start: usize,
    count: u32,
) -> Result<Option<Vec<FeatureEntityTableEntry>>, CodecError> {
    let Some(count) = usize::try_from(count).ok() else {
        return Ok(None);
    };
    let Some(remaining) = payload.len().checked_sub(body_start) else {
        return Ok(None);
    };
    if count > remaining / 2 {
        return Ok(None);
    }
    let mut entries = Vec::new();
    ctx.try_reserve_items(&mut entries, count, "creo feature table entries")?;
    Ok((move || {
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
            let related_class = RelatedClass::from_class_id(class_id);
            let (entry_payload, body_start) = if class_id == 200 {
                match psb::reference_id(payload, after_class) {
                    Ok((order, after_order)) => (
                        EntryPayload::Source {
                            entity: Some(order),
                        },
                        after_order,
                    ),
                    Err(_) => (EntryPayload::Source { entity: None }, after_class),
                }
            } else if let Some(class) = related_class {
                psb::reference_id(payload, after_class)
                    .ok()
                    .and_then(|(entity, after_related)| {
                        let state = match (class, payload.get(after_related)) {
                            (_, Some(&0)) => RelatedState::Zero,
                            (RelatedClass::Class2017, Some(&1)) => RelatedState::One,
                            _ => return None,
                        };
                        Some((
                            EntryPayload::Related {
                                class,
                                entity,
                                state,
                            },
                            after_related,
                        ))
                    })
                    .unwrap_or((
                        EntryPayload::Plain {
                            class: PlainClass::new(class_id)?,
                        },
                        after_class,
                    ))
            } else {
                (
                    EntryPayload::Plain {
                        class: PlainClass::new(class_id)?,
                    },
                    after_class,
                )
            };
            let terminal_state = match entry_payload {
                EntryPayload::Source { .. } => payload
                    .get(body_start)
                    .copied()
                    .filter(|state| matches!(state, 0 | 1)),
                EntryPayload::Related { state, .. } => Some(state.as_u8()),
                EntryPayload::Plain { .. } => None,
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
                payload: entry_payload,
                prefixed,
                offset,
                end_offset,
            });
            cursor = end_offset;
        }
        Some(entries)
    })())
}

/// Decode valid `AllFeatur` mixed generated-entity tables.
///
/// `feature_ids` must come from byte-decoded geometry ownership; no owner is
/// inferred from a table's neighbouring bytes or entity contents.
pub(crate) fn entity_tables(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    feature_ids: &BTreeSet<u32>,
    surface_ids: &BTreeSet<u32>,
) -> Result<Vec<FeatureEntityTable>, CodecError> {
    let spans = row_spans(ctx, payload, feature_ids)?;
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
        let Some(entries) = read_entries(ctx, &payload[..row_end], after_table_class + 2, count)?
        else {
            continue;
        };
        let mut table_surface_ids = BTreeSet::new();
        for entry in &entries {
            if surface_ids.contains(&entry.entity_id)
                && !table_surface_ids.contains(&entry.entity_id)
            {
                ctx.charge_collection_items(1, "creo feature table surface ids")?;
                table_surface_ids.insert(entry.entity_id);
            }
        }
        ctx.try_reserve_items(&mut tables, 1, "creo feature entity tables")?;
        tables.push(FeatureEntityTable {
            feature_id,
            table_class_id,
            entries,
            surface_ids: table_surface_ids,
            offset,
        });
    }
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use super::{entity_graph, entity_tables, read_entries};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const GRAPH: &[u8] = b"\xe0\0Sld_Features\0\xe0\0N\xff\0\xf7\0";

    fn run(items: u64, bytes: u64) -> Result<(usize, usize, String), CodecError> {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = bytes;
        let (ctx, _) =
            DecodeContext::from_root_bytes(GRAPH, &arena, &policy).expect("root graph is admitted");
        let (entities, references) = entity_graph(&ctx, GRAPH)?;
        Ok((entities.len(), references.len(), entities[1].name.clone()))
    }

    #[test]
    fn entity_graph_nodes_refuse_before_vec_growth() {
        assert_eq!(
            run(3, u64::MAX).expect("graph admitted"),
            (2, 1, "N\u{fffd}".into())
        );
        let error = run(0, u64::MAX).expect_err("root node needs a Vec item");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity graph nodes"));
    }

    #[test]
    fn entity_graph_references_refuse_before_vec_growth() {
        let error = run(2, u64::MAX).expect_err("reference needs a Vec item");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity graph references"));
    }

    #[test]
    fn entity_graph_lossy_name_refuses_before_retained_growth() {
        let error = run(3, 15).expect_err("replacement needs three retained bytes");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo feature entity name"));
    }

    #[test]
    fn feature_table_entries_refuse_before_counted_vec_reserve() {
        let payload = [10, 0x80, 200, 4, 0, 0xe3, 11, 0x80, 200, 7, 1, 0xf2, 0xf7];
        let run = |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
                .expect("root table is admitted");
            read_entries(&ctx, &payload, 0, 2)
        };
        assert_eq!(
            run(2)
                .expect("two entry slots admitted")
                .expect("complete table")
                .len(),
            2
        );
        let error = run(1).expect_err("two entry slots exceed one-item limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature table entries"));
    }

    fn limited_tables(limit: u64) -> Result<Vec<super::FeatureEntityTable>, CodecError> {
        let payload = crate::test_support::allfeatur_row(
            4,
            [0xeb, 0x04],
            917,
            &[0xf8, 1, 0xf7, 0x1d, 0xfb, 0xe3, 7, 0x80, 0xc8, 1, 0, 0xe3],
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("root row is admitted");
        entity_tables(
            &ctx,
            &payload,
            &std::collections::BTreeSet::from([4]),
            &std::collections::BTreeSet::from([7]),
        )
    }

    #[test]
    fn feature_table_surface_ids_refuse_before_btree_insert() {
        assert_eq!(limited_tables(8).expect("one table admitted").len(), 1);
        let error = limited_tables(6).expect_err("surface id node exceeds limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature table surface ids"));
    }

    #[test]
    fn feature_entity_tables_refuse_before_vec_growth() {
        let error = limited_tables(7).expect_err("table Vec item exceeds limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity tables"));
    }
}
