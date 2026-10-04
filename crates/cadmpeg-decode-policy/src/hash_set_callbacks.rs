// SPDX-License-Identifier: Apache-2.0
//! Proves the callbacks used by concrete hash collection lookups.

use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::DefId;

#[derive(Clone, Copy, Eq, PartialEq)]
enum CollectionKind {
    Map,
    Set,
}

/// Identifies the exact DecodeContext methods whose key receipts can cover a
/// hash collection lookup.
pub(crate) fn is_hash_lookup_context(tcx: TyCtxt<'_>, context: DefId) -> bool {
    context_lookup_route(tcx, context).is_some()
}

/// Returns `Some(true)` only when this DecodeContext route reaches its expected
/// physical std lookup and its concrete key, query, Borrow, and builder paths
/// are bounded. `Some(false)` means the wrapper has the expected lookup name,
/// but a physical identity or callback proof failed. Other calls in the
/// context method return `None`.
pub(crate) fn bounded_context_lookup<'tcx>(
    tcx: TyCtxt<'tcx>,
    context: DefId,
    callee: DefId,
    receiver: Ty<'tcx>,
    query: Ty<'tcx>,
) -> Option<bool> {
    let (expected_kind, expected_method) = context_lookup_route(tcx, context)?;
    if tcx
        .opt_item_name(callee)
        .is_none_or(|name| name.as_str() != expected_method)
    {
        return None;
    }
    let Some(actual_kind) = raw_lookup_route(tcx, callee) else {
        return Some(false);
    };
    if actual_kind != expected_kind {
        return Some(false);
    }
    Some(bounded_lookup(tcx, actual_kind, receiver, query))
}

/// Identifies a physical std HashMap/HashSet lookup and proves its concrete
/// callbacks. This helper also guards direct lookups outside DecodeContext.
pub(crate) fn bounded_raw_lookup<'tcx>(
    tcx: TyCtxt<'tcx>,
    callee: DefId,
    receiver: Ty<'tcx>,
    query: Ty<'tcx>,
) -> Option<bool> {
    let kind = raw_lookup_route(tcx, callee)?;
    Some(bounded_lookup(tcx, kind, receiver, query))
}

pub(crate) fn is_raw_hash_lookup(tcx: TyCtxt<'_>, callee: DefId) -> bool {
    raw_lookup_route(tcx, callee).is_some()
}

fn context_lookup_route(tcx: TyCtxt<'_>, context: DefId) -> Option<(CollectionKind, &'static str)> {
    [
        ("get_hash_map", CollectionKind::Map, "get"),
        ("get_mut_hash_map", CollectionKind::Map, "get_mut"),
        ("contains_key_hash_map", CollectionKind::Map, "contains_key"),
        ("remove_hash_map", CollectionKind::Map, "remove"),
        (
            "get_key_value_hash_map",
            CollectionKind::Map,
            "get_key_value",
        ),
        ("remove_entry_hash_map", CollectionKind::Map, "remove_entry"),
        ("contains_hash_set", CollectionKind::Set, "contains"),
        ("get_hash_set", CollectionKind::Set, "get"),
        ("remove_hash_set", CollectionKind::Set, "remove"),
        // Set equality uses the same concrete HashSet lookup after its own
        // source and collision work has been admitted.
        ("equal_hash_set", CollectionKind::Set, "contains"),
    ]
    .into_iter()
    .find_map(|(method, kind, lookup)| {
        types::decode_context_method(tcx, context, method).then_some((kind, lookup))
    })
}

fn raw_lookup_route(tcx: TyCtxt<'_>, callee: DefId) -> Option<CollectionKind> {
    for method in [
        "get",
        "get_mut",
        "contains_key",
        "remove",
        "get_key_value",
        "remove_entry",
    ] {
        if types::physical_inherent_method(
            tcx,
            callee,
            "std",
            &["collections", "hash", "map", "HashMap"],
            method,
        ) {
            return Some(CollectionKind::Map);
        }
    }
    for method in ["contains", "get", "remove"] {
        if types::physical_inherent_method(
            tcx,
            callee,
            "std",
            &["collections", "hash", "set", "HashSet"],
            method,
        ) {
            return Some(CollectionKind::Set);
        }
    }
    None
}

fn bounded_lookup<'tcx>(
    tcx: TyCtxt<'tcx>,
    kind: CollectionKind,
    receiver: Ty<'tcx>,
    query: Ty<'tcx>,
) -> bool {
    let receiver = types::reveal_opaque(tcx, receiver).peel_refs();
    let ty::Adt(owner, arguments) = receiver.kind() else {
        return false;
    };
    let arguments: Vec<_> = arguments.types().collect();
    let (collection, key, builder, allocator_tail) = match kind {
        CollectionKind::Map => {
            let Some(key) = arguments.first() else {
                return false;
            };
            let Some(builder) = arguments.get(2) else {
                return false;
            };
            (
                &["collections", "hash", "map", "HashMap"][..],
                *key,
                *builder,
                &arguments[3..],
            )
        }
        CollectionKind::Set => {
            let Some(key) = arguments.first() else {
                return false;
            };
            let Some(builder) = arguments.get(1) else {
                return false;
            };
            (
                &["collections", "hash", "set", "HashSet"][..],
                *key,
                *builder,
                &arguments[2..],
            )
        }
    };
    if !types::physical_item_path(tcx, owner.did(), "std", collection)
        || !standard_global_allocator_tail(tcx, allocator_tail)
    {
        return false;
    }

    let query = types::reveal_opaque(tcx, query);
    let ty::Ref(_, query, _) = query.kind() else {
        return false;
    };
    let query = types::reveal_opaque(tcx, *query);
    bounded_key_tree(tcx, key, &mut Vec::new())
        && bounded_key_tree(tcx, query, &mut Vec::new())
        && supported_borrow_pair(tcx, key, query)
        && standard_builder(tcx, builder)
}

fn supported_borrow_pair<'tcx>(tcx: TyCtxt<'tcx>, stored: Ty<'tcx>, query: Ty<'tcx>) -> bool {
    let stored = types::reveal_opaque(tcx, stored);
    let query = types::reveal_opaque(tcx, query);
    if tcx.erase_and_anonymize_regions(stored) == tcx.erase_and_anonymize_regions(query) {
        // The physical core blanket `Borrow<T> for T` is the only coherent
        // identity implementation, including for derived local key types.
        return true;
    }
    if let ty::Ref(_, borrowed, _) = stored.kind() {
        if tcx.erase_and_anonymize_regions(*borrowed) == tcx.erase_and_anonymize_regions(query) {
            // The physical core `Borrow<T> for &T` and `Borrow<T> for &mut T`
            // implementations return the existing referent without dispatch.
            return true;
        }
    }
    if matches!(stored.kind(), ty::Adt(owner, _)
        if types::physical_item_path(tcx, owner.did(), "alloc", &["string", "String"]))
        && matches!(query.kind(), ty::Str)
    {
        // alloc's concrete Borrow<str> for String returns the stored UTF-8
        // view without dispatching to a user implementation.
        return true;
    }
    if let ty::Adt(owner, arguments) = stored.kind() {
        if types::physical_item_path(tcx, owner.did(), "alloc", &["borrow", "Cow"])
            && arguments
                .types()
                .next()
                .is_some_and(|inner| matches!(inner.kind(), ty::Str))
            && matches!(query.kind(), ty::Str)
        {
            // Cow<str> owns exactly String; the pinned standard Borrow and
            // deref chain cannot reach a custom ToOwned::Owned callback.
            return true;
        }
    }
    false
}

/// Returns whether equality on this concrete value has a bounded field tree.
///
/// This proves only the `PartialEq` callback. The caller must separately prove
/// that the two `DecodeCost` receipts cover the compared operands.
pub(crate) fn bounded_equality<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> bool {
    bounded_equality_tree(tcx, value, &mut Vec::new())
}

fn bounded_equality_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    seen: &mut Vec<Ty<'tcx>>,
) -> bool {
    let value = types::reveal_opaque(tcx, value).peel_refs();
    if seen.contains(&value) || seen.len() >= tcx.recursion_limit().0 {
        return false;
    }

    let depth = seen.len();
    seen.push(value);
    let bounded = match value.kind() {
        ty::Bool | ty::Char | ty::Float(_) | ty::Int(_) | ty::Uint(_) | ty::Str => true,
        ty::Array(element, _) => bounded_equality_tree(tcx, *element, seen),
        ty::Tuple(fields) => fields
            .iter()
            .all(|field| bounded_equality_tree(tcx, field, seen)),
        ty::Adt(owner, arguments) if types::standard(tcx, owner.did()) => {
            standard_equality_tree(tcx, owner.did(), arguments.types().collect(), seen)
        }
        ty::Adt(owner, arguments) => {
            has_bounded_callback_impl(tcx, value, owner.did(), "core::cmp::PartialEq", "PartialEq")
                && owner.all_fields().all(|field| {
                    bounded_equality_tree(tcx, field.ty(tcx, arguments).skip_norm_wip(), seen)
                })
        }
        ty::Param(_) | ty::Alias(..) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => false,
        _ => false,
    };
    seen.truncate(depth);
    bounded
}

fn standard_equality_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    owner: DefId,
    arguments: Vec<Ty<'tcx>>,
    seen: &mut Vec<Ty<'tcx>>,
) -> bool {
    if types::physical_item_path(tcx, owner, "alloc", &["string", "String"]) {
        return true;
    }
    if types::physical_item_path(tcx, owner, "alloc", &["borrow", "Cow"]) {
        return matches!(arguments.as_slice(), [inner] if matches!(inner.kind(), ty::Str));
    }
    if types::physical_item_path(tcx, owner, "alloc", &["boxed", "Box"])
        || types::physical_item_path(tcx, owner, "core", &["option", "Option"])
    {
        return arguments
            .first()
            .is_some_and(|inner| bounded_equality_tree(tcx, *inner, seen));
    }
    false
}

fn bounded_key_tree<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> bool {
    let value = types::reveal_opaque(tcx, value).peel_refs();
    if seen.contains(&value) || seen.len() >= tcx.recursion_limit().0 {
        return false;
    }

    let depth = seen.len();
    seen.push(value);
    let bounded = match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Str => true,
        ty::Array(element, _) => bounded_key_tree(tcx, *element, seen),
        ty::Tuple(fields) => fields
            .iter()
            .all(|field| bounded_key_tree(tcx, field, seen)),
        ty::Adt(owner, arguments) if types::standard(tcx, owner.did()) => {
            standard_key_tree(tcx, owner.did(), arguments.types().collect(), seen)
        }
        ty::Adt(owner, arguments) => {
            has_bounded_callback_impl(tcx, value, owner.did(), "core::hash::Hash", "Hash")
                && has_bounded_callback_impl(
                    tcx,
                    value,
                    owner.did(),
                    "core::cmp::PartialEq",
                    "PartialEq",
                )
                && has_eq_marker_impl(tcx, value, owner.did())
                && owner.all_fields().all(|field| {
                    bounded_key_tree(tcx, field.ty(tcx, arguments).skip_norm_wip(), seen)
                })
        }
        ty::Param(_) | ty::Alias(..) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => false,
        _ => false,
    };
    seen.truncate(depth);
    bounded
}

fn standard_key_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    owner: DefId,
    arguments: Vec<Ty<'tcx>>,
    seen: &mut Vec<Ty<'tcx>>,
) -> bool {
    // These pinned library implementations hash and compare the string bytes
    // directly. They do not dispatch to a user callback.
    if types::physical_item_path(tcx, owner, "alloc", &["string", "String"]) {
        return true;
    }
    if types::physical_item_path(tcx, owner, "alloc", &["borrow", "Cow"]) {
        return matches!(
            arguments.as_slice(),
            [inner] if matches!(inner.kind(), ty::Str)
        );
    }
    // These standard wrappers delegate to the contained value once.
    if types::physical_item_path(tcx, owner, "alloc", &["boxed", "Box"])
        || types::physical_item_path(tcx, owner, "core", &["option", "Option"])
    {
        return arguments
            .first()
            .is_some_and(|inner| bounded_key_tree(tcx, *inner, seen));
    }
    false
}

fn has_bounded_callback_impl<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    owner: DefId,
    trait_path: &str,
    derive_name: &str,
) -> bool {
    let Some((crate_name, parts)) = (match trait_path {
        "core::hash::Hash" => Some(("core", &["hash", "Hash"][..])),
        "core::cmp::PartialEq" => Some(("core", &["cmp", "PartialEq"][..])),
        _ => None,
    }) else {
        return false;
    };
    let Some(trait_id) = tcx
        .all_traits_including_private()
        .find(|trait_id| types::physical_item_path(tcx, *trait_id, crate_name, parts))
    else {
        return false;
    };
    // HashSet compares keys with PartialEq<Self>; other Rhs implementations
    // on the same owner are not part of this callback.
    let candidates = trait_impls_for_owner(tcx, trait_id, value, owner)
        .into_iter()
        .filter(|implementation| {
            trait_path != "core::cmp::PartialEq"
                || partial_eq_self_impl(tcx, *implementation, value)
        })
        .collect::<Vec<_>>();
    !candidates.is_empty()
        && candidates.into_iter().all(|implementation| {
            builtin_derive_origin(tcx, implementation, derive_name)
                || crate::external_callbacks::bounded_imported_derive(
                    tcx,
                    value,
                    implementation,
                    derive_name,
                )
        })
}

fn partial_eq_self_impl<'tcx>(tcx: TyCtxt<'tcx>, implementation: DefId, value: Ty<'tcx>) -> bool {
    let ty::Adt(_, owner_arguments) = value.kind() else {
        return false;
    };
    if tcx.generics_of(implementation).count() != owner_arguments.len() {
        return false;
    }
    let Some(trait_ref) = tcx.impl_opt_trait_ref(implementation) else {
        return false;
    };
    let trait_ref = trait_ref.instantiate(tcx, owner_arguments).skip_norm_wip();
    trait_ref.self_ty() == value
        && trait_ref
            .args
            .types()
            .nth(1)
            .is_some_and(|right| types::reveal_opaque(tcx, right) == value)
}

fn has_eq_marker_impl<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, owner: DefId) -> bool {
    let Some(trait_id) = tcx
        .all_traits_including_private()
        .find(|trait_id| types::physical_item_path(tcx, *trait_id, "core", &["cmp", "Eq"]))
    else {
        return false;
    };
    !trait_impls_for_owner(tcx, trait_id, value, owner).is_empty()
}

fn trait_impls_for_owner<'tcx>(
    tcx: TyCtxt<'tcx>,
    trait_id: DefId,
    value: Ty<'tcx>,
    owner: DefId,
) -> Vec<DefId> {
    tcx.non_blanket_impls_for_ty(trait_id, value)
        .filter(|implementation| {
            matches!(
                tcx.type_of(*implementation)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .kind(),
                ty::Adt(definition, _) if definition.did() == owner
            )
        })
        .collect()
}

fn builtin_derive_origin(tcx: TyCtxt<'_>, implementation: DefId, derive_name: &str) -> bool {
    if !tcx.is_automatically_derived(implementation) {
        return false;
    }
    let method = match derive_name {
        "Hash" => "hash",
        "PartialEq" => "eq",
        _ => return false,
    };
    let mut spans: Vec<_> = tcx
        .associated_items(implementation)
        .in_definition_order()
        .filter(|item| item.name().as_str() == method)
        .map(|item| tcx.def_span(item.def_id))
        .collect();
    spans.push(tcx.def_span(implementation));
    spans.into_iter().any(|span| {
        span.macro_backtrace().any(|expansion| {
            matches!(
                expansion.kind,
                rustc_span::hygiene::ExpnKind::Macro(
                    rustc_span::hygiene::MacroKind::Derive,
                    name
                ) if name.as_str() == derive_name
            ) && expansion.macro_def_id.is_some_and(|macro_definition| {
                tcx.get_attrs_by_path(
                    macro_definition,
                    &[rustc_span::Symbol::intern("rustc_builtin_macro")],
                )
                .next()
                .is_some()
            })
        })
    })
}

fn standard_builder<'tcx>(tcx: TyCtxt<'tcx>, builder: Ty<'tcx>) -> bool {
    let ty::Adt(owner, arguments) = builder.kind() else {
        return false;
    };
    if !types::standard(tcx, owner.did()) {
        return false;
    }

    let arguments: Vec<_> = arguments.types().collect();
    // `RandomState::build_hasher` constructs the fixed standard
    // `DefaultHasher` with its stored SipHash keys.
    if types::physical_item_path(tcx, owner.did(), "std", &["hash", "random", "RandomState"]) {
        return arguments.is_empty();
    }
    // This standard wrapper calls `Default::default` for its concrete hasher.
    if types::physical_item_path(tcx, owner.did(), "core", &["hash", "BuildHasherDefault"]) {
        return match arguments.as_slice() {
            [hasher] => matches!(
                hasher.kind(),
                ty::Adt(hasher, _)
                    if types::physical_item_path(tcx, hasher.did(), "std", &["hash", "random", "DefaultHasher"])
            ),
            _ => false,
        };
    }
    false
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
