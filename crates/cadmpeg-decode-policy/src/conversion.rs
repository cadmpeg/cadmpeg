// SPDX-License-Identifier: Apache-2.0
//! Checked conversion proofs for reflexive, fixed-copy, and owned-text values.
use crate::{storage, types, Analysis};
use rustc_hir::Expr;
use rustc_middle::mir::{Body, Local, Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{self, Instance, TyCtxt};
use rustc_span::def_id::DefId;
use std::collections::HashSet;

fn reflexive_conversion<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    source: ty::Ty<'tcx>,
    output: ty::Ty<'tcx>,
) -> bool {
    (core_conversion_trait(tcx, definition, "From")
        || core_conversion_trait(tcx, definition, "Into"))
        && source == output
}

pub(crate) fn core_conversion_trait(tcx: TyCtxt<'_>, definition: DefId, expected: &str) -> bool {
    let method = if expected == "From" { "from" } else { "into" };
    tcx.trait_of_assoc(definition).is_some_and(|trait_id| {
        types::physical_item_path(tcx, trait_id, "core", &["convert", expected])
            && tcx
                .opt_item_name(definition)
                .is_some_and(|name| name.as_str() == method)
    })
}

pub(crate) fn forwarded_from<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    instance: Instance<'tcx>,
    source: ty::Ty<'tcx>,
    output: ty::Ty<'tcx>,
) -> Option<Instance<'tcx>> {
    if !tcx.is_mir_available(instance.def_id()) || cyclic(tcx.instance_mir(instance.def)) {
        return None;
    }
    let body = tcx.instance_mir(instance.def);
    let mut target = None;
    for block in body.basic_blocks.iter() {
        if block
            .statements
            .iter()
            .any(|statement| match &statement.kind {
                rustc_middle::mir::StatementKind::StorageLive(_)
                | rustc_middle::mir::StatementKind::StorageDead(_)
                | rustc_middle::mir::StatementKind::Nop => false,
                rustc_middle::mir::StatementKind::Assign(assignment) => {
                    !matches!(&assignment.1, Rvalue::Use(..))
                }
                _ => true,
            })
        {
            return None;
        }
        match &block.terminator().kind {
            rustc_middle::mir::TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } => {
                let function =
                    instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, func.ty(body, tcx)));
                let ty::FnDef(definition, arguments) = function.kind() else {
                    return None;
                };
                if !core_conversion_trait(tcx, *definition, "From") {
                    return None;
                }
                let input = args.first().map(|argument| {
                    instance.instantiate_mir(
                        tcx,
                        ty::EarlyBinder::bind(tcx, argument.node.ty(body, tcx)),
                    )
                })?;
                let result = instance.instantiate_mir(
                    tcx,
                    ty::EarlyBinder::bind(tcx, destination.ty(body, tcx).ty),
                );
                if input != source || result != output || target.is_some() {
                    return None;
                }
                let arguments = arguments.no_bound_vars()?;
                let arguments = tcx
                    .try_normalize_erasing_regions(
                        environment,
                        ty::Unnormalized::new_wip(arguments),
                    )
                    .ok()?;
                target = Instance::try_resolve(tcx, environment, *definition, arguments)
                    .ok()
                    .flatten();
            }
            rustc_middle::mir::TerminatorKind::Drop { .. }
            | rustc_middle::mir::TerminatorKind::InlineAsm { .. }
            | rustc_middle::mir::TerminatorKind::TailCall { .. }
            | rustc_middle::mir::TerminatorKind::SwitchInt { .. } => return None,
            _ => (),
        }
    }
    target
}

pub(crate) fn copy_conversion_is_fixed<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    definition: DefId,
    instance: Instance<'tcx>,
    source: ty::Ty<'tcx>,
    output: ty::Ty<'tcx>,
) -> bool {
    if reflexive_conversion(tcx, definition, source, output) {
        return true;
    }
    if !crate::fixed::copy_layout_is_fixed(tcx, environment, source)
        || !crate::fixed::copy_layout_is_fixed(tcx, environment, output)
    {
        return false;
    }
    if core_conversion_trait(tcx, definition, "From") {
        return crate::fixed::copy_conversion_body_is_fixed(tcx, instance);
    }
    if !core_conversion_trait(tcx, definition, "Into") {
        return false;
    }
    forwarded_from(tcx, environment, instance, source, output)
        .is_some_and(|from| crate::fixed::copy_conversion_body_is_fixed(tcx, from))
}

fn text(tcx: TyCtxt<'_>, value: ty::Ty<'_>) -> bool {
    matches!(value.peel_refs().kind(), ty::Str)
        || matches!(value.peel_refs().kind(), ty::Adt(owner, _) if types::standard(tcx, owner.did()) && tcx.item_name(owner.did()).as_str() == "String")
}

fn origin<'tcx>(
    body: &Body<'tcx>,
    operand: &Operand<'tcx>,
    seen: &mut HashSet<Local>,
) -> Option<usize> {
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) => *place,
        _ => return None,
    };
    if !place
        .projection
        .iter()
        .all(|element| matches!(element, rustc_middle::mir::ProjectionElem::Deref))
        || !seen.insert(place.local)
    {
        return None;
    }
    if body.basic_blocks.iter().any(|block| block.statements.iter().any(|statement|
        matches!(&statement.kind, StatementKind::Assign(value)
            if matches!(&value.1, Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Mut { .. }, source) if source.local == place.local)))) { return None; }
    let index = place.local.as_usize();
    if index > 0 && index <= body.arg_count {
        if body.basic_blocks.iter().any(|block| block.statements.iter().any(|statement| matches!(&statement.kind, StatementKind::Assign(value) if value.0.local == place.local))) { return None; }
        return Some(index - 1);
    }
    let mut result = None;
    for block in body.basic_blocks.iter() {
        if matches!(&block.terminator().kind, TerminatorKind::Call { destination, .. } if destination.local == place.local)
        {
            return None;
        }
        for statement in &block.statements {
            let StatementKind::Assign(value) = &statement.kind else {
                continue;
            };
            if value.0.local != place.local {
                continue;
            }
            let candidate = match &value.1 {
                Rvalue::Use(source, _) | Rvalue::Cast(_, source, _) => {
                    origin(body, source, &mut seen.clone())
                }
                Rvalue::Ref(_, _, source) => {
                    origin(body, &Operand::Copy(*source), &mut seen.clone())
                }
                _ => None,
            }?;
            if result.is_some_and(|previous| previous != candidate) {
                return None;
            }
            result = Some(candidate);
        }
    }
    result
}

pub(crate) fn parameters<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    instance: Instance<'tcx>,
    seen: &mut HashSet<Instance<'tcx>>,
) -> Option<Vec<usize>> {
    if !seen.insert(instance)
        || seen.len() > tcx.recursion_limit().0
        || !tcx.is_mir_available(instance.def_id())
    {
        return None;
    }
    let body = tcx.instance_mir(instance.def);
    if cyclic(body) {
        return None;
    }
    let mut conversions = Vec::new();
    for block in body.basic_blocks.iter() {
        let TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } = &block.terminator().kind
        else {
            continue;
        };
        let value = instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, func.ty(body, tcx)));
        let ty::FnDef(id, arguments) = value.kind() else {
            return None;
        };
        let arguments = arguments.no_bound_vars()?;
        let arguments = tcx
            .try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(arguments))
            .ok()?;
        let resolved = Instance::try_resolve(tcx, environment, *id, arguments)
            .ok()
            .flatten()?;
        let output = instance.instantiate_mir(
            tcx,
            ty::EarlyBinder::bind(tcx, destination.ty(body, tcx).ty),
        );
        let input = args.first().map(|arg| {
            instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, arg.node.ty(body, tcx)))
        });
        let name = tcx.opt_item_name(resolved.def_id());
        let raw_copy = types::standard(tcx, resolved.def_id())
            && name.is_some_and(|name| matches!(name.as_str(), "into" | "from" | "to_owned"))
            && matches!(output.kind(), ty::Adt(owner, _) if types::standard(tcx, owner.did()) && tcx.item_name(owner.did()).as_str() == "String")
            && input.is_some_and(|input| {
                text(tcx, input)
                    && (input != output || name.is_some_and(|name| name.as_str() == "to_owned"))
            });
        if raw_copy {
            conversions.push(origin(body, &args.first()?.node, &mut HashSet::new())?);
        } else if crate::production(tcx, resolved.def_id())
            && args
                .iter()
                .any(|arg| origin(body, &arg.node, &mut HashSet::new()).is_some())
        {
            for index in parameters(tcx, environment, resolved, &mut seen.clone())? {
                conversions.push(origin(body, &args.get(index)?.node, &mut HashSet::new())?);
            }
        }
    }
    Some(conversions)
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn admit_conversion(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        if self.flow.storage_extents.is_empty() && self.flow.storage_parameters.is_empty() {
            return false;
        }
        let Some((definition, operands)) = self.call(expression) else {
            return false;
        };
        if !operands.iter().any(|operand| {
            text(self.tcx, self.expr_ty(operand))
                || self
                    .key(operand, &mut Vec::new())
                    .is_some_and(|key| self.flow.storage_parameters.contains(&key))
        }) {
            return false;
        }
        let name = self.tcx.item_name(definition);
        let output = self.expr_ty(expression);
        let raw = types::standard(self.tcx, definition)
            && matches!(name.as_str(), "from" | "into" | "to_owned")
            && matches!(output.kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "String")
            && operands.first().is_some_and(|operand| {
                text(self.tcx, self.expr_ty(operand))
                    && (self.expr_ty(operand) != output || name.as_str() == "to_owned")
            })
            && self
                .implementation(expression, definition)
                .is_some_and(|id| types::standard(self.tcx, id));
        let indices = if raw {
            vec![0]
        } else {
            let Some(instance) = self.resolved_instance(expression, definition) else {
                return false;
            };
            if !self.checked_body(instance.def_id()) {
                return false;
            }
            let Some(indices) =
                parameters(self.tcx, self.typing_env(), instance, &mut HashSet::new())
            else {
                return false;
            };
            indices
        };
        let mut admitted: Vec<_> = operands.iter().map(|_| false).collect();
        for index in &indices {
            if indices.iter().filter(|other| *other == index).count() != 1 {
                continue;
            }
            let Some(operand) = operands.get(*index) else {
                continue;
            };
            let Some(key) = self.key(operand, &mut Vec::new()) else {
                continue;
            };
            if self.flow.mutated.contains(&key) {
                continue;
            }
            let Some(required) = self.scaled_storage_terms(&[crate::flow::ExtentTerm {
                factors: vec![key.clone()],
                coefficient: 1,
            }]) else {
                continue;
            };
            if self.flow.storage_parameters.remove(&key)
                || text(self.tcx, self.expr_ty(operand))
                    && storage::consume_terms(&mut self.flow.storage_extents, &required)
            {
                admitted[*index] = true;
            }
        }
        if admitted.iter().any(|value| *value) {
            self.findings
                .conversions
                .insert(expression.hir_id, admitted);
            if raw {
                self.findings.admitted_operations.insert(expression.hir_id);
            }
            return raw;
        }
        false
    }
}

pub(crate) fn operand_admitted<'tcx>(
    body: &Body<'tcx>,
    operand: &Operand<'tcx>,
    admitted: &[bool],
) -> bool {
    origin(body, operand, &mut HashSet::new())
        .is_some_and(|index| admitted.get(index) == Some(&true))
}

pub(crate) fn cyclic(body: &Body<'_>) -> bool {
    fn visit(
        body: &Body<'_>,
        block: rustc_middle::mir::BasicBlock,
        active: &mut HashSet<rustc_middle::mir::BasicBlock>,
        done: &mut HashSet<rustc_middle::mir::BasicBlock>,
    ) -> bool {
        if done.contains(&block) {
            return false;
        }
        if !active.insert(block) {
            return true;
        }
        for next in body.basic_blocks[block].terminator().successors() {
            if visit(body, next, active, done) {
                return true;
            }
        }
        active.remove(&block);
        done.insert(block);
        false
    }
    visit(
        body,
        rustc_middle::mir::START_BLOCK,
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
}
