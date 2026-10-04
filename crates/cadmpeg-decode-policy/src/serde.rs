// SPDX-License-Identifier: Apache-2.0
//! Deserialization admission is checked at its decode caller.
use crate::{types, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::DefId;

fn custom_deserializer(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    tcx.get_attrs_by_path(definition, &[rustc_span::Symbol::intern("serde")])
        .any(|attribute| {
            attribute.meta_item_list().is_some_and(|items| {
                items.iter().any(|item| {
                    let rustc_ast::ast::MetaItemInner::MetaItem(item) = item else {
                        return false;
                    };
                    item.path.segments.last().is_some_and(|segment| {
                        matches!(
                            segment.ident.name.as_str(),
                            "try_from"
                                | "from"
                                | "with"
                                | "deserialize_with"
                                | "default"
                                | "skip"
                                | "skip_deserializing"
                                | "flatten"
                                | "untagged"
                                | "remote"
                        )
                    })
                })
            })
        })
}

fn is_serde_derived_deserialize_impl(
    tcx: TyCtxt<'_>,
    implementation: DefId,
) -> bool {
    tcx.is_automatically_derived(implementation)
        && tcx.associated_items(implementation).in_definition_order()
            .find(|item| item.name().as_str() == "deserialize")
            .is_some_and(|method| tcx.def_span(method.def_id).macro_backtrace().any(|expansion| {
                matches!(expansion.kind,
                    rustc_span::hygiene::ExpnKind::Macro(
                        rustc_span::hygiene::MacroKind::Derive, _
                    ))
                    && expansion.macro_def_id.is_some_and(|macro_definition| {
                        types::physical_item_path(
                            tcx, macro_definition, "serde_derive", &["Deserialize"],
                        )
                    })
            }))
}

fn standard_map_key(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) => true,
        ty::Adt(owner, _) => {
            types::physical_item_path(tcx, owner.did(), "alloc", &["string", "String"])
        }
        _ => false,
    }
}

fn standard_hash_map_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    arguments: ty::GenericArgsRef<'tcx>,
    seen: &mut Vec<Ty<'tcx>>,
) -> bool {
    let arguments: Vec<_> = arguments.types().collect();
    arguments.len() >= 3
        && standard_map_key(tcx, arguments[0])
        && derived_tree(tcx, arguments[1], seen)
        && matches!(
            arguments[2].kind(),
            ty::Adt(owner, _) if types::physical_item_path(tcx, owner.did(), "std", &["hash", "random", "RandomState"])
        )
        && standard_global_allocator_tail(tcx, &arguments[3..])
}

fn standard_hash_set_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    arguments: ty::GenericArgsRef<'tcx>,
) -> bool {
    let arguments: Vec<_> = arguments.types().collect();
    arguments.len() >= 2
        && standard_map_key(tcx, arguments[0])
        && matches!(
            arguments[1].kind(),
            ty::Adt(owner, _) if types::physical_item_path(tcx, owner.did(), "std", &["hash", "random", "RandomState"])
        )
        && standard_global_allocator_tail(tcx, &arguments[2..])
}

fn standard_btree_map_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    arguments: ty::GenericArgsRef<'tcx>,
    seen: &mut Vec<Ty<'tcx>>,
) -> bool {
    let arguments: Vec<_> = arguments.types().collect();
    arguments.len() >= 2
        && standard_map_key(tcx, arguments[0])
        && derived_tree(tcx, arguments[1], seen)
        && standard_global_allocator_tail(tcx, &arguments[2..])
}

fn standard_btree_set_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    arguments: ty::GenericArgsRef<'tcx>,
) -> bool {
    let arguments: Vec<_> = arguments.types().collect();
    !arguments.is_empty()
        && standard_map_key(tcx, arguments[0])
        && standard_global_allocator_tail(tcx, &arguments[1..])
}

fn standard_global_allocator_tail(tcx: TyCtxt<'_>, arguments: &[Ty<'_>]) -> bool {
    arguments.len() <= 1
        && arguments.iter().all(|argument| {
            matches!(
                argument.kind(),
                ty::Adt(owner, _) if types::physical_item_path(tcx, owner.did(), "alloc", &["alloc", "Global"])
            )
        })
}

fn derived_tree<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> bool {
    let value = value.peel_refs();
    if seen.contains(&value) {
        return true;
    }
    if seen.len() >= tcx.recursion_limit().0 {
        return false;
    }
    let depth = seen.len();
    seen.push(value);
    let admitted = match value.kind() {
        ty::Adt(owner, args) if types::standard(tcx, owner.did()) => {
            match owner.did() {
                _ if types::physical_item_path(tcx, owner.did(), "std", &["collections", "hash", "map", "HashMap"]) => {
                    standard_hash_map_tree(tcx, args, seen)
                }
                _ if types::physical_item_path(tcx, owner.did(), "std", &["collections", "hash", "set", "HashSet"]) => {
                    standard_hash_set_tree(tcx, args)
                }
                _ if types::physical_item_path(tcx, owner.did(), "alloc", &["collections", "btree", "map", "BTreeMap"]) => {
                    standard_btree_map_tree(tcx, args, seen)
                }
                _ if types::physical_item_path(tcx, owner.did(), "alloc", &["collections", "btree", "set", "BTreeSet"]) => {
                    standard_btree_set_tree(tcx, args)
                }
                _ => args.types().all(|inner| derived_tree(tcx, inner, seen)),
            }
        }
        ty::Adt(owner, args) => {
            !custom_deserializer(tcx, owner.did())
                && tcx
                    .all_traits_including_private()
                    .filter(|id| types::serde_deserialize(tcx, *id))
                    .flat_map(|id| tcx.non_blanket_impls_for_ty(id, value))
                    .filter(|id| {
                        matches!(tcx.type_of(*id).instantiate_identity().skip_norm_wip().kind(),
                        ty::Adt(definition, _) if definition.did() == owner.did())
                    })
                    .any(|id| is_serde_derived_deserialize_impl(tcx, id))
                && owner.all_fields().all(|field| {
                    !custom_deserializer(tcx, field.did)
                        && derived_tree(tcx, field.ty(tcx, args).skip_norm_wip(), seen)
                })
                && owner
                    .variants()
                    .iter()
                    .all(|variant| !custom_deserializer(tcx, variant.def_id))
        }
        ty::Tuple(fields) => fields.iter().all(|inner| derived_tree(tcx, inner, seen)),
        ty::Array(inner, _) | ty::Slice(inner) => derived_tree(tcx, *inner, seen),
        ty::Param(_) | ty::Alias(..) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => false,
        _ => true,
    };
    seen.truncate(depth);
    admitted
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn deserialize_call(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, _)) = self.call(expression) else {
            return false;
        };
        let name = self.tcx.item_name(definition);
        let owner = self.tcx.crate_name(definition.krate);
        let typed = types::decode_context_method(self.tcx, definition, "parse_json");
        let raw = self
            .tcx
            .trait_of_assoc(definition)
            .is_some_and(|id| types::serde_deserialize(self.tcx, id))
            || owner.as_str() == "serde_json"
                && matches!(
                    name.as_str(),
                    "from_str" | "from_slice" | "from_reader" | "from_value" | "to_writer"
                )
                && !self.tcx.def_path_str(definition).contains("Deserializer");
        if !typed && !raw {
            return false;
        }
        if typed
            && self
                .call_arguments(expression)
                .and_then(|args| args.types().next())
                .is_some_and(|value| {
                    derived_tree(self.tcx, value, &mut Vec::new())
                        && crate::serde_bounds::safe_target(self.tcx, value)
                })
        {
            return false;
        }
        self.report(expression.span, "unproven_decode_charge",
            "deserialization requires DecodeContext::parse_json for a derived type tree or DecodeContext::parse_json_value for a value tree; custom Deserialize callbacks have no proven charge");
        true
    }
}
