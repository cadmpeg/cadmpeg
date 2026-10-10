// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for identity order.

use crate::index::identities::BorrowedIdentities;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::{btree_map::Entry, BTreeMap};

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

fn push_identity<'a>(
    ctx: &DecodeContext<'_>,
    seen: &mut BorrowedIdentities<'_, 'a, bool>,
    findings: &mut Vec<Finding>,
    id: &'a str,
) -> Result<(), CodecError> {
    let grammar_work = id
        .len()
        .checked_mul(4)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("validate identity grammar", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_work(u64_from_index(grammar_work), "validate identity grammar")?;
    if !crate::ids::is_valid_identity(id) {
        super::record_finding(
            ctx,
            findings,
            Check::Identity,
            Severity::Error,
            Some(id),
            format_args!("entity id does not match `<format>:<scope>:<kind>#<key>`"),
        )?;
    }
    let Some(visited) = seen.get_mut(ctx, id)? else {
        return Err(CodecError::malformed(
            "validation identity index omits an entity",
        ));
    };
    if std::mem::replace(visited, true) {
        super::record_finding(
            ctx,
            findings,
            Check::Identity,
            Severity::Error,
            Some(id),
            format_args!("entity id is not globally unique"),
        )?;
    }
    Ok(())
}

fn check_order<'a>(
    ctx: &DecodeContext<'_>,
    arena: &str,
    ids: impl IntoIterator<Item = &'a str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut previous: Option<&str> = None;
    for id in ids {
        ctx.charge_work(
            u64_from_index(id.len()).checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("compare validation arena order", u64::MAX - 1, u64::MAX)
            })?,
            "compare validation arena order",
        )?;
        if previous.is_some_and(|value| value >= id) {
            super::record_finding(
                ctx,
                findings,
                Check::ArenaOrder,
                Severity::Error,
                Some(id),
                format_args!("arena `{arena}` is not strictly sorted by id"),
            )?;
            return Ok(());
        }
        previous = Some(id);
    }
    Ok(())
}

macro_rules! define_model_identity_checks {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        fn check_model_identity_and_order<'a>(
            ctx: &DecodeContext<'_>,
            ir: &'a CadIr,
            seen: &mut BorrowedIdentities<'_, 'a, bool>,
            findings: &mut Vec<Finding>,
        ) -> Result<(), CodecError> {
            $(
                check_order(
                    ctx,
                    stringify!($field),
                    ir.model.$field.iter().map(crate::schema::EntitySchema::identity),
                    findings,
                )?;
                for entity in &ir.model.$field {
                    push_identity(ctx, seen, findings, crate::schema::EntitySchema::identity(entity))?;
                }
            )*
            Ok(())
        }
    };
}
crate::document::arena_registry!(define_model_identity_checks);

/// Check model and native identities and preserve arena-ordered findings.
pub(super) fn check_identity_and_order(
    ctx: &DecodeContext<'_>,
    view: crate::native::view::NativeView<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    // Build the complete identity table once. Ordered insertion for each visit
    // would shift the existing slots quadratically on large models. Every
    // occurrence of an identity updates the same last slot, preserving which
    // visit first reports a duplicate and the original finding order.
    let mut seen = BorrowedIdentities::build(ctx, |add| {
        macro_rules! collect_model_identities {
            ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
                $(for entity in &view.ir.model.$field {
                    add(crate::schema::EntitySchema::identity(entity), false)?;
                })*
            };
        }
        crate::document::arena_registry!(collect_model_identities);
        view.visit(
            |work| ctx.charge_work(u64_from_index(work), "validation identity arena scan"),
            |_, _, records| {
                for record in records.records() {
                    add(record.id(), false)?;
                }
                Ok(())
            },
        )
    })?;
    check_model_identity_and_order(ctx, view.ir, &mut seen, findings)?;
    let mut by_arena: (BTreeMap<String, Vec<&str>>, _) = (
        BTreeMap::new(),
        ctx.reserve_scoped(0, "validation native order storage")?,
    );
    view.visit(
        |work| ctx.charge_work(u64_from_index(work), "validation native arena scan"),
        |format, arena, records| {
            for record in records.records() {
                push_identity(ctx, &mut seen, findings, record.id())?;
            }
            if records.len() == 0 {
                return Ok(());
            }
            by_arena.1.with_storage(|| {
                let label = ctx.format_retained(
                    format_args!("native.{format}.{arena}"),
                    "validation native arena name",
                )?;
                let work = label
                    .len()
                    .checked_add(1)
                    .and_then(|bytes| {
                        by_arena
                            .0
                            .len()
                            .checked_add(1)
                            .and_then(|count| bytes.checked_mul(count))
                    })
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "group validation native arenas",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                ctx.charge_work(u64_from_index(work), "group validation native arenas")?;
                if !by_arena.0.contains_key(&label) {
                    ctx.charge_work(1, "validation native arena slots")?;
                }
                ctx.admit_btree_entry(&by_arena.0, &label, "validation native arena slots")?;
                let ids = match by_arena.0.entry(label) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => entry.insert(Vec::new()),
                };
                for record in records.records() {
                    ctx.charge_work(1, "validation native order scan")?;
                    ctx.push_vec(ids, record.id(), "validation native order slots")?;
                }
                Ok::<_, CodecError>(())
            })?;
            Ok(())
        },
    )?;
    for (arena, ids) in &by_arena.0 {
        check_order(ctx, arena, ids.iter().copied(), findings)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
