// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for identity order.

use crate::index::identities::BorrowedIdentities;
use std::collections::BTreeMap;

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

/// `seen` is absent when every visited identity is known to be distinct.
fn push_identity<'a>(
    seen: &mut Option<BorrowedIdentities<'a, bool>>,
    findings: &mut Vec<Finding>,
    id: &'a str,
) {
    if !crate::ids::is_valid_identity(id) {
        super::record_finding(
            findings,
            Check::Identity,
            Severity::Error,
            Some(id),
            format_args!("entity id does not match `<format>:<scope>:<kind>#<key>`"),
        );
    }
    let Some(seen) = seen else {
        return;
    };
    let visited = seen.get_mut(id);
    debug_assert!(
        visited.is_some(),
        "validation identity index contains each visited entity"
    );
    let Some(visited) = visited else {
        return;
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
            ir: &'a CadIr,
            seen: &mut Option<BorrowedIdentities<'a, bool>>,
            findings: &mut Vec<Finding>,
        ) {
            $(
                check_order(stringify!($field), ir.model.$field.iter().map(crate::schema::EntitySchema::identity), findings);
                for entity in &ir.model.$field {
                    push_identity(seen, findings, crate::schema::EntitySchema::identity(entity));
                }
            )*
            for state in &ir.model.feature_input_topologies {
                for operand in &state.operands {
                    push_identity(seen, findings, operand.id.as_str());
                }
            }
        }
    };
}
crate::document::arena_registry!(define_model_identity_checks);

/// Check model and native identities and preserve arena-ordered findings.
pub(super) fn check_identity_and_order(
    ids: &crate::index::ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let view = ids.native_view();
    // Build the complete identity table once. Ordered insertion for each visit
    // would shift the existing slots quadratically on large models. Every
    // occurrence of an identity updates the same last slot, preserving which
    // visit first reports a duplicate and the original finding order.
    let mut seen = (!ids.entity_identities_distinct()).then(|| BorrowedIdentities::build(|add| {
        macro_rules! collect_model_identities {
            ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
                $(for entity in &view.ir.model.$field {
                    add(crate::schema::EntitySchema::identity(entity), false);
                })*
            };
        }
        crate::document::arena_registry!(collect_model_identities);
        for state in &view.ir.model.feature_input_topologies {
            for operand in &state.operands {
                add(operand.id.as_str(), false);
            }
        }
        view.for_each(|_, _, records| {
            for record in records.records() {
                add(record.id(), false);
            }
        });
    }));
    check_model_identity_and_order(view.ir, &mut seen, findings);
    let mut by_arena: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    view.for_each(|format, arena, records| {
        for record in records.records() {
            push_identity(&mut seen, findings, record.id());
        }
        if records.len() == 0 {
            return;
        }
        by_arena
            .entry(format!("native.{format}.{arena}"))
            .or_default()
            .extend(
                records
                    .records()
                    .map(super::super::native::view::NativeEntity::id),
            );
    });
    for (arena, ids) in &by_arena {
        check_order(arena, ids.iter().copied(), findings);
    }
}

#[cfg(test)]
mod tests;
