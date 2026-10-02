// SPDX-License-Identifier: Apache-2.0
#![feature(rustc_private)]
//! Type-aware decode allocation and work admission checks.

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

mod allocation;
mod types;
mod extent;

use std::collections::{BTreeSet, HashSet};
use rustc_driver::{Callbacks, Compilation};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{Body, Expr, ExprKind};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::{TyCtxt, TypeckResults};
use rustc_span::def_id::{DefId, LocalDefId};
use rustc_span::Span;

struct DecodeCallbacks {
    findings: BTreeSet<String>,
}

impl Callbacks for DecodeCallbacks {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        let mut active = HashSet::new();
        for owner in tcx.hir_body_owners() {
            let body = tcx.hir_body_owned_by(owner);
            let mut scope = ContextScope { tcx, typeck: tcx.typeck(owner), present: false };
            scope.visit_body(body);
            if scope.present {
                active.insert(owner);
            }
        }
        // Closures retain the enclosing function's context, including closures
        // whose uncharged expressions do not name that context.
        for owner in tcx.hir_body_owners() {
            if tcx.def_kind(owner) != rustc_hir::def::DefKind::Closure { continue; }
            let mut parent = tcx.parent(owner.to_def_id());
            while parent.is_local() {
                if parent.as_local().is_some_and(|id| active.contains(&id)) {
                    active.insert(owner);
                    break;
                }
                if parent.index == rustc_span::def_id::CRATE_DEF_INDEX { break; }
                parent = tcx.parent(parent);
            }
        }
        for owner in active {
            if production(tcx, owner) {
                Analysis { tcx, typeck: tcx.typeck(owner), owner, findings: &mut self.findings }
                    .visit_body(tcx.hir_body_owned_by(owner));
            }
        }
        for finding in &self.findings { println!("{finding}"); }
        Compilation::Continue
    }
}

fn production(tcx: TyCtxt<'_>, owner: LocalDefId) -> bool {
    if !matches!(tcx.def_kind(owner), rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn | rustc_hir::def::DefKind::Closure) { return false; }
    let path = tcx.sess.source_map().lookup_char_pos(tcx.def_span(owner).source_callsite().lo()).file.name.prefer_local_unconditionally().to_string();
    let parts: Vec<&str> = path.split('/').collect();
    if parts.iter().any(|part| matches!(*part, "tests" | "test_support" | "golden_tests" | "integration_tests" | "benches" | "bin" | "writer" | "encode" | "write")) { return false; }
    if parts.last().is_some_and(|name| name.contains("test") || name.starts_with("writer") || matches!(*name, "zip_write.rs" | "export.rs" | "sketch_write.rs" | "write_generate.rs" | "write_prepare.rs")) { return false; }
    let symbol = tcx.crate_name(rustc_span::def_id::LOCAL_CRATE);
    let crate_name = symbol.as_str();
    std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some() || crate_name.starts_with("cadmpeg_codec_") || matches!(crate_name, "cadmpeg_core" | "cadmpeg_ir" | "cadmpeg_container" | "cadmpeg_asm" | "cadmpeg_parasolid" | "cadmpeg_protein")
}

struct ContextScope<'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    present: bool,
}

impl<'tcx> Visitor<'tcx> for ContextScope<'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        for parameter in body.params {
            self.present |= types::has_context(self.tcx, self.typeck.pat_ty(parameter.pat), &mut Vec::new());
        }
        self.visit_expr(body.value);
    }
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        self.present |= types::has_context(self.tcx, self.typeck.expr_ty(expression), &mut Vec::new());
        walk_expr(self, expression);
    }
}

struct Analysis<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    owner: LocalDefId,
    findings: &'a mut BTreeSet<String>,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn call(&self, expression: &'tcx Expr<'tcx>) -> Option<(DefId, Vec<&'tcx Expr<'tcx>>)> {
        match expression.kind {
            ExprKind::MethodCall(_, receiver, arguments, _) => {
                let definition = self.typeck.type_dependent_def_id(expression.hir_id)?;
                let mut operands = vec![receiver];
                operands.extend(arguments);
                Some((definition, operands))
            }
            ExprKind::Call(callee, arguments) => match self.typeck.expr_ty(callee).kind() {
                rustc_middle::ty::FnDef(definition, _) => Some((*definition, arguments.iter().collect())),
                _ => None,
            },
            _ => None,
        }
    }

    fn report(&mut self, span: Span, rule: &str, message: &str) {
        let location = self.tcx.sess.source_map().lookup_char_pos(span.source_callsite().lo());
        let path = location.file.name.prefer_local_unconditionally().to_string();
        let relative = std::env::current_dir().ok().and_then(|root| std::path::Path::new(&path).strip_prefix(root).ok().map(|path| path.display().to_string())).unwrap_or(path);
        self.findings.insert(format!("{rule}\t{relative}\t{}\t{message}", location.line));
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
        self.allocation(expression);
        walk_expr(self, expression);
    }
}

/// Runs rustc and returns whether decode-contract findings were emitted.
pub fn run(arguments: &[String]) -> bool {
    let mut callbacks = DecodeCallbacks { findings: BTreeSet::new() };
    rustc_driver::run_compiler(arguments, &mut callbacks);
    !callbacks.findings.is_empty()
}

#[cfg(test)]
mod tests;
