// SPDX-License-Identifier: Apache-2.0
//! Fixed operand facts across checked generic calls.
use rustc_hir::intravisit::{walk_pat, Visitor};
use rustc_hir::{HirId, Pat, PatKind};
use rustc_middle::mir::{Body, Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::LocalDefId;
use std::collections::HashSet;

pub(crate) fn bindings(tcx: TyCtxt<'_>, owner: LocalDefId, fixed: &[bool]) -> HashSet<HirId> {
    struct Bindings { ids: HashSet<HirId> }
    impl<'tcx> Visitor<'tcx> for Bindings {
        fn visit_pat(&mut self, pattern: &'tcx Pat<'tcx>) {
            if let PatKind::Binding(_, id, _, _) = pattern.kind { self.ids.insert(id); }
            walk_pat(self, pattern);
        }
    }
    let mut bindings = Bindings { ids: HashSet::new() };
    for (parameter, fixed) in tcx.hir_body_owned_by(owner).params.iter().zip(fixed) {
        if *fixed { bindings.visit_pat(parameter.pat); }
    }
    bindings.ids
}

pub(crate) fn mir_operand<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, operand: &Operand<'tcx>, fixed: &[bool], seen: &mut HashSet<rustc_middle::mir::Local>) -> bool {
    let place = match operand {
        Operand::Constant(_) => return crate::types::work(tcx, operand.ty(body, tcx), &mut Vec::new()) == crate::types::Shape::Fixed || matches!(operand.ty(body, tcx).peel_refs().kind(), ty::Str | ty::Slice(_)),
        Operand::Copy(place) | Operand::Move(place) => *place,
        _ => return false,
    };
    if !seen.insert(place.local) { return false; }
    if body.basic_blocks.iter().any(|block| block.statements.iter().any(|statement| matches!(&statement.kind, StatementKind::Assign(assignment) if matches!(&assignment.1, Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Mut { .. }, source) if source.local == place.local)))) { return false; }
    let parameter = place.local.as_usize();
    if parameter > 0 && parameter <= body.arg_count {
        return fixed.get(parameter - 1) == Some(&true) && !body.basic_blocks.iter().any(|block| block.statements.iter().any(|statement| matches!(&statement.kind, StatementKind::Assign(assignment) if assignment.0.local == place.local)));
    }
    let mut defined = false;
    for block in body.basic_blocks.iter() {
        for statement in &block.statements {
            let StatementKind::Assign(assignment) = &statement.kind else { continue; };
            let (target, value) = &**assignment;
            if target.local != place.local { continue; }
            defined = true;
            let admitted = match value {
                Rvalue::Use(source, _) | Rvalue::Cast(_, source, _) => mir_operand(tcx, body, source, fixed, &mut seen.clone()),
                Rvalue::Ref(_, _, source) => mir_operand(tcx, body, &Operand::Copy(*source), fixed, &mut seen.clone()),
                Rvalue::Aggregate(_, sources) => sources.iter().all(|source| mir_operand(tcx, body, source, fixed, &mut seen.clone())),
                _ => false,
            };
            if !admitted { return false; }
        }
        if let TerminatorKind::Call { func, args, destination, .. } = &block.terminator().kind {
            if destination.local != place.local { continue; }
            defined = true;
            let ty::FnDef(id, _) = func.ty(body, tcx).kind() else { return false; };
            if !crate::types::standard(tcx, *id) || !tcx.opt_item_name(*id).is_some_and(|name| matches!(name.as_str(), "from" | "into" | "to_owned" | "to_string" | "clone" | "as_str" | "as_bytes")) || !args.iter().all(|argument| mir_operand(tcx, body, &argument.node, fixed, &mut seen.clone())) { return false; }
        }
    }
    defined
}
