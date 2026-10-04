// SPDX-License-Identifier: Apache-2.0
//! Additional bounds for typed JSON targets.
//!
//! `DecodeContext::typed_json_storage` reserves twice `JsonBound::bytes` plus
//! `size_of::<T>()`. `json_bound` charges six `serde_json::Value` slots per
//! counted value and 4096 bytes per object entry. The pinned `String` is three
//! target `usize` words (`Vec<u8>`); a `Value::String` variant therefore gives
//! a target-layout lower bound for `Value`. Keep these limits aligned with
//! `cadmpeg-core/src/decode/tree.rs`'s `json_bound` and
//! `JSON_MAP_ENTRY_BOUND` constants.
use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;

// These are the pinned standard-library/hashbrown storage shapes used by the
// admitted containers. Hashbrown's raw table has one control byte per bucket
// and a control group no wider than 16 bytes; growth plus power-of-two rounding
// fits within four buckets per array item: for n >= 15, requested capacity
// needs at most ceil(8n/7) buckets before power-of-two rounding, so the new
// table has fewer than 16n/7 + 2 buckets; an old/new resize pair stays below
// 4n. Smaller tables use 4, 8 or 16 buckets and their excess is charged at the
// root. Alignment padding and trailing control groups are also charged there.
const HASH_TABLE_BUCKET_FACTOR: u64 = 4;
const HASH_TABLE_SIMD_GROUP_BYTES: u64 = 16;
const MAP_ENTRY_BYTES: u64 = 4096;
const BTREE_NODE_SLOTS: u64 = 11;
const BTREE_NODE_CHILDREN: u64 = 12;
const BTREE_MIN_NON_ROOT_SLOTS: u64 = 5;
const VEC_GROWTH_FACTOR: u64 = 3;

#[derive(Clone, Copy)]
struct ActiveType<'tcx> {
    value: Ty<'tcx>,
    input_steps: usize,
}

struct Proof<'tcx> {
    active: Vec<ActiveType<'tcx>>,
    input_steps: usize,
    type_limit: usize,
}

/// Proves bounded owned storage and input-consuming recursion for a derived
/// typed-JSON target. `serde.rs` separately proves that every non-standard
/// deserializer in the tree is a trusted derive and checks collection key and
/// hasher callbacks.
pub(crate) fn safe_target<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> bool {
    let mut proof = Proof {
        active: Vec::new(),
        input_steps: 0,
        type_limit: tcx.recursion_limit().0,
    };
    target(tcx, value, 0, &mut proof)
}

fn target<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    charged_for_value: u64,
    proof: &mut Proof<'tcx>,
) -> bool {
    if proof.active.len() >= proof.type_limit
        || value.has_aliases()
        || value.has_non_region_param()
        || value.has_escaping_bound_vars()
    {
        return false;
    }
    let value = value.peel_refs();
    if let Some(active) = proof.active.iter().rfind(|active| active.value == value) {
        return proof.input_steps > active.input_steps;
    }

    proof.active.push(ActiveType {
        value,
        input_steps: proof.input_steps,
    });
    let admitted = match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Str => true,
        ty::Tuple(fields) => fields
            .iter()
            .all(|field| consuming_child(tcx, field, proof)),
        // Fixed arrays occupy their enclosing value inline. Their element
        // layout is already included in the sized root or repeated parent slot.
        ty::Array(element, _) => consuming_child(tcx, *element, proof),
        // An unsized slice has no standalone Deserialize target. The supported
        // Box<[T]> path below is rejected until its Vec-to-box transient is
        // proved separately.
        ty::Slice(_) => false,
        ty::Adt(owner, arguments) if types::standard(tcx, owner.did()) => {
            standard_target(tcx, owner.did(), arguments, charged_for_value, proof)
        }
        ty::Adt(owner, arguments) if !owner.is_union() => {
            !owner.has_dtor(tcx)
                && derived_target(
                    tcx,
                    owner.did(),
                    *owner,
                    arguments,
                    charged_for_value,
                    proof,
                )
        }
        ty::Param(_) | ty::Alias(..) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => false,
        _ => false,
    };
    proof.active.pop();
    admitted
}

fn derived_target<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    owner: ty::AdtDef<'tcx>,
    arguments: ty::GenericArgsRef<'tcx>,
    charged_for_value: u64,
    proof: &mut Proof<'tcx>,
) -> bool {
    let variants = owner.variants();
    if owner.is_struct() {
        let fields = &owner.non_enum_variant().fields;
        let transparent = serde_transparent(tcx, definition);
        let newtype = fields.len() == 1
            && fields.iter().next().is_some_and(|field| {
                tcx.item_name(field.did).as_str().parse::<usize>().is_ok()
            });
        let consumes_input = !transparent && !newtype;
        fields.iter().all(|field| {
            let field_type = field.ty(tcx, arguments).skip_norm_wip();
            if consumes_input {
                consuming_child(tcx, field_type, proof)
            } else {
                target(tcx, field_type, charged_for_value, proof)
            }
        })
    } else if owner.is_enum() {
        variants.iter().all(|variant| {
            variant.fields.iter().all(|field| {
                consuming_child(
                    tcx,
                    field.ty(tcx, arguments).skip_norm_wip(),
                    proof,
                )
            })
        })
    } else {
        false
    }
}

fn serde_transparent(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    tcx.get_attrs_by_path(definition, &[rustc_span::Symbol::intern("serde")])
        .any(|attribute| {
            attribute.meta_item_list().is_some_and(|items| {
                items.iter().any(|item| {
                    let rustc_ast::ast::MetaItemInner::MetaItem(item) = item else {
                        return false;
                    };
                    item.path.segments.last().is_some_and(|segment| {
                        segment.ident.name.as_str() == "transparent"
                    })
                })
            })
        })
}

fn standard_target<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
    charged_for_value: u64,
    proof: &mut Proof<'tcx>,
) -> bool {
    let arguments: Vec<_> = arguments.types().collect();
    match definition {
        _ if types::physical_item_path(tcx, definition, "alloc", &["string", "String"]) => true,
        _ if types::physical_item_path(tcx, definition, "core", &["option", "Option"]) => {
            exactly_one(&arguments).is_some_and(|inner| {
                target(tcx, inner, charged_for_value, proof)
            })
        }
        _ if types::physical_item_path(tcx, definition, "core", &["result", "Result"]) => {
            arguments.len() == 2
                && arguments
                    .iter()
                    .all(|inner| consuming_child(tcx, *inner, proof))
        }
        _ if types::physical_item_path(tcx, definition, "alloc", &["vec", "Vec"]) => {
            one_with_global_allocator(tcx, &arguments).is_some_and(|element| {
                repeated_child(tcx, element, charged_for_value, proof)
            })
        }
        // VecDeque uses a ring buffer and BinaryHeap invokes T::Ord while it
        // restores heap order. Neither behavior is admitted by the Vec proof.
        _ if types::physical_item_path(tcx, definition, "alloc", &["collections", "vec_deque", "VecDeque"])
            || types::physical_item_path(tcx, definition, "alloc", &["collections", "binary_heap", "BinaryHeap"]) => false,
        _ if types::physical_item_path(tcx, definition, "alloc", &["collections", "linked_list", "LinkedList"]) => {
            one_with_global_allocator(tcx, &arguments).is_some_and(|element| {
                linked_list_child(tcx, element, charged_for_value, proof)
            })
        }
        _ if types::physical_item_path(tcx, definition, "alloc", &["boxed", "Box"]) => {
            one_with_global_allocator(tcx, &arguments).is_some_and(|inner| {
                match inner.kind() {
                    ty::Slice(_) | ty::Str => false,
                    _ => {
                        allocation_child(tcx, inner, charged_for_value, 0, proof)
                    }
                }
            })
        }
        _ if types::physical_item_path(tcx, definition, "alloc", &["rc", "Rc"])
            || types::physical_item_path(tcx, definition, "alloc", &["sync", "Arc"]) => {
            one_with_global_allocator(tcx, &arguments).is_some_and(|inner| {
                match inner.kind() {
                    ty::Slice(_) | ty::Str => false,
                    _ => {
                        let Some(header) = tcx
                            .data_layout
                            .pointer_size()
                            .bytes()
                            .checked_mul(2)
                            .and_then(|bytes| {
                                layout_size_align(tcx, inner)
                                    .and_then(|(_, alignment)| alignment.checked_sub(1))
                                    .and_then(|padding| bytes.checked_add(padding))
                            })
                        else {
                            return false;
                        };
                        allocation_child(tcx, inner, charged_for_value, header, proof)
                    }
                }
            })
        }
        _ if types::physical_item_path(tcx, definition, "std", &["collections", "hash", "map", "HashMap"]) => {
            let Some((key, value)) = hash_map_types(tcx, &arguments) else {
                return false;
            };
            within_value_envelope(tcx, charged_for_value)
                && hash_map_storage(tcx, key, value)
                && consuming_child(tcx, key, proof)
                && consuming_child(tcx, value, proof)
        }
        _ if types::physical_item_path(tcx, definition, "alloc", &["collections", "btree", "map", "BTreeMap"]) => {
            let Some((key, value)) = btree_map_types(tcx, &arguments) else {
                return false;
            };
            within_value_envelope(tcx, charged_for_value)
                && btree_map_storage(tcx, key, value)
                && consuming_child(tcx, key, proof)
                && consuming_child(tcx, value, proof)
        }
        _ if types::physical_item_path(tcx, definition, "std", &["collections", "hash", "set", "HashSet"]) => {
            let Some(key) = hash_set_type(tcx, &arguments) else {
                return false;
            };
            hash_set_storage(tcx, key, charged_for_value)
                && map_key(tcx, key)
                && consuming_child(tcx, key, proof)
        }
        _ if types::physical_item_path(tcx, definition, "alloc", &["collections", "btree", "set", "BTreeSet"]) => {
            let Some(key) = btree_set_type(tcx, &arguments) else {
                return false;
            };
            btree_set_storage(tcx, key, charged_for_value)
                && map_key(tcx, key)
                && consuming_child(tcx, key, proof)
        }
        _ => false,
    }
}

fn consuming_child<'tcx>(
    tcx: TyCtxt<'tcx>,
    child: Ty<'tcx>,
    proof: &mut Proof<'tcx>,
) -> bool {
    let Some(next) = proof.input_steps.checked_add(1) else {
        return false;
    };
    let prior = proof.input_steps;
    proof.input_steps = next;
    let admitted = target(tcx, child, 0, proof);
    proof.input_steps = prior;
    admitted
}

fn repeated_child<'tcx>(
    tcx: TyCtxt<'tcx>,
    child: Ty<'tcx>,
    charged_for_value: u64,
    proof: &mut Proof<'tcx>,
) -> bool {
    let Some(size) = layout_size(tcx, child) else {
        return false;
    };
    let Some(slot_limit) = repeated_slot_bytes(tcx) else {
        return false;
    };
    let Some(envelope) = typed_value_bytes(tcx) else {
        return false;
    };
    let Some(child_slots) = size.checked_mul(VEC_GROWTH_FACTOR) else {
        return false;
    };
    let Some(root_minimum) = size
        .checked_mul(vec_min_capacity(size))
        .and_then(|bytes| bytes.checked_add(charged_for_value))
    else {
        return false;
    };
    // At a reallocation peak, Vec keeps the old and new buffers live together.
    // Geometric growth bounds the old/new peak by three slots per parsed
    // member. Rust's pinned RawVec minimum initial capacity is eight for
    // one-byte items, four through 1024-byte items, and one for larger items.
    // The initial buffer is charged to the array node. For n array members,
    // typed storage
    // supplies 12 Value slots for each of n+1 JSON value nodes. Since one
    // Value is at least one String, a slot of at most 3 Strings gives
    // initial_capacity*slot + 3n*slot <= 12*(n+1)*Value. The minimum initial
    // buffer and transparent wrapper allocations are charged to the array
    // node; three peak-growth slots per member are charged to that member's
    // node.
    if size > slot_limit || child_slots > envelope || root_minimum > envelope {
        return false;
    }
    let Some(next) = proof.input_steps.checked_add(1) else {
        return false;
    };
    let prior_steps = proof.input_steps;
    proof.input_steps = next;
    let admitted = target(tcx, child, child_slots, proof);
    proof.input_steps = prior_steps;
    admitted
}

fn linked_list_child<'tcx>(
    tcx: TyCtxt<'tcx>,
    child: Ty<'tcx>,
    charged_for_value: u64,
    proof: &mut Proof<'tcx>,
) -> bool {
    let Some((size, alignment)) = layout_size_align(tcx, child) else {
        return false;
    };
    let pointer = tcx.data_layout.pointer_size().bytes();
    let Some(node_bytes) = alignment
        .checked_sub(1)
        .and_then(|padding| size.checked_add(padding))
        .and_then(|bytes| pointer.checked_mul(2).and_then(|links| bytes.checked_add(links)))
    else {
        return false;
    };
    if !within_value_envelope(tcx, charged_for_value)
        || !within_value_envelope(tcx, node_bytes)
    {
        return false;
    }
    let Some(next) = proof.input_steps.checked_add(1) else {
        return false;
    };
    let prior_steps = proof.input_steps;
    proof.input_steps = next;
    let admitted = target(tcx, child, node_bytes, proof);
    proof.input_steps = prior_steps;
    admitted
}

fn allocation_child<'tcx>(
    tcx: TyCtxt<'tcx>,
    child: Ty<'tcx>,
    charged_for_value: u64,
    allocation_overhead: u64,
    proof: &mut Proof<'tcx>,
) -> bool {
    let Some(size) = layout_size(tcx, child) else {
        return false;
    };
    let Some(total) = charged_for_value
        .checked_add(size)
        .and_then(|bytes| bytes.checked_add(allocation_overhead))
    else {
        return false;
    };
    within_value_envelope(tcx, total) && target(tcx, child, total, proof)
}

fn hash_map_storage<'tcx>(tcx: TyCtxt<'tcx>, key: Ty<'tcx>, value: Ty<'tcx>) -> bool {
    let pair = ty::Ty::new_tup(tcx, &[key, value]);
    let Some((pair_bytes, pair_align)) = layout_size_align(tcx, pair) else {
        return false;
    };
    let Some(table_bytes) = hash_table_entry_bytes(pair_bytes)
        .and_then(|bytes| bytes.checked_add(hash_table_root_overhead(tcx, pair_bytes, pair_align)?))
    else {
        return false;
    };
    // JSON object entries have their own 4096-byte allowance before the
    // typed 2x multiplier. Half of that is charged per entry for a growing
    // hash table's old and new buckets, control bytes, alignment padding and
    // control-group tails.
    table_bytes <= MAP_ENTRY_BYTES / 2
}

fn btree_map_storage<'tcx>(tcx: TyCtxt<'tcx>, key: Ty<'tcx>, value: Ty<'tcx>) -> bool {
    let pair = ty::Ty::new_tup(tcx, &[key, value]);
    let Some(node_bytes) = btree_node_bytes(tcx, pair, true) else {
        return false;
    };
    // Each allocated node has at least one current key/value entry, so there
    // are at most n nodes for n input object entries, including a split's new
    // node. Capping each full internal-node layout at 2048 bytes keeps the
    // aggregate below half of the core's 4096-byte pre-multiplier allowance.
    node_bytes <= MAP_ENTRY_BYTES / 2
}

fn hash_set_storage<'tcx>(
    tcx: TyCtxt<'tcx>,
    key: Ty<'tcx>,
    charged_for_value: u64,
) -> bool {
    let Some((key_bytes, key_align)) = layout_size_align(tcx, key) else {
        return false;
    };
    let Some(table_bytes) = hash_table_entry_bytes(key_bytes) else {
        return false;
    };
    let Some(root_overhead) = hash_table_root_overhead(tcx, key_bytes, key_align) else {
        return false;
    };
    let Some(root_bytes) = charged_for_value.checked_add(root_overhead) else {
        return false;
    };
    // HashSet serializes as an array. Each member gets only the 12 Value-slot
    // typed envelope, not the JSON object's 4096-byte entry allowance. The
    // root gets the old/new control-group tails; bucket slots and control
    // bytes are bounded by four per member across power-of-two growth.
    within_value_envelope(tcx, root_bytes)
        && typed_value_bytes(tcx).is_some_and(|limit| table_bytes <= limit)
}

fn btree_set_storage<'tcx>(
    tcx: TyCtxt<'tcx>,
    key: Ty<'tcx>,
    charged_for_value: u64,
) -> bool {
    let Some(node_bytes) = btree_node_bytes(tcx, key, true) else {
        return false;
    };
    let Some(value_envelope) = typed_value_bytes(tcx) else {
        return false;
    };
    let Some(root_and_first_member) = value_envelope.checked_mul(2) else {
        return false;
    };
    let Some(root_charge) = node_bytes.checked_add(charged_for_value) else {
        return false;
    };
    // A B-tree node (bounded as an internal node, even when it is a leaf) is
    // at most two JSON value envelopes after the root wrapper charge. Every
    // non-root node contains at least five keys, so each later node is paid by
    // the five corresponding array member envelopes. The root plus the array
    // node pays its fixed allocation.
    root_charge <= root_and_first_member
        && BTREE_MIN_NON_ROOT_SLOTS
            .checked_mul(value_envelope)
            .is_some_and(|budget| node_bytes <= budget)
}

fn btree_node_bytes<'tcx>(tcx: TyCtxt<'tcx>, slot_type: Ty<'tcx>, internal: bool) -> Option<u64> {
    let (slot_size, slot_align) = layout_size_align(tcx, slot_type)?;
    let pointer = tcx.data_layout.pointer_size().bytes();
    let slots = BTREE_NODE_SLOTS.checked_mul(slot_size)?;
    let links = if internal {
        BTREE_NODE_CHILDREN.checked_mul(pointer)?
    } else {
        0
    };
    // Pinned B-tree nodes carry a parent pointer and a length. Two alignment
    // slacks cover padding before the over-aligned slots and at node end.
    let padding = slot_align.checked_sub(1)?.checked_mul(2)?;
    let header = pointer.checked_mul(2)?.checked_add(padding)?;
    slots.checked_add(links)?.checked_add(header)
}

fn hash_table_entry_bytes(slot_bytes: u64) -> Option<u64> {
    slot_bytes.checked_add(1)?.checked_mul(HASH_TABLE_BUCKET_FACTOR)
}

fn hash_table_root_overhead(tcx: TyCtxt<'_>, slot_bytes: u64, slot_align: u64) -> Option<u64> {
    let group = hash_table_group_bytes(tcx)?;
    let small_table_buckets: u64 = if slot_bytes <= 1 {
        16
    } else if slot_bytes <= 3 {
        8
    } else {
        4
    };
    let Some(extra_small_buckets) = small_table_buckets.checked_sub(HASH_TABLE_BUCKET_FACTOR) else {
        return None;
    };
    let extra_small_bytes = extra_small_buckets.checked_mul(slot_bytes.checked_add(1)?)?;
    // Each simultaneously live old/new table can need less than one alignment
    // word of padding before the control array and one full trailing group.
    // The small-table bucket delta covers hashbrown's 16/8/4-bucket minimums.
    extra_small_bytes.checked_add(slot_align.checked_add(group)?.checked_mul(2)?)
}

fn hash_table_group_bytes(tcx: TyCtxt<'_>) -> Option<u64> {
    tcx.data_layout
        .pointer_size()
        .bytes()
        .checked_mul(2)
        .map(|target_word_group| target_word_group.max(HASH_TABLE_SIMD_GROUP_BYTES))
}

fn within_value_envelope(tcx: TyCtxt<'_>, bytes: u64) -> bool {
    typed_value_bytes(tcx).is_some_and(|limit| bytes <= limit)
}

fn string_size_floor(tcx: TyCtxt<'_>) -> Option<u64> {
    tcx.data_layout.pointer_size().bytes().checked_mul(3)
}

fn repeated_slot_bytes(tcx: TyCtxt<'_>) -> Option<u64> {
    string_size_floor(tcx)?.checked_mul(3)
}

fn vec_min_capacity(element_size: u64) -> u64 {
    if element_size == 1 {
        8
    } else if element_size <= 1024 {
        4
    } else {
        1
    }
}

fn typed_value_bytes(tcx: TyCtxt<'_>) -> Option<u64> {
    string_size_floor(tcx)?.checked_mul(12)
}

fn map_key(tcx: TyCtxt<'_>, key: Ty<'_>) -> bool {
    matches!(key.kind(), ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_))
        || matches!(key.kind(), ty::Adt(owner, _) if types::physical_item_path(tcx, owner.did(), "alloc", &["string", "String"]))
}

fn hash_map_types<'tcx>(
    tcx: TyCtxt<'tcx>,
    arguments: &[Ty<'tcx>],
) -> Option<(Ty<'tcx>, Ty<'tcx>)> {
    (arguments.len() >= 3
        && standard_path(tcx, arguments[2], "std::hash::random::RandomState")
        && global_allocator_tail(tcx, &arguments[3..]))
    .then(|| (arguments[0], arguments[1]))
}

fn hash_set_type<'tcx>(tcx: TyCtxt<'tcx>, arguments: &[Ty<'tcx>]) -> Option<Ty<'tcx>> {
    (arguments.len() >= 2
        && standard_path(tcx, arguments[1], "std::hash::random::RandomState")
        && global_allocator_tail(tcx, &arguments[2..]))
    .then(|| arguments[0])
}

fn btree_map_types<'tcx>(
    tcx: TyCtxt<'tcx>,
    arguments: &[Ty<'tcx>],
) -> Option<(Ty<'tcx>, Ty<'tcx>)> {
    (arguments.len() >= 2 && global_allocator_tail(tcx, &arguments[2..]))
        .then(|| (arguments[0], arguments[1]))
}

fn btree_set_type<'tcx>(tcx: TyCtxt<'tcx>, arguments: &[Ty<'tcx>]) -> Option<Ty<'tcx>> {
    (!arguments.is_empty() && global_allocator_tail(tcx, &arguments[1..]))
        .then(|| arguments[0])
}

fn one_with_global_allocator<'tcx>(tcx: TyCtxt<'tcx>, arguments: &[Ty<'tcx>]) -> Option<Ty<'tcx>> {
    (arguments.len() >= 1 && global_allocator_tail(tcx, &arguments[1..]))
        .then(|| arguments[0])
}

fn global_allocator_tail(tcx: TyCtxt<'_>, arguments: &[Ty<'_>]) -> bool {
    arguments.len() <= 1
        && arguments.iter().all(|argument| {
            standard_path(tcx, *argument, "alloc::alloc::Global")
        })
}

fn standard_path(tcx: TyCtxt<'_>, value: Ty<'_>, path: &str) -> bool {
    let Some((crate_name, item_path)) = path.split_once("::") else { return false; };
    let parts: Vec<_> = item_path.split("::").collect();
    matches!(value.kind(), ty::Adt(owner, _) if types::physical_item_path(tcx, owner.did(), crate_name, &parts))
}

fn exactly_one<'tcx>(arguments: &[Ty<'tcx>]) -> Option<Ty<'tcx>> {
    (arguments.len() == 1).then(|| arguments[0])
}

fn layout_size<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> Option<u64> {
    layout_size_align(tcx, value).map(|(size, _)| size)
}

fn layout_size_align<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> Option<(u64, u64)> {
    if value.has_aliases() || value.has_non_region_param() || value.has_escaping_bound_vars() {
        return None;
    }
    tcx.layout_of(ty::TypingEnv::fully_monomorphized().as_query_input(value))
        .ok()
        .map(|layout| (layout.size.bytes(), layout.align.abi.bytes()))
}
