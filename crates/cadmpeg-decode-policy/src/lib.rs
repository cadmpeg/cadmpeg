// SPDX-License-Identifier: Apache-2.0
#![feature(rustc_private)]
//! Type-aware decode allocation and work admission checks.

extern crate rustc_ast;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

mod allocation;
mod callee;
mod conversion;
mod extent;
mod external;
mod fixed;
mod flow;
mod instantiation;
mod storage;
mod scope;
mod types;
mod work;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::intravisit::Visitor;
use rustc_hir::{Expr, ExprKind};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::{TyCtxt, TypeckResults};
use rustc_span::def_id::{DefId, LocalDefId};
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Default)]
struct Findings {
    externals: BTreeSet<String>,
    conversions: HashMap<rustc_hir::HirId, Vec<bool>>,
    admitted_operations: HashSet<rustc_hir::HirId>,
    entries: BTreeMap<(String, usize, u32, u32, String), BTreeSet<String>>,
}

struct DecodeCallbacks {
    findings: Findings,
}

impl Callbacks for DecodeCallbacks {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        let mut owners: Vec<_> = tcx.hir_body_owners().collect();
        owners.sort_by_key(|owner| owner.local_def_index.as_u32());
        let graph = scope::collect(tcx, &owners);
        if std::env::var_os("CADMPEG_POLICY_GRAPH").is_some() {
            graph.print();
            return Compilation::Continue;
        }
        let reachable = graph.reachable();
        owners.retain(|owner| reachable.contains(&scope::key(tcx, owner.to_def_id())));
        let instantiations = instantiation::collect(tcx, &owners);
        let mut bodies = HashMap::new();
        for owner in owners {
            if production(tcx, owner.to_def_id()) {
                let mut findings = Findings::default();
                Analysis {
                    tcx,
                    typeck: tcx.typeck(owner),
                    typing_owner: owner,
                    arguments: None,
                    fixed_parameters: HashSet::new(),
                    flow: flow::Flow::default(),
                    findings: &mut findings,
                }
                .visit_body(tcx.hir_body_owned_by(owner));
                bodies.insert(owner, findings);
            }
        }
        let mut resolved = HashMap::<LocalDefId, BTreeSet<_>>::new();
        let mut unresolved = HashMap::<LocalDefId, BTreeSet<_>>::new();
        for instantiation in instantiations {
            if !instantiation.enumerated {
                Analysis {
                    tcx,
                    typeck: tcx.typeck(instantiation.caller),
                    typing_owner: instantiation.caller,
                    arguments: None,
                    fixed_parameters: HashSet::new(),
                    flow: flow::Flow::default(),
                    findings: &mut self.findings,
                }
                .report(
                    instantiation.span,
                    "unproven_decode_charge",
                    "generic instantiations exceed the compiler recursion limit",
                );
                continue;
            }
            let Some(local) = instantiation.instance.def_id().as_local() else {
                instantiation::check_imported(tcx, &instantiation, &mut self.findings);
                continue;
            };
            let mut concrete = Findings::default();
            Analysis {
                tcx,
                typeck: tcx.typeck(local),
                typing_owner: instantiation.caller,
                arguments: Some(instantiation.instance.args),
                fixed_parameters: instantiation.fixed_parameters.clone(),
                flow: flow::Flow::with_parameters(&instantiation.admitted_parameters),
                findings: &mut concrete,
            }
            .visit_body(tcx.hir_body_owned_by(local));
            self.findings
                .externals
                .extend(concrete.externals.iter().cloned());
            if let Some(symbolic) = bodies.get(&local) {
                resolved.entry(local).or_default().extend(
                    symbolic
                        .entries
                        .keys()
                        .filter(|key| key.4 == "unproven_decode_charge")
                        .cloned(),
                );
                unresolved.entry(local).or_default().extend(
                    concrete
                        .entries
                        .keys()
                        .filter(|key| {
                            key.4 == "unproven_decode_charge"
                                && symbolic.entries.get(*key) == concrete.entries.get(*key)
                        })
                        .cloned(),
                );
                for (key, messages) in concrete.entries {
                    if !symbolic.entries.keys().any(|site| {
                        site.0 == key.0
                            && site.2 == key.2
                            && site.3 == key.3
                            && site.4 == "unproven_decode_charge"
                    }) || symbolic.entries.get(&key).is_some_and(|original| {
                        key.4 != "unproven_decode_charge" || original == &messages
                    }) {
                        continue;
                    }
                    let types = instantiation
                        .instance
                        .args
                        .types()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Analysis {
                        tcx,
                        typeck: tcx.typeck(instantiation.caller),
                        typing_owner: instantiation.caller,
                        arguments: None,
                        fixed_parameters: HashSet::new(),
                        flow: flow::Flow::default(),
                        findings: &mut self.findings,
                    }
                    .report(
                        instantiation.span,
                        &key.4,
                        &format!(
                            "concrete instantiation <{types}> of {}: {}",
                            tcx.def_path_str(local),
                            messages.into_iter().collect::<Vec<_>>().join("; ")
                        ),
                    );
                }
            }
        }
        for (local, mut findings) in bodies {
            findings.entries.retain(|key, _| {
                !resolved.get(&local).is_some_and(|keys| keys.contains(key))
                    || unresolved
                        .get(&local)
                        .is_some_and(|keys| keys.contains(key))
            });
            self.findings.externals.extend(findings.externals);
            for (key, messages) in findings.entries {
                self.findings
                    .entries
                    .entry(key)
                    .or_default()
                    .extend(messages);
            }
        }
        for ((path, line, _, _, rule), messages) in &self.findings.entries {
            println!(
                "{rule}\t{path}\t{line}\t{}",
                messages.iter().cloned().collect::<Vec<_>>().join("; ")
            );
        }
        for operation in &self.findings.externals {
            println!("{operation}");
        }
        Compilation::Continue
    }
}

fn production(tcx: TyCtxt<'_>, owner: DefId) -> bool {
    if types::derived(tcx, owner)
        || !types::checked(tcx, owner)
        || !matches!(
            tcx.def_kind(owner),
            rustc_hir::def::DefKind::Fn
                | rustc_hir::def::DefKind::AssocFn
                | rustc_hir::def::DefKind::Closure
        )
    {
        return false;
    }
    let parent = tcx.parent(owner);
    if matches!(
        tcx.def_kind(parent),
        rustc_hir::def::DefKind::Impl { of_trait: true }
    ) && types::serde_serialize(tcx, tcx.impl_trait_ref(parent).skip_binder().def_id)
    {
        return false;
    }
    let path = tcx
        .sess
        .source_map()
        .lookup_char_pos(tcx.def_span(owner).source_callsite().lo())
        .file
        .name
        .prefer_local_unconditionally()
        .to_string();
    let parts: Vec<&str> = path.split('/').collect();
    if parts.iter().any(|part| {
        matches!(
            *part,
            "tests"
                | "test_support"
                | "golden_tests"
                | "integration_tests"
                | "benches"
                | "bin"

        )
    }) {
        return false;
    }
    if parts.last().is_some_and(|name| name.contains("test")) {
        return false;
    }
    true
}

struct Analysis<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    typing_owner: LocalDefId,
    arguments: Option<rustc_middle::ty::GenericArgsRef<'tcx>>,
    fixed_parameters: HashSet<rustc_hir::HirId>,
    flow: flow::Flow,
    findings: &'a mut Findings,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn typing_env(&self) -> rustc_middle::ty::TypingEnv<'tcx> {
        rustc_middle::ty::TypingEnv::post_analysis(self.tcx, self.typing_owner)
    }

    fn substitute<T>(&self, value: T) -> T
    where
        T: rustc_middle::ty::TypeFoldable<TyCtxt<'tcx>> + Copy,
    {
        self.arguments.map_or(value, |arguments| {
            rustc_middle::ty::EarlyBinder::bind(self.tcx, value)
                .instantiate(self.tcx, arguments)
                .skip_norm_wip()
        })
    }

    fn expr_ty(&self, expression: &Expr<'tcx>) -> rustc_middle::ty::Ty<'tcx> {
        let value = self.substitute(self.typeck.expr_ty(expression));
        self.tcx
            .try_normalize_erasing_regions(
                self.typing_env(),
                rustc_middle::ty::Unnormalized::new_wip(value),
            )
            .unwrap_or(value)
    }

    fn expr_ty_adjusted(&self, expression: &Expr<'tcx>) -> rustc_middle::ty::Ty<'tcx> {
        let value = self.substitute(self.typeck.expr_ty_adjusted(expression));
        self.tcx
            .try_normalize_erasing_regions(
                self.typing_env(),
                rustc_middle::ty::Unnormalized::new_wip(value),
            )
            .unwrap_or(value)
    }

    fn call(&self, expression: &'tcx Expr<'tcx>) -> Option<(DefId, Vec<&'tcx Expr<'tcx>>)> {
        match expression.kind {
            ExprKind::MethodCall(_, receiver, arguments, _) => {
                let definition = self.typeck.type_dependent_def_id(expression.hir_id)?;
                let mut operands = vec![receiver];
                operands.extend(arguments);
                Some((definition, operands))
            }
            ExprKind::Call(callee, arguments) => match self.expr_ty(callee).kind() {
                rustc_middle::ty::FnDef(definition, _) => {
                    Some((*definition, arguments.iter().collect()))
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn report(&mut self, span: Span, rule: &str, message: &str) {
        let location = self
            .tcx
            .sess
            .source_map()
            .lookup_char_pos(span.source_callsite().lo());
        let path = location
            .file
            .name
            .prefer_local_unconditionally()
            .to_string();
        let relative = std::env::current_dir()
            .ok()
            .and_then(|root| {
                std::path::Path::new(&path)
                    .strip_prefix(root)
                    .ok()
                    .map(|path| path.display().to_string())
            })
            .unwrap_or(path);
        let source = span.source_callsite();
        self.findings
            .entries
            .entry((
                relative,
                location.line,
                source.lo().0,
                source.hi().0,
                rule.to_owned(),
            ))
            .or_default()
            .insert(format!("[column {}] {message}", location.col.0 + 1));
    }

    fn shape_report(&mut self, expression: &Expr<'tcx>, shape: types::Shape, operation: &str) {
        match shape {
            types::Shape::Fixed => (),
            types::Shape::Dynamic => self.report(expression.span, "uncharged_decode_allocation", &format!("{operation} allocates input-sized storage; use a core charged copy, format or collection operation")),
            types::Shape::Unknown => self.report(expression.span, "unproven_decode_charge", &format!("{operation}: allocation extent or implementation is unresolved; use a concrete type or a core charged operation")),
        }
    }
}

impl<'tcx> Visitor<'tcx> for Analysis<'_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if std::env::var_os("CADMPEG_POLICY_EXTERNALS").is_some() {
            if let Some((definition, operands)) = self.call(expression).or_else(|| {
                let definition = self.typeck.type_dependent_def_id(expression.hir_id)?;
                let receiver = match expression.kind {
                    ExprKind::Binary(_, left, _)
                    | ExprKind::AssignOp(_, left, _)
                    | ExprKind::Index(left, _, _)
                    | ExprKind::Unary(_, left) => left,
                    _ => return None,
                };
                Some((definition, vec![receiver]))
            }) {
                let resolved = self
                    .implementation(expression, definition)
                    .unwrap_or(definition);
                if !types::checked(self.tcx, resolved)
                    && !matches!(
                        self.tcx.def_kind(resolved),
                        rustc_hir::def::DefKind::Ctor(_, _)
                    )
                {
                    self.findings.externals.insert(external::inventory_row(
                        self.tcx,
                        resolved,
                        operands.first().map(|operand| self.expr_ty(operand)),
                    ));
                }
            }
        }
        self.indirect(expression);
        self.allocation(expression);
        self.visit_work_expression(expression);
    }
}

/// Runs rustc and returns whether decode-contract findings were emitted.
pub fn run(arguments: &[String]) -> bool {
    let mut callbacks = DecodeCallbacks {
        findings: Findings::default(),
    };
    let mut arguments = arguments.to_vec();
    arguments.push("-Zalways-encode-mir".to_owned());
    rustc_driver::run_compiler(&arguments, &mut callbacks);
    !callbacks.findings.entries.is_empty()
}

#[cfg(test)]
mod integration_tests;
