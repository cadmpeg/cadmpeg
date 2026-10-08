// SPDX-License-Identifier: Apache-2.0
//! Calls through core's typed admission traits.
//!
//! `cadmpeg_core::decode::Admission` and `AdmissionScope` are sealed and
//! implemented only for the decode types (`DecodeContext`,
//! `ScopedReservation`) and the standard admission used outside decode. A
//! trait with `Admission` as a supertrait has the same two implementors.
//! Each decode-type method is one call to the core operation of the same
//! name, passing its parameters in order; the crate that holds that
//! implementation proves the shape. A call through the trait is then checked
//! as that core operation, instantiated with the call's own arguments, so the
//! operation's key, callback and storage proofs apply at the call. Decode
//! code that names the standard admission is reported, because its
//! operations charge nothing.

use crate::{types, Analysis};
use rustc_hir::{Expr, ExprKind};
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty::{self, GenericArgsRef, Instance, Ty, TyCtxt};
use rustc_span::def_id::{DefId, LocalDefId};

const ADMISSION: &[&str] = &["decode", "admission", "Admission"];
const SCOPE: &[&str] = &["decode", "admission", "AdmissionScope"];
const CONTEXT: &[&str] = &["decode", "context", "DecodeContext"];
const RESERVATION: &[&str] = &["decode", "budget", "ScopedReservation"];
const STANDARD: &[&[&str]] = &[
    &["decode", "admission", "StandardAdmission"],
    &["decode", "admission", "StandardScope"],
];

fn core_item(tcx: TyCtxt<'_>, definition: DefId, parts: &[&str]) -> bool {
    types::physical_item_path(tcx, definition, "cadmpeg_core", parts)
}

/// Core's admission or scope trait, or a trait with the admission trait as
/// a supertrait.
pub(crate) fn admission_trait(tcx: TyCtxt<'_>, trait_id: DefId) -> bool {
    core_item(tcx, trait_id, SCOPE)
        || rustc_type_ir::elaborate::supertrait_def_ids(tcx, trait_id)
            .any(|supertrait| core_item(tcx, supertrait, ADMISSION))
}

/// A decode type whose admission methods forward to core operations.
fn forwarding_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> Option<DefId> {
    match value.peel_refs().kind() {
        ty::Adt(owner, _)
            if core_item(tcx, owner.did(), CONTEXT) || core_item(tcx, owner.did(), RESERVATION) =>
        {
            Some(owner.did())
        }
        _ => None,
    }
}

/// The admission that charges nothing, which decode code does not use.
pub(crate) fn standard_admission(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    matches!(value.peel_refs().kind(), ty::Adt(owner, _)
        if STANDARD.iter().any(|parts| core_item(tcx, owner.did(), parts)))
}

fn forwarding_impl(tcx: TyCtxt<'_>, trait_id: DefId) -> Option<DefId> {
    tcx.all_impls(trait_id).find(|implementation| {
        forwarding_type(tcx, tcx.type_of(*implementation).skip_binder()).is_some()
    })
}

/// Whether the body is a method of an admission implementation for the
/// standard admission, which decode never runs.
pub(crate) fn standard_implementation(tcx: TyCtxt<'_>, owner: LocalDefId) -> bool {
    let Some(implementation) = tcx.opt_parent(owner.to_def_id()) else {
        return false;
    };
    if !matches!(
        tcx.def_kind(implementation),
        rustc_hir::def::DefKind::Impl { of_trait: true }
    ) {
        return false;
    }
    let header = tcx.impl_trait_header(implementation);
    admission_trait(tcx, header.trait_ref.skip_binder().def_id)
        && standard_admission(tcx, tcx.type_of(implementation).skip_binder())
}

impl<'tcx> Analysis<'_, 'tcx> {
    /// The core operation a call through an admission trait is checked as,
    /// with its generic arguments.
    pub(crate) fn admission_forward(
        &self,
        method: DefId,
        args: GenericArgsRef<'tcx>,
    ) -> Option<(DefId, GenericArgsRef<'tcx>)> {
        if args
            .types()
            .next()
            .is_some_and(|receiver| standard_admission(self.tcx, receiver))
        {
            return None;
        }
        let trait_id = self.tcx.trait_of_assoc(method)?;
        if !admission_trait(self.tcx, trait_id) {
            return None;
        }
        let implementation = forwarding_impl(self.tcx, trait_id)?;
        let impl_args = ty::GenericArgs::for_item(self.tcx, implementation, |_, _| {
            self.tcx.lifetimes.re_erased.into()
        });
        let receiver =
            ty::EarlyBinder::bind(self.tcx, self.tcx.type_of(implementation).skip_binder())
                .instantiate(self.tcx, impl_args)
                .skip_norm_wip();
        let args = self
            .tcx
            .mk_args_from_iter(std::iter::once(receiver.into()).chain(args.iter().skip(1)));
        let instance = Instance::try_resolve(self.tcx, self.typing_env(), method, args)
            .ok()
            .flatten()?;
        if !self.tcx.is_mir_available(instance.def_id()) {
            return None;
        }
        let body = self.tcx.instance_mir(instance.def);
        let mut target = None;
        for block in body.basic_blocks.iter() {
            if let TerminatorKind::Call { func, .. } = &block.terminator().kind {
                let callee = func.const_fn_def()?;
                if target.replace(callee).is_some() {
                    return None;
                }
            }
        }
        let (callee, callee_args) = target?;
        let callee_args = instance.instantiate_mir_and_normalize_erasing_regions(
            self.tcx,
            self.typing_env(),
            ty::EarlyBinder::bind(self.tcx, callee_args),
        );
        Some((callee, callee_args))
    }

    /// Reports a path to the standard admission in decode code.
    pub(crate) fn standard_admission_use(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        let standard_receiver = matches!(
            expression.kind,
            ExprKind::MethodCall(_, receiver, ..)
                if standard_admission(self.tcx, self.expr_ty(receiver))
        );
        let named = match expression.kind {
            ExprKind::Path(rustc_hir::QPath::Resolved(_, path)) => {
                !matches!(path.res, rustc_hir::def::Res::Local(_))
            }
            ExprKind::Path(_)
            | ExprKind::Struct(..)
            | ExprKind::Call(..)
            | ExprKind::MethodCall(..) => true,
            _ => false,
        };
        if !standard_receiver && !(named && standard_admission(self.tcx, self.expr_ty(expression)))
        {
            return false;
        }
        self.report(
            expression.span,
            "uncharged_decode_work",
            "the standard admission charges nothing, so decode code does not use it; replacement: pass the DecodeContext as the admission",
        );
        true
    }
}

/// Reports a decode-type admission method that is not one call to the core
/// operation of the same name with its parameters in order.
pub(crate) fn forwarding_findings(tcx: TyCtxt<'_>) -> Vec<(LocalDefId, String)> {
    let mut findings = Vec::new();
    for owner in tcx.hir_body_owners() {
        let Some(implementation) = tcx.opt_parent(owner.to_def_id()) else {
            continue;
        };
        if !matches!(
            tcx.def_kind(implementation),
            rustc_hir::def::DefKind::Impl { of_trait: true }
        ) {
            continue;
        }
        let header = tcx.impl_trait_header(implementation);
        if !admission_trait(tcx, header.trait_ref.skip_binder().def_id) {
            continue;
        }
        let Some(owner_type) = forwarding_type(tcx, tcx.type_of(implementation).skip_binder())
        else {
            continue;
        };
        if !forwards(tcx, owner, owner_type) {
            findings.push((
                owner,
                format!(
                    "admission method {} is not one call to the core operation of the same name with its parameters in order",
                    tcx.item_name(owner.to_def_id())
                ),
            ));
        }
    }
    findings
}

fn forwards(tcx: TyCtxt<'_>, owner: LocalDefId, owner_type: DefId) -> bool {
    let body = tcx.hir_body_owned_by(owner);
    let ExprKind::Block(block, _) = body.value.kind else {
        return false;
    };
    let (true, Some(tail)) = (block.stmts.is_empty(), block.expr) else {
        return false;
    };
    let ExprKind::Call(callee, arguments) = tail.kind else {
        return false;
    };
    let typeck = tcx.typeck(owner);
    let ty::FnDef(target, _) = typeck.expr_ty(callee).kind() else {
        return false;
    };
    let Some(target_impl) = tcx.opt_parent(*target) else {
        return false;
    };
    let inherent = matches!(
        tcx.def_kind(target_impl),
        rustc_hir::def::DefKind::Impl { of_trait: false }
    ) && matches!(tcx.type_of(target_impl).skip_binder().kind(), ty::Adt(adt, _) if adt.did() == owner_type);
    inherent
        && tcx.item_name(*target) == tcx.item_name(owner.to_def_id())
        && arguments.len() == body.params.len()
        && arguments
            .iter()
            .zip(body.params)
            .all(|(argument, parameter)| {
                matches!(argument.kind, ExprKind::Path(rustc_hir::QPath::Resolved(None, path))
                if matches!(path.res, rustc_hir::def::Res::Local(id) if id == parameter.pat.hir_id))
            })
}
