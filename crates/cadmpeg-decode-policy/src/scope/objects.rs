// SPDX-License-Identifier: Apache-2.0
//! Concrete implementations erased by unsizing coercions.
use super::{instances, key, Graph};
use crate::types;
use rustc_span::def_id::DefId;
use rustc_middle::ty::{self, Instance, Ty, TyCtxt};

pub(super) fn targets<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    source: Ty<'tcx>,
    target: Ty<'tcx>,
    instances: &mut Vec<(DefId, Instance<'tcx>)>,
) {
    let source = source.peel_refs();
    let target = target.peel_refs();
    if source == target {
        return;
    }
    match (source.kind(), target.kind()) {
        (_, ty::Dynamic(predicates, _)) if !matches!(source.kind(), ty::Dynamic(..)) => {
            let Some(principal) = predicates.principal() else {
                return;
            };
            for bound in
                rustc_type_ir::elaborate::supertraits(tcx, principal.with_self_ty(tcx, source))
            {
                let trait_ref = tcx.instantiate_bound_regions_with_erased(bound);
                for item in tcx.associated_items(trait_ref.def_id).in_definition_order() {
                    if !matches!(tcx.def_kind(item.def_id), rustc_hir::def::DefKind::AssocFn) {
                        continue;
                    }
                    let args = ty::GenericArgs::for_item(tcx, item.def_id, |parameter, _| {
                        usize::try_from(parameter.index)
                            .ok()
                            .and_then(|index| trait_ref.args.get(index))
                            .copied()
                            .unwrap_or_else(|| tcx.mk_param_from_def(parameter))
                    });
                    let args = match tcx
                        .try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(args))
                    {
                        Ok(args) => args,
                        Err(_) => {
                            continue;
                        }
                    };
                    match Instance::try_resolve(tcx, environment, item.def_id, args) {
                        Ok(Some(instance))
                            if !matches!(instance.def, ty::InstanceKind::Virtual(..)) =>
                        {
                            instances.push((item.def_id, instance))
                        }
                        _ => {
                            for implementation in tcx.all_impls(trait_ref.def_id) {
                                if let Some(method) = tcx
                                    .associated_items(implementation)
                                    .in_definition_order()
                                    .find(|method| method.name() == item.name())
                                {
                                    instances.push((item.def_id, Instance::new_raw(
                                        method.def_id,
                                        ty::GenericArgs::identity_for_item(tcx, method.def_id),
                                    )));
                                }
                            }
                            instances.push((item.def_id, Instance::new_raw(item.def_id, args)));
                        }
                    }
                }
            }
        }
        (ty::Adt(left, left_args), ty::Adt(right, right_args)) if left == right => {
            for (left, right) in left_args.types().zip(right_args.types()) {
                targets(tcx, environment, left, right, instances);
            }
        }
        (ty::RawPtr(left, _), ty::RawPtr(right, _)) => {
            targets(tcx, environment, *left, *right, instances);
        }
        (ty::Tuple(left), ty::Tuple(right)) => {
            for (left, right) in left.iter().zip(right.iter()) {
                targets(tcx, environment, left, right, instances);
            }
        }
        _ => (),
    }
}

/// Keep concrete MIR behind both the coercion and the called trait method.
pub(super) fn register<'tcx>(
    tcx: TyCtxt<'tcx>,
    graph: &mut Graph,
    pending: &mut Vec<instances::Concrete<'tcx>>,
    method: DefId,
    concrete: instances::Concrete<'tcx>,
) {
    if !types::checked(tcx, concrete.instance.def_id()) {
        return;
    }
    let target = format!("object:{}:{}:{}:{:?}", concrete.caller, key(tcx, method), key(tcx, concrete.instance.def_id()), concrete.instance.args);
    graph.objects.insert((concrete.caller, key(tcx, method), target.clone()));
    instances::enqueue(tcx, graph, pending, &target, concrete.instance, concrete.environment, concrete.depth);
}
