// SPDX-License-Identifier: Apache-2.0
//! Typed dependency closure and atomic preparation of surviving list edits.

use super::{reserve, PartOutput};
use crate::ids::UnknownId;
use crate::native::NativeRecord;
use crate::schema::{
    rewrite::typed::{
        prune::{DependencyPosition, DependencyReference, ReferencePruneEffect},
        RewriteIdentities,
    },
    EntitySchema,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Dependency finalization outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartDependencyPruning {
    /// Required dependencies are gone and surviving lists retain their order.
    Complete {
        /// Sorted identities removed by the dependency closure.
        dropped_records: Vec<String>,
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
        super::reserve_output(ctx, self.staged, &mut self.failures, 1)?;
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
    /// types before mutation. A surviving record whose pruned form cannot be
    /// represented or re-admitted is not formed: it joins the dropped set, and
    /// the closure runs again until no record changes (#215 §4.5 rules 1-2).
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
                    let owner = self.native_owners.get(record.id());
                    debug_assert!(
                        owner.is_some(),
                        "decoder error: native record {} has no registered typed owner",
                        record.id()
                    );
                    let Some(owner) = owner else {
                        return Err(CodecError::malformed(format_args!(
                            "native part record {} has no registered typed owner",
                            record.id()
                        )));
                    };
                    let mut edges = Vec::new();
                    (owner.visit)(ctx, record, &mut |edge| add_edge(ctx, &mut edges, edge))?;
                    add_record(ctx, &mut graph, record.id(), edges)?;
                }
            }
        }
        let required = required_dependents(ctx, &graph)?;
        let mut owners = BTreeMap::new();
        let mut deleted = BTreeSet::new();
        let mut pending = closure_seeds(ctx, &self.failures)?;
        let (replacements, mut native_replacements) = loop {
            extend_closure(ctx, &required, &mut owners, &mut deleted, pending)?;
            let mut not_formed = BTreeSet::new();
            let mut replacements = crate::document::Model::default();
            macro_rules! prepare_model {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
                    for record in &self.model.$field {
                        if !deleted.contains(record.identity()) {
                            let prepared = crate::native::prune::model(ctx, record, &deleted)?;
                            if !formed(prepared.effect, record.identity())? {
                                not_formed.insert(not_formed_seed(ctx, &graph, &owners, record.identity())?);
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
                        if !formed(prepared.effect, record.id())? {
                            not_formed.insert(not_formed_seed(ctx, &graph, &owners, record.id())?);
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
            if not_formed.is_empty() {
                break (replacements, native_replacements);
            }
            pending = not_formed;
        };
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
/// Whether a surviving record keeps a representable typed form.
///
/// A required reference to a dropped record, a fixed-cardinality list that
/// would lose an entry, or a failed typed re-admission means the record is not
/// formed (#215 §4.5 rule 1). An unknown reference position means its owner
/// type does not declare where the reference sits; that is a decoder error.
fn formed(effect: ReferencePruneEffect, id: &str) -> Result<bool, CodecError> {
    debug_assert!(
        !matches!(
            effect,
            ReferencePruneEffect::UnknownReferencePosition { .. }
        ),
        "decoder error: {effect:?} for part record {id}"
    );
    match effect {
        ReferencePruneEffect::Unchanged | ReferencePruneEffect::ListsChanged => Ok(true),
        ReferencePruneEffect::DropRequired
        | ReferencePruneEffect::UnrepresentablePrunedList { .. } => Ok(false),
        ReferencePruneEffect::UnknownReferencePosition { owner } => {
            Err(CodecError::malformed(format_args!(
                "typed owner {owner} does not declare the reference positions of part record {id}"
            )))
        }
    }
}

/// The failure home of a record that is not formed: the home of the first
/// dropped record it references.
fn not_formed_seed(
    ctx: &DecodeContext<'_>,
    graph: &BTreeMap<String, Vec<Edge>>,
    owners: &BTreeMap<String, UnknownId>,
    id: &str,
) -> Result<(UnknownId, String), CodecError> {
    let home = graph
        .get(id)
        .into_iter()
        .flatten()
        .find_map(|edge| owners.get(&edge.target))
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "part record {id} lost a reference that its typed visitor does not emit"
            ))
        })?;
    ctx.retain_bytes(
        storage_bytes(&[
            home.as_str().len(),
            id.len(),
            size_of::<(UnknownId, String)>(),
        ])?,
        "unformed dependency seed",
    )?;
    Ok((home.clone(), id.to_owned()))
}

fn required_dependents(
    ctx: &DecodeContext<'_>,
    graph: &BTreeMap<String, Vec<Edge>>,
) -> Result<BTreeMap<String, Vec<String>>, CodecError> {
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
    Ok(required)
}

fn closure_seeds(
    ctx: &DecodeContext<'_>,
    failures: &[FailureHome],
) -> Result<BTreeSet<(UnknownId, String)>, CodecError> {
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
    Ok(pending)
}

/// Drop `pending` and every record that transitively requires one of them.
fn extend_closure(
    ctx: &DecodeContext<'_>,
    required: &BTreeMap<String, Vec<String>>,
    owners: &mut BTreeMap<String, UnknownId>,
    deleted: &mut BTreeSet<String>,
    mut pending: BTreeSet<(UnknownId, String)>,
) -> Result<(), CodecError> {
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
        ctx.retain_bytes(
            storage_bytes(&[id.len(), size_of::<String>()])?,
            "deleted dependency identity",
        )?;
        deleted.insert(id.clone());
        owners.insert(id, home);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
