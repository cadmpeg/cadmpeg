// SPDX-License-Identifier: Apache-2.0
//! Core and container callbacks defer child obligations to concrete callers.
use crate::{external, types, Analysis};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{def::Res, Expr, ExprKind, HirId, MatchSource, Node, Pat, PatKind, StmtKind};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeckResults};
use rustc_span::def_id::DefId;

fn borrowed_identity_builder(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    types::physical_inherent_method(
        tcx,
        definition,
        "cadmpeg_ir",
        &["index", "identities", "BorrowedIdentities"],
        "build",
    )
}

fn allocated_vec_constructor(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    if !types::physical_inherent_method(tcx, definition, "alloc", &["vec", "Vec"], "new") {
        return false;
    }
    let signature = tcx.fn_sig(definition).instantiate_identity().skip_binder();
    signature.inputs().is_empty()
        && matches!(signature.output().kind(), ty::Adt(owner, _)
            if types::physical_item_path(tcx, owner.did(), "alloc", &["vec", "Vec"]))
}

fn core_decode_method(tcx: TyCtxt<'_>, definition: DefId, owner_path: &str, method: &str) -> bool {
    let Some(owner_path) = owner_path.strip_prefix("cadmpeg_core::") else {
        return false;
    };
    types::inherent_method_owner(tcx, definition, "cadmpeg_core", owner_path, method)
}

fn expression_call<'tcx>(
    typeck: &TypeckResults<'tcx>,
    expression: &'tcx Expr<'tcx>,
) -> Option<(DefId, Vec<&'tcx Expr<'tcx>>)> {
    match expression.kind {
        ExprKind::MethodCall(_, receiver, arguments, _) => {
            let definition = typeck.type_dependent_def_id(expression.hir_id)?;
            let mut operands = vec![receiver];
            operands.extend(arguments);
            Some((definition, operands))
        }
        ExprKind::Call(callee, arguments) => match typeck.expr_ty(callee).kind() {
            ty::FnDef(definition, _) => Some((*definition, arguments.iter().collect())),
            _ => None,
        },
        _ => None,
    }
}

fn local_binding(pattern: &Pat<'_>) -> Option<HirId> {
    if let PatKind::Binding(_, binding, _, None) = pattern.kind {
        Some(binding)
    } else {
        None
    }
}

fn local_path<'tcx>(
    typeck: &TypeckResults<'tcx>,
    expression: &'tcx Expr<'tcx>,
    binding: HirId,
) -> bool {
    let expression = strip_callback_wrappers(expression);
    matches!(expression.kind, ExprKind::Path(path) if typeck.qpath_res(&path, expression.hir_id) == Res::Local(binding))
}

fn local_path_binding<'tcx>(
    typeck: &TypeckResults<'tcx>,
    expression: &'tcx Expr<'tcx>,
) -> Option<HirId> {
    let expression = strip_callback_wrappers(expression);
    let ExprKind::Path(path) = expression.kind else {
        return None;
    };
    match typeck.qpath_res(&path, expression.hir_id) {
        Res::Local(binding) => Some(binding),
        _ => None,
    }
}

fn strip_callback_wrappers<'tcx>(mut expression: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    loop {
        match expression.kind {
            ExprKind::DropTemps(inner) | ExprKind::AddrOf(_, _, inner) => expression = inner,
            _ => return expression,
        }
    }
}

struct EmitterLocator<'tcx> {
    typeck: &'tcx TypeckResults<'tcx>,
    visit: HirId,
    calls: Vec<&'tcx Expr<'tcx>>,
    emitters: Vec<DefId>,
}

impl<'tcx> Visitor<'tcx> for EmitterLocator<'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Call(callee, arguments) = expression.kind {
            if let ExprKind::Path(path) = callee.kind {
                if self.typeck.qpath_res(&path, callee.hir_id) == Res::Local(self.visit) {
                    self.calls.push(expression);
                    for argument in arguments {
                        let argument = strip_callback_wrappers(argument);
                        if let ty::Closure(definition, _) = self.typeck.expr_ty(argument).kind() {
                            self.emitters.push(*definition);
                        }
                    }
                }
            }
        }
        walk_expr(self, expression);
    }
}

struct EmitterEvidence<'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    work: Vec<(&'tcx Expr<'tcx>, Vec<&'tcx Expr<'tcx>>)>,
    direct_push: bool,
    storage_callbacks: Vec<DefId>,
    storage_calls: Vec<&'tcx Expr<'tcx>>,
}

impl<'tcx> Visitor<'tcx> for EmitterEvidence<'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if matches!(expression.kind, ExprKind::Closure(_)) {
            if let ty::Closure(definition, _) = self.typeck.expr_ty(expression).kind() {
                if self.storage_callback(expression) {
                    self.storage_callbacks.push(*definition);
                }
            }
            return;
        }
        if let Some((definition, operands)) = expression_call(self.typeck, expression) {
            if core_decode_method(
                self.tcx,
                definition,
                "cadmpeg_core::decode::context::DecodeContext",
                "charge_work",
            ) {
                self.work.push((expression, operands));
            }
            if core_decode_method(
                self.tcx,
                definition,
                "cadmpeg_core::decode::context::DecodeContext",
                "push_vec",
            ) {
                self.direct_push = true;
            }
            if core_decode_method(
                self.tcx,
                definition,
                "cadmpeg_core::decode::budget::ScopedReservation",
                "with_storage",
            ) {
                self.storage_calls.push(expression);
            }
        }
        walk_expr(self, expression);
    }
}

impl<'tcx> EmitterEvidence<'tcx> {
    fn storage_callback(&self, expression: &'tcx Expr<'tcx>) -> bool {
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            let Node::Expr(parent) = node else {
                return false;
            };
            if let Some((definition, operands)) = expression_call(self.typeck, parent) {
                return core_decode_method(
                    self.tcx,
                    definition,
                    "cadmpeg_core::decode::budget::ScopedReservation",
                    "with_storage",
                ) && operands
                    .iter()
                    .any(|operand| strip_callback_wrappers(operand).hir_id == expression.hir_id);
            }
            if !matches!(
                parent.kind,
                ExprKind::DropTemps(_) | ExprKind::AddrOf(_, _, _)
            ) {
                return false;
            }
        }
        false
    }
}

struct PushVecFinder<'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    calls: usize,
}

struct MethodFinder<'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    owner: &'static str,
    method: &'static str,
    calls: Vec<(&'tcx Expr<'tcx>, Vec<&'tcx Expr<'tcx>>)>,
}

impl<'tcx> Visitor<'tcx> for MethodFinder<'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if matches!(expression.kind, ExprKind::Closure(_)) {
            return;
        }
        if let Some((definition, operands)) = expression_call(self.typeck, expression) {
            if core_decode_method(self.tcx, definition, self.owner, self.method) {
                self.calls.push((expression, operands));
            }
        }
        walk_expr(self, expression);
    }
}

impl<'tcx> Visitor<'tcx> for PushVecFinder<'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if matches!(expression.kind, ExprKind::Closure(_)) {
            return;
        }
        if expression_call(self.typeck, expression).is_some_and(|(definition, _)| {
            core_decode_method(
                self.tcx,
                definition,
                "cadmpeg_core::decode::context::DecodeContext",
                "push_vec",
            )
        }) {
            self.calls += 1;
        }
        walk_expr(self, expression);
    }
}

fn block_tail<'tcx>(expression: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    match expression.kind {
        ExprKind::Block(block, _) => block.expr,
        ExprKind::DropTemps(inner) | ExprKind::AddrOf(_, _, inner) => block_tail(inner),
        _ => Some(expression),
    }
}

fn statement_expression<'tcx>(statement: &rustc_hir::Stmt<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    match statement.kind {
        StmtKind::Semi(expression) | StmtKind::Expr(expression) => Some(expression),
        _ => None,
    }
}

fn literal_one(expression: &Expr<'_>) -> bool {
    matches!(expression.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 1))
}

fn literal_zero(expression: &Expr<'_>) -> bool {
    matches!(expression.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 0))
}

fn try_expression(expression: &Expr<'_>) -> bool {
    match expression.kind {
        ExprKind::Match(_, _, MatchSource::TryDesugar(_)) => true,
        ExprKind::DropTemps(inner) | ExprKind::AddrOf(_, _, inner) => try_expression(inner),
        _ => false,
    }
}

fn charges_identity_length<'tcx>(
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    expression: &'tcx Expr<'tcx>,
    id: HirId,
) -> bool {
    let Some((conversion, arguments)) = expression_call(typeck, expression) else {
        return false;
    };
    if !types::physical_item_path(
        tcx,
        conversion,
        "cadmpeg_core",
        &["decode", "view", "u64_from_index"],
    ) {
        return false;
    }
    let Some(length) = arguments.first().copied() else {
        return false;
    };
    let Some((length_method, operands)) = expression_call(typeck, length) else {
        return false;
    };
    tcx.crate_name(length_method.krate).as_str() == "core"
        && tcx.item_name(length_method).as_str() == "len"
        && operands.first().is_some_and(|receiver| {
            matches!(receiver.kind, ExprKind::Path(path) if typeck.qpath_res(&path, receiver.hir_id) == Res::Local(id))
                && matches!(typeck.expr_ty(receiver).peel_refs().kind(), ty::Str)
        })
}

fn only_charge_call<'tcx>(
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    expression: &'tcx Expr<'tcx>,
) -> Option<(&'tcx Expr<'tcx>, Vec<&'tcx Expr<'tcx>>)> {
    let mut evidence = EmitterEvidence {
        tcx,
        typeck,
        work: Vec::new(),
        direct_push: false,
        storage_callbacks: Vec::new(),
        storage_calls: Vec::new(),
    };
    evidence.visit_expr(expression);
    if evidence.work.len() == 1 {
        evidence.work.pop()
    } else {
        None
    }
}

fn charged_borrowed_identity_builder(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    if !borrowed_identity_builder(tcx, definition) {
        return false;
    }
    let Some(owner) = definition.as_local() else {
        return false;
    };
    let body = tcx.hir_body_owned_by(owner);
    let Some(context) = body
        .params
        .first()
        .and_then(|parameter| local_binding(parameter.pat))
    else {
        return false;
    };
    if body.params.len() != 2 {
        return false;
    }
    let Some(visit) = body
        .params
        .get(1)
        .and_then(|parameter| local_binding(parameter.pat))
    else {
        return false;
    };
    let ExprKind::Block(builder_block, _) = body.value.kind else {
        return false;
    };
    let typeck = tcx.typeck(owner);
    let mut locator = EmitterLocator {
        typeck,
        visit,
        calls: Vec::new(),
        emitters: Vec::new(),
    };
    locator.visit_expr(body.value);
    if locator.calls.len() != 1 || locator.emitters.len() != 1 {
        return false;
    }

    let emitter = locator.emitters[0];
    let Some(emitter_owner) = emitter.as_local() else {
        return false;
    };
    let emitter_typeck = tcx.typeck(emitter_owner);
    let emitter_body = tcx.hir_body_owned_by(emitter_owner);
    if emitter_body.params.len() != 2 {
        return false;
    }
    let Some(id) = emitter_body
        .params
        .first()
        .and_then(|parameter| local_binding(parameter.pat))
    else {
        return false;
    };
    let ExprKind::Block(block, _) = emitter_body.value.kind else {
        return false;
    };
    if block.stmts.len() < 2 {
        return false;
    }
    let Some(first_statement) = statement_expression(&block.stmts[0]) else {
        return false;
    };
    let Some(second_statement) = statement_expression(&block.stmts[1]) else {
        return false;
    };
    if !try_expression(first_statement) || !try_expression(second_statement) {
        return false;
    }
    let Some((first_work, first_operands)) = only_charge_call(tcx, emitter_typeck, first_statement)
    else {
        return false;
    };
    let Some((second_work, second_operands)) =
        only_charge_call(tcx, emitter_typeck, second_statement)
    else {
        return false;
    };
    if !first_operands
        .get(1)
        .is_some_and(|amount| literal_one(amount))
        || !second_operands
            .get(1)
            .is_some_and(|amount| charges_identity_length(tcx, emitter_typeck, amount, id))
        || !first_operands
            .first()
            .is_some_and(|receiver| local_path(emitter_typeck, receiver, context))
        || !second_operands
            .first()
            .is_some_and(|receiver| local_path(emitter_typeck, receiver, context))
    {
        return false;
    }

    let mut emitter_evidence = EmitterEvidence {
        tcx,
        typeck: emitter_typeck,
        work: Vec::new(),
        direct_push: false,
        storage_callbacks: Vec::new(),
        storage_calls: Vec::new(),
    };
    emitter_evidence.visit_expr(emitter_body.value);
    let Some(storage_call) = emitter_evidence.storage_calls.first().copied() else {
        return false;
    };
    if emitter_evidence.work.len() != 2
        || emitter_evidence.direct_push
        || emitter_evidence.storage_calls.len() != 1
        || emitter_evidence.storage_callbacks.len() != 1
        || !block_tail(emitter_body.value).is_some_and(|tail| {
            strip_callback_wrappers(tail).hir_id == strip_callback_wrappers(storage_call).hir_id
        })
    {
        return false;
    }
    let Some(storage_owner) = emitter_evidence.storage_callbacks[0].as_local() else {
        return false;
    };
    let storage_typeck = tcx.typeck(storage_owner);
    let storage_body = tcx.hir_body_owned_by(storage_owner);
    if !storage_body.params.is_empty() {
        return false;
    }
    let Some(storage_tail) = block_tail(storage_body.value) else {
        return false;
    };
    let Some((storage_push, push_operands)) = expression_call(storage_typeck, storage_tail) else {
        return false;
    };
    if !core_decode_method(
        tcx,
        storage_push,
        "cadmpeg_core::decode::context::DecodeContext",
        "push_vec",
    ) || push_operands.len() < 3
        || !matches!(storage_body.value.kind, ExprKind::Block(block, _) if block.stmts.is_empty())
    {
        return false;
    }
    let mut push_finder = PushVecFinder {
        tcx,
        typeck: storage_typeck,
        calls: 0,
    };
    push_finder.visit_expr(storage_body.value);
    let Some((_, with_storage_operands)) = expression_call(emitter_typeck, storage_call) else {
        return false;
    };
    let Some(storage_binding) = with_storage_operands
        .first()
        .and_then(|receiver| local_path_binding(emitter_typeck, receiver))
    else {
        return false;
    };
    let Some(values_binding) = push_operands
        .get(1)
        .and_then(|values| local_path_binding(storage_typeck, values))
    else {
        return false;
    };
    if !local_path(emitter_typeck, with_storage_operands[0], storage_binding)
        || !local_path(storage_typeck, push_operands[0], context)
        || !local_path(storage_typeck, push_operands[1], values_binding)
    {
        return false;
    }

    let mut reservation_call = None;
    let mut empty_values = false;
    let mut reservation_precedes_visit = false;
    let mut values_precede_visit = false;
    for statement in builder_block.stmts {
        let StmtKind::Let(local) = statement.kind else {
            continue;
        };
        let Some(binding) = local_binding(local.pat) else {
            continue;
        };
        let Some(initializer) = local.init else {
            continue;
        };
        if binding == storage_binding {
            if !try_expression(initializer) || initializer.span.lo() >= locator.calls[0].span.lo() {
                return false;
            }
            let mut finder = MethodFinder {
                tcx,
                typeck,
                owner: "cadmpeg_core::decode::context::DecodeContext",
                method: "reserve_scoped",
                calls: Vec::new(),
            };
            finder.visit_expr(initializer);
            if finder.calls.len() != 1
                || !finder.calls[0]
                    .1
                    .first()
                    .is_some_and(|receiver| local_path(typeck, receiver, context))
                || !finder.calls[0]
                    .1
                    .get(1)
                    .is_some_and(|amount| literal_zero(amount))
            {
                return false;
            }
            reservation_call = finder.calls.pop().map(|(call, _)| call);
            reservation_precedes_visit = true;
        }
        if binding == values_binding {
            let Some((constructor, arguments)) = expression_call(typeck, initializer) else {
                return false;
            };
            empty_values = allocated_vec_constructor(tcx, constructor)
                && arguments.is_empty()
                && matches!(typeck.expr_ty(initializer).kind(), ty::Adt(owner, _) if types::physical_item_path(tcx, owner.did(), "alloc", &["vec", "Vec"]));
            values_precede_visit = initializer.span.lo() < locator.calls[0].span.lo();
        }
    }
    let Some(reservation_call) = reservation_call else {
        return false;
    };
    if !empty_values || !reservation_precedes_visit || !values_precede_visit {
        return false;
    }

    let mut builder_findings = crate::Findings::default();
    let builder_analysis = Analysis {
        tcx,
        typeck,
        typing_owner: owner,
        arguments: None,
        fixed_parameters: std::collections::HashSet::new(),
        flow: crate::flow::Flow::default(),
        findings: &mut builder_findings,
    };
    let mut emitter_findings = crate::Findings::default();
    let emitter_analysis = Analysis {
        tcx,
        typeck: emitter_typeck,
        typing_owner: emitter_owner,
        arguments: None,
        fixed_parameters: std::collections::HashSet::new(),
        flow: crate::flow::Flow::default(),
        findings: &mut emitter_findings,
    };
    push_finder.calls == 1
        && push_operands
            .first()
            .is_some_and(|receiver| local_path(storage_typeck, receiver, context))
        && emitter_analysis.propagated(first_work)
        && emitter_analysis.propagated(second_work)
        && builder_analysis.propagated(reservation_call)
        && builder_analysis.propagated(locator.calls[0])
}

fn charged_borrowed_identity_invocation<'tcx>(
    tcx: TyCtxt<'tcx>,
    owner: rustc_span::def_id::LocalDefId,
    invocation: &'tcx Expr<'tcx>,
    callback: DefId,
) -> bool {
    let typeck = tcx.typeck(owner);
    let Some((definition, operands)) = expression_call(typeck, invocation) else {
        return false;
    };
    if !charged_borrowed_identity_builder(tcx, definition) || operands.len() != 2 {
        return false;
    }
    let supplied = strip_callback_wrappers(operands[1]);
    if !matches!(typeck.expr_ty(supplied).kind(), ty::Closure(definition, _) if *definition == callback)
    {
        return false;
    }
    let mut findings = crate::Findings::default();
    let analysis = Analysis {
        tcx,
        typeck,
        typing_owner: owner,
        arguments: None,
        fixed_parameters: std::collections::HashSet::new(),
        flow: crate::flow::Flow::default(),
        findings: &mut findings,
    };
    analysis.propagated(invocation)
}

fn callback_trait(tcx: TyCtxt<'_>, id: DefId) -> bool {
    [
        tcx.lang_items().fn_trait(),
        tcx.lang_items().fn_mut_trait(),
        tcx.lang_items().fn_once_trait(),
    ]
    .contains(&Some(id))
}
fn reader_traits(tcx: TyCtxt<'_>, id: DefId, seen: &mut Vec<DefId>, result: &mut Vec<DefId>) {
    if seen.contains(&id) {
        return;
    }
    seen.push(id);
    if types::standard(tcx, id) && matches!(tcx.item_name(id).as_str(), "Read" | "Seek") {
        result.push(id);
        return;
    }
    for entry in tcx.explicit_super_clauses_of(id).iter_identity_copied() {
        let (clause, _) = entry.skip_norm_wip();
        if let ty::ClauseKind::Trait(predicate) = clause.kind().skip_binder() {
            reader_traits(tcx, predicate.trait_ref.def_id, seen, result);
        }
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn provider_callback_types(
        &self,
        definition: DefId,
        args: ty::GenericArgsRef<'tcx>,
    ) -> Vec<Ty<'tcx>> {
        let crate_name = self.tcx.crate_name(definition.krate);
        if !matches!(crate_name.as_str(), "cadmpeg_core" | "cadmpeg_container")
            && !charged_borrowed_identity_builder(self.tcx, definition)
        {
            return Vec::new();
        }
        self.tcx
            .clauses_of(definition)
            .instantiate(self.tcx, args)
            .clauses
            .iter()
            .filter_map(|clause| match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate)
                    if callback_trait(self.tcx, predicate.trait_ref.def_id) =>
                {
                    Some(predicate.trait_ref.self_ty())
                }
                _ => None,
            })
            .collect()
    }

    pub(crate) fn borrowed_identity_callback_call(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let ExprKind::Call(callee, _) = expression.kind else {
            return false;
        };
        let ExprKind::Path(path) = callee.kind else {
            return false;
        };
        let Res::Local(binding) = self.typeck.qpath_res(&path, callee.hir_id) else {
            return false;
        };
        let callee_type = self.expr_ty(callee).peel_refs();
        let ty::Dynamic(predicates, _) = callee_type.kind() else {
            return false;
        };
        if predicates.principal().is_none_or(|principal| {
            self.tcx.lang_items().fn_mut_trait() != Some(principal.def_id())
        }) || !self.propagated(expression)
        {
            return false;
        }

        let closure = self
            .tcx
            .hir_enclosing_body_owner(expression.hir_id)
            .to_def_id();
        let Some(closure_local) = closure.as_local() else {
            return false;
        };
        if !matches!(self.tcx.def_kind(closure), rustc_hir::def::DefKind::Closure) {
            return false;
        }
        let closure_body = self.tcx.hir_body_owned_by(closure_local);
        if !closure_body
            .params
            .iter()
            .any(|parameter| local_binding(parameter.pat) == Some(binding))
        {
            return false;
        }

        let mut saw_callback_closure = false;
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            let Node::Expr(parent) = node else {
                continue;
            };
            match parent.kind {
                ExprKind::Closure(callback) => {
                    if saw_callback_closure || callback.def_id.to_def_id() != closure {
                        return false;
                    }
                    saw_callback_closure = true;
                }
                ExprKind::Call(_, _) if saw_callback_closure => {
                    let owner = self.tcx.hir_enclosing_body_owner(parent.hir_id);
                    return charged_borrowed_identity_invocation(self.tcx, owner, parent, closure);
                }
                _ => (),
            }
        }
        false
    }

    fn provider_reader_types(
        &self,
        definition: DefId,
        args: ty::GenericArgsRef<'tcx>,
    ) -> Vec<(Ty<'tcx>, DefId)> {
        if !matches!(
            self.tcx.crate_name(definition.krate).as_str(),
            "cadmpeg_core" | "cadmpeg_container"
        ) {
            return Vec::new();
        }
        let mut result = Vec::new();
        for clause in self
            .tcx
            .clauses_of(definition)
            .instantiate(self.tcx, args)
            .clauses
            .iter()
        {
            let ty::ClauseKind::Trait(predicate) = clause.kind().skip_binder() else {
                continue;
            };
            let mut traits = Vec::new();
            reader_traits(
                self.tcx,
                predicate.trait_ref.def_id,
                &mut Vec::new(),
                &mut traits,
            );
            for id in traits {
                result.push((predicate.trait_ref.self_ty().peel_refs(), id));
            }
        }
        result
    }

    fn audited_standard_reader(&self, value: Ty<'tcx>, trait_id: DefId) -> bool {
        let byte_sequence = |value: Ty<'tcx>| {
            matches!(value.peel_refs().kind(), ty::Slice(element) | ty::Array(element, _)
            if matches!(element.kind(), ty::Uint(ty::UintTy::U8)))
        };
        if self.tcx.item_name(trait_id).as_str() == "Read" && byte_sequence(value) {
            return true;
        }
        let ty::Adt(owner, args) = value.peel_refs().kind() else {
            return false;
        };
        if !types::standard(self.tcx, owner.did()) {
            return false;
        }
        match self.tcx.item_name(owner.did()).as_str() {
            "File" => true,
            "Cursor" => args.types().next().is_some_and(|value| byte_sequence(value)
                || matches!(value.peel_refs().kind(), ty::Adt(owner, args) if types::standard(self.tcx, owner.did())
                    && self.tcx.item_name(owner.did()).as_str() == "Vec"
                    && args.types().next().is_some_and(|element| matches!(element.kind(), ty::Uint(ty::UintTy::U8))))),
            _ => false,
        }
    }

    fn checked_reader(&self, value: Ty<'tcx>, trait_id: DefId) -> bool {
        if matches!(value.kind(), ty::Param(_)) {
            return self.provider_callback_parameter(value);
        }
        if self.audited_standard_reader(value, trait_id) {
            return true;
        }
        let names: &[&str] = if self.tcx.item_name(trait_id).as_str() == "Read" {
            &["read"]
        } else {
            &["seek", "rewind"]
        };
        names.iter().all(|name| {
            let Some(method) = self
                .tcx
                .associated_items(trait_id)
                .in_definition_order()
                .find(|item| item.name().as_str() == *name)
            else {
                return false;
            };
            Instance::try_resolve(
                self.tcx,
                self.typing_env(),
                method.def_id,
                self.tcx.mk_args(&[value.into()]),
            )
            .ok()
            .flatten()
            .is_some_and(|instance| {
                self.checked_body(instance.def_id())
                    || *name == "rewind"
                        && instance.def_id() == method.def_id
                        && types::standard(self.tcx, method.def_id)
            })
        })
    }

    pub(crate) fn provider_reader_call(
        &self,
        expression: &'tcx Expr<'tcx>,
        definition: DefId,
    ) -> bool {
        let Some(trait_id) = self.tcx.trait_of_assoc(definition) else {
            return false;
        };
        if !types::standard(self.tcx, trait_id)
            || !matches!(self.tcx.item_name(trait_id).as_str(), "Read" | "Seek")
        {
            return false;
        }
        if !matches!(
            self.tcx.item_name(definition).as_str(),
            "read" | "seek" | "rewind"
        ) {
            return false;
        }
        self.call(expression)
            .and_then(|(_, args)| args.first().copied())
            .is_some_and(|receiver| self.provider_callback_parameter(self.expr_ty(receiver)))
    }

    pub(crate) fn provider_callback_parameter(&self, value: Ty<'tcx>) -> bool {
        if !matches!(value.peel_refs().kind(), ty::Param(_)) {
            return false;
        }
        let owner = self.typeck.hir_owner.def_id.to_def_id();
        let args = ty::GenericArgs::identity_for_item(self.tcx, owner);
        self.provider_callback_types(owner, args)
            .contains(&value.peel_refs())
            || self
                .provider_reader_types(owner, args)
                .iter()
                .any(|(reader, _)| *reader == value.peel_refs())
    }
    pub(crate) fn callback_boundary(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        let Some(args) = self.call_arguments(expression) else {
            return;
        };
        let callbacks = self.provider_callback_types(definition, args);
        for operand in operands {
            let value = self.expr_ty(operand).peel_refs();
            if !callbacks.contains(&value) {
                continue;
            }
            let checked = match value.kind() {
                ty::Closure(id, _) => self.checked_body(*id),
                ty::FnDef(id, _) => {
                    matches!(self.tcx.def_kind(*id), rustc_hir::def::DefKind::Ctor(_, _))
                        || self.checked_body(*id)
                        || external::summary(self.tcx, *id, None).is_some_and(|summary| {
                            summary.work == external::Work::Fixed
                                || matches!(
                                    self.tcx.item_name(definition).as_str(),
                                    "is_sorted_by"
                                        | "stable_sort_by"
                                        | "stable_sort_by_key"
                                        | "sort_unstable_by"
                                        | "sort_unstable_by_key"
                                ) && summary.work == external::Work::Comparison
                        })
                }
                ty::Param(_) => self.provider_callback_parameter(value),
                _ => false,
            };
            if !checked {
                self.report(expression.span, "unproven_decode_charge", &format!(
                    "callback to {} has no concrete checked body; admit child work in a concrete callback", self.tcx.def_path_str(definition)));
            }
        }
        for (reader, trait_id) in self.provider_reader_types(definition, args) {
            if !self.checked_reader(reader, trait_id) {
                self.report(expression.span, "unproven_decode_charge", &format!(
                    "reader callback to {} has no concrete checked {} implementation; use a concrete reader with admitted child work",
                    self.tcx.def_path_str(definition), self.tcx.item_name(trait_id)));
            }
        }
    }
}
