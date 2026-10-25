// SPDX-License-Identifier: Apache-2.0
//! Typed closure of newly constructed neutral records over authentic source failures.

use super::super::reserve;
use super::{
    add_edge, add_record, closure_seeds, extend_closure, formed, not_formed_seed,
    required_dependents, storage_bytes, FailureHome,
};
use crate::document::Model;
use crate::schema::{rewrite::typed::RewriteIdentities, EntitySchema};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn prune_late_model(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    failures: &mut [FailureHome],
) -> Result<(), CodecError> {
    if failures.is_empty() {
        return Ok(());
    }
    let mut graph = BTreeMap::new();
    macro_rules! edges {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
            for record in &model.$field {
                let mut edges = Vec::new();
                record.visit_dependency_references(ctx, &mut |edge| add_edge(ctx, &mut edges, edge))?;
                add_record(ctx, &mut graph, record.identity(), edges)?;
            }
        )* };
    }
    crate::document::arena_registry!(edges);
    let required = required_dependents(ctx, &graph)?;
    let mut owners = BTreeMap::new();
    let mut deleted = BTreeSet::new();
    let mut pending = closure_seeds(ctx, failures)?;
    let replacements = loop {
        extend_closure(ctx, &required, &mut owners, &mut deleted, pending)?;
        let mut not_formed = BTreeSet::new();
        let mut replacements = Model::default();
        macro_rules! prepare {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
                for record in &model.$field {
                    if deleted.contains(record.identity()) { continue }
                    let prepared = crate::native::prune::model(ctx, record, &deleted)?;
                    if !formed(prepared.effect, record.identity())? {
                        not_formed.insert(not_formed_seed(ctx, &graph, &owners, record.identity())?);
                    } else if let Some((_, replacement)) = prepared.replacement {
                        reserve(ctx, &mut replacements.$field, 1)?;
                        replacements.$field.push(replacement);
                    }
                }
            )* };
        }
        crate::document::arena_registry!(prepare);
        if not_formed.is_empty() {
            break replacements;
        }
        pending = not_formed;
    };
    for (id, cause) in owners {
        let failure = failures
            .iter_mut()
            .find(|failure| failure.id == cause)
            .ok_or_else(|| CodecError::malformed("late dependency has no actual failure home"))?;
        if !failure.dropped.contains(&id) {
            ctx.retain_bytes(
                storage_bytes(&[id.len(), size_of::<String>()])?,
                "late unformed dependency identity",
            )?;
            failure.dropped.insert(id);
            failure.late_sponsored = true;
        }
    }
    let mut captures = Vec::new();
    macro_rules! capture_owners {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
            for record in &model.$field {
                if deleted.contains(record.identity()) {
                    if let Some(authority) = ctx.record_capture_authority(record.identity())? {
                        ctx.reserve_working(&mut captures, 1, "late pruned producer authorities")?;
                        captures.push(authority);
                    }
                }
            }
        )* };
    }
    crate::document::arena_registry!(capture_owners);
    ctx.retire_record_captures(&captures)?;
    macro_rules! commit {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(
            let mut updates = replacements.$field.into_iter().peekable();
            model.$field.retain_mut(|record| {
                if deleted.contains(record.identity()) { return false }
                if let Some(replacement) = updates.next_if(|value| value.identity() == record.identity()) { *record = replacement; }
                true
            });
        )* };
    }
    crate::document::arena_registry!(commit);
    Ok(())
}
