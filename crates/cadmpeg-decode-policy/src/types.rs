// SPDX-License-Identifier: Apache-2.0
use rustc_middle::ty::{self, Ty, TyCtxt};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape { Fixed, Dynamic, Unknown }

impl Shape {
    pub(crate) fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Dynamic, _) | (_, Self::Dynamic) => Self::Dynamic,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            _ => Self::Fixed,
        }
    }
}

pub(crate) fn standard(tcx: TyCtxt<'_>, definition: rustc_span::def_id::DefId) -> bool {
    matches!(tcx.crate_name(definition.krate).as_str(), "alloc" | "core" | "std")
}

pub(crate) fn heap<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> Shape {
    if seen.contains(&value) { return Shape::Fixed; }
    seen.push(value);
    match value.kind() {
        ty::Adt(definition, arguments) => {
            let name = tcx.item_name(definition.did());
            if standard(tcx, definition.did()) {
                match name.as_str() {
                    "String" | "Vec" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" | "VecDeque" | "BinaryHeap" | "LinkedList" | "PathBuf" | "OsString" => return Shape::Dynamic,
                    "Rc" | "Arc" => return Shape::Fixed,
                    "Box" => return arguments.types().next().map_or(Shape::Unknown, |inner| match inner.kind() {
                        ty::Slice(_) | ty::Str => Shape::Dynamic,
                        _ => heap(tcx, inner, seen),
                    }),
                    _ => (),
                }
            }
            definition.all_fields().fold(Shape::Fixed, |shape, field| shape.join(heap(tcx, field.ty(tcx, arguments).skip_norm_wip(), seen)))
        }
        ty::Tuple(fields) => fields.iter().fold(Shape::Fixed, |shape, field| shape.join(heap(tcx, field, seen))),
        ty::Array(element, _) => heap(tcx, *element, seen),
        ty::Param(_) | ty::Alias(_, _) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => Shape::Unknown,
        _ => Shape::Fixed,
    }
}

pub(crate) fn has_context<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> bool {
    let value = value.peel_refs();
    if seen.contains(&value) { return false; }
    seen.push(value);
    match value.kind() {
        ty::Adt(definition, arguments) => {
            tcx.item_name(definition.did()).as_str() == "DecodeContext" || definition.all_fields().any(|field| has_context(tcx, field.ty(tcx, arguments).skip_norm_wip(), seen))
        }
        ty::Tuple(fields) => fields.iter().any(|field| has_context(tcx, field, seen)),
        _ => false,
    }
}
