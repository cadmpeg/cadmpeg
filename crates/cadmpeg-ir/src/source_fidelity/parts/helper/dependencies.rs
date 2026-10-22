// SPDX-License-Identifier: Apache-2.0
//! Typed dependency closure and atomic preparation of surviving list edits.

use super::{PartOutput, reserve};
use crate::ids::UnknownId;
use crate::native::NativeRecord;
use crate::schema::{
    EntitySchema,
    rewrite::typed::{
        RewriteIdentities,
        prune::{DependencyPosition, DependencyReference, ReferencePruneEffect},
    },
};
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::{DecodeContext, u64_from_index};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

/// One typed owner whose list cannot yet represent the required deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredRecordPruning {
    /// Actual surviving record identity.
    pub record: String,
    /// Fixed/checked cardinality or missing typed position information.
    pub effect: ReferencePruneEffect,
}
/// Dependency finalization outcome; deferred output is not exportable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartDependencyPruning {
    /// Required dependencies are gone and surviving lists retain their order.
    Complete {
        /// Sorted identities removed by the dependency closure.
        dropped_records: Vec<String>,
    },
    /// No output records or lists were changed; owner representation work remains.
    Deferred {
        /// Surviving typed owners that cannot represent their pruned lists.
        records: Vec<DeferredRecordPruning>,
    },
}

type NativeVisit = fn(
    &DecodeContext<'_>,
    &NativeRecord,
    &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError>;
type NativePrepare =
    fn(&DecodeContext<'_>, &NativeRecord, &BTreeSet<String>) -> Result<NativeEdit, CodecError>;
#[derive(Debug, Clone, Copy)]
pub(super) struct NativeOwner {
    visit: NativeVisit,
    prepare: NativePrepare,
}
struct NativeEdit {
    effect: ReferencePruneEffect,
    replacement: Option<NativeRecord>,
}
impl NativeOwner {
    fn of<T: DeserializeOwned + Serialize + RewriteIdentities>() -> Self {
        fn prepare<T: DeserializeOwned + Serialize + RewriteIdentities>(
            ctx: &DecodeContext<'_>,
            record: &NativeRecord,
            deleted: &BTreeSet<String>,
        ) -> Result<NativeEdit, CodecError> {
            let prepared = crate::native::prune::record::<T>(ctx, record, deleted)?;
            Ok(NativeEdit {
                effect: prepared.effect,
                replacement: prepared.replacement.map(|(record, _)| record),
            })
        }
        Self {
            visit: crate::native::prune::visit::<T>,
            prepare: prepare::<T>,
        }
    }
}

#[derive(Debug)]
pub(super) struct FailureHome {
    id: UnknownId,
    pub(super) dropped: BTreeSet<String>,
    pub(super) loss_index: usize,
    prefix: String,
}
#[derive(Debug)]
struct Edge {
    target: String,
    position: DependencyPosition,
}

impl PartOutput {
    pub(super) fn register_native_owner<T: DeserializeOwned + Serialize + RewriteIdentities>(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<(), CodecError> {
        ctx.retain_bytes(
            storage_bytes(&[id.len(), size_of::<(String, NativeOwner)>()])?,
            "part native typed owner",
        )?;
        if self
            .native_owners
            .insert(id.to_owned(), NativeOwner::of::<T>())
            .is_some()
        {
            return Err(CodecError::malformed("duplicate part native typed owner"));
        }
        Ok(())
    }

    pub(super) fn record_failure(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: UnknownId,
        dropped: Vec<String>,
        prefix: String,
    ) -> Result<(), CodecError> {
        let bytes = dropped
            .iter()
            .try_fold(prefix.len(), |sum, id| sum.checked_add(id.len()))
            .and_then(|sum| sum.checked_add(size_of::<FailureHome>()))
            .ok_or_else(|| CodecError::malformed("part failure staging storage overflows"))?;
        ctx.retain_bytes(u64_from_index(bytes), "part failure staging")?;
        reserve(ctx, &mut self.failures, 1)?;
        self.failures.push(FailureHome {
            id,
            dropped: dropped.into_iter().collect(),
            loss_index: self
                .losses
                .len()
                .checked_sub(1)
                .ok_or_else(|| CodecError::malformed("part failure has no diagnostic"))?,
            prefix,
        });
        Ok(())
    }

    /// Drop transitively required records, then prune only typed reference lists.
    ///
    /// All list edits are prepared and re-admitted through their actual owning
    /// types before mutation. A deferred result leaves the staged output intact.
    /// This operation does not derive B1 classes or source-byte coverage.
    pub fn prune_dependencies(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<PartDependencyPruning, CodecError> {
        let mut graph = BTreeMap::<String, Vec<Edge>>::new();
        macro_rules! model_edges {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
                for record in &self.model.$field {
                    let mut edges = Vec::new();
                    record.visit_dependency_references(ctx, &mut |edge| add_edge(ctx, &mut edges, edge))?;
                    add_record(ctx, &mut graph, record.identity(), edges)?;
                }
            )* };
        }
        crate::document::arena_registry!(model_edges);
        for (_, namespace) in &self.native.0 {
            for records in namespace.arenas().values() {
                for record in records {
                    let Some(owner) = self.native_owners.get(record.id()) else {
                        return Ok(PartDependencyPruning::Deferred {
                            records: vec![DeferredRecordPruning {
                                record: record.id().to_owned(),
                                effect: ReferencePruneEffect::UnknownReferencePosition {
                                    owner: "unregistered native owner",
                                },
                            }],
                        });
                    };
                    let mut edges = Vec::new();
                    (owner.visit)(ctx, record, &mut |edge| add_edge(ctx, &mut edges, edge))?;
                    add_record(ctx, &mut graph, record.id(), edges)?;
                }
            }
        }
        let owners = closure(ctx, &graph, &self.failures)?;
        let mut deleted = BTreeSet::new();
        for id in owners.keys() {
            ctx.retain_bytes(
                storage_bytes(&[id.len(), size_of::<String>()])?,
                "deleted dependency identity",
            )?;
            deleted.insert(id.clone());
        }
        let mut deferred = Vec::new();
        let mut replacements = crate::document::Model::default();
        macro_rules! prepare_model {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
                for record in &self.model.$field {
                    if !deleted.contains(record.identity()) {
                        let prepared = crate::native::prune::model(ctx, record, &deleted)?;
                        if !prepared.effect.can_apply() {
                            defer(ctx, &mut deferred, record.identity(), prepared.effect)?;
                        } else if let Some((_, replacement)) = prepared.replacement {
                            reserve(ctx, &mut replacements.$field, 1)?;
                            replacements.$field.push(replacement);
                        }
                    }
                }
            )* };
        }
        crate::document::arena_registry!(prepare_model);
        let mut native_replacements = BTreeMap::new();
        for (_, namespace) in &self.native.0 {
            for records in namespace.arenas().values() {
                for record in records {
                    if deleted.contains(record.id()) {
                        continue;
                    }
                    let owner = &self.native_owners[record.id()];
                    let prepared = (owner.prepare)(ctx, record, &deleted)?;
                    if !prepared.effect.can_apply() {
                        defer(ctx, &mut deferred, record.id(), prepared.effect)?;
                    } else if let Some(record) = prepared.replacement {
                        ctx.retain_bytes(
                            storage_bytes(&[
                                record.id().len(),
                                size_of::<(String, NativeRecord)>(),
                            ])?,
                            "native dependency replacement",
                        )?;
                        native_replacements.insert(record.id().to_owned(), record);
                    }
                }
            }
        }
        if !deferred.is_empty() {
            return Ok(PartDependencyPruning::Deferred { records: deferred });
        }
        // Prepare diagnostic text before changing any surviving record or list.
        let mut messages = Vec::new();
        let mut drops = Vec::new();
        for failure in &self.failures {
            let mut assigned = BTreeSet::new();
            for (id, home) in &owners {
                if home == &failure.id {
                    ctx.retain_bytes(
                        storage_bytes(&[id.len(), size_of::<String>()])?,
                        "unreadable dependency identity",
                    )?;
                    assigned.insert(id.clone());
                }
            }
            let message = format!(
                "{} dropped={}",
                failure.prefix,
                serde_json::to_string(&assigned).map_err(CodecError::malformed)?
            );
            ctx.retain_bytes(u64_from_index(message.len()), "part dependency diagnostic")?;
            reserve(ctx, &mut messages, 1)?;
            reserve(ctx, &mut drops, 1)?;
            messages.push(message);
            drops.push(assigned);
        }
        macro_rules! commit_model {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
                let mut updates = replacements.$field.into_iter().peekable();
                self.model.$field.retain_mut(|record| {
                    if deleted.contains(record.identity()) { return false; }
                    if let Some(replacement) = updates.next_if(|value| value.identity() == record.identity()) { *record = replacement; }
                    true
                });
            )* };
        }
        crate::document::arena_registry!(commit_model);
        for namespace in self.native.0.values_mut() {
            for records in namespace.arenas_mut().values_mut() {
                records.retain_mut(|record| {
                    if deleted.contains(record.id()) {
                        return false;
                    }
                    if let Some(replacement) = native_replacements.remove(record.id()) {
                        *record = replacement;
                    }
                    true
                });
            }
        }
        let mut annotations = crate::annotations::AnnotationBuilder::resume(std::mem::take(
            &mut self.fidelity.annotations,
        ));
        for id in &deleted {
            annotations.remove_entity(id);
        }
        self.fidelity.annotations = annotations.build();
        self.fidelity
            .record_layout
            .retain(|id, _| !deleted.contains(id));
        self.fidelity
            .retained_records
            .retain(|id, _| !deleted.contains(id.as_str()));
        self.ids.retain(|id| !deleted.contains(id));
        self.native_owners.retain(|id, _| !deleted.contains(id));
        for ((failure, message), dropped) in self.failures.iter_mut().zip(messages).zip(drops) {
            self.losses[failure.loss_index].message = message;
            failure.dropped = dropped;
        }
        Ok(PartDependencyPruning::Complete {
            dropped_records: deleted.into_iter().collect(),
        })
    }
}

fn storage_bytes(lengths: &[usize]) -> Result<u64, CodecError> {
    lengths
        .iter()
        .try_fold(0_usize, |sum, length| sum.checked_add(*length))
        .map(u64_from_index)
        .ok_or_else(|| CodecError::malformed("part dependency storage overflows"))
}

fn add_edge(
    ctx: &DecodeContext<'_>,
    edges: &mut Vec<Edge>,
    edge: DependencyReference<'_>,
) -> Result<(), CodecError> {
    ctx.retain_bytes(
        u64_from_index(edge.target.len()),
        "part typed dependency target",
    )?;
    reserve(ctx, edges, 1)?;
    edges.push(Edge {
        target: edge.target.to_owned(),
        position: edge.position,
    });
    Ok(())
}
fn add_record(
    ctx: &DecodeContext<'_>,
    graph: &mut BTreeMap<String, Vec<Edge>>,
    id: &str,
    edges: Vec<Edge>,
) -> Result<(), CodecError> {
    ctx.retain_bytes(
        storage_bytes(&[id.len(), size_of::<(String, Vec<Edge>)>()])?,
        "part dependency record",
    )?;
    if graph.insert(id.to_owned(), edges).is_some() {
        return Err(CodecError::malformed("duplicate part dependency record"));
    }
    Ok(())
}
fn defer(
    ctx: &DecodeContext<'_>,
    records: &mut Vec<DeferredRecordPruning>,
    id: &str,
    effect: ReferencePruneEffect,
) -> Result<(), CodecError> {
    ctx.retain_bytes(u64_from_index(id.len()), "deferred typed dependency record")?;
    reserve(ctx, records, 1)?;
    records.push(DeferredRecordPruning {
        record: id.to_owned(),
        effect,
    });
    Ok(())
}
fn closure(
    ctx: &DecodeContext<'_>,
    graph: &BTreeMap<String, Vec<Edge>>,
    failures: &[FailureHome],
) -> Result<BTreeMap<String, UnknownId>, CodecError> {
    let mut required = BTreeMap::<String, Vec<String>>::new();
    for (id, edges) in graph {
        for edge in edges {
            if edge.position == DependencyPosition::Required && &edge.target != id {
                ctx.retain_bytes(
                    storage_bytes(&[
                        id.len(),
                        edge.target.len(),
                        size_of::<(String, Vec<String>)>(),
                    ])?,
                    "required dependency edge",
                )?;
                let values = required.entry(edge.target.clone()).or_default();
                reserve(ctx, values, 1)?;
                values.push(id.clone());
            }
        }
    }
    let mut pending = BTreeSet::new();
    for failure in failures {
        for id in &failure.dropped {
            ctx.retain_bytes(
                storage_bytes(&[
                    failure.id.as_str().len(),
                    id.len(),
                    size_of::<(UnknownId, String)>(),
                ])?,
                "dependency closure seed",
            )?;
            pending.insert((failure.id.clone(), id.clone()));
        }
    }
    let mut owners = BTreeMap::new();
    while let Some((home, id)) = pending.pop_first() {
        if owners.contains_key(&id) {
            continue;
        }
        if let Some(records) = required.get(&id) {
            for record in records {
                ctx.retain_bytes(
                    storage_bytes(&[
                        home.as_str().len(),
                        record.len(),
                        size_of::<(UnknownId, String)>(),
                    ])?,
                    "dependency closure frontier",
                )?;
                pending.insert((home.clone(), record.clone()));
            }
        }
        ctx.retain_bytes(
            storage_bytes(&[
                id.len(),
                home.as_str().len(),
                size_of::<(String, UnknownId)>(),
            ])?,
            "dropped dependency owner",
        )?;
        owners.insert(id, home);
    }
    Ok(owners)
}

#[cfg(test)]
mod tests;
