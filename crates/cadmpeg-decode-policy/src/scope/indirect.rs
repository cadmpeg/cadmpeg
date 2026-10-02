// SPDX-License-Identifier: Apache-2.0
//! Lifetime-erased callable signatures for indirect candidates.
use super::{key, Graph};
use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt};

pub(super) fn signature<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    value: Ty<'tcx>,
) -> Option<String> {
    let value = value.peel_refs();
    let signature = match value.kind() {
        ty::FnDef(..) | ty::FnPtr(..) => value.fn_sig(tcx),
        ty::Closure(_, args) => args.as_closure().sig(),
        _ => return None,
    };
    let signature = tcx.instantiate_bound_regions_with_erased(signature);
    let signature = tcx.try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(signature)).unwrap_or(signature);
    let signature = if matches!(value.kind(), ty::Closure(..)) {
        let argument = signature.inputs().first()?;
        let ty::Tuple(inputs) = argument.kind() else {
            return None;
        };
        tcx.mk_fn_sig_safe_rust_abi(inputs.iter(), signature.output())
    } else {
        signature
    };
    Some(ty::print::with_crate_prefix!(ty::print::with_no_trimmed_paths!(format!("{:?}", tcx.erase_and_anonymize_regions(signature)))))
}

pub(super) fn address<'tcx>(
    tcx: TyCtxt<'tcx>,
    graph: &mut Graph,
    environment: ty::TypingEnv<'tcx>,
    value: Ty<'tcx>,
) {
    let id = match value.peel_refs().kind() {
        ty::FnDef(id, _) | ty::Closure(id, _) => *id,
        _ => return,
    };
    if types::checked(tcx, id) {
        if let Some(signature) = signature(tcx, environment, value) {
            graph.addresses.insert((signature, key(tcx, id)));
        }
    }
}
