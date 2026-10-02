// SPDX-License-Identifier: Apache-2.0
//! External costs. Operand indices include the method receiver at index zero.
use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::DefId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Allocation {
    None,
    Growth,
    Clone,
    Cloned,
    Format,
    Conversion,
    Collect,
    Capacity,
    Repeat,
    Result,
    Reallocate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Work {
    Fixed,
    Receiver,
    Argument(usize),
    Iterator,
    Comparison,
    Format,
    Repeat,
    Conversion,
}

#[derive(Clone, Copy)]
pub(crate) struct Summary {
    pub(crate) allocation: Allocation,
    pub(crate) work: Work,
    pub(crate) zero_operand: Option<usize>,
    pub(crate) empty_operand: Option<usize>,
}

pub(crate) fn summary(
    tcx: TyCtxt<'_>,
    definition: DefId,
    receiver: Option<Ty<'_>>,
) -> Option<Summary> {
    let crate_name = tcx.crate_name(definition.krate);
    let name = tcx.opt_item_name(definition)?;
    let path = tcx.def_path_str(definition);
    let value = receiver.map(|value| value.peel_refs());
    let owner = value.and_then(|value| match value.kind() {
        ty::Adt(owner, _) => Some(tcx.item_name(owner.did())),
        _ => None,
    });
    let keyed = owner.is_some_and(|name| {
        matches!(
            name.as_str(),
            "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet"
        )
    });
    if !types::standard(tcx, definition) {
        let (allocation, work) = match (crate_name.as_str(), name.as_str()) {
            ("roxmltree", "parse" | "parse_with_options") => {
                (Allocation::Result, Work::Argument(0))
            }
            ("roxmltree", "attribute" | "attribute_node" | "has_attribute") => {
                (Allocation::None, Work::Receiver)
            }
            ("roxmltree", "root_element") => (Allocation::None, Work::Receiver),
            ("roxmltree", "has_tag_name") => (Allocation::None, Work::Argument(1)),
            (
                "roxmltree",
                "text" | "is_element" | "is_text" | "tag_name" | "name" | "namespace" | "root"
                | "children" | "descendants" | "attributes" | "range" | "parent",
            ) => (Allocation::None, Work::Fixed),
            ("serde_json", "from_str" | "from_slice") if path.contains("Deserializer") => {
                (Allocation::None, Work::Fixed)
            }
            ("serde_json", "from_str" | "from_slice") => (Allocation::Result, Work::Argument(0)),
            (
                "serde_json",
                "as_str" | "as_array" | "as_object" | "as_bool" | "as_i64" | "as_u64" | "as_f64"
                | "is_null",
            ) => (Allocation::None, Work::Fixed),
            ("serde_json", "get" | "get_mut") => (Allocation::None, Work::Argument(1)),
            _ => return None,
        };
        return Some(Summary {
            allocation,
            work,
            zero_operand: None,
            empty_operand: None,
        });
    }
    let (allocation, work) = match name.as_str() {
        "from_le_bytes" | "from_be_bytes" | "from_ne_bytes" | "to_le_bytes" | "to_be_bytes"
        | "to_ne_bytes"
            if path.contains("num::") =>
        {
            (Allocation::None, Work::Fixed)
        }
        "min" | "max" if path.contains("cmp::") => {
            if value.is_some_and(|value| matches!(value.kind(), ty::Adt(owner, _)
                if !types::standard(tcx, owner.did()))) { return None; }
            (Allocation::None, Work::Comparison)
        }
        "finish" if path.contains("hash::") => (Allocation::None, Work::Fixed),
        "write_str" if path.contains("fmt::") => (Allocation::None, Work::Argument(1)),
        "debug_struct_field1_finish" if path.contains("fmt::") => {
            (Allocation::None, Work::Argument(3))
        }
        "replace" | "take" | "swap" if path.contains("mem::") => (Allocation::None, Work::Fixed),
        "replace" | "take" if path.contains("option::") => (Allocation::None, Work::Fixed),
        "repeat" if path.contains("iter::") => (Allocation::None, Work::Fixed),
        "from_utf8_lossy" => (Allocation::Result, Work::Receiver),
        "insert" if path.contains("option::") => return None,
        "format" => (Allocation::Format, Work::Format),
        "clone" => (Allocation::Clone, Work::Receiver),
        "cloned" => (Allocation::Cloned, Work::Fixed),
        "to_string" | "to_owned" => (Allocation::Result, Work::Receiver),
        "to_vec" => (Allocation::Collect, Work::Receiver),
        "collect" | "from_iter" => (Allocation::Collect, Work::Iterator),
        "from" | "into" | "into_owned" => (Allocation::Conversion, Work::Conversion),
        "from_elem" | "repeat" => (Allocation::Repeat, Work::Repeat),
        "with_capacity" | "with_capacity_in" => (Allocation::Capacity, Work::Fixed),
        "into_boxed_slice" => (Allocation::Reallocate, Work::Fixed),
        "push" | "push_back" | "push_front" | "reserve" | "reserve_exact" | "try_reserve"
        | "try_reserve_exact" => (Allocation::Growth, Work::Fixed),
        "push_str" | "extend" | "extend_from_slice" | "append" => {
            (Allocation::Growth, Work::Argument(1))
        }
        "insert" if keyed => (Allocation::Growth, Work::Argument(1)),
        "insert" | "resize" | "resize_with" => (Allocation::Growth, Work::Receiver),
        "contains_key" | "get" | "get_mut" | "contains" | "remove" if keyed => {
            (Allocation::None, Work::Argument(1))
        }
        "get" | "get_mut" | "split_at" | "split_at_mut" | "windows" | "chunks" | "chunks_exact"
        | "chunks_mut" | "chunks_exact_mut" => (Allocation::None, Work::Fixed),
        "eq" | "cmp" | "partial_cmp" | "starts_with" | "ends_with" => {
            (Allocation::None, Work::Comparison)
        }
        "any" | "all" | "position" | "rposition" | "find" | "rfind" | "find_map" | "min"
        | "max" | "min_by" | "max_by" | "min_by_key" | "max_by_key" | "fold" | "try_fold"
        | "reduce" | "sum" | "product" | "count" | "for_each" | "try_for_each" => {
            (Allocation::None, Work::Iterator)
        }
        "replace" | "replacen" | "to_lowercase" | "to_uppercase" | "concat" | "join" => {
            (Allocation::Result, Work::Receiver)
        }
        "contains"
        | "hash"
        | "copy_from_slice"
        | "copy_within"
        | "trim"
        | "trim_start"
        | "trim_end"
        | "is_ascii"
        | "from_utf8"
        | "sort"
        | "sort_by"
        | "sort_by_key"
        | "sort_unstable"
        | "sort_unstable_by"
        | "sort_unstable_by_key"
        | "binary_search"
        | "binary_search_by"
        | "binary_search_by_key"
        | "retain"
        | "drain"
        | "clear"
        | "truncate"
        | "remove" => (Allocation::None, Work::Receiver),
        "new" if !["vec::Vec", "string::String", "collections::", "boxed::Box", "rc::Rc",
            "sync::Arc", "cell::", "sync::", "io::Cursor", "path::Path", "ffi::OsStr",
            "hash::", "ops::RangeInclusive", "fmt::Arguments", "fmt::rt", "fmt::Argument"]
            .iter().any(|owner| path.contains(owner)) => return None,
        "len"
        | "capacity"
        | "is_empty"
        | "as_str"
        | "as_bytes"
        | "as_slice"
        | "as_mut_slice"
        | "as_ref"
        | "as_mut"
        | "borrow"
        | "borrow_mut"
        | "deref"
        | "deref_mut"
        | "new"
        | "new_uninit"
        | "default"
        | "iter"
        | "iter_mut"
        | "into_iter"
        | "map"
        | "filter"
        | "filter_map"
        | "skip"
        | "take"
        | "enumerate"
        | "rev"
        | "zip"
        | "chain"
        | "peekable"
        | "fuse"
        | "copied"
        | "inspect"
        | "flat_map"
        | "flatten"
        | "step_by"
        | "skip_while"
        | "take_while"
        | "branch"
        | "from_output"
        | "from_residual"
        | "ok_or"
        | "ok_or_else"
        | "map_err"
        | "must_use"
        | "write_box_via_move"
        | "box_assume_init_into_vec_unsafe"
        | "size_of"
        | "align_of"
        | "unwrap_or"
        | "unwrap_or_else"
        | "is_some"
        | "is_none"
        | "is_some_and"
        | "is_none_or"
        | "is_ok"
        | "is_err"
        | "then"
        | "then_some"
        | "and_then"
        | "and"
        | "or"
        | "or_else"
        | "ok"
        | "err"
        | "transpose"
        | "try_from"
        | "try_into"
        | "checked_add"
        | "checked_mul"
        | "checked_sub"
        | "checked_div"
        | "checked_rem"
        | "checked_shl"
        | "checked_shr"
        | "black_box" => (Allocation::None, Work::Fixed),
        _ if path.contains("fmt::rt")
            || path.contains("fmt::Arguments")
            || path.contains("fmt::Argument") =>
        {
            (Allocation::None, Work::Fixed)
        }
        _ => return None,
    };
    let zero_operand = match name.as_str() {
        "from_elem" | "repeat" | "reserve" | "reserve_exact" | "try_reserve"
        | "try_reserve_exact" | "resize" | "resize_with" => Some(1),
        _ => None,
    };
    let empty_operand = match name.as_str() {
        "push_str" | "extend" | "extend_from_slice" => Some(1),
        _ => None,
    };
    Some(Summary {
        allocation,
        work,
        zero_operand,
        empty_operand,
    })
}
