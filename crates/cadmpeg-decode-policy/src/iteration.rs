// SPDX-License-Identifier: Apache-2.0
//! Generic core collectors defer source-step obligations to their callers.
use crate::{external, types, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Ty, TyCtxt};

#[derive(Clone, Copy, PartialEq, Eq)]
enum StepCost {
    Fixed,
    One,
    Multiple,
    Unknown,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn core_iterator_types(
        &self,
        definition: rustc_span::def_id::DefId,
        args: ty::GenericArgsRef<'tcx>,
    ) -> Vec<Ty<'tcx>> {
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core" {
            return Vec::new();
        }
        self.tcx
            .clauses_of(definition)
            .instantiate(self.tcx, args)
            .clauses
            .iter()
            .filter_map(|clause| match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate)
                    if types::standard(self.tcx, predicate.trait_ref.def_id)
                        && matches!(
                            self.tcx.item_name(predicate.trait_ref.def_id).as_str(),
                            "Iterator" | "IntoIterator"
                        ) =>
                {
                    Some(predicate.trait_ref.self_ty())
                }
                _ => None,
            })
            .collect()
    }

    fn core_iter_source_types(
        &self,
        definition: rustc_span::def_id::DefId,
        args: ty::GenericArgsRef<'tcx>,
    ) -> Vec<Ty<'tcx>> {
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core" {
            return Vec::new();
        }
        self.tcx
            .clauses_of(definition)
            .instantiate(self.tcx, args)
            .clauses
            .iter()
            .filter_map(|clause| match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate)
                    if self.core_iter_source_trait(predicate.trait_ref.def_id) =>
                {
                    Some(predicate.trait_ref.self_ty())
                }
                _ => None,
            })
            .collect()
    }

    fn core_iter_source_trait(&self, definition: rustc_span::def_id::DefId) -> bool {
        types::physical_item_path(
            self.tcx,
            definition,
            "cadmpeg_core",
            &["decode", "iter_source", "IterSource"],
        )
    }

    fn core_iterator_parameter(&self, value: Ty<'tcx>) -> bool {
        let owner = self.typeck.hir_owner.def_id.to_def_id();
        let sources =
            self.core_iterator_types(owner, ty::GenericArgs::identity_for_item(self.tcx, owner));
        let value = value.peel_refs();
        sources.contains(&value)
            || match value.kind() {
                ty::Alias(_, alias) => match alias.kind {
                    ty::AliasTyKind::Projection { def_id }
                        if self.tcx.item_name(def_id).as_str() == "IntoIter" =>
                    {
                        alias
                            .args
                            .types()
                            .next()
                            .is_some_and(|source| sources.contains(&source))
                    }
                    _ => false,
                },
                _ => false,
            }
    }

    pub(crate) fn checked_iterator_callback(&self, callback: Ty<'tcx>) -> bool {
        match callback.peel_refs().kind() {
            ty::Closure(id, _) => {
                !types::standard(self.tcx, *id) && self.checked_body(*id)
            }
            ty::FnDef(id, _) => {
                matches!(self.tcx.def_kind(*id), rustc_hir::def::DefKind::Ctor(_, _))
                    || !types::standard(self.tcx, *id) && self.checked_body(*id)
                    || external::summary(self.tcx, *id, None)
                        .is_some_and(|cost| cost.work == external::Work::Fixed)
            }
            ty::Param(_) => self.provider_callback_parameter(callback),
            _ => false,
        }
    }

    pub(crate) fn bounded_precharged_chars_any(
        &self,
        definition: rustc_span::def_id::DefId,
        receiver: Ty<'tcx>,
        callback: Ty<'tcx>,
    ) -> bool {
        if !types::physical_item_path(
            self.tcx,
            definition,
            "core",
            &["iter", "traits", "iterator", "Iterator", "any"],
        ) {
            return false;
        }
        let receiver = types::reveal_opaque(self.tcx, receiver.peel_refs());
        let ty::Adt(owner, arguments) = receiver.kind() else {
            return false;
        };
        if !self.is_core_admitted_iter(owner.did()) {
            return false;
        }
        let mut arguments = arguments.types();
        let Some(source) = arguments.next() else {
            return false;
        };
        let Some(mode) = arguments.next() else {
            return false;
        };
        matches!(source.kind(), ty::Adt(chars, _)
            if types::physical_item_path(self.tcx, chars.did(), "core", &["str", "iter", "Chars"]))
            && self.is_core_precharged_mode(mode)
            && self.iterator_step_cost(source) == StepCost::One
            && self.fixed_character_predicate(callback)
    }

    pub(crate) fn bounded_raw_chars_any(
        &self,
        definition: rustc_span::def_id::DefId,
        receiver: Ty<'tcx>,
        callback: Ty<'tcx>,
    ) -> bool {
        if !types::physical_item_path(
            self.tcx,
            definition,
            "core",
            &["iter", "traits", "iterator", "Iterator", "any"],
        ) {
            return false;
        }
        let receiver = self.normalized_iterator_type(receiver.peel_refs());
        let receiver = types::reveal_opaque(self.tcx, receiver);
        matches!(receiver.kind(), ty::Adt(chars, _)
            if types::physical_item_path(self.tcx, chars.did(), "core", &["str", "iter", "Chars"]))
            && self.iterator_step_cost(receiver) == StepCost::One
            && self.fixed_character_predicate(callback)
    }

    fn fixed_character_predicate(&self, callback: Ty<'tcx>) -> bool {
        let instance = match callback.peel_refs().kind() {
            ty::Closure(definition, arguments) => Some(ty::Instance {
                def: ty::InstanceKind::Item(*definition),
                args: arguments,
            }),
            ty::FnDef(definition, arguments) => arguments
                .no_bound_vars()
                .and_then(|arguments| {
                    ty::Instance::try_resolve(
                        self.tcx,
                        self.typing_env(),
                        *definition,
                        arguments,
                    )
                    .ok()
                    .flatten()
                }),
            _ => None,
        };
        let Some(instance) = instance else {
            return false;
        };
        if !self.tcx.is_mir_available(instance.def_id())
            || crate::conversion::cyclic(self.tcx.instance_mir(instance.def))
        {
            return false;
        }
        let body = self.tcx.instance_mir(instance.def);
        let mut calls = 0_usize;
        for block in body.basic_blocks.iter() {
            if block.statements.iter().any(|statement| {
                matches!(
                    &statement.kind,
                    rustc_middle::mir::StatementKind::Intrinsic(_)
                )
            }) {
                return false;
            }
            if let rustc_middle::mir::TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } = &block.terminator().kind
            {
                let ty::FnDef(definition, _) = func.ty(body, self.tcx).kind() else {
                    return false;
                };
                if !core_char_whitespace_method(self.tcx, *definition)
                    || args.len() != 1
                    || !args
                        .first()
                        .is_some_and(|argument| matches!(argument.node.ty(body, self.tcx).kind(), ty::Char))
                    || !matches!(destination.ty(body, self.tcx).ty.kind(), ty::Bool)
                {
                    return false;
                }
                calls = match calls.checked_add(1) {
                    Some(calls) => calls,
                    None => return false,
                };
            } else if matches!(
                &block.terminator().kind,
                rustc_middle::mir::TerminatorKind::Drop { .. }
                    | rustc_middle::mir::TerminatorKind::InlineAsm { .. }
                    | rustc_middle::mir::TerminatorKind::TailCall { .. }
            ) {
                return false;
            }
        }
        calls == 1
    }

    fn bounded_iterator_steps(&self, value: Ty<'tcx>) -> bool {
        let value = types::reveal_opaque(self.tcx, value.peel_refs());
        if self.core_iterator_parameter(value) {
            return true;
        }
        matches!(self.iterator_step_cost(value), StepCost::Fixed | StepCost::One)
    }

    pub(crate) fn bounded_iterator_step(&self, value: Ty<'tcx>) -> bool {
        matches!(
            self.iterator_step_cost(value),
            StepCost::Fixed | StepCost::One
        )
    }

    pub(crate) fn bounded_iterator_shape(&self, value: Ty<'tcx>) -> Option<types::Shape> {
        match self.iterator_step_cost(value) {
            StepCost::Fixed => Some(types::Shape::Fixed),
            StepCost::One => Some(types::Shape::Dynamic),
            StepCost::Multiple | StepCost::Unknown => None,
        }
    }

    fn iterator_step_cost(&self, value: Ty<'tcx>) -> StepCost {
        let value = self.normalized_iterator_type(value.peel_refs());
        let value = types::reveal_opaque(self.tcx, value);
        if self.core_iterator_parameter(value) {
            return StepCost::Unknown;
        }
        match value.kind() {
            ty::Array(..) | ty::Slice(_) | ty::Str => return StepCost::One,
            ty::Adt(owner, args) => {
                let name = self.tcx.item_name(owner.did());
                let path = self.tcx.def_path_str(owner.did());
                if self.is_core_admitted_iter(owner.did()) {
                    let mut arguments = args.types();
                    let Some(source) = arguments.next() else {
                        return StepCost::Unknown;
                    };
                    let Some(mode) = arguments.next() else {
                        return StepCost::Unknown;
                    };
                    if self.is_core_incremental_mode(mode) {
                        return self.iterator_step_cost(source);
                    }
                    return if self.is_core_precharged_mode(mode) {
                        StepCost::Fixed
                    } else {
                        StepCost::Unknown
                    };
                }
                if types::standard(self.tcx, owner.did()) {
                    let mut arguments = args.types();
                    return match name.as_str() {
                        "Option" | "Once" | "Empty" => StepCost::Fixed,
                        "IntoIter"
                            if path.contains("vec::")
                                || path.contains("array::")
                                || path.contains("option::")
                                || path.contains("result::") =>
                        {
                            StepCost::One
                        }
                        "Iter" | "IterMut"
                            if (path.contains("slice::")
                                || path.contains("vec::")
                                || path.contains("vec_deque::"))
                                && !path.contains("hash") =>
                        {
                            StepCost::One
                        }
                        "ToLowercase" | "ToUppercase"
                            if path.contains("char::") =>
                        {
                            StepCost::Fixed
                        }
                        "Bytes"
                            if types::physical_item_path(
                                self.tcx,
                                owner.did(),
                                "core",
                                &["str", "iter", "Bytes"],
                            ) =>
                        {
                            StepCost::One
                        }
                        "Chars"
                            if types::physical_item_path(
                                self.tcx,
                                owner.did(),
                                "core",
                                &["str", "iter", "Chars"],
                            ) =>
                        {
                            StepCost::One
                        }
                        "Range" | "RangeInclusive" | "RangeFrom"
                            if path.contains("ops::range")
                                && arguments.next().is_some_and(|element| {
                                    matches!(element.kind(), ty::Uint(_) | ty::Int(_) | ty::Char)
                                }) =>
                        {
                            StepCost::One
                        }
                        "Map" | "Inspect" => {
                            let source = arguments.next().map_or(StepCost::Unknown, |source| {
                                self.iterator_step_cost(source)
                            });
                            let callback = arguments
                                .next()
                                .is_some_and(|callback| self.checked_iterator_callback(callback));
                            if callback { source } else { StepCost::Unknown }
                        }
                        "Enumerate" | "Rev" | "Copied" | "Fuse" | "Peekable" | "Take" => {
                            arguments.next().map_or(StepCost::Unknown, |source| {
                                self.iterator_step_cost(source)
                            })
                        }
                        "Flatten" => {
                            let Some(source) = arguments.next() else {
                                return StepCost::Unknown;
                            };
                            if types::iteration(self.tcx, source) == types::Shape::Fixed {
                                return StepCost::Fixed;
                            }
                            if self.iterator_step_cost(source) != StepCost::One {
                                return StepCost::Unknown;
                            }
                            self.iterator_item(source)
                                .and_then(|item| self.fixed_array_item_length(item))
                                .filter(|length| *length > 0)
                                .map_or(StepCost::Unknown, |_| StepCost::One)
                        }
                        "Chain" => {
                            let left = arguments.next().map_or(StepCost::Unknown, |source| {
                                self.iterator_step_cost(source)
                            });
                            let right = arguments.next().map_or(StepCost::Unknown, |source| {
                                self.iterator_step_cost(source)
                            });
                            match (left, right) {
                                (StepCost::Unknown, _) | (_, StepCost::Unknown) => StepCost::Unknown,
                                (StepCost::Multiple, _) | (_, StepCost::Multiple) => {
                                    StepCost::Unknown
                                }
                                (StepCost::Fixed, StepCost::Fixed) => StepCost::Fixed,
                                _ => StepCost::One,
                            }
                        }
                        "Zip" => {
                            let left = arguments.next().map_or(StepCost::Unknown, |source| {
                                self.iterator_step_cost(source)
                            });
                            let right = arguments.next().map_or(StepCost::Unknown, |source| {
                                self.iterator_step_cost(source)
                            });
                            Self::combine_step_cost(left, right)
                        }
                        _ => StepCost::Unknown,
                    };
                }
                if types::physical_item_path(self.tcx, owner.did(), "roxmltree", &["Children"]) {
                    return StepCost::One;
                }
                StepCost::Unknown
            }
            _ => StepCost::Unknown,
        }
    }

    fn normalized_iterator_type(&self, value: Ty<'tcx>) -> Ty<'tcx> {
        self.tcx
            .try_normalize_erasing_regions(
                self.typing_env(),
                ty::Unnormalized::new_wip(types::reveal_opaque(self.tcx, value)),
            )
            .unwrap_or(value)
    }

    fn combine_step_cost(left: StepCost, right: StepCost) -> StepCost {
        match (left, right) {
            (StepCost::Unknown, _) | (_, StepCost::Unknown) => StepCost::Unknown,
            (StepCost::Multiple, _) | (_, StepCost::Multiple) => StepCost::Multiple,
            (StepCost::One, StepCost::One) => StepCost::Multiple,
            (StepCost::One, StepCost::Fixed) | (StepCost::Fixed, StepCost::One) => {
                StepCost::One
            }
            (StepCost::Fixed, StepCost::Fixed) => StepCost::Fixed,
        }
    }

    fn fixed_array_item_length(&self, value: Ty<'tcx>) -> Option<u64> {
        let value = match value.kind() {
            ty::Ref(_, inner, _) => *inner,
            _ => value,
        };
        let ty::Array(_, length) = value.kind() else {
            return None;
        };
        length.try_to_target_usize(self.tcx)
    }

    fn is_core_admitted_iter(&self, definition: rustc_span::def_id::DefId) -> bool {
        types::physical_item_path(
            self.tcx,
            definition,
            "cadmpeg_core",
            &["decode", "scan", "AdmittedIter"],
        )
    }

    fn is_core_incremental_mode(&self, value: Ty<'tcx>) -> bool {
        matches!(value.kind(), ty::Adt(definition, _)
            if types::physical_item_path(self.tcx, definition.did(), "cadmpeg_core",
                &["decode", "iter_source", "IncrementalAdmission"]))
    }

    fn is_core_precharged_mode(&self, value: Ty<'tcx>) -> bool {
        matches!(value.kind(), ty::Adt(definition, _)
            if types::physical_item_path(self.tcx, definition.did(), "cadmpeg_core",
                &["decode", "iter_source", "Precharged"]))
    }

    pub(crate) fn precharged_iterator_lineage(&self, value: Ty<'tcx>) -> bool {
        self.precharged_iterator_lineage_at(value, 0)
    }

    pub(crate) fn contains_precharged_iterator(&self, value: Ty<'tcx>) -> bool {
        self.contains_precharged_iterator_at(value, 0)
    }

    fn contains_precharged_iterator_at(&self, value: Ty<'tcx>, depth: usize) -> bool {
        if depth >= self.tcx.recursion_limit().0 {
            return false;
        }
        let value = types::reveal_opaque(self.tcx, value.peel_refs());
        let ty::Adt(owner, arguments) = value.kind() else {
            return false;
        };
        if self.is_core_admitted_iter(owner.did()) {
            return arguments
                .types()
                .nth(1)
                .is_some_and(|mode| self.is_core_precharged_mode(mode));
        }
        if !types::standard(self.tcx, owner.did()) {
            return false;
        }
        let name = self.tcx.item_name(owner.did());
        let arguments: Vec<_> = arguments.types().collect();
        let nested = |source| self.contains_precharged_iterator_at(source, depth + 1);
        match name.as_str() {
            "Map" | "Filter" | "FilterMap" | "MapWhile" | "Scan" | "TakeWhile"
            | "SkipWhile" | "Inspect" | "Enumerate" | "Rev" | "Take" | "Skip"
            | "Fuse" | "Peekable" | "Flatten" | "DecodeUtf16" => {
                arguments.first().is_some_and(|source| nested(*source))
            }
            "Chain" | "Zip" | "FlatMap" => arguments.iter().any(|source| nested(*source)),
            _ => false,
        }
    }

    fn precharged_iterator_lineage_at(&self, value: Ty<'tcx>, depth: usize) -> bool {
        if depth >= 64 {
            return false;
        }
        let value = types::reveal_opaque(self.tcx, value.peel_refs());
        let ty::Adt(owner, arguments) = value.kind() else {
            return false;
        };
        if self.is_core_admitted_iter(owner.did()) {
            return arguments
                .types()
                .nth(1)
                .is_some_and(|mode| self.is_core_precharged_mode(mode));
        }
        if !types::standard(self.tcx, owner.did()) {
            return false;
        }
        let name = self.tcx.item_name(owner.did());
        let mut arguments = arguments.types();
        let Some(source) = arguments.next() else {
            return false;
        };
        let nested = |source| self.precharged_iterator_lineage_at(source, depth + 1);
        match name.as_str() {
            "Map" | "Filter" | "FilterMap" | "MapWhile" | "Scan" | "TakeWhile"
            | "SkipWhile" | "Inspect" => {
                let Some(callback) = arguments.next() else {
                    return false;
                };
                nested(source)
                    && self.checked_iterator_callback(callback)
            }
            "Enumerate" | "Rev" | "Take" | "Skip" | "Fuse" | "Peekable" | "DecodeUtf16" => {
                nested(source) && self.bounded_iterator_step(value)
            }
            "Chain" => {
                let Some(other) = arguments.next() else {
                    return false;
                };
                (nested(source)
                    && (nested(other) || types::iteration(self.tcx, other) == types::Shape::Fixed)
                    || nested(other)
                        && types::iteration(self.tcx, source) == types::Shape::Fixed)
                    && self.bounded_iterator_step(value)
            }
            "Zip" => {
                let Some(other) = arguments.next() else {
                    return false;
                };
                (nested(source)
                    && (nested(other)
                        || self.bounded_iterator_step(other)))
                    || (nested(other)
                        && self.bounded_iterator_step(source))
            }
            "FlatMap" => {
                let Some(inner) = arguments.next() else {
                    return false;
                };
                let Some(callback) = arguments.next() else {
                    return false;
                };
                nested(source)
                    && self.checked_iterator_callback(callback)
                    && (types::iteration(self.tcx, inner) == types::Shape::Fixed
                        || self.iterator_step_cost(inner) == StepCost::Fixed)
            }
            "Flatten" => {
                nested(source)
                    && self
                        .iterator_item(source)
                        .is_some_and(|item| self.fixed_array_item_length(item).is_some())
            }
            _ => false,
        }
    }

    fn admitted_source_is_bounded(&self, value: Ty<'tcx>) -> bool {
        let value = types::reveal_opaque(self.tcx, value.peel_refs());
        let ty::Adt(owner, arguments) = value.kind() else {
            return match value.kind() {
                ty::Array(..) | ty::Slice(_) | ty::Str => true,
                _ => false,
            };
        };
        let name = self.tcx.item_name(owner.did());
        let path = self.tcx.def_path_str(owner.did());
        if types::physical_item_path(
            self.tcx,
            owner.did(),
            "cadmpeg_core",
            &["decode", "iter_source", "IncrementalSource"],
        ) {
            return arguments
                .types()
                .next()
                .is_some_and(|source| matches!(self.iterator_step_cost(source), StepCost::Fixed | StepCost::One));
        }
        if types::physical_item_path(self.tcx, owner.did(), "roxmltree", &["Children"]) {
            return true;
        }
        if types::physical_item_path(self.tcx, owner.did(), "serde_json", &["map", "Map"]) {
            return arguments.types().count() == 2
                && arguments.types().next().is_some_and(|key| {
                    matches!(key.kind(), ty::Adt(key, _)
                        if types::physical_item_path(self.tcx, key.did(), "alloc", &["string", "String"]))
                })
                && arguments.types().nth(1).is_some_and(|value| {
                    matches!(value.kind(), ty::Adt(value, _)
                        if types::physical_item_path(self.tcx, value.did(), "serde_json", &["value", "Value"]))
                });
        }
        if !types::standard(self.tcx, owner.did()) {
            return false;
        }
        match name.as_str() {
            "Vec" | "Option" | "VecDeque" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet"
            | "String" => true,
            "Range" | "RangeInclusive" => path.contains("ops::range"),
            _ => false,
        }
    }

    fn extend_source_is_bounded(&self, value: Ty<'tcx>) -> bool {
        let value = types::reveal_opaque(self.tcx, value.peel_refs());
        match value.kind() {
            ty::Array(..) | ty::Slice(_) => true,
            ty::Adt(owner, arguments) => {
                let name = self.tcx.item_name(owner.did());
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "cadmpeg_core",
                    &["decode", "iter_source", "IncrementalSource"],
                ) {
                    return arguments.types().next().is_some_and(|source| {
                        matches!(
                            self.iterator_step_cost(source),
                            StepCost::Fixed | StepCost::One
                        )
                    });
                }
                types::standard(self.tcx, owner.did())
                    && matches!(name.as_str(), "Vec" | "Option")
            }
            _ => false,
        }
    }

    fn owned_source_drop_callbacks_are_bounded(&self, value: Ty<'tcx>) -> Option<bool> {
        let value = types::reveal_opaque(self.tcx, value);
        match value.kind() {
            ty::Ref(..) => Some(true),
            ty::Array(element, _) => Some(self.drop_callbacks_are_bounded(*element, &mut Vec::new())),
            ty::Adt(owner, arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "cadmpeg_core",
                    &["decode", "iter_source", "IncrementalSource"],
                ) =>
            {
                Some(arguments.types().next().is_some_and(|iterator| {
                    self.drop_callbacks_are_bounded(iterator, &mut Vec::new())
                }))
            }
            ty::Adt(owner, arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "alloc",
                    &["vec", "Vec"],
                ) || types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "core",
                    &["option", "Option"],
                ) =>
            {
                Some(arguments.types().next().is_some_and(|item| {
                    self.drop_callbacks_are_bounded(item, &mut Vec::new())
                }))
            }
            ty::Adt(owner, arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "alloc",
                    &["collections", "btree", "map", "BTreeMap"],
                ) || types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "serde_json",
                    &["map", "Map"],
                ) =>
            {
                let items: Vec<_> = arguments.types().take(2).collect();
                Some(
                    items.len() == 2
                        && items
                            .into_iter()
                            .all(|item| self.drop_callbacks_are_bounded(item, &mut Vec::new())),
                )
            }
            _ => None,
        }
    }

    fn drop_callbacks_are_bounded(&self, value: Ty<'tcx>, seen: &mut Vec<Ty<'tcx>>) -> bool {
        let value = types::reveal_opaque(self.tcx, value);
        if seen.contains(&value) {
            // A recursive owned layout can run its drop glue once per nested
            // allocation. This proof does not bound that recursion.
            return false;
        }
        if seen.len() >= self.tcx.recursion_limit().0 {
            return false;
        }

        let depth = seen.len();
        seen.push(value);
        let bounded = match value.kind() {
            ty::Bool | ty::Char | ty::Float(_) | ty::Int(_) | ty::Uint(_) | ty::Str => true,
            ty::Ref(..) | ty::RawPtr(..) | ty::FnDef(..) | ty::FnPtr(..) => true,
            ty::Array(element, _) => self.drop_callbacks_are_bounded(*element, seen),
            ty::Tuple(fields) => fields
                .iter()
                .all(|field| self.drop_callbacks_are_bounded(field, seen)),
            ty::Closure(_, arguments) => arguments
                .as_closure()
                .upvar_tys()
                .iter()
                .all(|capture| self.drop_callbacks_are_bounded(capture, seen)),
            ty::Adt(owner, _arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "alloc",
                    &["string", "String"],
                ) =>
            {
                true
            }
            ty::Adt(owner, arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "core",
                    &["iter", "adapters", "chain", "Chain"],
                ) =>
            {
                let iterators: Vec<_> = arguments.types().take(2).collect();
                iterators.len() == 2
                    && iterators
                        .into_iter()
                        .all(|iterator| self.drop_callbacks_are_bounded(iterator, seen))
            }
            ty::Adt(owner, arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "core",
                    &["iter", "sources", "once", "Once"],
                ) =>
            {
                arguments
                    .types()
                    .next()
                    .is_some_and(|item| self.drop_callbacks_are_bounded(item, seen))
            }
            ty::Adt(owner, _arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "core",
                    &["slice", "iter", "Iter"],
                ) || types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "core",
                    &["slice", "iter", "IterMut"],
                ) =>
            {
                // Slice iterators own only pointers and a borrow marker. Their
                // destruction does not visit or drop the referenced elements.
                true
            }
            ty::Adt(owner, arguments)
                if types::standard(self.tcx, owner.did())
                    && (types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["boxed", "Box"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["rc", "Rc"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["sync", "Arc"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "core",
                        &["option", "Option"],
                    )) =>
            {
                arguments
                    .types()
                    .next()
                    .is_some_and(|item| self.drop_callbacks_are_bounded(item, seen))
            }
            // A nested dynamic collection drops its contents by walking its
            // own runtime-sized extent, which the outer source did not admit.
            ty::Adt(owner, _arguments)
                if (types::standard(self.tcx, owner.did())
                    && (types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["vec", "Vec"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["collections", "vec_deque", "VecDeque"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["collections", "linked_list", "LinkedList"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["collections", "binary_heap", "BinaryHeap"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "std",
                        &["collections", "hash", "map", "HashMap"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "std",
                        &["collections", "hash", "set", "HashSet"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["collections", "btree", "map", "BTreeMap"],
                    ) || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "alloc",
                        &["collections", "btree", "set", "BTreeSet"],
                    )))
                    || types::physical_item_path(
                        self.tcx,
                        owner.did(),
                        "serde_json",
                        &["map", "Map"],
                    ) => false,
            ty::Adt(owner, arguments)
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "alloc",
                    &["borrow", "Cow"],
                ) =>
            {
                arguments
                    .types()
                    .next()
                    .is_some_and(|item| self.drop_callbacks_are_bounded(item, seen))
            }
            ty::Adt(owner, _arguments) if owner.has_dtor(self.tcx) => false,
            ty::Adt(owner, arguments) => owner
                .all_fields()
                .all(|field| self.drop_callbacks_are_bounded(field.ty(self.tcx, arguments).skip_norm_wip(), seen)),
            _ => false,
        };
        seen.truncate(depth);
        bounded
    }

    fn core_extend_vec(&self, definition: rustc_span::def_id::DefId) -> bool {
        types::decode_context_method(self.tcx, definition, "extend_vec")
    }

    pub(crate) fn iterator_boundary(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        if types::physical_inherent_method(
            self.tcx,
            definition,
            "cadmpeg_core",
            &["decode", "iter_source", "IncrementalSource"],
            "new",
        ) {
            if operands.first().is_some_and(|source| {
                !self.bounded_iterator_step(self.expr_ty(source))
            }) {
                self.report(
                    expression.span,
                    "unproven_decode_charge",
                    "incremental source constructor requires a fixed or one-step producer; replacement: DecodeContext::admit_iter",
                );
            }
            return;
        }
        let Some(args) = self.call_arguments(expression) else {
            return;
        };
        let sources = self.core_iterator_types(definition, args);
        let admitted_sources = self.core_iter_source_types(definition, args);
        let source_owner = self.tcx.def_path_str(definition);
        if self.core_extend_vec(definition)
            && operands
                .get(2)
                .is_some_and(|source| !self.extend_source_is_bounded(self.expr_ty(source)))
        {
            self.report(
                expression.span,
                "unproven_decode_charge",
                &format!(
                    "{source_owner} source does not have a fixed or one-step producer bound"
                ),
            );
        }
        for operand in operands {
            let value = self.expr_ty(operand);
            if sources
                .iter()
                .any(|source| *source == value || *source == value.peel_refs())
                && !self.bounded_iterator_steps(value)
            {
                self.report(expression.span, "unproven_decode_charge", &format!("{source_owner} source can scan before yielding; admit its bounded base before applying adapters"));
            }
            if admitted_sources
                .iter()
                .any(|source| *source == value || *source == value.peel_refs())
            {
                if !self.admitted_source_is_bounded(value) {
                    self.report(expression.span, "unproven_decode_charge", &format!("{source_owner} IterSource can perform uncharged source work before yielding; use a precharged source or wrap a one-step iterator in IncrementalSource"));
                } else if self
                    .owned_source_drop_callbacks_are_bounded(value)
                    .is_some_and(|bounded| !bounded)
                {
                    self.report(expression.span, "unproven_decode_charge", &format!("{source_owner} owned source can drop an item with an unproven destructor callback"));
                }
            }
        }
    }

    pub(crate) fn core_iterator_metadata(
        &self,
        expression: &'tcx Expr<'tcx>,
        definition: rustc_span::def_id::DefId,
    ) -> bool {
        if !types::standard(self.tcx, definition)
            || !matches!(
                self.tcx.item_name(definition).as_str(),
                "into_iter" | "size_hint"
            )
        {
            return false;
        }
        self.call(expression)
            .and_then(|(_, args)| args.first().copied())
            .is_some_and(|receiver| self.core_iterator_parameter(self.expr_ty(receiver)))
    }

    pub(crate) fn core_iterator_next(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, operands)) = self.call(expression) else {
            return false;
        };
        if !types::standard(self.tcx, definition)
            || self.tcx.item_name(definition).as_str() != "next"
        {
            return false;
        }
        if !operands
            .first()
            .is_some_and(|receiver| self.core_iterator_parameter(self.expr_ty(receiver)))
        {
            return false;
        }
        let required = self.flow.iterations;
        let Some(term) = self
            .flow
            .work
            .iter_mut()
            .filter(|credit| !credit.opaque)
            .flat_map(|credit| &mut credit.extents)
            .find(|term| term.factors.is_empty() && term.coefficient >= required)
        else {
            return false;
        };
        term.coefficient -= required;
        self.record_key_work_proof(expression);
        true
    }
}

fn core_char_whitespace_method(tcx: TyCtxt<'_>, definition: rustc_span::def_id::DefId) -> bool {
    if tcx.crate_name(definition.krate).as_str() != "core"
        || tcx
            .opt_item_name(definition)
            .is_none_or(|name| name.as_str() != "is_whitespace")
        || !matches!(
            tcx.def_kind(tcx.parent(definition)),
            rustc_hir::def::DefKind::Impl { of_trait: false }
        )
    {
        return false;
    }
    let implementation = tcx.parent(definition);
    matches!(
        tcx.type_of(implementation)
            .instantiate_identity()
            .skip_norm_wip()
            .kind(),
        ty::Char
    )
}
