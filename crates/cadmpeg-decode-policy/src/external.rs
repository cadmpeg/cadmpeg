// SPDX-License-Identifier: Apache-2.0
//! External costs. Operand indices include the method receiver at index zero.
use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::DefId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Allocation {
    None,
    Input(usize),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Work {
    Fixed,
    Receiver,
    Capacity,
    Argument(usize),
    Arguments(&'static [usize]),
    Iterator,
    TextCharacter,
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

fn borrowed_byte_cursor(tcx: TyCtxt<'_>, archive: Ty<'_>) -> bool {
    let ty::Adt(_, archive_args) = archive.peel_refs().kind() else {
        return false;
    };
    let Some(reader) = archive_args.types().next() else {
        return false;
    };
    let ty::Adt(cursor, cursor_args) = reader.peel_refs().kind() else {
        return false;
    };
    if !types::standard(tcx, cursor.did()) || tcx.item_name(cursor.did()).as_str() != "Cursor" {
        return false;
    }
    let Some(bytes) = cursor_args.types().next() else {
        return false;
    };
    let ty::Ref(_, bytes, _) = bytes.kind() else {
        return false;
    };
    match bytes.kind() {
        ty::Slice(element) | ty::Array(element, _) => {
            matches!(element.kind(), ty::Uint(ty::UintTy::U8))
        }
        _ => false,
    }
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
    let standard_string = receiver.is_some_and(|value| types::standard_string(tcx, value));
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
    if types::derived(tcx, definition) {
        let (allocation, work) = match name.as_str() {
            "clone" => (Allocation::Clone, Work::Receiver),
            "eq" | "ne" | "cmp" | "partial_cmp" => (Allocation::None, Work::Comparison),
            "hash" | "fmt" => (Allocation::None, Work::Receiver),
            "default" => (Allocation::Result, Work::Fixed),
            "deserialize" => (Allocation::Input(0), Work::Receiver),
            "serialize" => (Allocation::Input(0), Work::Receiver),
            _ => return None,
        };
        return Some(Summary {
            allocation,
            work,
            zero_operand: None,
            empty_operand: None,
        });
    }
    if !types::standard(tcx, definition) {
        let (allocation, work) = match (crate_name.as_str(), name.as_str()) {
            (
                "serde" | "serde_core",
                "deserialize" | "deserialize_any" | "deserialize_map" | "deserialize_option"
                | "deserialize_str" | "deserialize_string" | "deserialize_struct" | "next_value"
                | "next_value_seed" | "next_key" | "next_key_seed" | "next_element"
                | "next_element_seed",
            ) => (Allocation::Input(0), Work::Receiver),
            ("serde" | "serde_core", "custom" | "duplicate_field" | "missing_field") => {
                (Allocation::Input(0), Work::Argument(0))
            }
            ("serde" | "serde_core", "new" | "size_hint" | "is_human_readable") => {
                (Allocation::None, Work::Fixed)
            }
            (
                "serde" | "serde_core",
                "serialize"
                | "collect_seq"
                | "collect_map"
                | "collect_str"
                | "serialize_bytes"
                | "serialize_seq"
                | "serialize_map"
                | "serialize_str"
                | "serialize_some"
                | "serialize_field"
                | "serialize_entry"
                | "serialize_element"
                | "serialize_key"
                | "serialize_value"
                | "serialize_newtype_struct"
                | "serialize_newtype_variant",
            ) => (Allocation::Input(1), Work::Argument(1)),
            (
                "serde" | "serde_core",
                "end"
                | "skip_field"
                | "serialize_bool"
                | "serialize_char"
                | "serialize_f32"
                | "serialize_f64"
                | "serialize_i128"
                | "serialize_i16"
                | "serialize_i32"
                | "serialize_i64"
                | "serialize_i8"
                | "serialize_u128"
                | "serialize_u16"
                | "serialize_u32"
                | "serialize_u64"
                | "serialize_u8"
                | "serialize_none"
                | "serialize_struct"
                | "serialize_struct_variant"
                | "serialize_tuple"
                | "serialize_tuple_struct"
                | "serialize_tuple_variant"
                | "serialize_unit"
                | "serialize_unit_struct"
                | "serialize_unit_variant",
            ) => (Allocation::None, Work::Fixed),
            ("serde_json", "deserialize_any" | "deserialize" | "deserialize_map") => {
                (Allocation::Input(0), Work::Receiver)
            }
            ("serde_json", "custom") => (Allocation::Input(0), Work::Argument(0)),
            ("serde_json", "clone") => (Allocation::Clone, Work::Receiver),
            ("serde_json", "eq" | "ne" | "cmp" | "partial_cmp" | "lt" | "le" | "gt" | "ge") => {
                (Allocation::None, Work::Comparison)
            }
            ("serde_json", "index" | "index_mut") => (Allocation::None, Work::Argument(1)),
            ("roxmltree", "eq" | "ne") => (Allocation::None, Work::Fixed),
            ("serde_json", "fmt") => (Allocation::None, Work::Receiver),
            ("serde_json", "serialize" | "to_value" | "from_value" | "to_vec") => {
                (Allocation::Input(0), Work::Receiver)
            }
            ("serde_json", "to_writer") => (Allocation::Input(1), Work::Argument(1)),
            ("serde_json", "end" | "retain") => (Allocation::None, Work::Receiver),
            ("serde_json", "insert") => (Allocation::Growth, Work::Argument(1)),
            ("serde_json", "entry") => (Allocation::Input(1), Work::Argument(1)),
            ("serde_json", "contains_key" | "remove") => (Allocation::None, Work::Argument(1)),
            (
                "serde_json",
                "new" | "len" | "is_empty" | "is_object" | "keys" | "values" | "values_mut" | "key"
                | "from" | "from_f64" | "is_f64" | "pretty",
            ) => (Allocation::None, Work::Fixed),
            ("serde_value", "new") => (Allocation::None, Work::Fixed),
            ("serde_value", "to_value") => (Allocation::Input(0), Work::Receiver),
            ("roxmltree", "value" | "id" | "ancestors" | "next_siblings" | "next" | "default") => {
                (Allocation::None, Work::Fixed)
            }
            ("crc32fast", "new" | "finalize") => (Allocation::None, Work::Fixed),
            ("crc32fast", "update") => (Allocation::None, Work::Argument(1)),
            ("crc32fast", "hash") => (Allocation::None, Work::Argument(0)),
            ("digest", "new" | "finalize") => (Allocation::None, Work::Fixed),
            ("digest", "update") => (Allocation::None, Work::Argument(1)),
            ("digest", "digest") => (Allocation::None, Work::Argument(0)),
            ("base64", "decoded_len_estimate") => (Allocation::None, Work::Fixed),
            ("base64", "decode") => (Allocation::Input(1), Work::Argument(1)),
            ("base64", "decode_slice" | "encode_slice") => (Allocation::None, Work::Argument(1)),
            ("memchr", "memchr2") => (Allocation::None, Work::Argument(2)),
            ("memchr", "find" | "rfind") => (Allocation::None, Work::Arguments(&[0, 1])),
            ("memchr", "find_iter" | "rfind_iter") => (Allocation::None, Work::Argument(1)),
            ("memchr", "memchr_iter") => (Allocation::None, Work::Fixed),
            ("regex", "new") => (Allocation::Input(0), Work::Argument(0)),
            ("regex", "build") => (Allocation::Input(0), Work::Receiver),
            ("regex", "is_match") => (Allocation::None, Work::Argument(1)),
            ("regex", "dfa_size_limit" | "size_limit") => (Allocation::None, Work::Fixed),
            ("encoding_rs", "for_bom" | "new_decoder_without_bom_handling") => {
                (Allocation::None, Work::Fixed)
            }
            ("encoding_rs", "for_label") => (Allocation::None, Work::Argument(0)),
            ("encoding_rs", "decode" | "decode_without_bom_handling") => {
                (Allocation::Input(1), Work::Argument(1))
            }
            ("encoding_rs", "decode_to_utf8_without_replacement") => {
                (Allocation::None, Work::Argument(1))
            }
            ("rmp", "from_u8" | "to_u8") => (Allocation::None, Work::Fixed),
            ("flate2", "new" | "total_in" | "total_out") => (Allocation::None, Work::Fixed),
            ("flate2", "decompress" | "read") => (Allocation::None, Work::Argument(1)),
            ("lzma_rs", "lzma_decompress_with_options") => {
                (Allocation::Input(0), Work::Argument(0))
            }
            ("zstd_safe", "decompress_stream") => (Allocation::Input(2), Work::Argument(2)),
            ("zstd_safe", "find_frame_compressed_size") => (Allocation::None, Work::Argument(0)),
            ("zstd_safe", "try_create" | "around" | "pos" | "set_parameter" | "get_error_name") => {
                (Allocation::None, Work::Fixed)
            }
            ("zstd_sys", "ZSTD_getErrorCode") => (Allocation::None, Work::Fixed),
            ("zip", "new") if path.contains("ZipArchive") => {
                (Allocation::Input(0), Work::Argument(0))
            }
            ("zip", "by_index") => (Allocation::Input(0), Work::Receiver),
            // zip 8.6 borrows indexed metadata and reads only the fixed local header.
            ("zip", "by_index_raw")
                if value.is_some_and(|value| borrowed_byte_cursor(tcx, value)) =>
            {
                (Allocation::None, Work::Fixed)
            }
            ("zip", "name_for_index") => (Allocation::None, Work::Fixed),
            ("zip", "start_file") => (Allocation::Input(1), Work::Argument(1)),
            ("zip", "finish") => (Allocation::Input(0), Work::Receiver),
            (
                "zip",
                "len"
                | "file_names"
                | "central_directory_start"
                | "central_header_start"
                | "compressed_size"
                | "compression"
                | "crc32"
                | "data_start"
                | "encrypted"
                | "header_start"
                | "name"
                | "size"
                | "get_metadata"
                | "default"
                | "new"
                | "compression_method"
                | "last_modified_time",
            ) => (Allocation::None, Work::Fixed),
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
        "eq" if path.ends_with("::ptr::eq") => (Allocation::None, Work::Fixed),
        "add" | "sub" | "mul" | "div" | "rem" | "neg" | "not" | "bitand" | "bitor" | "bitxor"
        | "shl" | "shr" => {
            if value.is_some_and(|value| {
                matches!(
                    value.kind(),
                    ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_)
                )
            }) || path.contains("num::nonzero")
            {
                (Allocation::None, Work::Fixed)
            } else {
                (Allocation::Result, Work::Receiver)
            }
        }
        "from_str_radix" if path.contains("num::") => (Allocation::None, Work::Argument(0)),
        // Keep the fixed summary scoped to the standard range receiver; a
        // custom method named `end` may allocate or inspect input.
        "end"
            if path.contains("RangeInclusive")
                && owner.is_some_and(|owner| owner.as_str() == "RangeInclusive")
                && value.is_some_and(|value| {
                    matches!(value.kind(), ty::Adt(range, _) if types::standard(tcx, range.did()))
                }) =>
        {
            (Allocation::None, Work::Fixed)
        }
        _ if (path.contains("num::<impl ")
            || path.contains("f32::<impl f32>")
            || path.contains("f64::<impl f64>")
            || path.contains("char::methods::<impl char>"))
            && matches!(
                name.as_str(),
                "abs"
                    | "abs_diff"
                    | "acos"
                    | "acosh"
                    | "asin"
                    | "asinh"
                    | "atan"
                    | "atan2"
                    | "atanh"
                    | "cast_signed"
                    | "cast_unsigned"
                    | "ceil"
                    | "checked_abs"
                    | "checked_add"
                    | "checked_div"
                    | "checked_ilog10"
                    | "checked_ilog2"
                    | "checked_mul"
                    | "checked_neg"
                    | "checked_next_power_of_two"
                    | "checked_pow"
                    | "checked_shl"
                    | "checked_sub"
                    | "clamp"
                    | "copysign"
                    | "cos"
                    | "cosh"
                    | "count_ones"
                    | "decode_utf16"
                    | "div_ceil"
                    | "div_euclid"
                    | "encode_utf16"
                    | "encode_utf8"
                    | "eq_ignore_ascii_case"
                    | "escape_debug"
                    | "exp"
                    | "exp_m1"
                    | "floor"
                    | "fract"
                    | "from"
                    | "from_be_bytes"
                    | "from_bits"
                    | "from_digit"
                    | "from_le_bytes"
                    | "from_ne_bytes"
                    | "from_str_radix"
                    | "from_u32"
                    | "hypot"
                    | "ilog10"
                    | "ilog2"
                    | "is_alphabetic"
                    | "is_alphanumeric"
                    | "is_ascii"
                    | "is_ascii_alphabetic"
                    | "is_ascii_alphanumeric"
                    | "is_ascii_control"
                    | "is_ascii_digit"
                    | "is_ascii_graphic"
                    | "is_ascii_hexdigit"
                    | "is_ascii_lowercase"
                    | "is_ascii_uppercase"
                    | "is_ascii_whitespace"
                    | "is_control"
                    | "is_finite"
                    | "is_infinite"
                    | "is_multiple_of"
                    | "is_nan"
                    | "is_negative"
                    | "is_normal"
                    | "is_power_of_two"
                    | "is_sign_negative"
                    | "is_sign_positive"
                    | "is_whitespace"
                    | "isqrt"
                    | "leading_zeros"
                    | "len_utf16"
                    | "len_utf8"
                    | "ln"
                    | "log10"
                    | "max"
                    | "midpoint"
                    | "min"
                    | "mul_add"
                    | "next_down"
                    | "next_multiple_of"
                    | "next_up"
                    | "overflowing_add"
                    | "overflowing_sub"
                    | "powf"
                    | "powi"
                    | "recip"
                    | "rem_euclid"
                    | "rotate_left"
                    | "round"
                    | "round_ties_even"
                    | "signum"
                    | "sin"
                    | "sin_cos"
                    | "sinh"
                    | "sqrt"
                    | "tan"
                    | "tanh"
                    | "to_ascii_lowercase"
                    | "to_ascii_uppercase"
                    | "to_be_bytes"
                    | "to_bits"
                    | "to_degrees"
                    | "to_digit"
                    | "to_le_bytes"
                    | "to_lowercase"
                    | "to_ne_bytes"
                    | "to_radians"
                    | "to_uppercase"
                    | "total_cmp"
                    | "trailing_zeros"
                    | "trunc"
                    | "try_from"
                    | "unsigned_abs"
                    | "wrapping_add"
                    | "wrapping_shl"
                    | "wrapping_sub"
            ) =>
        {
            (Allocation::None, Work::Fixed)
        }
        "index" | "index_mut" if path.contains("ops::Index") => (Allocation::None, Work::Fixed),
        "decode_utf16" if path.contains("char::") => (Allocation::None, Work::Fixed),
        "max" if path.contains("cmp::Ord") => (Allocation::None, Work::Comparison),
        "entry" if path.contains("HashMap") || path.contains("BTreeMap") => {
            (Allocation::None, Work::Argument(1))
        }
        "new" if path.contains("num::NonZero") => (Allocation::None, Work::Fixed),
        "unzip" if path.contains("option::") => (Allocation::None, Work::Fixed),
        "discriminant_value" | "unreachable" | "discriminant" | "size_of_val" | "drop" => {
            (Allocation::None, Work::Fixed)
        }
        "parse" if path.contains("str::") => (Allocation::Result, Work::Receiver),
        "eq_ignore_ascii_case" => (Allocation::None, Work::Comparison),
        "from_fn" if path.contains("array::") => (Allocation::None, Work::Fixed),
        "next" | "next_back" | "size_hint"
            if path.contains("iter::") || path.contains("Iterator") =>
        {
            (Allocation::None, Work::Fixed)
        }
        "clone_from" => (Allocation::Clone, Work::Argument(1)),
        "fmt"
            if path.contains("fmt::num")
                || value.is_some_and(|value| {
                    matches!(
                        value.kind(),
                        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_)
                    )
                }) =>
        {
            (Allocation::None, Work::Fixed)
        }
        "fmt" => (Allocation::None, Work::Receiver),
        "debug_struct" | "debug_tuple" | "debug_list" | "debug_set" | "debug_map" | "finish"
            if path.contains("fmt::") =>
        {
            (Allocation::None, Work::Fixed)
        }
        "field" if path.contains("DebugStruct") => (Allocation::None, Work::Argument(2)),
        "pad" if path.contains("fmt::") => (Allocation::None, Work::Argument(1)),
        "write_fmt" => (Allocation::None, Work::Argument(1)),
        "write" if path.contains("fmt::") => (Allocation::None, Work::Argument(1)),
        "write_char" if standard_string => (Allocation::Growth, Work::TextCharacter),
        "write_char" if path.contains("fmt::") => (Allocation::None, Work::Fixed),
        "first" | "last" if keyed => (Allocation::None, Work::Fixed),
        "first"
        | "last"
        | "first_mut"
        | "last_mut"
        | "first_chunk"
        | "last_chunk"
        | "split_first"
        | "split_first_mut"
        | "split_last"
        | "split_last_mut"
        | "split_first_chunk"
        | "as_chunks"
        | "as_flattened"
        | "split_at_checked"
        | "split_at_mut_checked"
        | "swap"
            if path.contains("slice::") || path.contains("str::") =>
        {
            (Allocation::None, Work::Fixed)
        }
        "strip_prefix" | "strip_suffix" => (Allocation::None, Work::Comparison),
        "bytes"
        | "chars"
        | "char_indices"
        | "encode_utf16"
        | "lines"
        | "split"
        | "rsplit"
        | "split_terminator"
        | "split_whitespace"
        | "split_ascii_whitespace"
        | "matches"
        | "match_indices"
            if path.contains("str::") || path.contains("slice::") =>
        {
            (Allocation::None, Work::Fixed)
        }
        "split_once" | "rsplit_once" | "trim_matches" | "trim_start_matches"
        | "trim_end_matches" | "trim_ascii_end" => (Allocation::None, Work::Receiver),
        "make_ascii_lowercase"
        | "make_ascii_uppercase"
        | "reverse"
        | "rotate_left"
        | "rotate_right"
        | "fill"
        | "partition_point"
        | "dedup"
        | "dedup_by"
        | "dedup_by_key"
        | "retain_mut"
            if path.contains("slice::") || path.contains("vec::") || path.contains("str::") =>
        {
            (Allocation::None, Work::Receiver)
        }
        "extend_from_within" | "replace_range" | "splice" => (Allocation::Growth, Work::Receiver),
        "split_off" => (Allocation::Result, Work::Receiver),
        "extract_if" => (Allocation::None, Work::Fixed),
        "shrink_to_fit" => (Allocation::Reallocate, Work::Receiver),
        "entry" | "get_key_value" | "remove_entry" if keyed => {
            (Allocation::None, Work::Argument(1))
        }
        "entry" if owner.is_some_and(|name| name.as_str() == "Map") => {
            (Allocation::None, Work::Argument(1))
        }
        "or_insert" | "or_insert_with" | "or_default" if path.contains("collections::") => {
            (Allocation::Input(0), Work::Fixed)
        }
        "pop" if path.contains("BinaryHeap") => (Allocation::None, Work::Receiver),
        "push" if path.contains("BinaryHeap") => (Allocation::Growth, Work::Receiver),
        "pop_first" | "difference" | "intersection" | "union" => (Allocation::None, Work::Fixed),
        "is_subset" | "is_disjoint" => (Allocation::None, Work::Receiver),
        "unzip" if path.contains("Iterator") => (Allocation::Collect, Work::Iterator),
        "last" | "nth" if path.contains("Iterator") => (Allocation::None, Work::Iterator),
        "new" if path.contains("io::BufWriter") => (Allocation::Result, Work::Fixed),
        "read" => (Allocation::None, Work::Argument(1)),
        "write_all" => (Allocation::None, Work::Argument(1)),
        "to_os_string" | "with_file_name" | "to_ascii_lowercase" | "to_ascii_uppercase" => {
            (Allocation::Result, Work::Receiver)
        }
        "from_utf16_lossy" => (Allocation::Result, Work::Receiver),
        "to_str" => (Allocation::None, Work::Receiver),
        "make_mut" if path.contains("Arc") => (Allocation::Clone, Work::Receiver),
        "other" if path.contains("io::error") => (Allocation::Result, Work::Fixed),
        "panic" | "panic_fmt" | "assert_failed" => (Allocation::None, Work::Fixed),
        "by_ref" | "cycle" | "map_while" | "scan" | "empty" | "once" | "repeat_n"
        | "repeat_with" | "successors" | "from_fn" | "peek" | "pop" | "pop_front"
        | "swap_remove" | "remainder" | "keys" | "values" | "values_mut" | "into_keys"
        | "into_values" | "into_vec" | "key" | "into_mut" | "and_modify" | "identity"
        | "each_ref" | "from_ref" | "from_mut" | "from_raw" | "as_ptr" | "leak" | "into_inner"
        | "into_bytes" | "set" | "get_or_init" | "get_or_insert" | "get_or_insert_with"
        | "as_deref" | "as_deref_mut" | "map_or" | "map_or_else" | "unwrap_or_default"
        | "unwrap" | "is_ok_and" | "is_break" | "start" | "extension" | "file_name"
        | "file_stem" | "strong_count" | "fetch_add" | "with" | "then_with" | "is_eq" | "is_gt"
        | "is_le" | "is_lt" | "valid_up_to" | "error_len" | "kind" | "rewind" | "seek"
        | "stream_position" | "flush" => (Allocation::None, Work::Fixed),
        "call" | "call_mut" | "call_once" if path.contains("ops::") => {
            (Allocation::None, Work::Fixed)
        }
        "lt" | "le" | "gt" | "ge" | "ne" => (Allocation::None, Work::Comparison),
        "from_le_bytes" | "from_be_bytes" | "from_ne_bytes" | "to_le_bytes" | "to_be_bytes"
        | "to_ne_bytes"
            if path.contains("num::") =>
        {
            (Allocation::None, Work::Fixed)
        }
        "min" | "max" | "clamp" | "reverse" if path.contains("cmp::") => {
            if value.is_some_and(|value| {
                matches!(value.kind(), ty::Adt(owner, _)
                if !types::standard(tcx, owner.did()))
            }) {
                return None;
            }
            (Allocation::None, Work::Comparison)
        }
        "finish" if path.contains("hash::") => (Allocation::None, Work::Fixed),
        "write_str" if standard_string => (Allocation::Growth, Work::Argument(1)),
        "write_str" if path.contains("fmt::") => (Allocation::None, Work::Argument(1)),
        "debug_struct_field1_finish" if path.contains("fmt::") => {
            (Allocation::None, Work::Argument(3))
        }
        "replace" | "take" | "swap" if path.contains("mem::") => (Allocation::None, Work::Fixed),
        "replace" | "take" if path.contains("option::") => (Allocation::None, Work::Fixed),
        "repeat" if path.contains("iter::") => (Allocation::None, Work::Fixed),
        "from_utf8_lossy" => (Allocation::Result, Work::Receiver),
        "insert" if path.contains("option::") => (Allocation::None, Work::Fixed),
        "format" => (Allocation::Format, Work::Format),
        "clone" => (Allocation::Clone, Work::Receiver),
        "cloned" => (Allocation::Cloned, Work::Fixed),
        "to_string" | "to_owned" => (Allocation::Result, Work::Receiver),
        "to_vec" => (Allocation::Collect, Work::Receiver),
        "collect" | "from_iter" => (Allocation::Collect, Work::Iterator),
        "from" | "into" | "into_owned" => (Allocation::Conversion, Work::Conversion),
        "from_elem" | "repeat" => (Allocation::Repeat, Work::Repeat),
        "with_capacity" | "with_capacity_in" => (Allocation::Capacity, Work::Fixed),
        "into_boxed_slice" => (Allocation::Reallocate, Work::Receiver),
        "push" if standard_string => (Allocation::Growth, Work::TextCharacter),
        "push" | "push_back" | "push_front" | "reserve" | "reserve_exact" | "try_reserve"
        | "try_reserve_exact" => (Allocation::Growth, Work::Fixed),
        "push_str" | "extend" | "extend_from_slice" | "append" => {
            (Allocation::Growth, Work::Argument(1))
        }
        "retain" if owner.is_some_and(|owner| matches!(owner.as_str(), "HashMap" | "HashSet")) => {
            (Allocation::None, Work::Capacity)
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
        "new"
            if ![
                "vec::Vec",
                "string::String",
                "collections::",
                "boxed::Box",
                "rc::Rc",
                "sync::Arc",
                "cell::",
                "sync::",
                "io::Cursor",
                "path::Path",
                "ffi::OsStr",
                "hash::",
                "ops::RangeInclusive",
                "fmt::Arguments",
                "fmt::rt",
                "fmt::Argument",
            ]
            .iter()
            .any(|owner| path.contains(owner)) =>
        {
            return None
        }
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
        "push_str" | "write_str" | "extend" | "extend_from_slice" => Some(1),
        _ => None,
    };
    Some(Summary {
        allocation,
        work,
        zero_operand,
        empty_operand,
    })
}

pub(crate) fn inventory_row(
    tcx: TyCtxt<'_>,
    definition: DefId,
    receiver: Option<Ty<'_>>,
) -> String {
    let path = tcx.def_path_str(definition);
    match summary(tcx, definition, receiver) {
        Some(cost) => format!(
            "external_operation\t{path}\t{:?}\t{:?}",
            cost.allocation, cost.work
        ),
        None => format!("external_operation\t{path}\tMISSING\tMISSING"),
    }
}
