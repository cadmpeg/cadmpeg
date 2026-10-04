// SPDX-License-Identifier: Apache-2.0
//! Bounds source-side work admitted by the structural projection serializer.

use crate::structural_derived::{SerializedChild, SerializerMode};
use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::DefId;

/// Returns whether the concrete Serde implementation exposes each variable
/// child only after the structural projector admits its parent.
pub(crate) fn is_bounded_source<'tcx>(tcx: TyCtxt<'tcx>, source: Ty<'tcx>) -> bool {
    bounded_tree(tcx, source, SerializerMode::Plain, &mut Vec::new(), &mut 0)
}

fn bounded_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    mode: SerializerMode,
    seen: &mut Vec<(Ty<'tcx>, SerializerMode, usize)>,
    emitted_nodes: &mut usize,
) -> bool {
    let source = types::reveal_opaque(tcx, source).peel_refs();
    if let Some((_, _, previous)) = seen.iter().rev().find(|(active, active_mode, _)| {
        *active == source && *active_mode == mode
    }) {
        return *emitted_nodes > *previous;
    }
    if seen.len() >= tcx.recursion_limit().0 {
        return false;
    }
    let depth = seen.len();
    let emission_depth = *emitted_nodes;
    seen.push((source, mode, emission_depth));
    let bounded = match source.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Str => {
            mode != SerializerMode::Flattened
                && (mode == SerializerMode::Tagged || bump_emitted_node(emitted_nodes))
        }
        ty::Array(element, _) | ty::Slice(element) => {
            mode != SerializerMode::Flattened
                && (mode == SerializerMode::Tagged
                    || bounded_children(tcx, [*element], true, seen, emitted_nodes))
        }
        ty::Tuple(fields) => {
            mode != SerializerMode::Flattened
                && (mode == SerializerMode::Tagged
                    || bounded_children(tcx, fields.iter(), true, seen, emitted_nodes))
        }
        ty::Adt(owner, arguments) if types::standard(tcx, owner.did()) => {
            standard_source(tcx, owner.did(), arguments.types().collect(), mode, seen, emitted_nodes)
        }
        ty::Adt(owner, _) if !owner.is_union() => {
            if mode == SerializerMode::Flattened && !owner.is_struct() {
                false
            } else if let Some(json) = crate::structural_json::source_children(tcx, source) {
                mode != SerializerMode::Flattened && {
                    let number = types::physical_item_path(tcx, owner.did(), "serde_json", &["number", "Number"]);
                    let charged = json.charged_parent && !(number && mode == SerializerMode::Tagged);
                    let child_mode = if json.charged_parent { SerializerMode::Plain } else { mode };
                    bounded_serialized_children(tcx, json.children.into_iter().map(|ty| SerializedChild {
                        ty,
                        charged_parent: charged,
                        mode: child_mode,
                    }), seen, emitted_nodes)
                }
            } else {
                bounded_implementation(tcx, source, *owner, mode, seen, emitted_nodes)
            }
        }
        _ => false,
    };
    seen.truncate(depth);
    *emitted_nodes = emission_depth;
    bounded
}

fn bounded_serialized_children<'tcx>(
    tcx: TyCtxt<'tcx>,
    children: impl IntoIterator<Item = SerializedChild<'tcx>>,
    seen: &mut Vec<(Ty<'tcx>, SerializerMode, usize)>,
    emitted_nodes: &mut usize,
) -> bool {
    let parent_depth = *emitted_nodes;
    for child in children {
        *emitted_nodes = parent_depth;
        if child.charged_parent && !bump_emitted_node(emitted_nodes)
            || !bounded_tree(tcx, child.ty, child.mode, seen, emitted_nodes)
        {
            *emitted_nodes = parent_depth;
            return false;
        }
    }
    *emitted_nodes = parent_depth;
    true
}

fn bounded_children<'tcx>(
    tcx: TyCtxt<'tcx>,
    children: impl IntoIterator<Item = Ty<'tcx>>,
    charged_parent: bool,
    seen: &mut Vec<(Ty<'tcx>, SerializerMode, usize)>,
    emitted_nodes: &mut usize,
) -> bool {
    bounded_serialized_children(tcx, children.into_iter().map(|ty| SerializedChild {
        ty,
        charged_parent,
        mode: SerializerMode::Plain,
    }), seen, emitted_nodes)
}

fn bump_emitted_node(emitted_nodes: &mut usize) -> bool {
    let Some(next) = emitted_nodes.checked_add(1) else { return false; };
    *emitted_nodes = next;
    true
}

fn standard_source<'tcx>(
    tcx: TyCtxt<'tcx>,
    owner: DefId,
    arguments: Vec<Ty<'tcx>>,
    mode: SerializerMode,
    seen: &mut Vec<(Ty<'tcx>, SerializerMode, usize)>,
    emitted_nodes: &mut usize,
) -> bool {
    if types::physical_item_path(tcx, owner, "alloc", &["boxed", "Box"]) {
        let [inner, allocator] = arguments.as_slice() else { return false; };
        return matches!(allocator.kind(), ty::Adt(definition, _)
            if types::physical_item_path(tcx, definition.did(), "alloc", &["alloc", "Global"]))
            && bounded_tree(tcx, *inner, mode, seen, emitted_nodes);
    }
    if [
        ("alloc", &["rc", "Rc"][..]),
        ("alloc", &["sync", "Arc"][..]),
    ].iter().any(|(crate_name, parts)| types::physical_item_path(tcx, owner, crate_name, parts)) {
        return arguments.first().is_some_and(|inner| bounded_tree(tcx, *inner, mode, seen, emitted_nodes));
    }
    if mode == SerializerMode::Flattened {
        return false;
    }
    if types::physical_item_path(tcx, owner, "alloc", &["string", "String"]) {
        return mode == SerializerMode::Tagged || bump_emitted_node(emitted_nodes);
    }
    if types::physical_item_path(tcx, owner, "alloc", &["borrow", "Cow"]) {
        return arguments.first().is_some_and(|inner| {
            matches!(inner.kind(), ty::Str | ty::Slice(_))
                && bounded_tree(tcx, *inner, mode, seen, emitted_nodes)
        });
    }
    if types::physical_item_path(tcx, owner, "core", &["option", "Option"])
        || types::physical_item_path(tcx, owner, "alloc", &["vec", "Vec"])
        || types::physical_item_path(tcx, owner, "alloc", &["collections", "btree", "set", "BTreeSet"])
    {
        // TaggedSerializer refuses these protocols before it reads a child.
        return mode == SerializerMode::Tagged
            || arguments.first().is_some_and(|inner| bounded_children(tcx, [*inner], true, seen, emitted_nodes));
    }
    if types::physical_item_path(tcx, owner, "alloc", &["collections", "btree", "map", "BTreeMap"]) {
        return arguments.len() >= 2
            && bounded_children(tcx, arguments.into_iter().take(2), true, seen, emitted_nodes);
    }
    false
}

fn bounded_implementation<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    owner: ty::AdtDef<'tcx>,
    mode: SerializerMode,
    seen: &mut Vec<(Ty<'tcx>, SerializerMode, usize)>,
    emitted_nodes: &mut usize,
) -> bool {
    let candidates: Vec<_> = tcx.all_traits_including_private()
        .filter(|trait_id| serde_serialize_trait(tcx, *trait_id))
        .flat_map(|trait_id| tcx.non_blanket_impls_for_ty(trait_id, source))
        .filter(|implementation| matches!(tcx.type_of(*implementation)
            .instantiate_identity().skip_norm_wip().kind(),
            ty::Adt(definition, _) if definition.did() == owner.did()))
        .collect();
    let [implementation] = candidates.as_slice() else { return false; };
    let Some(method) = tcx.associated_items(*implementation).in_definition_order()
        .find(|item| item.name().as_str() == "serialize").map(|item| item.def_id)
    else { return false; };
    let derived = tcx.is_automatically_derived(*implementation)
        && (!implementation.is_local() || is_serde_derived_impl(tcx, *implementation, method));
    if derived {
        if let Some(children) = crate::structural_derived::derived_children(tcx, source, method, mode) {
            return bounded_serialized_children(tcx, children, seen, emitted_nodes);
        }
        if let Some(wire) = crate::structural_fixed::derived_into_source(tcx, source, method) {
            return bounded_tree(tcx, wire, mode, seen, emitted_nodes);
        }
        return false;
    }
    if crate::structural_scalar::is_bounded_impl(tcx, method) {
        return mode != SerializerMode::Flattened
            && (mode == SerializerMode::Tagged || bump_emitted_node(emitted_nodes));
    }
    if let Some(fields) = crate::structural_record::record_field_types(tcx, source, method) {
        return bounded_children(tcx, fields, mode != SerializerMode::Flattened, seen, emitted_nodes);
    }
    if mode != SerializerMode::Flattened {
        if let Some(fields) = crate::structural_map::declared_map_field_types(tcx, source, method) {
            return bounded_children(tcx, fields, true, seen, emitted_nodes);
        }
    }
    crate::structural_wire::delegated_source(tcx, source, method)
        .is_some_and(|wire| bounded_tree(tcx, wire, mode, seen, emitted_nodes))
}

fn serde_serialize_trait(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    ["serde", "serde_core"].iter().any(|crate_name| {
        types::physical_item_path(tcx, definition, crate_name, &["ser", "Serialize"])
    })
}

fn is_serde_derived_impl(tcx: TyCtxt<'_>, implementation: DefId, method: DefId) -> bool {
    tcx.is_automatically_derived(implementation)
        && tcx.def_span(method).macro_backtrace().any(|expansion| {
            matches!(expansion.kind, rustc_span::hygiene::ExpnKind::Macro(
                rustc_span::hygiene::MacroKind::Derive, _
            )) && expansion.macro_def_id.is_some_and(|definition| {
                types::physical_item_path(tcx, definition, "serde_derive", &["Serialize"])
            })
        })
}
