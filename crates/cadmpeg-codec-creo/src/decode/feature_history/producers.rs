// SPDX-License-Identifier: Apache-2.0
//! Entity producers and source-ordered surface bindings.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use crate::feature::entity::FeatureEntityTable;
use std::collections::HashMap;

type Position = (usize, usize, usize, usize);

struct ProducerEntry {
    owner: u32,
    offset: usize,
    position: Position,
}

struct ProducerFacts {
    owner: Option<u32>,
    entries: Vec<ProducerEntry>,
}

struct SurfaceBinding {
    surface: u32,
    table_offset: usize,
    entry_offset: usize,
}

pub(super) struct ProducerRows<'ctx> {
    producers: HashMap<u32, ProducerFacts>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx> ProducerRows<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, tables: &[FeatureEntityTable]) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo entity producer index")?;
        let mut producers = HashMap::new();
        for (table_index, table) in ctx.admit_iter(tables, "creo entity producer tables")?.enumerate() {
            for (entry_index, entry) in ctx.admit_iter(&table.entries, "creo entity producer entries")?.enumerate() {
                if entry.class_id() == 200 {
                    let facts = storage.with_storage(|| ctx.entry_hash_map(&mut producers, entry.entity_id, "creo entity producer nodes"))?
                        .or_insert_with(|| ProducerFacts { owner: Some(table.feature_id), entries: Vec::new() });
                    if facts.owner != Some(table.feature_id) { facts.owner = None; }
                    storage.with_storage(|| ctx.push_vec(&mut facts.entries, ProducerEntry {
                        owner: table.feature_id, offset: entry.offset,
                        position: (table.offset, entry.offset, table_index, entry_index),
                    }, "creo entity producer witnesses"))?;
                }

            }
        }
        Ok(Self { producers, _storage: storage })
    }

    pub(super) fn owner(&self, entity: u32) -> Option<u32> {
        self.producers.get(&entity).and_then(|facts| facts.owner)
    }

    pub(super) fn preceding_owner(&self, ctx: &DecodeContext<'_>, entity: u32, offset: usize) -> Result<Option<u32>, CodecError> {
        let Some(facts) = self.producers.get(&entity) else { return Ok(None); };
        Ok(crate::decode::uniqueness::exactly_one_by(ctx, &facts.entries,
            |entry| Ok(entry.offset < offset), "creo preceding entity producer witnesses")?.map(|entry| entry.owner))
    }

    pub(super) fn has_prior_producer(&self, ctx: &DecodeContext<'_>, entity: u32, consumer: u32, position: Position) -> Result<bool, CodecError> {
        let Some(facts) = self.producers.get(&entity) else { return Ok(false); };
        Ok(crate::decode::uniqueness::exactly_one_by(ctx, &facts.entries,
            |entry| Ok(entry.owner != consumer && entry.position < position), "creo prior knit producer witnesses")?.is_some())
    }

}

pub(super) struct SurfaceBindings<'ctx> {
    bindings: HashMap<(u32, u32), Vec<SurfaceBinding>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx> SurfaceBindings<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, tables: &[FeatureEntityTable]) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo entity surface binding index")?;
        let mut bindings = HashMap::new();
        for table in ctx.admit_iter(tables, "creo entity surface binding tables")? {
            if table.table_class_id != 100 { continue; }
            for entry in ctx.admit_iter(&table.entries, "creo entity surface binding entries")? {
                let rows = storage.with_storage(|| ctx.entry_hash_map(&mut bindings, (table.feature_id, entry.entity_id), "creo entity surface binding nodes"))?.or_insert_with(Vec::new);
                storage.with_storage(|| ctx.push_vec(rows, SurfaceBinding {
                    surface: entry.class_id(), table_offset: table.offset, entry_offset: entry.offset,
                }, "creo entity surface bindings"))?;
            }
        }
        Ok(Self { bindings, _storage: storage })
    }

    pub(super) fn preceding_surface(&self, ctx: &DecodeContext<'_>, owner: u32, entity: u32, offset: usize) -> Result<Option<u32>, CodecError> {
        let Some(rows) = self.bindings.get(&(owner, entity)) else { return Ok(None); };
        Ok(crate::decode::uniqueness::exactly_one_by(ctx, rows,
            |entry| Ok(entry.table_offset < offset && entry.entry_offset < offset), "creo preceding entity surface bindings")?.map(|entry| entry.surface))
    }
}

#[cfg(test)]
mod tests {
    use super::ProducerRows;
    use crate::feature::entity::{entry_payload, FeatureEntityTable, FeatureEntityTableEntry};

    fn table(owner: u32, entity: u32, offset: usize) -> FeatureEntityTable {
        FeatureEntityTable::new(owner, 67, vec![FeatureEntityTableEntry {
            payload: entry_payload(200, None, None, None), entity_id: entity,
            prefixed: true, offset: offset + 1, end_offset: offset + 2,
        }], &std::collections::BTreeSet::new(), offset)
    }

    #[test]
    fn producer_consensus_and_preceding_witness_uniqueness_are_distinct() {
        let tables = [table(3, 101, 10), table(3, 101, 20), table(7, 102, 30), table(9, 102, 40)];
        crate::decode::with_test_decode_ctx(|ctx| {
            let rows = ProducerRows::new(ctx, &tables)?;
            assert_eq!(rows.owner(101), Some(3));
            assert_eq!(rows.owner(102), None);
            assert_eq!(rows.owner(103), None);
            assert_eq!(rows.preceding_owner(ctx, 101, 11)?, None);
            assert_eq!(rows.preceding_owner(ctx, 101, 12)?, Some(3));
            assert_eq!(rows.preceding_owner(ctx, 101, 22)?, None);
            assert!(rows.has_prior_producer(ctx, 101, 7, (20, 20, 1, 0))?);
            assert!(!rows.has_prior_producer(ctx, 101, 7, (30, 0, 2, 0))?);
            assert!(!rows.has_prior_producer(ctx, 101, 3, (30, 0, 2, 0))?);
            assert_eq!(rows.preceding_owner(ctx, 102, 35)?, Some(7));
            Ok::<_, cadmpeg_core::CodecError>(())
        }).expect("service producer queries");
        let error = crate::test_support::last_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo entity producer witnesses", |ctx| {
                let rows = ProducerRows::new(ctx, &tables)?;
                Ok::<_, cadmpeg_core::CodecError>(rows.owner(101))
            });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.operation == "creo entity producer witnesses"));
    }
}
