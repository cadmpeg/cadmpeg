// SPDX-License-Identifier: Apache-2.0
//! Iterator traversal costs and the source-step obligations of core collectors.
use crate::{external, types, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Ty};

/// What a complete traversal costs beyond the work of checked callbacks.
/// The order runs from the strongest guarantee to the weakest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StepCost {
    /// A constant number of steps, each doing constant work.
    Constant,
    /// Every base visit was charged by `DecodeContext::admit_iter` before the
    /// first step, and each visit does constant work.
    Prepaid,
    /// Each step does constant work beyond prepaid visits, but the number of
    /// steps is not charged.
    One,
    /// A step can perform unbounded work before yielding.
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
            ty::Closure(id, _) => !types::standard(self.tcx, *id) && self.checked_body(*id),
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

    /// Every callable operand of an iterator consumer has a checked body.
    /// Operands that are not callables (fold seeds, counts) carry no work.
    pub(crate) fn consumer_callbacks_checked(&self, operands: &[Ty<'tcx>]) -> bool {
        operands
            .iter()
            .all(|operand| match operand.peel_refs().kind() {
                ty::Closure(..) | ty::FnDef(..) => self.checked_iterator_callback(*operand),
                ty::FnPtr(..) | ty::Dynamic(..) => false,
                ty::Param(_) => {
                    self.checked_iterator_callback(*operand)
                        || !self.callable_parameter(operand.peel_refs())
                }
                _ => true,
            })
    }

    fn callable_parameter(&self, value: Ty<'tcx>) -> bool {
        self.typing_env().param_env.caller_bounds().any(|clause| {
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate) => {
                    predicate.self_ty() == value
                        && self
                            .tcx
                            .fn_trait_kind_from_def_id(predicate.def_id())
                            .is_some()
                }
                _ => false,
            }
        })
    }

    /// Every base visit of the iterator was charged before its first step.
    pub(crate) fn prepaid_iterator(&self, value: Ty<'tcx>) -> bool {
        self.iterator_step_cost(value) == StepCost::Prepaid
    }

    /// The shape a traversal of the iterator has for work accounting:
    /// constant or prepaid traversals need no further charge, one-step
    /// traversals need one per step, and unknown steps have no shape.
    pub(crate) fn bounded_iterator_shape(&self, value: Ty<'tcx>) -> Option<types::Shape> {
        match self.iterator_step_cost(value) {
            StepCost::Constant | StepCost::Prepaid => Some(types::Shape::Fixed),
            StepCost::One => Some(types::Shape::Dynamic),
            StepCost::Unknown => None,
        }
    }

    pub(crate) fn iterator_step_cost(&self, value: Ty<'tcx>) -> StepCost {
        self.iterator_step_cost_at(value, 0)
    }

    fn iterator_step_cost_at(&self, value: Ty<'tcx>, depth: usize) -> StepCost {
        if depth >= self.tcx.recursion_limit().0 {
            return StepCost::Unknown;
        }
        let value = self.normalized_iterator_type(value.peel_refs());
        let value = types::reveal_opaque(self.tcx, value);
        if self.core_iterator_parameter(value) {
            return StepCost::Unknown;
        }
        let ty::Adt(owner, arguments) = value.kind() else {
            return match value.kind() {
                ty::Array(..) => StepCost::Constant,
                ty::Slice(_) | ty::Str => StepCost::One,
                _ => StepCost::Unknown,
            };
        };
        if types::admitted_iter(self.tcx, value) {
            return StepCost::Prepaid;
        }
        if types::physical_item_path(self.tcx, owner.did(), "roxmltree", &["Children"]) {
            return StepCost::One;
        }
        if !types::standard(self.tcx, owner.did()) {
            return StepCost::Unknown;
        }
        let name = self.tcx.item_name(owner.did());
        let path = self.tcx.def_path_str(owner.did());
        let arguments: Vec<_> = arguments.types().collect();
        let cost = |index: usize| {
            arguments.get(index).map_or(StepCost::Unknown, |source| {
                self.iterator_step_cost_at(*source, depth + 1)
            })
        };
        let callback = || {
            arguments
                .last()
                .is_some_and(|callback| self.checked_iterator_callback(*callback))
        };
        match name.as_str() {
            "Option" | "Result" | "Once" | "Empty" => StepCost::Constant,
            "IntoIter" | "Iter" | "IterMut"
                if path.contains("array::")
                    || path.contains("option::")
                    || path.contains("result::") =>
            {
                StepCost::Constant
            }
            "ToLowercase" | "ToUppercase" if path.contains("char::") => StepCost::Constant,
            "IntoIter" if path.contains("vec::") => StepCost::One,
            "Iter" | "IterMut"
                if (path.contains("slice::")
                    || path.contains("vec::")
                    || path.contains("vec_deque::"))
                    && !path.contains("hash") =>
            {
                StepCost::One
            }
            "Bytes" | "Chars"
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "core",
                    &["str", "iter", name.as_str()],
                ) =>
            {
                StepCost::One
            }
            "Range" | "RangeInclusive" | "RangeFrom"
                if types::physical_item_path(
                    self.tcx,
                    owner.did(),
                    "core",
                    &["ops", "range", name.as_str()],
                ) && arguments.first().is_some_and(|element| {
                    matches!(element.kind(), ty::Uint(_) | ty::Int(_) | ty::Char)
                }) =>
            {
                StepCost::One
            }
            "FromFn" | "Successors" if callback() => StepCost::One,
            // Each step forwards to one base step and one checked callback.
            "Map" | "Inspect" | "MapWhile" | "Scan" | "TakeWhile" => {
                if callback() {
                    cost(0)
                } else {
                    StepCost::Unknown
                }
            }
            // One step may consume many base steps; only a prepaid or constant
            // base pays for the skipped visits.
            "Filter" | "FilterMap" | "SkipWhile" if !callback() => StepCost::Unknown,
            "Filter" | "FilterMap" | "SkipWhile" | "Skip" | "StepBy" => match cost(0) {
                base @ (StepCost::Constant | StepCost::Prepaid) => base,
                StepCost::One | StepCost::Unknown => StepCost::Unknown,
            },
            "Enumerate" | "Rev" | "Copied" | "Fuse" | "Peekable" | "Take" | "DecodeUtf16" => {
                cost(0)
            }
            "Cloned" => {
                let element = arguments
                    .first()
                    .and_then(|source| self.iterator_item(*source))
                    .map(|item| item.peel_refs());
                if element.is_some_and(|element| {
                    self.tcx
                        .type_is_copy_modulo_regions(self.typing_env(), element)
                        || self.clone_shape(element) == types::Shape::Fixed
                }) {
                    cost(0)
                } else {
                    StepCost::Unknown
                }
            }
            "Chain" => cost(0).max(cost(1)),
            // Zip stops at its shorter side, so the shorter side bounds the
            // constant-work steps of the other.
            "Zip" => {
                let (left, right) = (cost(0), cost(1));
                if left == StepCost::Unknown || right == StepCost::Unknown {
                    StepCost::Unknown
                } else {
                    left.min(right)
                }
            }
            "Flatten" => {
                let inner = arguments
                    .first()
                    .and_then(|source| self.iterator_item(*source));
                self.flattened_cost(cost(0), inner, depth)
            }
            "FlatMap" if callback() => {
                self.flattened_cost(cost(0), arguments.get(1).copied(), depth)
            }
            _ => StepCost::Unknown,
        }
    }

    /// A flattened traversal pays for its inner iterators only when each one
    /// has a constant count; a one-step outer also needs every inner to yield,
    /// or a single step could skip any number of empty inner iterators.
    fn flattened_cost(&self, outer: StepCost, inner: Option<Ty<'tcx>>, depth: usize) -> StepCost {
        let Some(inner) = inner else {
            return StepCost::Unknown;
        };
        let inner = inner.peel_refs();
        let constant_count = self.iterator_step_cost_at(inner, depth + 1) == StepCost::Constant;
        let nonempty_array = matches!(inner.kind(), ty::Array(_, length)
            if length.try_to_target_usize(self.tcx).is_some_and(|length| length > 0));
        match outer {
            StepCost::Constant | StepCost::Prepaid if constant_count => outer,
            StepCost::One if nonempty_array => StepCost::One,
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

    /// Checks the source operands of core operations that step a caller's
    /// iterator, and the refusal of each iterator admission.
    pub(crate) fn iterator_boundary(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        if types::decode_context_method(self.tcx, definition, "admit_iter")
            && !self.propagated(expression)
        {
            self.report(
                expression.span,
                "unproven_decode_charge",
                "DecodeContext::admit_iter refusal is not propagated; replacement: propagate the admission Result with ?",
            );
        }
        let Some(args) = self.call_arguments(expression) else {
            return;
        };
        let sources = self.core_iterator_types(definition, args);
        let source_owner = self.tcx.def_path_str(definition);
        for operand in operands {
            let value = self.expr_ty(operand);
            if sources
                .iter()
                .any(|source| *source == value || *source == value.peel_refs())
                && !self.core_iterator_parameter(value)
                && self.iterator_step_cost(value) == StepCost::Unknown
            {
                self.report(expression.span, "unproven_decode_charge", &format!("{source_owner} source can scan before yielding; admit its bounded base before applying adapters"));
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
