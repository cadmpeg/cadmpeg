// SPDX-License-Identifier: Apache-2.0
//! Decode roots and resolved production call reachability.
mod instances;
mod listing;
mod objects;

use crate::{flow, types, Analysis, Findings};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{def::Res, Expr, ExprKind};
use rustc_middle::ty::{self, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::{DefId, LocalDefId};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Default)]
pub(crate) struct Graph {
    roots: BTreeMap<String, String>,
    edges: BTreeSet<(String, String)>,
    uncertain: BTreeSet<String>,
    addresses: BTreeSet<String>,
    symbolic_roots: BTreeSet<String>,
    symbolic_edges: BTreeSet<(String, String)>,
    bodies: BTreeMap<String, listing::Body>,
}

impl Graph {
    pub(crate) fn print(&self) {
        for (id, body) in &self.bodies {
            println!(
                "decode_body\t{id}\t{}\t{}\t{}\t{}\t{}",
                body.path, body.line, body.name, body.reason, body.eligible
            );
        }
        for (root, name) in &self.roots {
            println!("decode_root\t{root}\t{name}");
        }
        for (caller, callee) in &self.edges {
            println!("decode_edge\t{caller}\t{callee}");
        }
        for root in &self.symbolic_roots {
            println!("decode_symbolic_root\t{root}");
        }
        for (caller, callee) in &self.symbolic_edges {
            println!("decode_symbolic_edge\t{caller}\t{callee}");
        }
        for caller in &self.uncertain {
            println!("decode_uncertain\t{caller}");
        }
        for callee in &self.addresses {
            println!("decode_address\t{callee}");
        }
    }

    pub(crate) fn print_unreachable(&self, reached: &BTreeSet<String>) {
        for (id, body) in &self.bodies {
            if !body.eligible || !reached.contains(id) {
                println!(
                    "unreachable_decode_body\t{}\t{}\t{}\t{}",
                    body.path, body.line, body.name, body.reason
                );
            }
        }
    }

    pub(crate) fn reachable(&self) -> BTreeSet<String> {
        if let Some(path) = std::env::var_os("CADMPEG_POLICY_SCOPE") {
            match std::fs::read_to_string(path) {
                Ok(source) => return source.lines().map(str::to_owned).collect(),
                Err(error) => {
                    eprintln!("cannot read decode scope: {error}");
                    std::process::exit(2);
                }
            }
        }
        let mut reached: BTreeSet<_> = self.roots.keys().cloned().collect();
        let mut symbolic = self.symbolic_roots.clone();
        loop {
            let mut added = false;
            for (caller, callee) in &self.edges {
                if reached.contains(caller) {
                    added |= reached.insert(callee.clone());
                }
            }
            for (caller, callee) in &self.symbolic_edges {
                if symbolic.contains(caller) {
                    added |= symbolic.insert(callee.clone());
                    added |= reached.insert(callee.clone());
                }
            }
            if self.uncertain.iter().any(|caller| reached.contains(caller)) {
                for callee in &self.addresses {
                    added |= reached.insert(callee.clone());
                }
            }
            if !added {
                return reached;
            }
        }
    }
}

fn codec_input_method(tcx: TyCtxt<'_>, owner: DefId) -> bool {
    let parent = tcx.parent(owner);
    if !matches!(
        tcx.def_kind(parent),
        rustc_hir::def::DefKind::Impl { of_trait: true }
    ) {
        return false;
    }
    let trait_id = tcx.impl_trait_ref(parent).skip_binder().def_id;
    let codec = matches!(tcx.item_name(trait_id).as_str(), "Codec" | "CodecBackend")
        && (tcx.crate_name(trait_id.krate).as_str() == "cadmpeg_ir"
            && tcx.item_name(tcx.parent(trait_id)).as_str() == "codec"
            || std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some());
    codec
        && matches!(
            tcx.item_name(owner).as_str(),
            "detect_impl"
                | "inspect_impl"
                | "decode_impl"
                | "detect"
                | "inspect"
                | "decode"
                | "decode_with_context"
        )
}

fn root(tcx: TyCtxt<'_>, owner: LocalDefId) -> bool {
    if !crate::production(tcx, owner.to_def_id()) {
        return false;
    }
    codec_input_method(tcx, owner.to_def_id())
        || matches!(
            tcx.def_kind(owner),
            rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
        ) && tcx.visibility(owner).is_public()
            && tcx
                .fn_sig(owner)
                .instantiate_identity()
                .skip_binder()
                .inputs()
                .iter()
                .any(|input| types::has_context(tcx, *input, &mut Vec::new()))
}

pub(crate) fn collect<'tcx>(tcx: TyCtxt<'tcx>, owners: &[LocalDefId]) -> Graph {
    let mut graph = Graph::default();
    let mut pending = Vec::new();
    for owner in owners {
        if let Some(body) = listing::body(tcx, *owner) {
            graph.bodies.insert(key(tcx, owner.to_def_id()), body);
        }
        if root(tcx, *owner) {
            if ty::GenericArgs::identity_for_item(tcx, *owner).has_non_region_param() {
                graph.uncertain.insert(key(tcx, owner.to_def_id()));
                graph.symbolic_roots.insert(key(tcx, owner.to_def_id()));
            }
            graph
                .roots
                .insert(key(tcx, owner.to_def_id()), tcx.def_path_str(*owner));
        }
        let derived_operation = types::derived(tcx, owner.to_def_id())
            && tcx.opt_item_name(owner.to_def_id()).is_some_and(|name| {
                matches!(
                    name.as_str(),
                    "clone" | "eq" | "partial_cmp" | "cmp" | "hash" | "fmt" | "default"
                )
            });
        let initializer = matches!(
            tcx.def_kind(*owner),
            rustc_hir::def::DefKind::Static { .. }
                | rustc_hir::def::DefKind::Const { .. }
                | rustc_hir::def::DefKind::AssocConst { .. }
                | rustc_hir::def::DefKind::AnonConst
        );
        if !crate::production(tcx, owner.to_def_id()) && !derived_operation && !initializer {
            continue;
        }
        if let Some(method) = tcx
            .opt_associated_item(owner.to_def_id())
            .and_then(|item| item.trait_item_def_id())
        {
            graph
                .symbolic_edges
                .insert((key(tcx, method), key(tcx, owner.to_def_id())));
        }
        let mut findings = Findings::default();
        Calls {
            analysis: Analysis {
                tcx,
                typeck: tcx.typeck(*owner),
                typing_owner: *owner,
                arguments: None,
                fixed_parameters: HashSet::new(),
                flow: flow::Flow::default(),
                findings: &mut findings,
            },
            caller: key(tcx, owner.to_def_id()),
            graph: &mut graph,
            direct_callee: false,
            pending: &mut pending,
        }
        .visit_body(tcx.hir_body_owned_by(*owner));
    }
    instances::expand(tcx, &mut graph, pending);
    graph
}

struct Calls<'a, 'b, 'tcx> {
    analysis: Analysis<'a, 'tcx>,
    caller: String,
    graph: &'b mut Graph,
    direct_callee: bool,
    pending: &'b mut Vec<instances::Concrete<'tcx>>,
}

impl<'tcx> Calls<'_, '_, 'tcx> {
    fn edge(&mut self, callee: DefId) {
        if types::checked(self.analysis.tcx, callee) {
            self.graph
                .edges
                .insert((self.caller.clone(), key(self.analysis.tcx, callee)));
        }
    }

    fn coercion(&mut self, source: ty::Ty<'tcx>, target: ty::Ty<'tcx>) {
        let deferred =
            source.has_non_region_param() && !root(self.analysis.tcx, self.analysis.typing_owner);
        let mut instances = Vec::new();
        if !objects::targets(
            self.analysis.tcx,
            self.analysis.typing_env(),
            source,
            target,
            &mut instances,
        ) && !deferred
        {
            self.graph.uncertain.insert(self.caller.clone());
        }
        for instance in instances {
            if deferred {
                if types::checked(self.analysis.tcx, instance.def_id()) {
                    self.graph.symbolic_edges.insert((
                        self.caller.clone(),
                        key(self.analysis.tcx, instance.def_id()),
                    ));
                }
                continue;
            }
            instances::enqueue(
                self.analysis.tcx,
                self.graph,
                self.pending,
                &self.caller,
                instance,
                self.analysis.typing_env(),
                0,
            );
            if types::checked(self.analysis.tcx, instance.def_id()) {
                self.graph
                    .addresses
                    .insert(key(self.analysis.tcx, instance.def_id()));
            }
        }
    }

    fn method(&mut self, definition: DefId, resolved: Option<DefId>) {
        if let Some(id) = resolved {
            self.edge(id);
            return;
        }
        let Some(trait_id) = self.analysis.tcx.trait_of_assoc(definition) else {
            self.edge(definition);
            return;
        };
        self.edge(definition);
        self.graph
            .symbolic_edges
            .insert((self.caller.clone(), key(self.analysis.tcx, definition)));
        let deferred =
            ty::GenericArgs::identity_for_item(self.analysis.tcx, self.analysis.typing_owner)
                .has_non_region_param()
                && !root(self.analysis.tcx, self.analysis.typing_owner);
        if deferred {
            return;
        }
        self.graph.uncertain.insert(self.caller.clone());
        for implementation in self.analysis.tcx.all_impls(trait_id) {
            let item = self
                .analysis
                .tcx
                .associated_items(implementation)
                .in_definition_order()
                .find(|item| item.name() == self.analysis.tcx.item_name(definition));
            if let Some(item) = item {
                self.edge(item.def_id);
            }
        }
    }
}

impl<'tcx> Visitor<'tcx> for Calls<'_, '_, 'tcx> {
    fn visit_anon_const(&mut self, constant: &'tcx rustc_hir::AnonConst) {
        self.edge(constant.def_id.to_def_id());
    }

    fn visit_inline_const(&mut self, constant: &'tcx rustc_hir::ConstBlock) {
        self.edge(constant.def_id.to_def_id());
    }

    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(ref path) = expression.kind {
            if let Res::Def(kind, id) = self.analysis.typeck.qpath_res(path, expression.hir_id) {
                if matches!(
                    kind,
                    rustc_hir::def::DefKind::Static { .. }
                        | rustc_hir::def::DefKind::Const { .. }
                        | rustc_hir::def::DefKind::AssocConst { .. }
                ) {
                    self.edge(id);
                }
            }
        }
        let mut source = self.analysis.expr_ty(expression);
        for adjustment in self.analysis.typeck.expr_adjustments(expression) {
            let target = self.analysis.substitute(adjustment.target);
            if matches!(
                adjustment.kind,
                ty::adjustment::Adjust::Pointer(ty::adjustment::PointerCoercion::Unsize)
            ) {
                self.coercion(source, target);
            }
            source = target;
        }
        if let ExprKind::Cast(operand, _) = expression.kind {
            self.coercion(
                self.analysis.expr_ty_adjusted(operand),
                self.analysis.expr_ty(expression),
            );
        }
        if let Some((id, _)) = self.analysis.call(expression) {
            if let Some(instance) = self.analysis.resolved_instance(expression, id) {
                instances::enqueue(
                    self.analysis.tcx,
                    self.graph,
                    self.pending,
                    &self.caller,
                    instance,
                    self.analysis.typing_env(),
                    0,
                );
            }
        }
        let value = self.analysis.expr_ty(expression);
        let reference = match value.peel_refs().kind() {
            ty::FnDef(id, args) => args.no_bound_vars().and_then(|args| {
                ty::Instance::try_resolve(self.analysis.tcx, self.analysis.typing_env(), *id, args)
                    .ok()
                    .flatten()
            }),
            ty::Closure(id, args) => Some(ty::Instance::new_raw(*id, args)),
            _ => None,
        };
        if !self.direct_callee {
            if let Some(instance) = reference {
                instances::enqueue(
                    self.analysis.tcx,
                    self.graph,
                    self.pending,
                    &self.caller,
                    instance,
                    self.analysis.typing_env(),
                    0,
                );
            }
        }
        if !self.direct_callee {
            if let Some(instance) = reference {
                if types::checked(self.analysis.tcx, instance.def_id()) {
                    self.graph
                        .addresses
                        .insert(key(self.analysis.tcx, instance.def_id()));
                }
            }
        }
        if let ExprKind::Call(callee, _) = expression.kind {
            if matches!(
                self.analysis.expr_ty(callee).peel_refs().kind(),
                ty::FnPtr(..)
            ) {
                self.graph.uncertain.insert(self.caller.clone());
            }
        }
        if let ty::Closure(id, _) = self.analysis.expr_ty(expression).peel_refs().kind() {
            self.edge(*id);
        }
        if let Some((id, _)) = self.analysis.call(expression) {
            self.method(id, self.analysis.implementation(expression, id));
            if let Some(custom) = self.analysis.custom_trait(expression, id) {
                self.edge(custom);
            }
        } else if let Some(id) = self
            .analysis
            .typeck
            .type_dependent_def_id(expression.hir_id)
        {
            self.method(id, self.analysis.implementation(expression, id));
            if let Some(custom) = self.analysis.custom_trait(expression, id) {
                self.edge(custom);
            }
        } else if let ty::FnDef(id, _) = self.analysis.expr_ty(expression).kind() {
            self.edge(*id);
        }
        if let ExprKind::Call(callee, arguments) = expression.kind {
            let direct = self.direct_callee;
            self.direct_callee = matches!(self.analysis.expr_ty(callee).kind(), ty::FnDef(..));
            self.visit_expr(callee);
            self.direct_callee = false;
            for argument in arguments {
                self.visit_expr(argument);
            }
            self.direct_callee = direct;
        } else if !matches!(expression.kind, ExprKind::Closure(_)) {
            let direct = self.direct_callee;
            self.direct_callee = false;
            walk_expr(self, expression);
            self.direct_callee = direct;
        }
    }
}

pub(crate) fn key(tcx: TyCtxt<'_>, definition: DefId) -> String {
    format!("{:?}", tcx.def_path_hash(definition))
}
