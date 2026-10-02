// SPDX-License-Identifier: Apache-2.0
//! Lifetime-erased callable signatures for indirect candidates.
use super::{key, EdgeKind, Graph};
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
    caller: &str,
    value: Ty<'tcx>,
    stored: Ty<'tcx>,
) {
    let target = match value.peel_refs().kind() {
        ty::FnDef(id, args) => {
            let instance = args.no_bound_vars().and_then(|args| ty::Instance::try_resolve(tcx, environment, *id, args).ok().flatten());
            match instance {
                Some(instance) if matches!(instance.def, ty::InstanceKind::Virtual(..)) => {
                    let target = format!("virtual:{}:{:?}", key(tcx, *id), instance.args);
                    graph.nodes.insert(target.clone(), format!("trait-object function address {}", tcx.def_path_str(*id)));
                    graph.edges.insert((caller.to_owned(), target.clone(), EdgeKind::FunctionAddress));
                    graph.trait_calls.insert((target.clone(), key(tcx, *id)));
                    graph.object_calls.insert((target.clone(), key(tcx, *id)));
                    target
                }
                Some(instance) if types::checked(tcx, instance.def_id()) => key(tcx, instance.def_id()),
                None if types::checked(tcx, *id) => key(tcx, *id),
                _ => return,
            }
        }
        ty::Closure(id, _) if types::checked(tcx, *id) => key(tcx, *id),
        _ => return,
    };
    for value in [value, stored] {
        if let Some(signature) = signature(tcx, environment, value) {
            graph.addresses.insert((signature, target.clone()));
        }
    }
}
