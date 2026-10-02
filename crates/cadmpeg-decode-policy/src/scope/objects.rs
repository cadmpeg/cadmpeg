// SPDX-License-Identifier: Apache-2.0
//! Concrete implementations erased by unsizing coercions.
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeVisitableExt};

pub(super) fn targets<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    source: Ty<'tcx>,
    target: Ty<'tcx>,
    instances: &mut Vec<Instance<'tcx>>,
) -> bool {
    let source = source.peel_refs();
    let target = target.peel_refs();
    if source == target {
        return true;
    }
    match (source.kind(), target.kind()) {
        (_, ty::Dynamic(predicates, _)) if !matches!(source.kind(), ty::Dynamic(..)) => {
            let Some(principal) = predicates.principal() else {
                return true;
            };
            let mut resolved = true;
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
                            resolved = false;
                            continue;
                        }
                    };
                    match Instance::try_resolve(tcx, environment, item.def_id, args) {
                        Ok(Some(instance))
                            if !matches!(instance.def, ty::InstanceKind::Virtual(..)) =>
                        {
                            instances.push(instance)
                        }
                        _ => {
                            resolved = false;
                            for implementation in tcx.all_impls(trait_ref.def_id) {
                                if let Some(method) = tcx
                                    .associated_items(implementation)
                                    .in_definition_order()
                                    .find(|method| method.name() == item.name())
                                {
                                    instances.push(Instance::new_raw(
                                        method.def_id,
                                        ty::GenericArgs::identity_for_item(tcx, method.def_id),
                                    ));
                                }
                            }
                            instances.push(Instance::new_raw(item.def_id, args));
                        }
                    }
                }
            }
            resolved && !source.has_non_region_param()
        }
        (ty::Adt(left, left_args), ty::Adt(right, right_args)) if left == right => left_args
            .types()
            .zip(right_args.types())
            .fold(true, |resolved, (left, right)| {
                targets(tcx, environment, left, right, instances) && resolved
            }),
        (ty::RawPtr(left, _), ty::RawPtr(right, _)) => {
            targets(tcx, environment, *left, *right, instances)
        }
        (ty::Tuple(left), ty::Tuple(right)) => left
            .iter()
            .zip(right.iter())
            .fold(true, |resolved, (left, right)| {
                targets(tcx, environment, left, right, instances) && resolved
            }),
        (_, ty::Dynamic(..)) => true,
        _ => true,
    }
}
