// SPDX-License-Identifier: Apache-2.0
//! Lifetime-erased callable signatures for indirect candidates.
use super::{key, pattern, pattern::Pattern, EdgeKind, Graph};
use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;

pub(super) fn signature<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    value: Ty<'tcx>,
) -> Option<Pattern> {
    let value = value.peel_refs();
    let signature = match value.kind() {
        ty::FnDef(..) | ty::FnPtr(..) => value.fn_sig(tcx),
        ty::Closure(_, args) => args.as_closure().sig(),
        _ => return None,
    };
    let signature = tcx.instantiate_bound_regions_with_erased(signature);
    let signature = tcx
        .try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(signature))
        .unwrap_or(signature);
    let signature = if matches!(value.kind(), ty::Closure(..)) {
        let argument = signature.inputs().first()?;
        let ty::Tuple(inputs) = argument.kind() else {
            return None;
        };
        tcx.mk_fn_sig_safe_rust_abi(inputs.iter(), signature.output())
    } else {
        signature
    };
    Some(pattern::function(
        tcx,
        tcx.erase_and_anonymize_regions(signature),
        0,
    ))
}

pub(super) fn method_signature<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    method: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
) -> Pattern {
    let signature = tcx
        .fn_sig(method)
        .instantiate(tcx, arguments)
        .skip_norm_wip();
    let signature = tcx.instantiate_bound_regions_with_erased(signature);
    let signature = tcx
        .try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(signature))
        .unwrap_or(signature);
    let receiver = tcx
        .opt_associated_item(method)
        .is_some_and(|item| item.is_method());
    let parent_count = tcx.generics_of(method).parent_count;
    let trait_args = if tcx.trait_of_assoc(method).is_some() {
        tcx.mk_args(&arguments[..parent_count])
    } else {
        tcx.impl_trait_ref(tcx.parent(method))
            .instantiate(tcx, arguments)
            .skip_norm_wip()
            .args
    };
    let mut children = vec![pattern::function(
        tcx,
        tcx.erase_and_anonymize_regions(signature),
        usize::from(receiver),
    )];
    let trait_args = tcx
        .try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(trait_args))
        .unwrap_or(trait_args);
    // Self is selected by the implementation; the remaining trait arguments constrain dispatch.
    children.extend(pattern::arguments(tcx, tcx.mk_args(&trait_args[1..])));
    children.extend(pattern::arguments(
        tcx,
        tcx.mk_args(&arguments[parent_count..]),
    ));
    Pattern::Rigid("method".into(), children)
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
            let instance = args.no_bound_vars().and_then(|args| {
                ty::Instance::try_resolve(tcx, environment, *id, args)
                    .ok()
                    .flatten()
            });
            match instance {
                Some(instance) if matches!(instance.def, ty::InstanceKind::Virtual(..)) => {
                    let target = format!("virtual:{}:{:?}", key(tcx, *id), instance.args);
                    graph.nodes.insert(
                        target.clone(),
                        format!("trait-object function address {}", tcx.def_path_str(*id)),
                    );
                    graph.edges.insert((
                        caller.to_owned(),
                        target.clone(),
                        EdgeKind::FunctionAddress,
                    ));
                    let signature = method_signature(tcx, environment, *id, instance.args);
                    graph
                        .trait_calls
                        .insert((target.clone(), key(tcx, *id), signature.clone()));
                    graph
                        .object_calls
                        .insert((target.clone(), key(tcx, *id), signature));
                    target
                }
                Some(instance) if types::checked(tcx, instance.def_id()) => {
                    key(tcx, instance.def_id())
                }
                None if types::checked(tcx, *id) => key(tcx, *id),
                _ => return,
            }
        }
        ty::Closure(id, args)
            if types::checked(tcx, *id) && args.as_closure().upvar_tys().is_empty() =>
        {
            key(tcx, *id)
        }
        _ => return,
    };
    if value.has_non_region_param() {
        graph.symbolic_candidates.insert(target.clone());
    }
    for value in [value, stored] {
        if let Some(signature) = signature(tcx, environment, value) {
            graph.addresses.insert((signature, target.clone()));
        }
    }
}
