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
    let segments = BorrowedIdentities::build(|add| {
        for curve in &ir.model.curves {
            if let CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) =
                &curve.geometry
            {
                add(curve.id.as_str(), &segments[..])?;
            }
        }
        Ok(())
    })?;
    let mut complete = BorrowedIdentities::build(|_| Ok(()))?;
    let mut active = BorrowedIdentities::build(|_| Ok(()))?;
    let mut roots = BorrowedIdentities::build(|_| Ok(()))?;
    for identity in segments.identities() {
        roots.insert_unique(identity, ());
    }
    let mut ordered = Vec::new();
    for identity in roots.identities() {
        ordered.push(identity);
    }
    ordered.sort_by(Ord::cmp);
    let mut stack = Vec::new();
    for root in ordered {
        if complete.contains(root) {
            continue;
        }
        let Some(children) = segments.get(root) else {
            continue;
        };
        active.insert_unique(root, ());
        let frame = Frame {
            node: root,
            children,
            next_child: 0,
            _depth: ctx.enter_nested("composite curve traversal depth")?,
        };
        stack.push(frame);
        while let Some(frame) = stack.last_mut() {
            if frame.next_child >= frame.children.len() {
                let node = frame.node;
                stack.pop();
                active.remove(node);
                complete.insert_unique(node, ());
                continue;
            }
            let child = frame.children[frame.next_child].curve.as_str();
            frame.next_child += 1;
            let Some(children) = segments.get(child) else {
                continue;
            };
            if complete.contains(child) {
                continue;
            }
            if !active.insert_unique(child, ()) {
                crate::validate::record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(child),
                    format_args!("composite curve graph contains a cycle"),
                );
                continue;
            }
            let frame = Frame {
                node: child,
                children,
                next_child: 0,
                _depth: ctx.enter_nested("composite curve traversal depth")?,
            };
            stack.push(frame);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
