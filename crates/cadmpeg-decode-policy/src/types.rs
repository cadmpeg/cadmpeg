// SPDX-License-Identifier: Apache-2.0
use rustc_middle::ty::{self, Ty, TyCtxt, TypeVisitableExt};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Fixed,
    Dynamic,
    Unknown,
}

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
    matches!(
        tcx.crate_name(definition.krate).as_str(),
        "alloc" | "core" | "std"
    )
}

pub(crate) fn checked(tcx: TyCtxt<'_>, definition: rustc_span::def_id::DefId) -> bool {
    let name = tcx.crate_name(definition.krate);
    name.as_str().starts_with("cadmpeg_codec_")
        || matches!(
            name.as_str(),
            "cadmpeg_core"
                | "cadmpeg_ir"
                | "cadmpeg_container"
                | "cadmpeg_asm"
                | "cadmpeg_parasolid"
                | "cadmpeg_protein"
        )
        || definition.is_local() && std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some()
}

pub(crate) fn slot_storage<'tcx>(tcx: TyCtxt<'tcx>, element: Ty<'tcx>) -> Shape {
    if element.has_aliases() || element.has_non_region_param() || element.has_escaping_bound_vars()
    {
        return Shape::Unknown;
    }
    match tcx.layout_of(ty::TypingEnv::fully_monomorphized().as_query_input(element)) {
        Ok(layout) if layout.size.bytes() == 0 => Shape::Fixed,
        Ok(_) => Shape::Dynamic,
        Err(_) => Shape::Unknown,
    }
}

pub(crate) fn heap<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> Shape {
    if seen.contains(&value) {
        return Shape::Unknown;
    }
    let depth = seen.len();
    seen.push(value);
    let shape = (|| match value.kind() {
        ty::Adt(definition, arguments) => {
            let name = tcx.item_name(definition.did());
            if standard(tcx, definition.did()) {
                match name.as_str() {
                    "Vec" | "VecDeque" | "BinaryHeap" => {
                        return arguments
                            .types()
                            .next()
                            .map_or(Shape::Unknown, |element| slot_storage(tcx, element));
                    }
                    "String" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" | "LinkedList"
                    | "PathBuf" | "OsString" => return Shape::Dynamic,
                    "Rc" | "Arc" => return Shape::Fixed,
                    "Box" => {
                        return arguments
                            .types()
                            .next()
                            .map_or(Shape::Unknown, |inner| match inner.kind() {
                                ty::Slice(element) => slot_storage(tcx, *element),
                                ty::Str => Shape::Dynamic,
                                _ => heap(tcx, inner, seen),
                            })
                    }
                    _ => (),
                }
            }
            definition.all_fields().fold(Shape::Fixed, |shape, field| {
                shape.join(heap(tcx, field.ty(tcx, arguments).skip_norm_wip(), seen))
            })
        }
        ty::Tuple(fields) => fields.iter().fold(Shape::Fixed, |shape, field| {
            shape.join(heap(tcx, field, seen))
        }),
        ty::Array(element, _) => {
            if value.has_aliases()
                || value.has_non_region_param()
                || value.has_escaping_bound_vars()
            {
                return Shape::Unknown;
            }
            match tcx.layout_of(ty::TypingEnv::fully_monomorphized().as_query_input(value)) {
                Ok(layout) if layout.size.bytes() == 0 => Shape::Fixed,
                Err(_) => Shape::Unknown,
                _ => heap(tcx, *element, seen),
            }
        }
        ty::Param(_) | ty::Alias(_, _) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => {
            Shape::Unknown
        }
        _ => Shape::Fixed,
    })();
    seen.truncate(depth);
    shape
}

pub(crate) fn has_context<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    seen: &mut Vec<Ty<'tcx>>,
) -> bool {
    let value = value.peel_refs();
    if seen.contains(&value) {
        return false;
    }
    seen.push(value);
    match value.kind() {
        ty::Adt(definition, arguments) => {
            if standard(tcx, definition.did())
                && matches!(
                    tcx.item_name(definition.did()).as_str(),
                    "Box"
                        | "Rc"
                        | "Arc"
                        | "Vec"
                        | "VecDeque"
                        | "BinaryHeap"
                        | "LinkedList"
                        | "HashMap"
                        | "HashSet"
                        | "BTreeMap"
                        | "BTreeSet"
                )
            {
                return arguments.types().any(|inner| has_context(tcx, inner, seen));
            }
            (tcx.item_name(definition.did()).as_str() == "DecodeContext"
                && (tcx.crate_name(definition.did().krate).as_str() == "cadmpeg_core"
                    || std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some()))
                || definition
                    .all_fields()
                    .any(|field| has_context(tcx, field.ty(tcx, arguments).skip_norm_wip(), seen))
        }
        ty::Tuple(fields) => fields.iter().any(|field| has_context(tcx, field, seen)),
        ty::Array(element, _) | ty::Slice(element) => has_context(tcx, *element, seen),
        _ => false,
    }
}

pub(crate) fn work<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> Shape {
    let value = value.peel_refs();
    if seen.contains(&value) {
        return Shape::Unknown;
    }
    let depth = seen.len();
    seen.push(value);
    let shape = (|| match value.kind() {
        ty::Str | ty::Slice(_) => Shape::Dynamic,
        ty::Param(_) | ty::Alias(..) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => {
            Shape::Unknown
        }
        ty::Array(element, _) => work(tcx, *element, seen),
        ty::Tuple(fields) => fields.iter().fold(Shape::Fixed, |shape, field| {
            shape.join(work(tcx, field, seen))
        }),
        ty::Adt(definition, arguments) => {
            let name = tcx.item_name(definition.did());
            if standard(tcx, definition.did()) && matches!(name.as_str(), "Box" | "Rc" | "Arc") {
                return arguments
                    .types()
                    .next()
                    .map_or(Shape::Unknown, |inner| work(tcx, inner, seen));
            }
            if standard(tcx, definition.did())
                && matches!(
                    name.as_str(),
                    "String"
                        | "Vec"
                        | "HashMap"
                        | "HashSet"
                        | "BTreeMap"
                        | "BTreeSet"
                        | "VecDeque"
                        | "BinaryHeap"
                        | "LinkedList"
                        | "PathBuf"
                        | "OsString"
                )
            {
                return Shape::Dynamic;
            }
            if name.as_str() == "View"
                && tcx.crate_name(definition.did().krate).as_str() == "cadmpeg_core"
            {
                return Shape::Dynamic;
            }
            definition.all_fields().fold(Shape::Fixed, |shape, field| {
                shape.join(work(tcx, field.ty(tcx, arguments).skip_norm_wip(), seen))
            })
        }
        _ => Shape::Fixed,
    })();
    seen.truncate(depth);
    shape
}

pub(crate) fn iteration(tcx: TyCtxt<'_>, value: Ty<'_>) -> Shape {
    let value = value.peel_refs();
    if matches!(value.kind(), ty::Array(_, _)) {
        return Shape::Fixed;
    }
    match value.kind() {
        ty::Str | ty::Slice(_) => Shape::Dynamic,
        ty::Adt(definition, _) => {
            let path = tcx.def_path_str(definition.did());
            let name = tcx.item_name(definition.did());
            if standard(tcx, definition.did())
                && matches!(name.as_str(), "Option" | "Result" | "Once" | "Empty")
            {
                return Shape::Fixed;
            }
            if standard(tcx, definition.did())
                && path.contains("array::")
                && name.as_str() == "IntoIter"
            {
                return Shape::Fixed;
            }
            if standard(tcx, definition.did())
                && (path.contains("slice::")
                    || path.contains("str::iter")
                    || path.contains("collections")
                    || path.contains("vec::")
                    || path.contains("range::"))
                || name.as_str() == "View"
                    && tcx.crate_name(definition.did().krate).as_str() == "cadmpeg_core"
            {
                return Shape::Dynamic;
            }
            Shape::Unknown
        }
        _ => Shape::Unknown,
    }
}

pub(crate) fn derived(tcx: TyCtxt<'_>, definition: rustc_span::def_id::DefId) -> bool {
    let generated = tcx.def_span(definition).macro_backtrace().any(|expansion| {
        matches!(
            expansion.kind,
            rustc_span::hygiene::ExpnKind::Macro(rustc_span::hygiene::MacroKind::Derive, _)
        )
    });
    let parent = tcx.parent(definition);
    generated
        || matches!(tcx.def_kind(parent), rustc_hir::def::DefKind::Impl { .. })
            && tcx.is_automatically_derived(parent)
}
