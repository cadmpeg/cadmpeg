// SPDX-License-Identifier: Apache-2.0
//! Deserialization admission is checked at its decode caller.
use crate::{types, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Ty, TyCtxt};

fn derived_tree<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> bool {
    let value = value.peel_refs();
    if seen.contains(&value) {
        return true;
    }
    let depth = seen.len();
    seen.push(value);
    let admitted = match value.kind() {
        ty::Adt(owner, args) if types::standard(tcx, owner.did()) => {
            args.types().all(|inner| derived_tree(tcx, inner, seen))
        }
        ty::Adt(owner, args) => {
            let derived = tcx
                .all_traits_including_private()
                .filter(|id| types::serde_deserialize(tcx, *id))
                .flat_map(|id| tcx.non_blanket_impls_for_ty(id, value))
                .filter(|id| {
                    matches!(tcx.type_of(*id).instantiate_identity().skip_norm_wip().kind(),
                    ty::Adt(definition, _) if definition.did() == owner.did())
                })
                .any(|id| tcx.is_automatically_derived(id));
            derived
                && owner
                    .all_fields()
                    .all(|field| derived_tree(tcx, field.ty(tcx, args).skip_norm_wip(), seen))
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
        let typed = owner.as_str() == "cadmpeg_core" && name.as_str() == "parse_json";
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
                .is_some_and(|value| derived_tree(self.tcx, value, &mut Vec::new()))
        {
            return false;
        }
        self.report(expression.span, "unproven_decode_charge",
            "deserialization requires DecodeContext::parse_json for a derived type tree or DecodeContext::parse_json_value for a value tree; custom Deserialize callbacks have no proven charge");
        true
    }
}
