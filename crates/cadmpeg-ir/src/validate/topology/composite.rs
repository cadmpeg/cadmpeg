// SPDX-License-Identifier: Apache-2.0
//! Composite carrier dependency cycles.

use crate::document::CadIr;
use crate::geometry::{CompositeCurveSegment, CurveGeometry, SolvedCurveGeometry};
use crate::index::identities::BorrowedIdentities;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

struct Frame<'ir, 'ctx> {
    node: &'ir str,
    children: &'ir [CompositeCurveSegment],
    next_child: usize,
    _depth: DepthGuard<'ctx>,
}

pub(super) fn check(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let segments = BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.curves {
            ctx.charge_work(1, "composite curve row")?;
            if let CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) =
                &curve.geometry
            {
                add(curve.id.as_str(), &segments[..])?;
            }
        }
        Ok(())
    })?;
    let mut complete = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    let mut active = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    let mut roots = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for identity in segments.identities() {
        ctx.charge_work(1, "composite curve root scan")?;
        roots.insert_unique(identity, ())?;
    }
    let mut storage = ctx.reserve_scoped(0, "composite curve traversal")?;
    let mut ordered = Vec::new();
    for identity in roots.identities() {
        ctx.charge_work(1, "composite curve ordered root scan")?;
        storage.with_storage(|| {
            ctx.push_vec(&mut ordered, identity, "composite curve ordered roots")
        })?;
    }
    ctx.stable_sort_by(
        &mut ordered,
        Ord::cmp,
        |identity| identity.len(),
        "sort composite curve roots",
    )?;
    let mut stack = Vec::new();
    for root in ordered {
        ctx.charge_work(1, "composite curve root visit")?;
        if complete.contains(ctx, root)? {
            continue;
        }
        let Some(children) = segments.get(ctx, root)? else {
            continue;
        };
        active.insert_unique(root, ())?;
        let frame = Frame {
            node: root,
            children,
            next_child: 0,
            _depth: ctx.enter_nested("composite curve traversal depth")?,
        };
        storage
            .with_storage(|| ctx.push_vec(&mut stack, frame, "composite curve traversal frames"))?;
        while let Some(frame) = stack.last_mut() {
            ctx.charge_work(1, "composite curve frame visit")?;
            if frame.next_child >= frame.children.len() {
                let node = frame.node;
                stack.pop();
                active.remove(node)?;
                complete.insert_unique(node, ())?;
                continue;
            }
            let child = frame.children[frame.next_child].curve.as_str();
            frame.next_child += 1;
            let Some(children) = segments.get(ctx, child)? else {
                continue;
            };
            if complete.contains(ctx, child)? {
                continue;
            }
            if !active.insert_unique(child, ())? {
                crate::validate::record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(child),
                    format_args!("composite curve graph contains a cycle"),
                )?;
                continue;
            }
            let frame = Frame {
                node: child,
                children,
                next_child: 0,
                _depth: ctx.enter_nested("composite curve traversal depth")?,
            };
            storage.with_storage(|| {
                ctx.push_vec(&mut stack, frame, "composite curve traversal frames")
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
