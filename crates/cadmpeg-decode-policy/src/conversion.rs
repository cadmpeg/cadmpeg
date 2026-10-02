// SPDX-License-Identifier: Apache-2.0
//! Single owned text conversions admitted by operand byte receipts.
use crate::{storage, types, Analysis};
use rustc_hir::Expr;
use rustc_middle::mir::{Body, Local, Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{self, Instance, TyCtxt};
use std::collections::HashSet;

fn text(tcx: TyCtxt<'_>, value: ty::Ty<'_>) -> bool {
    matches!(value.peel_refs().kind(), ty::Str)
        || matches!(value.peel_refs().kind(), ty::Adt(owner, _) if types::standard(tcx, owner.did()) && tcx.item_name(owner.did()).as_str() == "String")
}

fn origin<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, operand: &Operand<'tcx>, seen: &mut HashSet<Local>) -> Option<usize> {
    let place = match operand { Operand::Copy(place) | Operand::Move(place) => *place, _ => return None };
    if !place.projection.iter().all(|element| matches!(element, rustc_middle::mir::ProjectionElem::Deref)) || !seen.insert(place.local) { return None; }
    let index = place.local.as_usize();
    if index > 0 && index <= body.arg_count {
        if body.basic_blocks.iter().any(|block| block.statements.iter().any(|statement| matches!(&statement.kind, StatementKind::Assign(value) if value.0.local == place.local))) { return None; }
        return Some(index - 1);
    }
    let mut result = None;
    for block in body.basic_blocks.iter() {
        if matches!(&block.terminator().kind, TerminatorKind::Call { destination, .. } if destination.local == place.local) { return None; }
        for statement in &block.statements {
            let StatementKind::Assign(value) = &statement.kind else { continue; };
            if value.0.local != place.local { continue; }
            let candidate = match &value.1 {
                Rvalue::Use(source, _) | Rvalue::Cast(_, source, _) => origin(tcx, body, source, &mut seen.clone()),
                Rvalue::Ref(_, _, source) => origin(tcx, body, &Operand::Copy(*source), &mut seen.clone()),
                _ => None,
            }?;
            if result.is_some_and(|previous| previous != candidate) { return None; }
            result = Some(candidate);
        }
    }
    result
}

pub(crate) fn parameters<'tcx>(tcx: TyCtxt<'tcx>, environment: ty::TypingEnv<'tcx>, instance: Instance<'tcx>, seen: &mut HashSet<Instance<'tcx>>) -> Option<Vec<usize>> {
    if !seen.insert(instance) || seen.len() > tcx.recursion_limit().0 || !tcx.is_mir_available(instance.def_id()) { return None; }
    let body = tcx.instance_mir(instance.def);
    let mut conversions = Vec::new();
    for block in body.basic_blocks.iter() {
        let TerminatorKind::Call { func, args, destination, .. } = &block.terminator().kind else { continue; };
        let value = instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, func.ty(body, tcx)));
        let ty::FnDef(id, arguments) = value.kind() else { return None; };
        let arguments = arguments.no_bound_vars()?;
        let arguments = tcx.try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(arguments)).ok()?;
        let resolved = Instance::try_resolve(tcx, environment, *id, arguments).ok().flatten()?;
        let output = instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, destination.ty(body, tcx).ty));
        let input = args.first().map(|arg| instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, arg.node.ty(body, tcx))));
        let name = tcx.opt_item_name(resolved.def_id());
        let raw_copy = types::standard(tcx, resolved.def_id())
            && name.is_some_and(|name| matches!(name.as_str(), "into" | "from" | "to_owned"))
            && matches!(output.kind(), ty::Adt(owner, _) if types::standard(tcx, owner.did()) && tcx.item_name(owner.did()).as_str() == "String")
            && input.is_some_and(|input| text(tcx, input) && (input != output || name.is_some_and(|name| name.as_str() == "to_owned")));
        if raw_copy {
            conversions.push(origin(tcx, body, &args.first()?.node, &mut HashSet::new())?);
        } else if crate::production(tcx, resolved.def_id()) && args.iter().any(|arg| origin(tcx, body, &arg.node, &mut HashSet::new()).is_some()) {
            for index in parameters(tcx, environment, resolved, &mut seen.clone())? {
                conversions.push(origin(tcx, body, &args.get(index)?.node, &mut HashSet::new())?);
            }
        }
    }
    Some(conversions)
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn admit_conversion(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        if self.flow.storage_extents.is_empty() && self.flow.storage_parameters.is_empty() { return false; }
        let Some((definition, operands)) = self.call(expression) else { return false; };
        if !operands.iter().any(|operand| text(self.tcx, self.expr_ty(operand))
            || self.key(operand, &mut Vec::new()).is_some_and(|key| self.flow.storage_parameters.contains(&key))) { return false; }
        let name = self.tcx.item_name(definition);
        let output = self.expr_ty(expression);
        let raw = types::standard(self.tcx, definition)
            && matches!(name.as_str(), "from" | "into" | "to_owned")
            && matches!(output.kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "String")
            && operands.first().is_some_and(|operand| text(self.tcx, self.expr_ty(operand)) && (self.expr_ty(operand) != output || name.as_str() == "to_owned"))
            && self.implementation(expression, definition).is_some_and(|id| types::standard(self.tcx, id));
        let indices = if raw { vec![0] } else {
            let Some(instance) = self.resolved_instance(expression, definition) else { return false; };
            if !self.checked_body(instance.def_id()) { return false; }
            let Some(indices) = parameters(self.tcx, self.typing_env(), instance, &mut HashSet::new()) else { return false; };
            indices
        };
        let mut admitted = vec![false; operands.len()];
        for index in &indices {
            if indices.iter().filter(|other| *other == index).count() != 1 { continue; }
            let Some(operand) = operands.get(*index) else { continue; };
            let Some(key) = self.key(operand, &mut Vec::new()) else { continue; };
            if self.flow.mutated.contains(&key) { continue; }
            let required = [crate::flow::ExtentTerm { factors: vec![key.clone()], coefficient: self.flow.iterations }];
            if self.flow.storage_parameters.remove(&key)
                || text(self.tcx, self.expr_ty(operand)) && storage::consume_terms(&mut self.flow.storage_extents, &required) {
                admitted[*index] = true;
            }
        }
        if admitted.iter().any(|value| *value) {
            self.findings.conversions.insert(expression.hir_id, admitted);
            if raw { self.findings.admitted_operations.insert(expression.hir_id); }
            return raw;
        }
        false
    }
}

pub(crate) fn operand_admitted<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, operand: &Operand<'tcx>, admitted: &[bool]) -> bool {
    origin(tcx, body, operand, &mut HashSet::new()).is_some_and(|index| admitted.get(index) == Some(&true))
}
