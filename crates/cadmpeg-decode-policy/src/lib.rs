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
mod extent;
mod external;
mod flow;
mod instantiation;
mod types;
mod work;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::intravisit::Visitor;
use rustc_hir::{Expr, ExprKind};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::{TyCtxt, TypeckResults};
use rustc_span::def_id::{DefId, LocalDefId};
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default)]
struct Findings {
    entries: BTreeMap<(String, usize, u32, u32, String), BTreeSet<String>>,
}

struct DecodeCallbacks {
    findings: Findings,
}

impl Callbacks for DecodeCallbacks {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        let mut owners: Vec<_> = tcx.hir_body_owners().collect();
        owners.sort_by_key(|owner| owner.local_def_index.as_u32());
        let instantiations = instantiation::collect(tcx, &owners);
        let mut bodies = HashMap::new();
        for owner in owners {
            if production(tcx, owner) {
                let mut findings = Findings::default();
                Analysis {
                    tcx,
                    typeck: tcx.typeck(owner),
                    typing_owner: owner,
                    arguments: None,
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
                Analysis { tcx, typeck: tcx.typeck(instantiation.caller), typing_owner: instantiation.caller,
                    arguments: None, flow: flow::Flow::default(), findings: &mut self.findings }
                    .report(instantiation.span, "unproven_decode_charge", "generic instantiations exceed the compiler recursion limit");
                continue;
            }
            let Some(local) = instantiation.instance.def_id().as_local() else {
                instantiation::check_imported(tcx, &instantiation, &mut self.findings);
                continue;
            };
            let mut concrete = Findings::default();
            Analysis { tcx, typeck: tcx.typeck(local), typing_owner: instantiation.caller,
                arguments: Some(instantiation.instance.args), flow: flow::Flow::default(),
                findings: &mut concrete }.visit_body(tcx.hir_body_owned_by(local));
            if let Some(symbolic) = bodies.get(&local) {
                resolved.entry(local).or_default().extend(symbolic.entries.keys()
                    .filter(|key| key.4 == "unproven_decode_charge").cloned());
                unresolved.entry(local).or_default().extend(concrete.entries.keys()
                    .filter(|key| key.4 == "unproven_decode_charge").cloned());
                for (key, messages) in concrete.entries {
                    if symbolic.entries.get(&key).is_some_and(|original| key.4 != "unproven_decode_charge" || original == &messages) {
                        continue;
                    }
                    let types = instantiation.instance.args.types().map(|value| value.to_string())
                        .collect::<Vec<_>>().join(", ");
                    Analysis { tcx, typeck: tcx.typeck(instantiation.caller), typing_owner: instantiation.caller,
                        arguments: None, flow: flow::Flow::default(), findings: &mut self.findings }
                        .report(instantiation.span, &key.4, &format!("concrete instantiation <{types}> of {}: {}",
                            tcx.def_path_str(local), messages.into_iter().collect::<Vec<_>>().join("; ")));
                }

            }
        }
        for (local, mut findings) in bodies {
            findings.entries.retain(|key, _| !resolved.get(&local).is_some_and(|keys| keys.contains(key))
                || unresolved.get(&local).is_some_and(|keys| keys.contains(key)));
            for (key, messages) in findings.entries {
                self.findings.entries.entry(key).or_default().extend(messages);
            }
        }
        for ((path, line, _, _, rule), messages) in &self.findings.entries {
            println!(
                "{rule}\t{path}\t{line}\t{}",
                messages.iter().cloned().collect::<Vec<_>>().join("; ")
            );
        }
        Compilation::Continue
    }
}

fn production(tcx: TyCtxt<'_>, owner: LocalDefId) -> bool {
    if !matches!(
        tcx.def_kind(owner),
        rustc_hir::def::DefKind::Fn
            | rustc_hir::def::DefKind::AssocFn
            | rustc_hir::def::DefKind::Closure
    ) {
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
                | "writer"
        )
    }) {
        return false;
    }
    if parts
        .windows(2)
        .any(|pair| pair[0] == "history" && matches!(pair[1], "encode" | "write"))
    {
        return false;
    }
    if parts.last().is_some_and(|name| {
        name.contains("test")
            || name.starts_with("writer")
            || matches!(*name, "zip_write.rs" | "export.rs")
    }) {
        return false;
    }
    if parts.windows(2).any(|pair| {
        pair[0] == "resolved_features"
            && matches!(
                pair[1],
                "sketch_write.rs" | "write_generate.rs" | "write_prepare.rs"
            )
    }) {
        return false;
    }
    let symbol = tcx.crate_name(rustc_span::def_id::LOCAL_CRATE);
    let crate_name = symbol.as_str();
    std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some()
        || crate_name.starts_with("cadmpeg_codec_")
        || matches!(
            crate_name,
            "cadmpeg_core"
                | "cadmpeg_ir"
                | "cadmpeg_container"
                | "cadmpeg_asm"
                | "cadmpeg_parasolid"
                | "cadmpeg_protein"
        )
}

struct Analysis<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    typing_owner: LocalDefId,
    arguments: Option<rustc_middle::ty::GenericArgsRef<'tcx>>,
    flow: flow::Flow,
    findings: &'a mut Findings,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn typing_env(&self) -> rustc_middle::ty::TypingEnv<'tcx> {
        rustc_middle::ty::TypingEnv::post_analysis(self.tcx, self.typing_owner)
    }

    fn substitute<T>(&self, value: T) -> T
    where T: rustc_middle::ty::TypeFoldable<TyCtxt<'tcx>> + Copy {
        self.arguments.map_or(value, |arguments|
            rustc_middle::ty::EarlyBinder::bind(self.tcx, value).instantiate(self.tcx, arguments).skip_norm_wip())
    }

    fn expr_ty(&self, expression: &Expr<'tcx>) -> rustc_middle::ty::Ty<'tcx> {
        let value = self.substitute(self.typeck.expr_ty(expression));
        self.tcx.try_normalize_erasing_regions(self.typing_env(),
            rustc_middle::ty::Unnormalized::new_wip(value)).unwrap_or(value)
    }

    fn expr_ty_adjusted(&self, expression: &Expr<'tcx>) -> rustc_middle::ty::Ty<'tcx> {
        let value = self.substitute(self.typeck.expr_ty_adjusted(expression));
        self.tcx.try_normalize_erasing_regions(self.typing_env(),
            rustc_middle::ty::Unnormalized::new_wip(value)).unwrap_or(value)
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
