// SPDX-License-Identifier: Apache-2.0
//! Admit complete, codec-resolved native ownership against the current document.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

use super::{CadIr, NativeOwnerResolver, NativeOwnership};

const RESOLVE: &str = "resolve typed native source owners";

fn allocate<T: Clone>(
    ctx: &DecodeContext<'_>,
    count: usize,
    value: T,
) -> Result<Vec<T>, CodecError> {
    let bytes = u64_from_index(count)
        .checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| CodecError::malformed("native owner allocation size overflows"))?;
    ctx.retain_bytes(bytes, RESOLVE)?;
    let mut values = ctx.alloc_filled(count, value, RESOLVE)?;
    values.clear();
    Ok(values)
}

pub(super) fn resolve(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    resolver: &dyn NativeOwnerResolver,
) -> Result<Vec<NativeOwnership>, CodecError> {
    let count = ir
        .native
        .0
        .values()
        .flat_map(|namespace| namespace.arenas().values())
        .try_fold(0_usize, |count, records| count.checked_add(records.len()))
        .ok_or_else(|| CodecError::malformed("native owner record count overflows"))?;
    let all_count = count
        .checked_add(ir.model.entity_count())
        .ok_or_else(|| CodecError::malformed("native owner identity count overflows"))?;
    let mut identities = allocate(ctx, all_count, "")?;
    let mut expected = allocate(ctx, count, "")?;
    ir.visit_neutral_provenance(&mut |record| {
        identities.push(record.id());
        Ok(())
    })?;
    for namespace in ir.native.0.values() {
        for (arena, records) in namespace.arenas() {
            for record in records {
                identities.push(record.id());
                if arena != "unknowns" {
                    expected.push(record.id());
                }
            }
        }
    }
    identities.sort_unstable();
    if identities.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(CodecError::malformed(
            "duplicate identity in native ownership document",
        ));
    }
    expected.sort_unstable();
    let bytes = u64_from_index(expected.len())
        .checked_mul(u64_from_index(std::mem::size_of::<NativeOwnership>()))
        .ok_or_else(|| CodecError::malformed("native owner allocation size overflows"))?;
    ctx.retain_bytes(bytes, RESOLVE)?;
    let mut owners = Vec::new();
    owners.try_reserve_exact(expected.len()).map_err(|_| {
        CodecError::from(ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            RESOLVE,
        ))
    })?;
    resolver.visit_owners(ctx, ir, &mut |owner| {
        if owners.len() == expected.len() || expected.binary_search(&owner.record.as_str()).is_err()
        {
            return Err(CodecError::malformed(format_args!(
                "native owner resolver emitted unexpected record {}",
                owner.record
            )));
        }
        if let Some(target) = &owner.owner {
            if identities.binary_search(&target.as_str()).is_err() {
                return Err(CodecError::malformed(format_args!(
                    "native record {} owner {} does not exist in the document",
                    owner.record, target
                )));
            }
        }
        let text_bytes = owner
            .record
            .as_str()
            .len()
            .checked_add(owner.owner.as_ref().map_or(0, |owner| owner.as_str().len()))
            .ok_or_else(|| CodecError::malformed("native owner identity size overflows"))?;
        ctx.retain_bytes(u64_from_index(text_bytes), RESOLVE)?;
        owners.push(owner);
        Ok(())
    })?;
    owners.sort_unstable_by(|left, right| left.record.cmp(&right.record));
    for (index, expected) in expected.iter().enumerate() {
        if owners.get(index).map(|owner| owner.record.as_str()) != Some(*expected) {
            return Err(CodecError::malformed(format_args!(
                "native record {expected} requires one typed owner resolution"
            )));
        }
    }
    Ok(owners)
}
