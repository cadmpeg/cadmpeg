// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for identity order.

use crate::index::identities::BorrowedIdentities;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

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
    if !crate::ids::is_valid_identity(id) {
        super::record_finding(
            findings,
            Check::Identity,
            Severity::Error,
            Some(id),
            format_args!("entity id does not match `<format>:<scope>:<kind>#<key>`"),
        );
    }
    let Some(visited) = seen.get_mut(ctx, id)? else {
        return Err(CodecError::malformed(
            "validation identity index omits an entity",
        ));
    };
    if std::mem::replace(visited, true) {
        super::record_finding(
            findings,
            Check::Identity,
            Severity::Error,
            Some(id),
            format_args!("entity id is not globally unique"),
        );
    }
    Ok(())
}

fn check_order<'a>(
    arena: &str,
    ids: impl IntoIterator<Item = &'a str>,
    findings: &mut Vec<Finding>,
) {
    let mut previous: Option<&str> = None;
    for id in ids {
        if previous.is_some_and(|value| value >= id) {
            super::record_finding(
                findings,
                Check::ArenaOrder,
                Severity::Error,
                Some(id),
                format_args!("arena `{arena}` is not strictly sorted by id"),
            );
            return;
        }
        previous = Some(id);
    }
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
                check_order(stringify!($field), ir.model.$field.iter().map(crate::schema::EntitySchema::identity), findings);
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
        view.visit::<cadmpeg_core::CodecError>(|_, _, records| {
            for record in records.records() {
                add(record.id(), false)?;
            }
            Ok(())
        })
    })?;
    check_model_identity_and_order(ctx, view.ir, &mut seen, findings)?;
    let mut by_arena: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    view.visit::<cadmpeg_core::CodecError>(|format, arena, records| {
        for record in records.records() {
            push_identity(ctx, &mut seen, findings, record.id())?;
        }
        if records.len() == 0 {
            return Ok(());
        }
        by_arena
            .entry(format!("native.{format}.{arena}"))
            .or_default()
            .extend(
                records
                    .records()
                    .map(super::super::native::view::NativeEntity::id),
            );
        Ok(())
    })?;
    for (arena, ids) in &by_arena {
        check_order(arena, ids.iter().copied(), findings);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
