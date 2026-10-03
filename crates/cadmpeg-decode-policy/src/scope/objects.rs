// SPDX-License-Identifier: Apache-2.0
//! Concrete implementations erased by unsizing coercions.
use super::{indirect, instances, key, pattern::Pattern, EdgeKind, Graph};
use crate::types;
use rustc_span::def_id::DefId;
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeVisitableExt};

pub(super) struct Target<'tcx> {
    method: DefId,
    instance: Instance<'tcx>,
    signature: Pattern,
}

pub(super) fn targets<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    source: Ty<'tcx>,
    target: Ty<'tcx>,
    instances: &mut Vec<Target<'tcx>>,
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
                    if !matches!(tcx.def_kind(item.def_id), rustc_hir::def::DefKind::AssocFn) || tcx.generics_require_sized_self(item.def_id) {
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
                            instances.push(Target { method: item.def_id, signature: indirect::method_signature(tcx, environment, item.def_id, args), instance })
                        }
                        _ => {
                            for implementation in tcx.all_impls(trait_ref.def_id) {
                                if let Some(method) = tcx
                                    .associated_items(implementation)
                                    .in_definition_order()
                                    .find(|method| method.name() == item.name())
                                {
                                    let args = ty::GenericArgs::identity_for_item(tcx, method.def_id);
                                    instances.push(Target { method: item.def_id, instance: Instance::new_raw(method.def_id, args), signature: indirect::method_signature(tcx, environment, method.def_id, args) });
                                }
                            }
                            instances.push(Target { method: item.def_id, instance: Instance::new_raw(item.def_id, args), signature: indirect::method_signature(tcx, environment, item.def_id, args) });
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

/// Keep concrete MIR behind method selection and compatible call types.
pub(super) fn register<'tcx>(
    tcx: TyCtxt<'tcx>,
    graph: &mut Graph,
    pending: &mut Vec<instances::Concrete<'tcx>>,
    caller: &str,
    candidate: Target<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    depth: usize,
) {
    let Target { method, instance, signature } = candidate;
    if !types::checked(tcx, instance.def_id()) {
        return;
    }
    let target = format!("object:{}:{}:{:?}", key(tcx, method), key(tcx, instance.def_id()), instance.args);
    graph.nodes.insert(target.clone(), format!("trait-object instance {} {:?}", tcx.def_path_str(instance.def_id()), instance.args));
    graph.method_impls.insert((key(tcx, method), signature.clone(), target.clone()));
    if instance.args.has_non_region_param() {
        graph.symbolic_instances.insert(target.clone());
    }
    graph.objects.insert((caller.to_owned(), key(tcx, method), signature, target.clone()));
    instances::enqueue(tcx, graph, pending, &target, instance, environment, (depth, EdgeKind::TraitObjectCall));
}
