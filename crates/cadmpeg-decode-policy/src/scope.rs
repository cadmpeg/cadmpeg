// SPDX-License-Identifier: Apache-2.0
//! Decode roots and resolved production call reachability.
mod indirect;
mod instances;
mod listing;
mod objects;
mod pattern;

use crate::{flow, types, Analysis, Findings};
use pattern::Pattern;
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{def::Res, Expr, ExprKind};
use rustc_middle::ty::{self, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::{DefId, LocalDefId};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum EdgeKind {
    DirectCall,
    ConstantEvaluation,
    FunctionAddress,
    TraitObjectCall,
    GenericInstantiation,
}

impl EdgeKind {
    fn label(self) -> &'static str {
        match self {
            Self::DirectCall => "direct call",
            Self::ConstantEvaluation => "constant evaluation",
            Self::FunctionAddress => "function address",
            Self::TraitObjectCall => "trait-object call",
            Self::GenericInstantiation => "generic instantiation",
        }
    }
}

#[derive(Default)]
pub(crate) struct Graph {
    roots: BTreeMap<String, String>,
    edges: BTreeSet<(String, String, EdgeKind)>,
    nodes: BTreeMap<String, String>,
    addresses: BTreeSet<(Pattern, String)>,
    pointer_calls: BTreeSet<(String, Pattern)>,
    symbolic_pointer_calls: BTreeSet<(String, Pattern)>,
    trait_calls: BTreeSet<(String, String, Pattern)>,
    symbolic_calls: BTreeSet<(String, String, Pattern)>,
    method_impls: BTreeSet<(String, Pattern, String)>,
    objects: BTreeSet<(String, String, Pattern, String)>,
    object_calls: BTreeSet<(String, String, Pattern)>,
    symbolic_roots: BTreeSet<String>,
    symbolic_instances: BTreeSet<String>,
    symbolic_candidates: BTreeSet<String>,
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
            println!("decode_body_span\t{id}\t{}", body.end);
        }
        for (id, name) in &self.nodes {
            println!("decode_node\t{id}\t{name}");
        }
        for (root, name) in &self.roots {
            println!("decode_root\t{root}\t{name}");
        }
        for (caller, callee, kind) in &self.edges {
            println!("decode_edge\t{caller}\t{callee}\t{}", kind.label());
        }
        for root in &self.symbolic_roots {
            println!("decode_symbolic_root\t{root}");
        }
        for instance in &self.symbolic_instances {
            println!("decode_symbolic_instance\t{instance}");
        }
        for candidate in &self.symbolic_candidates {
            println!("decode_symbolic_candidate\t{candidate}");
        }
        for (caller, callee) in &self.symbolic_edges {
            println!("decode_symbolic_edge\t{caller}\t{callee}");
        }
        for (signature, callee) in &self.addresses {
            println!("decode_address\t{}\t{callee}", signature.wire());
        }
        for (caller, signature) in &self.pointer_calls {
            println!("decode_pointer_call\t{caller}\t{}", signature.wire());
        }
        for (caller, signature) in &self.symbolic_pointer_calls {
            println!(
                "decode_symbolic_pointer_call\t{caller}\t{}",
                signature.wire()
            );
        }
        for (caller, method, signature) in &self.trait_calls {
            println!(
                "decode_trait_call\t{caller}\t{method}\t{}",
                signature.wire()
            );
        }
        for (caller, method, signature) in &self.symbolic_calls {
            println!(
                "decode_symbolic_call\t{caller}\t{method}\t{}",
                signature.wire()
            );
        }
        for (method, signature, target) in &self.method_impls {
            println!(
                "decode_method_impl\t{method}\t{}\t{target}",
                signature.wire()
            );
        }
        for (caller, method, signature, target) in &self.objects {
            println!(
                "decode_object\t{caller}\t{method}\t{}\t{target}",
                signature.wire()
            );
        }
        for (caller, method, signature) in &self.object_calls {
            println!(
                "decode_object_call\t{caller}\t{method}\t{}",
                signature.wire()
            );
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
        let mut constants = BTreeSet::new();
        loop {
            let mut added = false;
            for instance in &self.symbolic_instances {
                if reached.contains(instance) {
                    added |= symbolic.insert(instance.clone());
                }
            }
            for (caller, callee, kind) in &self.edges {
                if reached.contains(caller) {
                    if *kind == EdgeKind::ConstantEvaluation {
                        added |= constants.insert(callee.clone());
                    } else {
                        added |= reached.insert(callee.clone());
                    }
                }
                if constants.contains(caller) {
                    if *kind == EdgeKind::FunctionAddress {
                        added |= reached.insert(callee.clone());
                    } else {
                        added |= constants.insert(callee.clone());
                    }
                }
            }
            for (caller, callee) in &self.symbolic_edges {
                if constants.contains(caller) {
                    added |= constants.insert(callee.clone());
                }
                if symbolic.contains(caller) {
                    added |= symbolic.insert(callee.clone());
                    added |= reached.insert(callee.clone());
                }
            }
            for (caller, method, signature, target) in &self.objects {
                if reached.contains(caller)
                    && self
                        .object_calls
                        .iter()
                        .any(|(caller, called, called_signature)| {
                            called == method
                                && reached.contains(caller)
                                && signature.compatible(called_signature)
                        })
                {
                    added |= reached.insert(target.clone());
                    if symbolic.contains(caller) {
                        added |= symbolic.insert(target.clone());
                    }
                }
            }
            for (caller, signature, deferred) in self
                .pointer_calls
                .iter()
                .map(|(caller, signature)| (caller, signature, false))
                .chain(
                    self.symbolic_pointer_calls
                        .iter()
                        .map(|(caller, signature)| (caller, signature, true)),
                )
            {
                if if deferred {
                    symbolic.contains(caller)
                } else {
                    reached.contains(caller)
                } {
                    for (candidate, target) in &self.addresses {
                        if signature.compatible(candidate) {
                            added |= reached.insert(target.clone());
                            if self.symbolic_candidates.contains(target) {
                                added |= symbolic.insert(target.clone());
                            }
                        }
                    }
                }
            }
            for (caller, method, signature) in &self.trait_calls {
                if reached.contains(caller) {
                    for (candidate, candidate_signature, target) in &self.method_impls {
                        if candidate == method && signature.compatible(candidate_signature) {
                            added |= reached.insert(target.clone());
                            if self.symbolic_candidates.contains(target) {
                                added |= symbolic.insert(target.clone());
                            }
                        }
                    }
                }
            }
            for (caller, method, signature) in &self.symbolic_calls {
                if symbolic.contains(caller) {
                    for (candidate, candidate_signature, target) in &self.method_impls {
                        if candidate == method && signature.compatible(candidate_signature) {
                            added |= reached.insert(target.clone());
                            added |= symbolic.insert(target.clone());
                        }
                    }
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
    types::closed_admission_body(tcx, owner.to_def_id())
        || codec_input_method(tcx, owner.to_def_id())
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
        if matches!(tcx.def_kind(*owner), rustc_hir::def::DefKind::AssocFn) {
            let method = tcx
                .opt_associated_item(owner.to_def_id())
                .and_then(|item| item.trait_item_def_id())
                .or_else(|| {
                    tcx.trait_of_assoc(owner.to_def_id())
                        .map(|_| owner.to_def_id())
                });
            if let Some(method) = method {
                if ty::GenericArgs::identity_for_item(tcx, *owner).has_non_region_param() {
                    graph
                        .symbolic_candidates
                        .insert(key(tcx, owner.to_def_id()));
                }
                let signature = indirect::method_signature(
                    tcx,
                    ty::TypingEnv::post_analysis(tcx, *owner),
                    owner.to_def_id(),
                    ty::GenericArgs::identity_for_item(tcx, *owner),
                );
                graph.method_impls.insert((
                    key(tcx, method),
                    signature,
                    key(tcx, owner.to_def_id()),
                ));
            }
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
        self.edge_kind(
            callee,
            if constant_owner(self.analysis.tcx, callee) {
                EdgeKind::ConstantEvaluation
            } else {
                EdgeKind::DirectCall
            },
        );
    }

    fn edge_kind(&mut self, callee: DefId, kind: EdgeKind) {
        if types::checked(self.analysis.tcx, callee) {
            self.graph
                .edges
                .insert((self.caller.clone(), key(self.analysis.tcx, callee), kind));
        }
    }

    fn coercion(&mut self, source: ty::Ty<'tcx>, target: ty::Ty<'tcx>) {
        let mut instances = Vec::new();
        objects::targets(
            self.analysis.tcx,
            self.analysis.typing_env(),
            source,
            target,
            &mut instances,
        );
        for target in instances {
            objects::register(
                self.analysis.tcx,
                self.graph,
                self.pending,
                &self.caller,
                target,
                self.analysis.typing_env(),
                0,
            );
        }
    }

    fn method(&mut self, expression: &'tcx Expr<'tcx>, definition: DefId, resolved: Option<DefId>) {
        if let Some(id) = resolved {
            self.edge(id);
            return;
        }
        if !matches!(
            self.analysis.tcx.def_kind(definition),
            rustc_hir::def::DefKind::AssocFn
        ) || self.analysis.tcx.trait_of_assoc(definition).is_none()
        {
            self.edge(definition);
            return;
        }
        let arguments = self
            .analysis
            .call_arguments(expression)
            .unwrap_or_else(|| self.analysis.typeck.node_args(expression.hir_id));
        if arguments.len() != self.analysis.tcx.generics_of(definition).count() {
            return;
        }
        let object = arguments
            .types()
            .next()
            .is_some_and(|value| matches!(value.peel_refs().kind(), ty::Dynamic(..)));
        let signature = indirect::method_signature(
            self.analysis.tcx,
            self.analysis.typing_env(),
            definition,
            arguments,
        );
        let deferred = arguments.has_non_region_param()
            && !root(self.analysis.tcx, self.analysis.typing_owner);
        if object && !deferred {
            self.graph.object_calls.insert((
                self.caller.clone(),
                key(self.analysis.tcx, definition),
                signature.clone(),
            ));
        }
        self.graph.symbolic_calls.insert((
            self.caller.clone(),
            key(self.analysis.tcx, definition),
            signature.clone(),
        ));
        if !deferred {
            self.graph.trait_calls.insert((
                self.caller.clone(),
                key(self.analysis.tcx, definition),
                signature,
            ));
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
            indirect::address(
                self.analysis.tcx,
                self.graph,
                self.analysis.typing_env(),
                &self.caller,
                self.analysis.expr_ty(operand),
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
                    (0, EdgeKind::DirectCall),
                );
                if crate::conversion::core_conversion_trait(
                    self.analysis.tcx,
                    id,
                    "Into",
                ) {
                    if let Some((_, operands)) = self.analysis.call(expression) {
                        if let Some(source) = operands.first().map(|operand| {
                            self.analysis.expr_ty(operand)
                        }) {
                            if let Some(from) = crate::conversion::forwarded_from(
                                self.analysis.tcx,
                                self.analysis.typing_env(),
                                instance,
                                source,
                                self.analysis.expr_ty(expression),
                            ) {
                                instances::enqueue(
                                    self.analysis.tcx,
                                    self.graph,
                                    self.pending,
                                    &self.caller,
                                    from,
                                    self.analysis.typing_env(),
                                    (0, EdgeKind::DirectCall),
                                );
                            }
                        }
                    }
                }
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
                    (0, EdgeKind::FunctionAddress),
                );
            }
        }
        if !self.direct_callee && reference.is_some() {
            indirect::address(
                self.analysis.tcx,
                self.graph,
                self.analysis.typing_env(),
                &self.caller,
                value,
                self.analysis.expr_ty_adjusted(expression),
            );
        }
        if let ExprKind::Call(callee, _) = expression.kind {
            let value = self.analysis.expr_ty(callee);
            if matches!(value.peel_refs().kind(), ty::FnPtr(..)) {
                if let Some(signature) =
                    indirect::signature(self.analysis.tcx, self.analysis.typing_env(), value)
                {
                    let deferred = value.has_non_region_param()
                        && !root(self.analysis.tcx, self.analysis.typing_owner);
                    if deferred {
                        self.graph
                            .symbolic_pointer_calls
                            .insert((self.caller.clone(), signature));
                    } else {
                        self.graph
                            .pointer_calls
                            .insert((self.caller.clone(), signature));
                    }
                }
            }
        }
        if let ty::Closure(id, _) = self.analysis.expr_ty(expression).peel_refs().kind() {
            self.edge_kind(*id, EdgeKind::FunctionAddress);
        }
        if let Some((id, _)) = self.analysis.call(expression) {
            self.method(expression, id, self.analysis.implementation(expression, id));
            if let Some(custom) = self.analysis.custom_trait(expression, id) {
                self.edge(custom);
            }
        } else if let Some(id) = self
            .analysis
            .typeck
            .type_dependent_def_id(expression.hir_id)
        {
            self.method(expression, id, self.analysis.implementation(expression, id));
            if let Some(custom) = self.analysis.custom_trait(expression, id) {
                self.edge(custom);
            }
        } else if let ty::FnDef(id, _) = self.analysis.expr_ty(expression).kind() {
            self.edge_kind(
                *id,
                if self.direct_callee {
                    EdgeKind::DirectCall
                } else {
                    EdgeKind::FunctionAddress
                },
            );
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

fn constant_owner(tcx: TyCtxt<'_>, owner: DefId) -> bool {
    matches!(
        tcx.def_kind(owner),
        rustc_hir::def::DefKind::Static { .. }
            | rustc_hir::def::DefKind::Const { .. }
            | rustc_hir::def::DefKind::AssocConst { .. }
            | rustc_hir::def::DefKind::AnonConst
    )
}
