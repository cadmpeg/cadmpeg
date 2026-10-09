// SPDX-License-Identifier: Apache-2.0
//! Transactions that copy only the identities selected for editing.

use super::{
    admit_identity_work, AnnotationBuilder, Annotations, ExactnessNote, FieldName, NonEmptyMap,
};
use crate::provenance::Exactness;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

/// A sparse copy of selected annotation identities with live temporary storage.
/// The base remains borrowed and unchanged until preparation succeeds.
#[derive(Debug)]
pub struct SparseAnnotationTransaction<'base, 'ctx> {
    base: &'base Annotations,
    changes: Annotations,
    selected: BTreeSet<String>,
    storage: ScopedReservation<'ctx>,
    ctx: &'ctx DecodeContext<'ctx>,
}

/// An admitted annotation replacement with no remaining fallible application work.
#[derive(Debug)]
pub struct PreparedAnnotationDelta {
    changes: Annotations,
    selected: BTreeSet<String>,
}

impl Annotations {
    /// Start a transaction without scanning or copying the existing tables.
    pub fn sparse_transaction<'base, 'ctx>(
        &'base self,
        ctx: &'ctx DecodeContext<'ctx>,
        operation: &'static str,
    ) -> Result<SparseAnnotationTransaction<'base, 'ctx>, CodecError> {
        Ok(SparseAnnotationTransaction {
            base: self,
            changes: Annotations::default(),
            selected: BTreeSet::new(),
            storage: ctx.reserve_scoped(0, operation)?,
            ctx,
        })
    }
    /// Admit owned complete replacements without copying the prior tables.
    /// Entries absent from `changes` remain unchanged.
    /// The owned storage in `changes` must already be admitted.
    pub fn prepare_overlay<'ctx>(
        &self,
        ctx: &'ctx DecodeContext<'ctx>,
        changes: Annotations,
        operation: &'static str,
    ) -> Result<PreparedAnnotationDelta, CodecError> {
        let mut transaction = self.sparse_transaction(ctx, operation)?;
        transaction.storage.with_storage(|| {
            for id in changes.provenance.keys().chain(changes.exactness.keys()) {
                admit_identity_work(
                    ctx,
                    transaction.selected.len(),
                    id.len(),
                    3,
                    "sparse annotation selection",
                )?;
                if !transaction.selected.contains(id) {
                    let key = ctx.copy_retained_text(id, "sparse annotation selection")?;
                    ctx.insert_btree_set(
                        &mut transaction.selected,
                        key,
                        "sparse annotation selection",
                    )?;
                }
            }
            Ok::<_, CodecError>(())
        })?;
        transaction.changes = changes;
        transaction.prepare()
    }
}

impl<'base> SparseAnnotationTransaction<'base, '_> {
    /// Borrow the unchanged base tables.
    pub fn base(&self) -> &'base Annotations {
        self.base
    }

    /// Read only the selected candidate annotations.
    pub fn annotations(&self) -> &Annotations {
        &self.changes
    }

    /// Copy one identity's annotations once, then edit them in temporary storage.
    /// The callback may change or remove only entries at `id`.
    fn edit<T, E: From<CodecError>>(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut Annotations) -> Result<T, E>,
    ) -> Result<T, E> {
        let ctx = self.ctx;
        admit_identity_work(
            ctx,
            self.selected.len(),
            id.len(),
            3,
            "sparse annotation selection",
        )?;
        if !self.selected.contains(id) {
            self.storage.with_storage(|| {
                let key = ctx.copy_retained_text(id, "sparse annotation selection")?;
                ctx.insert_btree_set(&mut self.selected, key, "sparse annotation selection")?;
                copy_identity(ctx, self.base, &mut self.changes, id)
            })?;
        }
        self.storage.with_storage(|| apply(&mut self.changes))
    }

    pub(crate) fn select(&mut self, id: &str) -> Result<(), CodecError> {
        self.edit(id, |_| Ok::<_, CodecError>(()))
    }

    pub(crate) fn selected(&self, id: &str) -> Result<bool, CodecError> {
        admit_identity_work(
            self.ctx,
            self.selected.len(),
            id.len(),
            1,
            "sparse annotation selection",
        )?;
        Ok(self.selected.contains(id))
    }

    /// Set a field override without copying other entity annotations.
    pub fn field_exactness(
        &mut self,
        id: &str,
        field: &str,
        exactness: Exactness,
    ) -> Result<(), super::AnnotationFieldError> {
        let ctx = self.ctx;
        self.edit(id, |annotations| {
            let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
            let result = builder
                .field_exactness(ctx, id, field, exactness)
                .map(|_| ());
            *annotations = builder.build();
            result
        })
    }

    /// Replace provenance for one selected identity.
    pub fn note(
        &mut self,
        id: &str,
        stream: &super::StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) -> Result<(), CodecError> {
        let ctx = self.ctx;
        self.edit(id, |annotations| {
            let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
            let result = builder.note(ctx, id, stream, offset, tag);
            *annotations = builder.build();
            result
        })
    }

    /// Remove both annotation entries at one selected identity.
    pub fn remove_entity(&mut self, id: &str) -> Result<(), CodecError> {
        let ctx = self.ctx;
        self.edit(id, |annotations| {
            admit_identity_work(
                ctx,
                annotations.provenance.len(),
                id.len(),
                1,
                "sparse annotation removal",
            )?;
            admit_identity_work(
                ctx,
                annotations.exactness.len(),
                id.len(),
                1,
                "sparse annotation removal",
            )?;
            annotations.provenance.remove(id);
            annotations.exactness.remove(id);
            Ok::<_, CodecError>(())
        })
    }

    /// Set exactness while preserving existing field overrides at the selected identity.
    pub fn exactness(&mut self, id: &str, exactness: Exactness) -> Result<(), CodecError> {
        let ctx = self.ctx;
        self.edit(id, |annotations| {
            let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
            let result = builder.exactness(ctx, id, exactness).map(|_| ());
            *annotations = builder.build();
            result
        })
    }

    /// Admit all destination nodes and retained storage before either table changes.
    pub fn prepare(mut self) -> Result<PreparedAnnotationDelta, CodecError> {
        let ctx = self.ctx;
        let mut provenance_len = self.base.provenance.len();
        let mut exactness_len = self.base.exactness.len();
        for id in &self.selected {
            admit_identity_work(
                ctx,
                self.changes.provenance.len(),
                id.len(),
                2,
                "sparse annotation application",
            )?;
            admit_identity_work(
                ctx,
                self.changes.exactness.len(),
                id.len(),
                2,
                "sparse annotation application",
            )?;
            admit_identity_work(
                ctx,
                provenance_len,
                id.len(),
                2,
                "sparse annotation application",
            )?;
            admit_identity_work(
                ctx,
                exactness_len,
                id.len(),
                2,
                "sparse annotation application",
            )?;
            self.storage.with_storage(|| {
                if self.changes.provenance.contains_key(id) && !self.base.provenance.contains_key(id) {
                    ctx.admit_btree_node_storage::<String, crate::provenance::AnnotationProvenance>(provenance_len, "sparse annotation application")?;
                    ctx.charge_collection_items(1, "sparse annotation application")?;
                    provenance_len += 1;
                }
                if self.changes.exactness.contains_key(id) && !self.base.exactness.contains_key(id) {
                    ctx.admit_btree_node_storage::<String, ExactnessNote>(exactness_len, "sparse annotation application")?;
                    ctx.charge_collection_items(1, "sparse annotation application")?;
                    exactness_len += 1;
                }
                Ok::<_, CodecError>(())
            })?;
        }
        self.storage.commit()?;
        Ok(PreparedAnnotationDelta {
            changes: self.changes,
            selected: self.selected,
        })
    }
}

impl PreparedAnnotationDelta {
    /// Apply the admitted replacements and deletions without rebuilding either table.
    /// The destination must be the unchanged base used for preparation. Moving
    /// the base is permitted; intervening annotation edits are not.
    pub fn apply(mut self, annotations: &mut Annotations) {
        for id in self.selected {
            if let Some((key, value)) = self.changes.provenance.remove_entry(&id) {
                annotations.provenance.insert(key, value);
            } else {
                annotations.provenance.remove(&id);
            }
            if let Some((key, value)) = self.changes.exactness.remove_entry(&id) {
                annotations.exactness.insert(key, value);
            } else {
                annotations.exactness.remove(&id);
            }
        }
    }
}

fn copy_identity(
    ctx: &DecodeContext<'_>,
    base: &Annotations,
    changes: &mut Annotations,
    id: &str,
) -> Result<(), CodecError> {
    admit_identity_work(
        ctx,
        base.provenance.len(),
        id.len(),
        1,
        "sparse annotation provenance lookup",
    )?;
    admit_identity_work(
        ctx,
        base.exactness.len(),
        id.len(),
        1,
        "sparse annotation exactness lookup",
    )?;
    if let Some(source) = base.provenance.get(id) {
        admit_identity_work(
            ctx,
            changes.provenance.len(),
            id.len(),
            2,
            "sparse annotation provenance insertion",
        )?;
        let key = ctx.copy_retained_text(id, "sparse annotation provenance")?;
        let source = source.try_clone_for_decode(ctx, "sparse annotation provenance")?;
        ctx.insert_btree_map(
            &mut changes.provenance,
            key,
            source,
            "sparse annotation provenance",
        )?;
    }
    if let Some(note) = base.exactness.get(id) {
        admit_identity_work(
            ctx,
            changes.exactness.len(),
            id.len(),
            2,
            "sparse annotation exactness insertion",
        )?;
        let key = ctx.copy_retained_text(id, "sparse annotation exactness")?;
        let mut fields = BTreeMap::new();
        for (field, value) in note.fields() {
            admit_identity_work(
                ctx,
                fields.len(),
                field.as_str().len(),
                2,
                "sparse annotation field insertion",
            )?;
            let field =
                FieldName(ctx.copy_retained_text(field.as_str(), "sparse annotation fields")?);
            ctx.insert_btree_map(&mut fields, field, *value, "sparse annotation fields")?;
        }
        let note = match note {
            ExactnessNote::Entity { entity, .. } => ExactnessNote::Entity {
                entity: *entity,
                fields,
            },
            ExactnessNote::Fields { .. } => ExactnessNote::Fields {
                fields: NonEmptyMap(fields),
            },
        };
        ctx.insert_btree_map(
            &mut changes.exactness,
            key,
            note,
            "sparse annotation exactness",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
