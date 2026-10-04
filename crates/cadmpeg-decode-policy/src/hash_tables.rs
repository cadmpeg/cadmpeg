// SPDX-License-Identifier: Apache-2.0
//! Keyed hash-table operations and the traversals decode code does not perform.
//!
//! Decode code uses `HashMap` and `HashSet` only for keyed lookup, insertion
//! and removal. Their iteration order is unspecified and a scan walks the
//! allocated table, whose extent has no exact public bound, so traversal,
//! whole-table comparison, cloning, scan-retain and drain are reported with
//! the ordered replacement. A keyed operation hashes and compares its key
//! through `Hash` and `PartialEq` callbacks; core charges the key's
//! `DecodeCost` for them, which covers a field-wise derived or standard
//! implementation. Collision probing is not charged per probe, so the table
//! must use the randomly keyed standard hasher.

use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;

#[derive(Clone, Copy, Eq, PartialEq)]
enum CollectionKind {
    Map,
    Set,
}

const MAP: &[&str] = &["collections", "hash", "map", "HashMap"];
const SET: &[&str] = &["collections", "hash", "set", "HashSet"];

/// The replacement named for a hash-table traversal.
pub(crate) const TRAVERSAL_REPLACEMENT: &str = "BTreeMap or BTreeSet for collections that decode traverses or compares, or Vec; keep HashMap and HashSet for keyed lookup, insertion and removal through DecodeContext operations";

fn collection_kind<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> Option<CollectionKind> {
    let ty::Adt(owner, _) = types::reveal_opaque(tcx, value).peel_refs().kind() else {
        return None;
    };
    if types::physical_item_path(tcx, owner.did(), "std", MAP) {
        Some(CollectionKind::Map)
    } else if types::physical_item_path(tcx, owner.did(), "std", SET) {
        Some(CollectionKind::Set)
    } else {
        None
    }
}

/// Returns the traversal a call performs over a hash table, if any.
pub(crate) fn traversal<'tcx>(
    tcx: TyCtxt<'tcx>,
    callee: DefId,
    receiver: Option<Ty<'tcx>>,
) -> Option<&'static str> {
    const MAP_TRAVERSALS: &[&str] = &[
        "iter",
        "iter_mut",
        "keys",
        "values",
        "values_mut",
        "into_keys",
        "into_values",
        "drain",
        "retain",
        "extract_if",
    ];
    const SET_TRAVERSALS: &[&str] = &[
        "iter",
        "drain",
        "retain",
        "extract_if",
        "difference",
        "symmetric_difference",
        "intersection",
        "union",
        "is_disjoint",
        "is_subset",
        "is_superset",
    ];
    for method in MAP_TRAVERSALS {
        if types::physical_inherent_method(tcx, callee, "std", MAP, method) {
            return Some(method);
        }
    }
    for method in SET_TRAVERSALS {
        if types::physical_inherent_method(tcx, callee, "std", SET, method) {
            return Some(method);
        }
    }
    let receiver = receiver?;
    collection_kind(tcx, receiver)?;
    let owner = tcx.trait_of_assoc(callee)?;
    let name = tcx.item_name(callee);
    [
        (
            "core",
            &["iter", "traits", "collect", "IntoIterator"][..],
            "into_iter",
        ),
        ("core", &["cmp", "PartialEq"][..], "eq"),
        ("core", &["cmp", "PartialEq"][..], "ne"),
        ("core", &["clone", "Clone"][..], "clone"),
        ("core", &["clone", "Clone"][..], "clone_from"),
    ]
    .into_iter()
    .find(|(crate_name, parts, method)| {
        name.as_str() == *method && types::physical_item_path(tcx, owner, crate_name, parts)
    })
    .map(|(_, _, method)| method)
}

/// Returns whether a binary comparison compares a hash table as a whole.
pub(crate) fn whole_table<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> bool {
    collection_kind(tcx, value).is_some()
        || matches!(types::reveal_opaque(tcx, value).peel_refs().kind(), ty::Adt(owner, arguments)
            if types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"])
                && arguments.types().next().is_some_and(|inner| collection_kind(tcx, inner).is_some()))
}

/// Identifies the DecodeContext methods whose key receipts cover one keyed
/// hash-table operation.
pub(crate) fn is_hash_lookup_context(tcx: TyCtxt<'_>, context: DefId) -> bool {
    context_lookup_route(tcx, context).is_some()
}

/// Returns `Some(true)` only when this DecodeContext route reaches its expected
/// physical std operation and its concrete key, query, Borrow and builder are
/// bounded. `Some(false)` means the wrapper has the expected operation name,
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

/// Identifies a physical std keyed operation and proves its concrete
/// callbacks. This also guards direct operations outside DecodeContext.
pub(crate) fn bounded_raw_lookup<'tcx>(
    tcx: TyCtxt<'tcx>,
    callee: DefId,
    receiver: Ty<'tcx>,
    query: Ty<'tcx>,
) -> Option<bool> {
    let kind = raw_lookup_route(tcx, callee)?;
    // A generic key is proven where a concrete instance supplies it.
    if receiver.has_non_region_param() || query.has_non_region_param() {
        return None;
    }
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
        ("insert_hash_map", CollectionKind::Map, "insert"),
        ("contains_hash_set", CollectionKind::Set, "contains"),
        ("get_hash_set", CollectionKind::Set, "get"),
        ("remove_hash_set", CollectionKind::Set, "remove"),
        ("insert_hash_set", CollectionKind::Set, "insert"),
        ("insert_string_set", CollectionKind::Set, "insert"),
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
        "insert",
        "entry",
    ] {
        if types::physical_inherent_method(tcx, callee, "std", MAP, method) {
            return Some(CollectionKind::Map);
        }
    }
    for method in ["contains", "get", "remove", "take", "replace", "insert"] {
        if types::physical_inherent_method(tcx, callee, "std", SET, method) {
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
    let (collection, key, builder) = match kind {
        CollectionKind::Map => (MAP, arguments.first(), arguments.get(2)),
        CollectionKind::Set => (SET, arguments.first(), arguments.get(1)),
    };
    let (Some(key), Some(builder)) = (key, builder) else {
        return false;
    };
    if !types::physical_item_path(tcx, owner.did(), "std", collection) {
        return false;
    }
    // A lookup borrows its query; an insertion moves the key itself.
    let query = match types::reveal_opaque(tcx, query).kind() {
        ty::Ref(_, query, _) => types::reveal_opaque(tcx, *query),
        _ => types::reveal_opaque(tcx, query),
    };
    bounded_key(tcx, *key)
        && bounded_key(tcx, query)
        && supported_borrow_pair(tcx, *key, query)
        && random_state(tcx, *builder)
}

fn supported_borrow_pair<'tcx>(tcx: TyCtxt<'tcx>, stored: Ty<'tcx>, query: Ty<'tcx>) -> bool {
    let stored = tcx.erase_and_anonymize_regions(types::reveal_opaque(tcx, stored));
    let query = tcx.erase_and_anonymize_regions(types::reveal_opaque(tcx, query));
    if stored == query {
        // The blanket `Borrow<T> for T` is the only coherent identity
        // implementation, including for local key types.
        return true;
    }
    if let ty::Ref(_, borrowed, _) = stored.kind() {
        if *borrowed == query {
            // `Borrow<T> for &T` returns the existing referent.
            return true;
        }
    }
    let ty::Adt(owner, arguments) = stored.kind() else {
        return false;
    };
    let inner = arguments.types().next();
    // String, Cow<str>, Box<T> and Vec<T> borrow their stored contents.
    types::physical_item_path(tcx, owner.did(), "alloc", &["string", "String"])
        && matches!(query.kind(), ty::Str)
        || types::physical_item_path(tcx, owner.did(), "alloc", &["borrow", "Cow"])
            && inner.is_some_and(|inner| matches!(inner.kind(), ty::Str))
            && matches!(query.kind(), ty::Str)
        || types::physical_item_path(tcx, owner.did(), "alloc", &["boxed", "Box"])
            && inner == Some(query)
        || types::physical_item_path(tcx, owner.did(), "alloc", &["vec", "Vec"])
            && matches!(query.kind(), ty::Slice(element) if Some(*element) == inner)
}

/// Returns whether equality on this concrete value is field-wise, so the
/// operands' `DecodeCost` bounds its comparison.
pub(crate) fn bounded_equality<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> bool {
    bounded_tree(tcx, value, false, &mut Vec::new())
}

/// Returns whether hashing and equality on this concrete key are field-wise.
fn bounded_key<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> bool {
    bounded_tree(tcx, value, true, &mut Vec::new())
}

fn bounded_tree<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    hashed: bool,
    seen: &mut Vec<Ty<'tcx>>,
) -> bool {
    let value = types::reveal_opaque(tcx, value).peel_refs();
    // A recursive type compares or hashes each finite value once per node.
    if seen.contains(&value) {
        return true;
    }
    if seen.len() >= tcx.recursion_limit().0 {
        return false;
    }
    let depth = seen.len();
    seen.push(value);
    let bounded = match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Str => true,
        ty::Array(element, _) | ty::Slice(element) => bounded_tree(tcx, *element, hashed, seen),
        ty::Tuple(fields) => fields
            .iter()
            .all(|field| bounded_tree(tcx, field, hashed, seen)),
        ty::Adt(owner, arguments) if types::standard(tcx, owner.did()) => {
            let arguments: Vec<_> = arguments.types().collect();
            if types::physical_item_path(tcx, owner.did(), "alloc", &["string", "String"]) {
                true
            } else if types::physical_item_path(tcx, owner.did(), "alloc", &["borrow", "Cow"]) {
                matches!(arguments.as_slice(), [inner] if matches!(inner.kind(), ty::Str))
            } else if types::physical_item_path(tcx, owner.did(), "alloc", &["boxed", "Box"])
                || types::physical_item_path(tcx, owner.did(), "alloc", &["vec", "Vec"])
                || types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"])
            {
                arguments
                    .first()
                    .is_some_and(|inner| bounded_tree(tcx, *inner, hashed, seen))
            } else {
                false
            }
        }
        ty::Adt(owner, arguments) => {
            (!hashed || derived_impls(tcx, value, owner.did(), &["hash", "Hash"]))
                && derived_impls(tcx, value, owner.did(), &["cmp", "PartialEq"])
                && owner.all_fields().all(|field| {
                    bounded_tree(tcx, field.ty(tcx, arguments).skip_norm_wip(), hashed, seen)
                })
        }
        _ => false,
    };
    seen.truncate(depth);
    bounded
}

/// Every implementation of the trait for the concrete owner comes from a
/// derive expansion. `PartialEq` impls with another right-hand type are not the
/// callback a keyed table or `DecodeContext::equal` invokes.
fn derived_impls<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    owner: DefId,
    trait_path: &[&str],
) -> bool {
    let Some(trait_id) = tcx
        .all_traits_including_private()
        .find(|trait_id| types::physical_item_path(tcx, *trait_id, "core", trait_path))
    else {
        return false;
    };
    let partial_eq = trait_path == ["cmp", "PartialEq"];
    let candidates: Vec<_> = tcx
        .non_blanket_impls_for_ty(trait_id, value)
        .filter(|implementation| {
            matches!(
                tcx.type_of(*implementation)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .kind(),
                ty::Adt(definition, _) if definition.did() == owner
            )
        })
        .filter(|implementation| !partial_eq || partial_eq_self_impl(tcx, *implementation, value))
        .collect();
    // A derive expansion marks its impl; a hand-written marker is not derived.
    !candidates.is_empty()
        && candidates.into_iter().all(|implementation| {
            tcx.is_automatically_derived(implementation)
                && tcx.def_span(implementation).from_expansion()
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

/// `RandomState` seeds each table's SipHash keys at random, so a file cannot
/// choose colliding keys. A fixed-key builder such as
/// `BuildHasherDefault<DefaultHasher>` lets it.
fn random_state(tcx: TyCtxt<'_>, builder: Ty<'_>) -> bool {
    matches!(builder.kind(), ty::Adt(owner, arguments)
        if types::physical_item_path(tcx, owner.did(), "std", &["hash", "random", "RandomState"])
            && arguments.is_empty())
}
