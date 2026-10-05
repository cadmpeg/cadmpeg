// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for identity order.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashSet};

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

/// Every identity met so far, held in temporary storage.
struct Seen<'ctx, 'a> {
    identities: HashSet<&'a str>,
    storage: ScopedReservation<'ctx>,
}

fn push_identity<'a>(
    ctx: &DecodeContext<'_>,
    seen: &mut Seen<'_, 'a>,
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
    let inserted = seen.storage.with_storage(|| {
        ctx.insert_hash_set(&mut seen.identities, id, "validation identity uniqueness")
    })?;
    if !inserted {
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

fn check_order<T>(
    ctx: &DecodeContext<'_>,
    arena: &str,
    values: &[T],
    identity: impl Fn(&T) -> &str,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut previous: Option<&str> = None;
    for value in ctx.admit_iter(values, "compare validation arena order")? {
        let id = identity(value);
        let unordered = match previous {
            Some(value) => {
                ctx.compare(value, id, "compare validation arena order")?
                    != std::cmp::Ordering::Less
            }
            None => false,
        };
        if unordered {
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
            seen: &mut Seen<'_, 'a>,
            findings: &mut Vec<Finding>,
        ) -> Result<(), CodecError> {
            $(
                check_order(
                    ctx,
                    stringify!($field),
                    &ir.model.$field,
                    crate::schema::EntitySchema::identity,
                    findings,
                )?;
                for entity in ctx.admit_iter(&ir.model.$field, "validation model identity scan")? {
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
    let mut seen = Seen {
        identities: HashSet::new(),
        storage: ctx.reserve_scoped(0, "validation identity uniqueness")?,
    };
    check_model_identity_and_order(ctx, view.ir, &mut seen, findings)?;
    let mut by_arena: (BTreeMap<String, Vec<&str>>, _) = (
        BTreeMap::new(),
        ctx.reserve_scoped(0, "validation native order storage")?,
    );
    view.visit(
        |work| ctx.charge_work(u64_from_index(work), "validation native arena scan"),
        |format, arena, records| {
            use crate::native::view::{NativeArena, NativeEntity};
            let (products, sources, order): (&[_], &[_], &[_]) = match records {
                NativeArena::Product(records) => (records, &[], &[]),
                NativeArena::Source(records, order) => (&[], records, order),
            };
            for record in ctx
                .admit_iter(products, "validation native identity scan")?
                .map(NativeEntity::Product)
                .chain(
                    ctx.admit_iter(order, "validation native identity scan")?
                        .map(|index| NativeEntity::Source(&sources[*index])),
                )
            {
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
                let ids = ctx
                    .entry_btree_map(&mut by_arena.0, label, "group validation native arenas")?
                    .or_default();
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
        check_order(ctx, arena, ids, |id| *id, findings)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
