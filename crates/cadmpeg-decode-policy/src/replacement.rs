// SPDX-License-Identifier: Apache-2.0
//! Replacement methods selected from the operation and its concrete receiver.
use crate::{types, Analysis};
use rustc_hir::{BinOpKind, Expr, ExprKind};
use rustc_middle::ty::{self, Ty};

impl<'tcx> Analysis<'_, 'tcx> {
    fn replacement_kind(&self, value: Ty<'tcx>) -> &str {
        match value.peel_refs().kind() {
            ty::Str => "text",
            ty::Slice(element) | ty::Array(element, _) => {
                if matches!(element.kind(), ty::Uint(ty::UintTy::U8)) {
                    "bytes"
                } else {
                    "slice"
                }
            }
            ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) => {
                match self.tcx.item_name(owner.did()).as_str() {
                    "String" => "text",
                    "Vec" => "vector",
                    "HashMap" => "hash_map",
                    "BTreeMap" => "btree_map",
                    "BinaryHeap" => "heap",
                    "HashSet" => "hash_set",
                    "BTreeSet" => "btree_set",
                    _ => "other",
                }
            }
            _ => "other",
        }
    }

    pub(crate) fn replacement(&self, expression: &'tcx Expr<'tcx>, operation: &str) -> String {
        if let ExprKind::Binary(operator, left, _) = expression.kind {
            return method(match operator.node {
                BinOpKind::Eq | BinOpKind::Ne
                    if self.replacement_kind(self.expr_ty(left)) == "bytes" =>
                {
                    "equal_bytes"
                }
                BinOpKind::Eq | BinOpKind::Ne => "equal",
                _ => "compare",
            });
        }
        if matches!(operation, "for loop" | "loop") {
            return fallback(operation);
        }
        let Some((definition, operands)) = self.call(expression) else {
            return fallback(operation);
        };
        let name = self.tcx.item_name(definition);
        let kind = operands
            .first()
            .map_or("other", |value| self.replacement_kind(self.expr_ty(value)));
        let result = self.replacement_kind(self.expr_ty(expression));
        let map = matches!(kind, "hash_map" | "btree_map");
        let set = matches!(kind, "hash_set" | "btree_set");
        if kind == "text" {
            match name.as_str() {
                "push" | "write_char" => return method("push_retained_char"),
                "push_str" | "write_str" => return method("append_retained"),
                "with_capacity" => return method("retained_string"),
                "insert" => {
                    return "DecodeContext::replace_text_range over index..index with value.encode_utf8(&mut [0; 4])".to_owned()
                }
                "extend" => {
                    return "DecodeContext::admit_iter followed by DecodeContext::push_retained_char for char/&char items or DecodeContext::append_retained for &str/String/Cow<str>/Box<str> items".to_owned()
                }
                _ => (),
            }
        }
        if map
            && matches!(
                name.as_str(),
                "get"
                    | "get_mut"
                    | "contains_key"
                    | "remove"
                    | "remove_entry"
                    | "get_key_value"
                    | "entry"
                    | "insert"
            )
        {
            let stem = match name.as_str() {
                "get_mut" => "get_mut",
                "contains_key" => "contains_key",
                "remove_entry" => "remove_entry",
                "get_key_value" => "get_key_value",
                "entry" => "entry",
                "insert" => "insert",
                "remove" => "remove",
                _ => "get",
            };
            return method(&format!("{stem}_{kind}"));
        }
        if set
            && matches!(
                name.as_str(),
                "get" | "contains" | "remove" | "insert" | "is_subset" | "is_disjoint"
            )
        {
            return method(&format!("{}_{kind}", name.as_str()));
        }
        if kind == "heap" && matches!(name.as_str(), "pop" | "push") {
            return method(if name.as_str() == "pop" {
                "pop_heap"
            } else {
                "push_heap"
            });
        }
        match name.as_str() {
            "contains" if kind == "text" => method("contains_text"),
            "contains" if matches!(kind, "slice" | "bytes" | "vector") => method("contains"),
            "find" if kind == "text" => method("find_text"),
            "rfind" if kind == "text" => method("rfind_text"),
            "find" | "rfind" if kind == "bytes" => method(if name.as_str() == "find" { "find_bytes" } else { "rfind_bytes" }),
            "any" | "all" | "find" | "find_map" | "position" | "rposition" | "count" | "fold" | "try_fold" | "sum" | "min" | "max" | "min_by" | "max_by" | "min_by_key" | "max_by_key" if matches!(kind, "slice" | "bytes" | "vector") => {
                let operation = match name.as_str() {
                    "any" => "any_by", "all" => "all_by", "find" => "find_by", "position" => "position_by",
                    "rposition" => "rposition_by", "try_fold" => "fold", other => other,
                };
                method(operation)
            }
            "clone" | "clone_from" | "to_owned" | "into_owned" | "from" | "into" if kind == "text" || result == "text" => method("copy_retained_text"),
            "to_vec" | "to_owned" | "clone" if kind == "bytes" => method("copy_slice"),
            "to_vec" | "clone" | "to_owned" if matches!(kind, "slice" | "vector") => "DecodeContext::collect_vec with charged child construction; use DecodeContext::copy_slice for Copy elements".to_owned(),
            "collect" | "from_iter" if result == "text" => method("collect_text"),
            "with_capacity" if result == "text" => method("retained_string"),
            "collect" | "from_iter" if matches!(result, "hash_map" | "btree_map" | "hash_set" | "btree_set") => method(match result {
                "hash_map" => "collect_hash_map", "hash_set" => "collect_hash_set", "btree_set" => "collect_btree_set", _ => "collect_scoped_btree_map",
            }),
            "eq" | "ne" if kind == "bytes" => method("equal_bytes"),
            "eq" | "ne" => method("equal"),
            "cmp" | "partial_cmp" | "lt" | "le" | "gt" | "ge" => method("compare"),
            _ => fallback(name.as_str()),
        }
    }
}

fn method(name: &str) -> String {
    format!("DecodeContext::{name}")
}

pub(crate) fn fallback(name: &str) -> String {
    let operation = match name {
        "for loop" | "any" | "all" | "find" | "find_map" | "position" | "rposition" | "nth" | "last" | "count" | "fold" | "try_fold" | "reduce" | "sum" | "for_each" | "try_for_each" => "admit_iter",
        "loop" => "charge_work",
        "format!" | "format" | "to_string" | "write_str" | "Display output extent unresolved" => "format_retained",
        "from_utf8" | "to_str" => "validate_utf8",
        "from_utf8_lossy" => "copy_retained_lossy_utf8",
        "from_utf16_lossy" => "utf16le_lossy_text",
        "parse" | "from_str" => "parse_text",
        "from_str_radix" => "parse_radix",
        "eq_ignore_ascii_case" => "eq_ignore_ascii_case",
        "hash" => "hash_value",
        "binary_search" => "binary_search", "binary_search_by" => "binary_search_by", "binary_search_by_key" => "binary_search_by_key",
        "partition_point" => "partition_point",
        "sort" | "sort_by" => "stable_sort_by", "sort_by_key" => "stable_sort_by_key",
        "sort_unstable" | "sort_unstable_by" => "sort_unstable_by", "sort_unstable_by_key" => "sort_unstable_by_key",
        "trim" => "trim_text", "trim_start" => "trim_start_text", "trim_end" => "trim_end_text",
        "trim_matches" => "trim_matches", "trim_start_matches" => "trim_start_matches", "trim_end_matches" => "trim_end_matches", "trim_ascii_end" => "trim_ascii_end",
        "strip_prefix" => "strip_prefix", "strip_suffix" => "strip_suffix", "starts_with" => "starts_with", "ends_with" => "ends_with",
        "split_once" => "split_once", "rsplit_once" => "rsplit_once",
        "is_ascii" => "is_ascii", "make_ascii_lowercase" => "make_ascii_lowercase", "make_ascii_uppercase" => "make_ascii_uppercase",
        "to_ascii_lowercase" => "to_ascii_lowercase", "to_ascii_uppercase" => "to_ascii_uppercase", "to_lowercase" => "to_lowercase", "to_uppercase" => "to_uppercase",
        "replace" => "replace_text", "replace_range" => "replace_text_range",
        "push_str" => "append_retained", "join" => "join_retained",
        "collect" | "from_iter" | "vector collection storage reuse" | "owning vector iterator collection may move or copy" => "collect_vec",
        "cloned iterator child copies" => "copy_retained_strings",
        "push" => "push_vec",
        "extend" => "extend_vec", "extend_from_slice" => "extend_from_slice", "extend_from_within" => "extend_from_within", "append" => "append_vec",
        "copy_from_slice" => "copy_into", "copy_within" => "copy_within", "reverse" => "reverse", "rotate_left" => "rotate_left", "rotate_right" => "rotate_right",
        "retain" => "retain_vec", "retain_mut" => "retain_mut", "dedup" => "dedup_vec", "dedup_by" => "dedup_by", "dedup_by_key" => "dedup_by_key",
        "clear" => "clear_vec", "truncate" => "truncate_vec", "fill" => "fill", "fill_with" => "fill_with",
        "resize" | "resize child Clone" | "alloc_filled child Clone" | "alloc_filled reaches resize child Clone" => "resize_with",
        "resize_with" => "resize_with", "drain" => "drain_vec", "splice" => "splice_vec", "split_off" => "split_off_vec", "shrink_to_fit" => "shrink_vec",
        "into_boxed_slice" | "into_boxed_slice may shrink/reallocate: capacity equality unresolved" => "into_boxed_slice",
        "with_capacity" => "collection_vec", "derived Default" => "collect_indexed_vec",
        "attribute" | "has_tag_name" | "root_element" => "charge_work",
        "parse_with_options" => "parse_xml",
        "deserialize" | "deserialize_any" | "deserialize_map" | "from_value" => "parse_json",
        "serialize" | "to_value" | "to_writer" | "custom" | "end" => return "DecodeContext::parse_json_value for a value-tree decode; rebuild concrete owned fields with DecodeContext::collect_vec and DecodeContext::format_retained; custom Serde callbacks remain unproven".to_owned(),
        "unzip" => "unzip_vec",
        "decode" => return "DecodeContext::collection_vec for output storage and DecodeContext::charge_work for the checked input extent; use a slice decoder".to_owned(),
        "decompress" | "decompress_stream" | "lzma_decompress_with_options" => "begin_expand",
        "by_index" | "by_index_raw" => return "cadmpeg_container::ArchiveSnapshot::new followed by ArchiveSnapshot::open; DecodeContext::begin_expand admits decompression".to_owned(),
        "comparison" | "custom comparison work" => "equal",
        _ => return "DecodeContext::charge_work for the resolved operand extent and DecodeContext::reserve_scoped for checked temporary bytes; resolve the concrete implementation and retain unproven status until its bound is known".to_owned(),
    };
    method(operation)
}

pub(crate) fn diagnostic(message: &str) -> String {
    for shape in [
        "alloc_filled reaches resize child Clone",
        "alloc_filled child Clone",
        "resize child Clone",
        "into_boxed_slice may shrink/reallocate: capacity equality unresolved",
        "owning vector iterator collection may move or copy",
        "vector collection storage reuse",
        "cloned iterator child copies",
        "Display output extent unresolved",
        "derived Default",
        "custom comparison work",
    ] {
        if message.contains(shape) {
            return fallback(shape);
        }
    }
    let name = message.split([':', ' ']).next().unwrap_or("");
    fallback(name)
}
