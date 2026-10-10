// SPDX-License-Identifier: Apache-2.0
//! Generated entries in source order with fixed-key membership indexes.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::entity::FeatureEntityTableEntry;

#[derive(Debug, Clone, Default)]
pub(crate) struct EntityRows {
    rows: Vec<FeatureEntityTableEntry>,
    membership: HashMap<u32, bool>,
    surfaces: BTreeSet<u32>,
    ordered_surfaces: Vec<u32>,
}

impl PartialEq for EntityRows {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows && self.surfaces == other.surfaces
    }
}
impl Eq for EntityRows {}

impl std::ops::Deref for EntityRows {
    type Target = [FeatureEntityTableEntry];
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

impl<'a> IntoIterator for &'a EntityRows {
    type Item = &'a FeatureEntityTableEntry;
    type IntoIter = std::slice::Iter<'a, FeatureEntityTableEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

impl EntityRows {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        rows: Vec<FeatureEntityTableEntry>,
        model_surfaces: &BTreeSet<u32>,
    ) -> Result<Self, CodecError> {
        let mut result = Self {
            rows,
            ..Self::default()
        };
        for row in ctx.admit_iter(&result.rows, "creo generated entry index traversal")? {
            let surface = ctx.contains_btree_set(
                model_surfaces,
                &row.entity_id,
                "creo generated entry surface lookup",
            )?;
            ctx.entry_hash_map(
                &mut result.membership,
                row.entity_id,
                "creo generated entry identity index",
            )?
            .or_insert(surface);
            if surface {
                ctx.insert_btree_set(
                    &mut result.surfaces,
                    row.entity_id,
                    "creo feature table surface ids",
                )?;
                ctx.push_vec(
                    &mut result.ordered_surfaces,
                    row.entity_id,
                    "creo generated entry surface order",
                )?;
            }
        }
        Ok(result)
    }

    pub(crate) fn as_slice(&self) -> &[FeatureEntityTableEntry] {
        &self.rows
    }
    pub(crate) fn contains_surface(&self, id: u32) -> bool {
        self.membership.get(&id) == Some(&true)
    }
    pub(crate) fn contains_non_surface(&self, id: u32) -> bool {
        self.membership.get(&id) == Some(&false)
    }
    pub(crate) fn surfaces(&self) -> &BTreeSet<u32> {
        &self.surfaces
    }
    pub(crate) fn surfaces_in_order(&self) -> impl Iterator<Item = u32> + '_ {
        self.ordered_surfaces.iter().copied()
    }

    /// Rebase source offsets without changing indexed identities.
    pub(crate) fn relocate_offsets(
        &mut self,
        ctx: &DecodeContext<'_>,
        base: usize,
    ) -> Result<(), CodecError> {
        for row in ctx.admit_iter(&mut self.rows, "creo record child relocation traversal")? {
            row.offset += base;
            row.end_offset += base;
        }
        Ok(())
    }

    /// Borrow only the source offsets that a fixture changes.
    #[cfg(test)]
    pub(crate) fn offsets_mut(&mut self) -> impl Iterator<Item = (&mut usize, &mut usize)> {
        self.rows
            .iter_mut()
            .map(|row| (&mut row.offset, &mut row.end_offset))
    }
}

#[cfg(test)]
impl EntityRows {
    pub(crate) fn from_fixture(
        rows: Vec<FeatureEntityTableEntry>,
        model_surfaces: &BTreeSet<u32>,
    ) -> Self {
        let surfaces = rows
            .iter()
            .map(|row| row.entity_id)
            .filter(|id| model_surfaces.contains(id))
            .collect();
        let mut result = Self {
            rows,
            surfaces,
            ..Self::default()
        };
        result.reindex_fixture();
        result
    }

    fn reindex_fixture(&mut self) {
        self.membership = self.surfaces.iter().copied().map(|id| (id, true)).collect();
        self.ordered_surfaces = Vec::new();
        for row in &self.rows {
            let surface = self.surfaces.contains(&row.entity_id);
            self.membership.insert(row.entity_id, surface);
            if surface {
                self.ordered_surfaces.push(row.entity_id);
            }
        }
    }

    pub(crate) fn replace(&mut self, rows: Vec<FeatureEntityTableEntry>) {
        self.rows = rows;
        self.reindex_fixture();
    }

    pub(crate) fn mark_surfaces(&mut self, model_surfaces: &BTreeSet<u32>) {
        self.surfaces = self
            .rows
            .iter()
            .map(|row| row.entity_id)
            .filter(|id| model_surfaces.contains(id))
            .collect();
        self.reindex_fixture();
    }

    pub(crate) fn edit(&mut self, index: usize, edit: impl FnOnce(&mut FeatureEntityTableEntry)) {
        edit(&mut self.rows[index]);
        self.reindex_fixture();
    }

    pub(crate) fn push(&mut self, row: FeatureEntityTableEntry) {
        self.rows.push(row);
        self.reindex_fixture();
    }

    pub(crate) fn insert(&mut self, index: usize, row: FeatureEntityTableEntry) {
        self.rows.insert(index, row);
        self.reindex_fixture();
    }

    pub(crate) fn retain(&mut self, keep: impl FnMut(&FeatureEntityTableEntry) -> bool) {
        self.rows.retain(keep);
        self.reindex_fixture();
    }

    pub(crate) fn pop(&mut self) -> Option<FeatureEntityTableEntry> {
        let row = self.rows.pop();
        self.reindex_fixture();
        row
    }
}

#[cfg(test)]
mod tests {
    use super::EntityRows;
    use crate::feature::entity::dummy_table_entry;
    use std::collections::BTreeSet;

    #[test]
    fn entry_indexes_preserve_order_and_fixture_surface_cache() {
        let mut rows = EntityRows::from_fixture(
            vec![
                dummy_table_entry(7),
                dummy_table_entry(9),
                dummy_table_entry(7),
            ],
            &BTreeSet::from([7]),
        );
        assert_eq!(rows.surfaces_in_order().collect::<Vec<_>>(), [7, 7]);
        assert!(rows.contains_non_surface(9));
        rows.edit(1, |row| row.entity_id = 11);
        assert!(!rows.contains_non_surface(9));
        assert!(rows.contains_non_surface(11));
        rows.pop();
        rows.pop();
        rows.pop();
        assert!(rows.contains_surface(7));
        assert!(rows.surfaces_in_order().next().is_none());
        rows.push(dummy_table_entry(11));
        rows.mark_surfaces(&BTreeSet::from([11]));
        assert!(!rows.contains_surface(7));
        assert!(rows.contains_surface(11));
        assert_eq!(rows.surfaces_in_order().collect::<Vec<_>>(), [11]);
    }

    #[test]
    fn offset_relocation_admits_actual_rows_before_mutation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let fixture = || {
            let mut first = dummy_table_entry(7);
            first.offset = 3;
            first.end_offset = 5;
            let mut second = dummy_table_entry(9);
            second.offset = 8;
            second.end_offset = 11;
            EntityRows::from_fixture(vec![first, second], &BTreeSet::from([7]))
        };
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &(["creo record child relocation traversal"]), |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut rows = fixture();
            let original_rows = rows.clone();
            let result = rows.relocate_offsets(&ctx, 17);
            let completed = result.is_ok();
            if !completed {
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("two present rows need two visits");
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.operation, "creo record child relocation traversal");
                assert_eq!(original.used, 0);
                assert_eq!(original.additional, 2);
                assert_eq!(rows, original_rows);
                assert!(matches!(rows.relocate_offsets(&ctx, 17),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!(rows, original_rows);
                assert_eq!(ctx.resource_refusal(), Some(original));
            } else {
                result.expect("exact two-row traversal");
                assert_eq!((rows[0].offset, rows[0].end_offset), (20, 22));
                assert_eq!((rows[1].offset, rows[1].end_offset), (25, 28));
                assert!(rows.contains_surface(7));
                assert!(rows.contains_non_surface(9));
                assert_eq!(rows.surfaces(), &BTreeSet::from([7]));
                assert_eq!(rows.surfaces_in_order().collect::<Vec<_>>(), [7]);
                for (row, before) in rows.iter().zip(original_rows.iter()) {
                    assert_eq!(row.entity_id, before.entity_id);
                    assert_eq!(row.payload, before.payload);
                    assert_eq!(row.prefixed, before.prefixed);
                }
                let original = ctx.charge_work_limit(1, "after actual row relocation")
                    .expect_err("the two admitted visits use the exact cap");
                assert_eq!(original.used, 2);
                assert_eq!(original.additional, 1);
                assert!(matches!(rows.relocate_offsets(&ctx, 17),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((rows[0].offset, rows[0].end_offset), (20, 22));
            }

            if completed { Ok(()) } else {
                Err(CodecError::ResourceLimit(ctx.resource_refusal().expect("original refusal")))
            }
});
    }

    #[test]
    fn empty_offset_relocation_is_free_and_preserves_original_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut rows = EntityRows::default();
        rows.relocate_offsets(&ctx, 17).expect("no source rows");
        assert!(rows.is_empty());
        assert!(ctx.resource_refusal().is_none());
        let original = ctx.charge_work_limit(1, "after empty relocation").expect_err("zero cap");
        assert!(matches!(rows.relocate_offsets(&ctx, 17),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(rows.is_empty());
    }
}
