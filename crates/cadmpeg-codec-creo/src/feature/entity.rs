// SPDX-License-Identifier: Apache-2.0
//! `AllFeatur` generated-entity tables and walker-order entity graph.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::psb;

use super::entity_rows::EntityRows;
use super::rows::row_spans;

/// One `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureEntityTable {
    /// Owning feature of a bounded `AllFeatur` feature row.
    pub(crate) feature_id: u32,
    /// Entity-class identifier following the table's `f7` marker.
    pub(crate) table_class_id: u32,
    /// Structurally bounded records in their declared generated-entity order.
    pub(crate) entries: EntityRows,
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
        Self {
            feature_id,
            table_class_id,
            entries: EntityRows::from_fixture(entries, model_surface_ids),
            offset,
        }
    }

    #[cfg(test)]
    pub(crate) fn entry_ids(&self) -> Vec<u32> {
        self.entries.iter().map(|entry| entry.entity_id).collect()
    }

    #[cfg(test)]
    pub(crate) fn surface_ids(&self) -> Vec<u32> {
        self.surface_ids_iter().collect()
    }

    /// Iterate materialized surface identities in declared entry order.
    pub(crate) fn surface_ids_iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.entries.surfaces_in_order()
    }

    /// Return whether this table materializes one surface identity.
    pub(crate) fn contains_surface_id(&self, entity_id: u32) -> bool {
        self.entries.contains_surface(entity_id)
    }

    /// Borrow the distinct materialized surface identities.
    pub(crate) fn unique_surface_ids(&self) -> &BTreeSet<u32> {
        self.entries.surfaces()
    }

    #[cfg(test)]
    pub(crate) fn non_surface_entity_ids_iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.entries
            .iter()
            .map(|entry| entry.entity_id)
            .filter(|id| !self.entries.contains_surface(*id))
    }

    pub(crate) fn contains_non_surface_entity_id(&self, entity_id: u32) -> bool {
        self.entries.contains_non_surface(entity_id)
    }

    #[cfg(test)]
    pub(crate) fn non_surface_entity_ids(&self) -> Vec<u32> {
        self.non_surface_entity_ids_iter().collect()
    }
}

#[cfg(test)]
impl FeatureEntityTable {
    pub(crate) fn mark_surface_ids(&mut self, surface_ids: impl IntoIterator<Item = u32>) {
        let model_surface_ids = surface_ids.into_iter().collect();
        self.entries.mark_surfaces(&model_surface_ids);
    }

    pub(crate) fn with_surface_ids(mut self, surface_ids: impl IntoIterator<Item = u32>) -> Self {
        self.mark_surface_ids(surface_ids);
        self
    }

    pub(crate) fn mark_surface_id(&mut self, entity_id: u32) {
        let mut model_surface_ids = self.entries.surfaces().clone();
        model_surface_ids.insert(entity_id);
        self.mark_surface_ids(model_surface_ids);
    }

    pub(crate) fn unmark_surface_id(&mut self, entity_id: u32) {
        let mut model_surface_ids = self.entries.surfaces().clone();
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
    Related(RelatedPayload),
    /// Any other class, or a related class whose pair did not parse.
    Plain {
        /// The positional entry class.
        class: PlainClass,
    },
}

/// Related entity with the state domain of its owning class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RelatedPayload {
    class: RelatedClass,
    entity: u32,
    state: RelatedState,
}

impl RelatedPayload {
    pub(crate) fn new(class: RelatedClass, entity: u32, state: RelatedState) -> Option<Self> {
        if state == RelatedState::One && class != RelatedClass::Class2017 {
            return None;
        }
        Some(Self {
            class,
            entity,
            state,
        })
    }
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
            (Some(entity), Some(state)) => RelatedPayload::new(class, entity, state)
                .map_or_else(|| plain_payload(class_id), EntryPayload::Related),
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
            EntryPayload::Related(related) => related.class.class_id(),
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
            EntryPayload::Related(related) => Some(related.entity),
            _ => None,
        }
    }

    pub(crate) fn related_entity_state(&self) -> Option<u8> {
        match self.payload {
            EntryPayload::Related(related) => Some(related.state.as_u8()),
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
pub(super) fn generated_class_200_source_entity_ids(
    ctx: &DecodeContext<'_>,
    table: &FeatureEntityTable,
) -> Result<BTreeSet<u32>, CodecError> {
    let mut ids = BTreeSet::new();
    for id in ctx
        .admit_iter(
            table.entries.as_slice(),
            "creo generated source entry traversal",
        )?
        .filter_map(FeatureEntityTableEntry::source_entity_id)
    {
        ctx.insert_btree_set(&mut ids, id, "creo generated source entity ID nodes")?;
    }
    Ok(ids)
}

/// Decode the implicit named-record entity table and every canonical `f7`
/// reference, preserving both source context and unresolved target IDs.
pub(crate) fn entity_graph(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<(Vec<FeatureEntity>, Vec<FeatureEntityReference>), CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if !payload.starts_with(b"\xe0\0Sld_Features\0") {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut entities = Vec::new();
    let mut references = Vec::new();
    let mut source = None;
    let mut offset = 0;
    while offset < payload.len() {
        if payload[offset] == psb::token::NAMED_RECORD {
            ctx.next_charged(
                &mut (offset..payload.len()),
                "creo feature entity token traversal",
            )?;
            let Some(rest) = payload.get(offset + 2..) else {
                break;
            };
            let Some(name_len) = ctx.position_by(
                rest,
                |byte| Ok(*byte == 0),
                "creo feature entity name terminator",
            )?
            else {
                break;
            };
            let entity_id = u32::try_from(entities.len())
                .map_err(|_| CodecError::malformed("creo feature entity id exceeds u32"))?;
            ctx.reserve_vec(&mut entities, 1, "creo feature entity graph nodes")?;
            let name =
                ctx.copy_retained_lossy_utf8(&rest[..name_len], "creo feature entity name")?;
            entities.push(FeatureEntity {
                entity_id,
                type_byte: payload[offset + 1],
                name,
                offset,
            });
            source = Some(entity_id);
            offset += name_len + 3;
            continue;
        }
        let Some(token) = psb::token_at(ctx, payload, offset)? else {
            break;
        };
        offset += token.length;
        if token.kind == psb::TokenKind::EntityReference {
            let Ok((target_entity_id, _)) = psb::reference_id(payload, token.offset + 1) else {
                continue;
            };
            ctx.push_vec(
                &mut references,
                FeatureEntityReference {
                    source_entity_id: source,
                    target_entity_id,
                    offset: token.offset,
                },
                "creo feature entity graph references",
            )?;
        }
    }
    Ok((entities, references))
}

pub(super) fn read_entries(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body_start: usize,
    count: u32,
) -> Result<Option<Vec<FeatureEntityTableEntry>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(count) = usize::try_from(count).ok() else {
        return Ok(None);
    };
    let Some(remaining) = payload.len().checked_sub(body_start) else {
        return Ok(None);
    };
    if count > remaining / 2 {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "creo feature entry candidates")?;
    let mut entries = Vec::new();
    let mut cursor = body_start;
    let mut indices = 0..count;
    while !indices.is_empty() {
        if cursor >= payload.len() {
            return Ok(None);
        }
        let Some(index) = ctx.next_charged(&mut indices, "creo feature entry traversal")? else {
            break;
        };
        let Some(EntryPrefix {
            id,
            payload: entry_payload,
            prefixed,
            offset,
            body_start,
            terminal_state,
        }) = entry_prefix(payload, cursor, index)
        else {
            return Ok(None);
        };
        let terminal_table_separator = (index + 1 == count
            && terminal_state.is_some()
            && payload.get(body_start + 1..body_start + 3)
                == Some(&[0xf2, psb::token::ENTITY_REF]))
        .then_some(body_start + 1);
        let end_offset = if let Some(end_offset) = terminal_table_separator {
            end_offset
        } else {
            let Some(rest) = payload.get(body_start..) else {
                return Ok(None);
            };
            let Some(relative) = ctx.position_by(
                rest,
                |byte| Ok(*byte == 0xe3),
                "creo feature entry terminator",
            )?
            else {
                return Ok(None);
            };
            body_start + relative + 1
        };
        storage.with_storage(|| {
            ctx.push_vec(
                &mut entries,
                FeatureEntityTableEntry {
                    entity_id: id,
                    payload: entry_payload,
                    prefixed,
                    offset,
                    end_offset,
                },
                "creo feature table entries",
            )
        })?;
        cursor = end_offset;
    }
    let entries = storage.commit_value(entries)?;
    Ok(Some(entries))
}

struct EntryPrefix {
    id: u32,
    payload: EntryPayload,
    prefixed: bool,
    offset: usize,
    body_start: usize,
    terminal_state: Option<u8>,
}

fn entry_prefix(payload: &[u8], mut cursor: usize, index: usize) -> Option<EntryPrefix> {
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
                    EntryPayload::Related(RelatedPayload::new(class, entity, state)?),
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
        EntryPayload::Related(related) => Some(related.state.as_u8()),
        EntryPayload::Plain { .. } => None,
    };
    Some(EntryPrefix {
        id,
        payload: entry_payload,
        prefixed,
        offset,
        body_start,
        terminal_state,
    })
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
    let mut span_storage = ctx.reserve_scoped(0, "creo feature entity row spans")?;
    let spans = span_storage.with_storage(|| row_spans(ctx, payload, feature_ids))?;
    let mut span_rows = spans.iter();
    let mut span = if span_rows.len() == 0 {
        None
    } else {
        ctx.next_charged(&mut span_rows, "creo generated entity span traversal")?
    };
    let mut tables = Vec::new();
    for offset in ctx.admit_iter(0..payload.len(), "creo generated entity byte traversal")? {
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
        while span.is_some_and(|(_, end, _)| offset >= *end) {
            span = if span_rows.len() == 0 {
                None
            } else {
                ctx.next_charged(&mut span_rows, "creo generated entity span traversal")?
            };
        }
        let Some(&(_, row_end, feature_id)) = span.filter(|(start, _, _)| *start <= offset) else {
            continue;
        };
        let Some(entries) = read_entries(ctx, &payload[..row_end], after_table_class + 2, count)?
        else {
            continue;
        };
        let entries = EntityRows::new(ctx, entries, surface_ids)?;
        ctx.reserve_vec(&mut tables, 1, "creo feature entity tables")?;
        tables.push(FeatureEntityTable {
            feature_id,
            table_class_id,
            entries,
            offset,
        });
    }
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use super::{dummy_table_entry, entity_graph, entity_tables, read_entries, FeatureEntityTable};
    use super::{RelatedClass, RelatedPayload, RelatedState};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const GRAPH: &[u8] = b"\xe0\0Sld_Features\0\xe0\0N\xff\0\xf7\0";

    #[test]
    fn entity_graph_fallback_token_has_one_psb_visit() {
        const PAYLOAD: &[u8] = b"\xe0\0Sld_Features\0\xf7\x01";
        let needed = |payload| {
            crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |work| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = work;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                entity_graph(&ctx, payload)
            })
        };
        assert_eq!(needed(PAYLOAD), needed(&PAYLOAD[..PAYLOAD.len() - 2]) + 1);
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &[
                "creo feature entity token traversal",
                "creo PSB token traversal",
            ],
            |work| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = work;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let (entities, references) = entity_graph(&ctx, PAYLOAD)?;
                assert_eq!(entities.len(), 1);
                assert_eq!(references.len(), 1);
                assert_eq!(references[0].target_entity_id, 1);
                Ok::<_, CodecError>(())
            },
        );
    }

    #[test]
    fn related_payload_enforces_class_state_domain() {
        for class in [
            RelatedClass::Class210,
            RelatedClass::Class214,
            RelatedClass::Class219,
            RelatedClass::Class2017,
        ] {
            assert!(RelatedPayload::new(class, 1, RelatedState::Zero).is_some());
            assert_eq!(
                RelatedPayload::new(class, 1, RelatedState::One).is_some(),
                class == RelatedClass::Class2017
            );
        }
    }

    #[test]
    fn entity_table_borrowed_readers_preserve_duplicate_source_order() {
        let table = FeatureEntityTable::new(
            4,
            29,
            vec![
                dummy_table_entry(7),
                dummy_table_entry(9),
                dummy_table_entry(9),
            ],
            &std::collections::BTreeSet::from([7]),
            12,
        );
        assert_eq!(table.entry_ids(), [7, 9, 9]);
        assert_eq!(
            table.surface_ids_iter().collect::<Vec<_>>(),
            table.surface_ids()
        );
        assert_eq!(
            table.non_surface_entity_ids_iter().collect::<Vec<_>>(),
            [9, 9]
        );
        assert!(table.contains_surface_id(7));
        assert!(table.contains_non_surface_entity_id(9));
        assert!(!table.contains_non_surface_entity_id(7));
        assert!(!table.contains_non_surface_entity_id(11));
    }

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
            run(u64::MAX, u64::MAX).expect("graph admitted"),
            (2, 1, "N\u{fffd}".into())
        );
        let error = run(
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo feature entity graph nodes"),
                |cap| run(cap, u64::MAX),
            ),
            u64::MAX,
        )
        .expect_err("root node needs a Vec item");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity graph nodes"));
    }

    #[test]
    fn entity_graph_references_refuse_before_vec_growth() {
        let error = run(
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo feature entity graph references"),
                |cap| run(cap, u64::MAX),
            ),
            u64::MAX,
        )
        .expect_err("reference needs a Vec item");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity graph references"));
    }

    #[test]
    fn entity_graph_lossy_name_refuses_before_retained_growth() {
        let error = run(
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo feature entity name"),
                |cap| run(u64::MAX, cap),
            ),
        )
        .expect_err("replacement needs three retained bytes");
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
            run(u64::MAX)
                .expect("two entry slots admitted")
                .expect("complete table")
                .len(),
            2
        );
        let error = run(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo feature table entries"),
            run,
        ))
        .expect_err("entry growth exceeds its boundary");
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
        assert_eq!(
            limited_tables(u64::MAX).expect("one table admitted").len(),
            1
        );
        let error = limited_tables(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo feature table surface ids"),
            limited_tables,
        ))
        .expect_err("surface id node exceeds limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature table surface ids"));
    }

    #[test]
    fn feature_entity_tables_refuse_before_vec_growth() {
        let error = limited_tables(crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo feature entity tables"),
            limited_tables,
        ))
        .expect_err("table Vec item exceeds limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity tables"));
    }
    #[test]
    fn entity_graph_lossy_name_refuses_copy_work() {
        let (entities, references) =
            crate::test_support::assert_work_boundaries(&["creo feature entity name"], |ctx| {
                entity_graph(ctx, GRAPH)
            });
        assert_eq!((entities.len(), references.len()), (2, 1));
        assert_eq!(entities[1].name, "N\u{fffd}");
    }
    #[test]
    fn entity_scans_refuse_at_work_boundaries() {
        crate::test_support::assert_work_boundaries(
            &[
                "creo feature entity token traversal",
                "creo feature entity name terminator",
                "creo feature entity name",
            ],
            |ctx| entity_graph(ctx, GRAPH),
        );
        let payload = [10, 0x80, 200, 4, 0, 0xe3, 11, 0x80, 200, 7, 1, 0xf2, 0xf7];
        crate::test_support::assert_work_boundaries(
            &[
                "creo feature entry traversal",
                "creo feature entry terminator",
            ],
            |ctx| read_entries(ctx, &payload, 0, 2),
        );
    }

    #[test]
    fn entry_candidates_charge_present_entries_and_terminators() {
        for count in 0..=3usize {
            let payload: Vec<_> = (0..count)
                .flat_map(|index| [u8::try_from(index).expect("fixture value fits u8"), 1, 0xe3])
                .collect();
            // Each entry visits one counted slot and its first-byte terminator.
            // At most three entries fit the first four-slot Vec allocation.
            let total = 2 * u64::try_from(count).expect("fixture value fits u64");
            crate::test_support::assert_refusal_order(
                ResourceDimension::WorkUnits,
                &((0..total)
                    .map(|cap| {
                        if cap % 2 == 0 {
                            "creo feature entry traversal"
                        } else {
                            "creo feature entry terminator"
                        }
                    })
                    .collect::<Vec<_>>()),
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let result = read_entries(
                        &ctx,
                        &payload,
                        0,
                        u32::try_from(count).expect("fixture value fits u32"),
                    );
                    let completed = result.is_ok();
                    if completed {
                        let entries = result.expect("exact entry work").expect("complete entries");
                        assert_eq!(entries.len(), count);
                        for (index, entry) in entries.iter().enumerate() {
                            assert_eq!(
                                (
                                    entry.entity_id,
                                    entry.class_id(),
                                    entry.prefixed,
                                    entry.offset,
                                    entry.end_offset
                                ),
                                (
                                    u32::try_from(index).expect("fixture value fits u32"),
                                    1,
                                    false,
                                    3 * index,
                                    3 * index + 3
                                )
                            );
                        }
                        assert_eq!(ctx.resource_refusal(), None);
                        let refusal = ctx
                            .charge_work_limit(1, "after entry visits")
                            .expect_err("exact cap");
                        assert_eq!(
                            (refusal.dimension, refusal.used, refusal.additional),
                            (ResourceDimension::WorkUnits, total, 1)
                        );
                    } else {
                        let original = ctx.resource_refusal().expect("present operation refuses");
                        assert!(
                            matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                        );
                        assert_eq!(
                            (
                                original.dimension,
                                original.used,
                                original.additional,
                                original.operation
                            ),
                            (
                                ResourceDimension::WorkUnits,
                                cap,
                                1,
                                if cap % 2 == 0 {
                                    "creo feature entry traversal"
                                } else {
                                    "creo feature entry terminator"
                                }
                            )
                        );
                    }

                    if completed {
                        Ok(())
                    } else {
                        Err(CodecError::ResourceLimit(
                            ctx.resource_refusal().expect("original refusal"),
                        ))
                    }
                },
            );
        }
    }

    #[test]
    fn entry_candidates_stop_after_invalid_first_prefix() {
        let payload = [0xf7, 0xe3];
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &(["creo feature entry traversal"]),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = read_entries(&ctx, &payload, 0, 1);
                let completed = result.is_ok();
                if completed {
                    assert_eq!(result.expect("one invalid-prefix visit"), None);
                    let refusal = ctx
                        .charge_work_limit(1, "after invalid entry")
                        .expect_err("exact cap");
                    assert_eq!(
                        (refusal.dimension, refusal.used, refusal.additional),
                        (ResourceDimension::WorkUnits, 1, 1)
                    );
                } else {
                    let original = ctx.resource_refusal().expect("first entry refuses");
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    assert_eq!(
                        (
                            original.dimension,
                            original.used,
                            original.additional,
                            original.operation
                        ),
                        (
                            ResourceDimension::WorkUnits,
                            0,
                            1,
                            "creo feature entry traversal"
                        )
                    );
                }

                if completed {
                    Ok(())
                } else {
                    Err(CodecError::ResourceLimit(
                        ctx.resource_refusal().expect("original refusal"),
                    ))
                }
            },
        );
    }

    #[test]
    fn entity_tables_do_not_visit_absent_spans() {
        for count in 0..=2usize {
            let payload = vec![0xf8; count];
            // No row prefix or counted table exists. Each present byte is
            // visited by the row-boundary scan and the table-byte scan once.
            let total = 2 * u64::try_from(count).expect("fixture value fits u64");
            crate::test_support::assert_refusal_order(
                ResourceDimension::WorkUnits,
                &(if count == 0 {
                    Vec::new()
                } else {
                    vec![
                        "creo feature row boundary scan",
                        "creo generated entity byte traversal",
                    ]
                }),
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    policy.limits.max_materialized_bytes = 0;
                    policy.limits.max_retained_bytes = 0;
                    policy.limits.max_collection_items = 0;
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let result = entity_tables(
                        &ctx,
                        &payload,
                        &std::collections::BTreeSet::default(),
                        &std::collections::BTreeSet::default(),
                    );
                    let completed = result.is_ok();
                    if completed {
                        assert!(result.expect("exact present-byte work").is_empty());
                        let refusal = ctx
                            .charge_work_limit(1, "after entity table bytes")
                            .expect_err("exact cap");
                        assert_eq!(
                            (refusal.dimension, refusal.used, refusal.additional),
                            (ResourceDimension::WorkUnits, total, 1)
                        );
                    } else {
                        let original = ctx.resource_refusal().expect("present byte refuses");
                        assert!(
                            matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                        );
                        // Each admit_iter admits its entire exact-size byte range
                        // before execution, so the failing pass requests n at once.
                        let before_table_scan =
                            cap < u64::try_from(count).expect("fixture value fits u64");
                        assert_eq!(
                            (
                                original.dimension,
                                original.used,
                                original.additional,
                                original.operation
                            ),
                            (
                                ResourceDimension::WorkUnits,
                                if before_table_scan {
                                    0
                                } else {
                                    u64::try_from(count).expect("fixture value fits u64")
                                },
                                u64::try_from(count).expect("fixture value fits u64"),
                                if before_table_scan {
                                    "creo feature row boundary scan"
                                } else {
                                    "creo generated entity byte traversal"
                                }
                            )
                        );
                    }

                    if completed {
                        Ok(())
                    } else {
                        Err(CodecError::ResourceLimit(
                            ctx.resource_refusal().expect("original refusal"),
                        ))
                    }
                },
            );
        }
    }

    #[test]
    fn entity_fixed_recovery_routes_preserve_original_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let check = |refused| {
            let results = [
                entity_graph(&ctx, &[])
                    .map(|(entities, references)| entities.is_empty() && references.is_empty()),
                read_entries(&ctx, &[], 0, 0)
                    .map(|entries| entries.is_some_and(|entries| entries.is_empty())),
                read_entries(&ctx, &[], 1, 0).map(|entries| entries.is_none()),
                read_entries(&ctx, &[], 0, 1).map(|entries| entries.is_none()),
                entity_tables(
                    &ctx,
                    &[],
                    &std::collections::BTreeSet::default(),
                    &std::collections::BTreeSet::default(),
                )
                .map(|tables| tables.is_empty()),
            ];
            for result in results {
                if refused {
                    let original = ctx.resource_refusal().expect("seeded original refusal");
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                } else {
                    assert!(result.expect("free fixed recovery"));
                }
            }
        };
        check(false);
        assert_eq!(ctx.resource_refusal(), None);
        let original = ctx
            .charge_work_limit(1, "after fixed entity recovery")
            .expect_err("zero cap");
        check(true);
        assert_eq!(ctx.resource_refusal(), Some(original));
    }

    #[test]
    fn entry_candidates_admit_present_prefixes_after_variable_width_entries() {
        use std::mem::size_of;

        // A first entry can consume all bytes despite the initial count bound.
        // A partial second prefix still exists and must be visited.
        for (payload, declared, terminator_visits, second_present) in [
            ([7, 1, 0, 0, 0, 0xe3], 2, 4u64, false),
            ([7, 1, 0, 0, 0, 0xe3], 3, 4u64, false),
            ([7, 1, 0, 0, 0xe3, 0xf7], 2, 3u64, true),
        ] {
            let total = 1 + terminator_visits + u64::from(second_present);
            let backing = u64::try_from(4 * size_of::<super::FeatureEntityTableEntry>())
                .expect("fixture value fits u64");
            crate::test_support::assert_refusal_order(
                ResourceDimension::WorkUnits,
                &((0..total)
                    .map(|cap| {
                        if cap == 0 || (second_present && cap == 1 + terminator_visits) {
                            "creo feature entry traversal"
                        } else {
                            "creo feature entry terminator"
                        }
                    })
                    .collect::<Vec<_>>()),
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    policy.limits.max_materialized_bytes = backing;
                    policy.limits.max_retained_bytes = 0;
                    policy.limits.max_collection_items = 1;
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let result = read_entries(&ctx, &payload, 0, declared);
                    let completed = result.is_ok();
                    if completed {
                        assert_eq!(result.expect("only present input work"), None);
                        assert!(ctx.resource_refusal().is_none());
                        // The rejected first candidate dropped its entire Vec.
                        let storage = ctx
                            .reserve_scoped(backing, "after rejected entry backing")
                            .expect("all candidate storage was released");
                        drop(storage);
                        let original = ctx
                            .charge_work_limit(1, "after variable-width entry")
                            .expect_err("exact input work used");
                        assert_eq!((original.used, original.additional), (total, 1));
                        assert!(matches!(read_entries(&ctx, &payload, 0, declared),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                        assert_eq!(ctx.resource_refusal(), Some(original));
                    } else {
                        let original = ctx.resource_refusal().expect("present operation refuses");
                        assert!(
                            matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                        );
                        let at_entry = cap == 0 || (second_present && cap == 1 + terminator_visits);
                        assert_eq!(
                            (
                                original.dimension,
                                original.used,
                                original.additional,
                                original.operation
                            ),
                            (
                                ResourceDimension::WorkUnits,
                                cap,
                                1,
                                if at_entry {
                                    "creo feature entry traversal"
                                } else {
                                    "creo feature entry terminator"
                                }
                            )
                        );
                        assert!(matches!(read_entries(&ctx, &payload, 0, declared),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                        assert_eq!(ctx.resource_refusal(), Some(original));
                    }

                    if completed {
                        Ok(())
                    } else {
                        Err(CodecError::ResourceLimit(
                            ctx.resource_refusal().expect("original refusal"),
                        ))
                    }
                },
            );
        }
    }
}
