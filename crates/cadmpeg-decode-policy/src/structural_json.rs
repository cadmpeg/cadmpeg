// SPDX-License-Identifier: Apache-2.0
//! Source children of the concrete serde_json projection protocol.

use crate::types;
use rustc_middle::ty::{self, Ty, TyCtxt};

pub(crate) struct JsonChildren<'tcx> {
    pub charged_parent: bool,
    pub children: Vec<Ty<'tcx>>,
}

/// The library's Value protocol borrows every child. Arrays delegate to Vec;
/// objects declare their length before a full borrowed map traversal. The
/// physical fields reject the alternate number and map representations.
pub(crate) fn source_children<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
) -> Option<JsonChildren<'tcx>> {
    let ty::Adt(owner, arguments) = source.kind() else {
        return None;
    };
    let definition = owner.did();
    if types::physical_item_path(tcx, definition, "serde_json", &["value", "Value"]) {
        if !owner.is_enum() || owner.variants().len() != 6 || owner.has_dtor(tcx) {
            return None;
        }
        let mut children = Vec::new();
        for (variant, expected) in owner
            .variants()
            .iter()
            .zip(["Null", "Bool", "Number", "String", "Array", "Object"])
        {
            if variant.name.as_str() != expected
                || variant.fields.len() != usize::from(expected != "Null")
            {
                return None;
            }
            for field in &variant.fields {
                let child = field.ty(tcx, arguments).skip_norm_wip();
                let valid = match expected {
                    "Bool" => matches!(child.kind(), ty::Bool),
                    "Number" => physical_type(tcx, child, "serde_json", &["number", "Number"]),
                    "String" => physical_type(tcx, child, "alloc", &["string", "String"]),
                    "Array" => matches!(child.kind(), ty::Adt(child_owner, child_args)
                        if types::physical_item_path(tcx, child_owner.did(), "alloc", &["vec", "Vec"])
                            && child_args.types().next() == Some(source)),
                    "Object" => physical_type(tcx, child, "serde_json", &["map", "Map"]),
                    _ => false,
                };
                if !valid {
                    return None;
                }
                children.push(child);
            }
        }
        return Some(JsonChildren {
            charged_parent: false,
            children,
        });
    }
    if types::physical_item_path(tcx, definition, "serde_json", &["number", "Number"]) {
        if !owner.is_struct() || owner.has_dtor(tcx) || owner.all_fields().count() != 1 {
            return None;
        }
        let representation = owner
            .all_fields()
            .next()?
            .ty(tcx, arguments)
            .skip_norm_wip();
        let ty::Adt(number, number_args) = representation.kind() else {
            return None;
        };
        if !types::physical_item_path(tcx, number.did(), "serde_json", &["number", "N"])
            || !number.is_enum()
            || number.has_dtor(tcx)
            || number.variants().len() != 3
        {
            return None;
        }
        for (variant, (name, expected)) in number.variants().iter().zip([
            ("PosInt", tcx.types.u64),
            ("NegInt", tcx.types.i64),
            ("Float", tcx.types.f64),
        ]) {
            if variant.name.as_str() != name
                || variant.fields.len() != 1
                || variant
                    .fields
                    .iter()
                    .next()?
                    .ty(tcx, number_args)
                    .skip_norm_wip()
                    != expected
            {
                return None;
            }
        }
        return Some(JsonChildren {
            charged_parent: true,
            children: Vec::new(),
        });
    }
    if !types::physical_item_path(tcx, definition, "serde_json", &["map", "Map"])
        || !owner.is_struct()
        || owner.has_dtor(tcx)
        || owner.all_fields().count() != 1
    {
        return None;
    }
    let representation = owner
        .all_fields()
        .next()?
        .ty(tcx, arguments)
        .skip_norm_wip();
    let ty::Adt(map, map_args) = representation.kind() else {
        return None;
    };
    if !types::physical_item_path(
        tcx,
        map.did(),
        "alloc",
        &["collections", "btree", "map", "BTreeMap"],
    ) {
        return None;
    }
    let children: Vec<_> = map_args.types().take(2).collect();
    if children.len() != 2
        || !physical_type(tcx, children[0], "alloc", &["string", "String"])
        || !physical_type(tcx, children[1], "serde_json", &["value", "Value"])
    {
        return None;
    }
    Some(JsonChildren {
        charged_parent: true,
        children,
    })
}

fn physical_type(tcx: TyCtxt<'_>, value: Ty<'_>, crate_name: &str, path: &[&str]) -> bool {
    matches!(value.kind(), ty::Adt(owner, _)
        if types::physical_item_path(tcx, owner.did(), crate_name, path))
}
