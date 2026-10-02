// SPDX-License-Identifier: Apache-2.0
#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{Body, Expr, ExprKind};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::{Ty, TyCtxt, TypeckResults};
use rustc_span::def_id::DefId;

struct DecodeCallbacks;

impl Callbacks for DecodeCallbacks {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        for owner in tcx.hir_body_owners() {
            let body = tcx.hir_body_owned_by(owner);
            let typeck = tcx.typeck(owner);
            let mut scope = ContextScope { tcx, typeck, present: false };
            scope.visit_body(body);
            if scope.present {
                Allocation { tcx, typeck }.visit_body(body);
            }
        }
        Compilation::Continue
    }
}

struct ContextScope<'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
    present: bool,
}

impl<'tcx> Visitor<'tcx> for ContextScope<'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        for parameter in body.params {
            self.present |= context_type(self.tcx, self.typeck.pat_ty(parameter.pat));
        }
        self.visit_expr(body.value);
    }

    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        self.present |= context_type(self.tcx, self.typeck.expr_ty(expression));
        walk_expr(self, expression);
    }
}

fn context_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    match value.peel_refs().kind() {
        rustc_middle::ty::Adt(definition, _) => {
            tcx.def_path_str(definition.did()).ends_with("::DecodeContext")
        }
        _ => false,
    }
}

struct Allocation<'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
}

impl<'tcx> Allocation<'tcx> {
    fn call(&self, expression: &Expr<'tcx>) -> Option<(DefId, Vec<&'tcx Expr<'tcx>>)> {
        match expression.kind {
            ExprKind::MethodCall(_, receiver, arguments, _) => {
                let definition = self.typeck.type_dependent_def_id(expression.hir_id)?;
                let mut operands = vec![receiver];
                operands.extend(arguments);
                Some((definition, operands))
            }
            ExprKind::Call(callee, arguments) => {
                if let rustc_middle::ty::FnDef(definition, _) = self.typeck.expr_ty(callee).kind() {
                    Some((*definition, arguments.iter().collect()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn heap_type(&self, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> bool {
        if seen.contains(&value) {
            return false;
        }
        seen.push(value);
        match value.kind() {
            rustc_middle::ty::Adt(definition, arguments) => {
                let path = self.tcx.def_path_str(definition.did());
                if matches!(path.as_str(), "alloc::string::String" | "std::string::String" | "alloc::vec::Vec" | "std::vec::Vec" | "alloc::boxed::Box" | "std::boxed::Box") {
                    return true;
                }
                definition.all_fields().any(|field| self.heap_type(field.ty(self.tcx, arguments).skip_norm_wip(), seen))
            }
            rustc_middle::ty::Tuple(fields) => fields.iter().any(|field| self.heap_type(field, seen)),
            rustc_middle::ty::Array(element, _) => self.heap_type(*element, seen),
            _ => false,
        }
    }

    fn fixed_operand(&self, expression: &Expr<'tcx>) -> bool {
        match expression.kind {
            ExprKind::Lit(_) => true,
            ExprKind::AddrOf(_, _, inner) => self.fixed_operand(inner),
            _ => match self.typeck.expr_ty(expression).peel_refs().kind() {
                rustc_middle::ty::Bool | rustc_middle::ty::Char | rustc_middle::ty::Int(_)
                | rustc_middle::ty::Uint(_) | rustc_middle::ty::Float(_) => true,
                _ => false,
            },
        }
    }
}

impl<'tcx> Visitor<'tcx> for Allocation<'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if let Some((definition, operands)) = self.call(expression) {
            let symbol = self.tcx.item_name(definition);
            let name = symbol.as_str();
            if matches!(name, "to_string" | "to_owned" | "clone")
                && self.heap_type(self.typeck.expr_ty(expression), &mut Vec::new())
                && operands.first().is_some_and(|operand| !self.fixed_operand(operand))
            {
                let span = expression.span.source_callsite();
                self.tcx.dcx().struct_span_warn(
                    span,
                    format!("uncharged_decode_allocation: {name} creates input-sized owned storage; use the core charged copy or format operation"),
                ).emit();
            }
        }
        walk_expr(self, expression);
    }
}

/// Runs the pinned compiler with decode-contract diagnostics.
pub fn run() {
    let mut arguments: Vec<String> = std::env::args().collect();
    if arguments.get(1).is_some_and(|argument| argument.ends_with("rustc")) {
        arguments.remove(1);
    }
    rustc_driver::run_compiler(&arguments, &mut DecodeCallbacks);
}
