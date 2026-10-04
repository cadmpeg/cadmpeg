// SPDX-License-Identifier: Apache-2.0
//! Prove handwritten serializers that assemble and delegate a bounded Wire value.

use crate::{external, types};
use rustc_middle::mir::{
    self, BasicBlock, BorrowKind, Local, Operand, Place, Rvalue, StatementKind, TerminatorKind,
    RETURN_PLACE,
};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt};
use rustc_span::{def_id::DefId, Spanned};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Data { source: bool, fixed: bool },
    Serializer,
    Callback(DefId),
    Result,
}

#[derive(Clone, Copy)]
struct Delegate {
    block: BasicBlock,
    method: DefId,
}

struct Trace<'tcx> {
    tcx: TyCtxt<'tcx>,
    active_methods: Vec<DefId>,
    active_locals: HashSet<(DefId, Local)>,
    delegate: Option<Delegate>,
}

#[derive(Clone, Copy)]
struct Method<'tcx> {
    definition: DefId,
    instance: Option<Instance<'tcx>>,
}

impl<'tcx> Method<'tcx> {
    fn instantiate_ty(self, tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
        self.instance.map_or(ty, |instance| {
            instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, ty))
        })
    }
}

/// Returns the concrete Wire type when a handwritten serializer assembles it
/// from fixed values and immutable source projections, then returns its
/// `Serialize::serialize` result unchanged.
pub(crate) fn delegated_source<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    method: DefId,
) -> Option<Ty<'tcx>> {
    if !tcx.is_mir_available(method) {
        return None;
    }
    let instance = crate::structural_derived::serialize_instance(tcx, source, method)?;
    let concrete_body = crate::structural_derived::instantiated_body(tcx, instance);
    let body = &concrete_body;
    if body.arg_count != 2 || crate::conversion::cyclic(body) {
        return None;
    }
    let receiver = body.local_decls.get(Local::from_usize(1))?.ty.peel_refs();
    if receiver != source.peel_refs() {
        return None;
    }

    let reachable = reachable_blocks(body);
    let mut delegate = None;
    let mut wire = None;
    for block in &reachable {
        let TerminatorKind::Call {
            func,
            args,
            target: Some(_),
            ..
        } = &body.basic_blocks[*block].terminator().kind
        else {
            continue;
        };
        let ty::FnDef(definition, _) = func.ty(body, tcx).kind() else {
            continue;
        };
        if !serde_serialize_call(tcx, *definition) {
            continue;
        }
        if args.len() != 2 || delegate.is_some() {
            return None;
        }
        let candidate = args[0].node.ty(body, tcx).peel_refs();
        if candidate == source.peel_refs() {
            return None;
        }
        delegate = Some(Delegate {
            block: *block,
            method: *definition,
        });
        wire = Some(candidate);
    }
    let delegate = delegate?;
    let wire = wire?;
    let ty::Adt(owner, _) = wire.kind() else {
        return None;
    };
    if owner.is_union() {
        return None;
    }

    let mut trace = Trace {
        tcx,
        active_methods: vec![method],
        active_locals: HashSet::new(),
        delegate: Some(delegate),
    };
    if !body_is_bounded(
        &mut trace,
        Method {
            definition: method,
            instance: None,
        },
        body,
        &[
            Origin::Data {
                source: true,
                fixed: false,
            },
            Origin::Serializer,
        ],
    ) {
        return None;
    }
    if trace.local_origin(
        Method {
            definition: method,
            instance: None,
        },
        body,
        RETURN_PLACE,
        &[
            Origin::Data {
                source: true,
                fixed: false,
            },
            Origin::Serializer,
        ],
    )? != Origin::Result
    {
        return None;
    }
    Some(wire)
}

impl<'tcx> Trace<'tcx> {
    fn local_origin(
        &mut self,
        method: Method<'tcx>,
        body: &mir::Body<'tcx>,
        local: Local,
        arguments: &[Origin],
    ) -> Option<Origin> {
        let index = local.as_usize();
        if index > 0 && index <= body.arg_count {
            let origin = *arguments.get(index - 1)?;
            return Some(origin);
        }
        if !self.active_locals.insert((method.definition, local)) {
            return None;
        }

        let mut origin = None;
        let reachable = reachable_blocks(body);
        for (block, data) in body.basic_blocks.iter_enumerated() {
            if !reachable.contains(&block) {
                continue;
            }
            for statement in &data.statements {
                if let StatementKind::Assign(assignment) = &statement.kind {
                    let (target, value) = &**assignment;
                    if target.local == local {
                        if !target.projection.is_empty() {
                            self.active_locals.remove(&(method.definition, local));
                            return None;
                        }
                        let value = self.rvalue_origin(method, body, value, arguments)?;
                        origin = Some(merge(origin, value)?);
                    }
                }
            }
            if let TerminatorKind::Call {
                func,
                args,
                destination,
                target: Some(_),
                ..
            } = &data.terminator().kind
            {
                if destination.local == local {
                    if !destination.projection.is_empty() {
                        self.active_locals.remove(&(method.definition, local));
                        return None;
                    }
                    let value = self.call_origin(method, block, body, func, args, arguments)?;
                    origin = Some(merge(origin, value)?);
                }
            }
        }
        self.active_locals.remove(&(method.definition, local));
        origin
    }

    fn operand_origin(
        &mut self,
        method: Method<'tcx>,
        body: &mir::Body<'tcx>,
        operand: &Operand<'tcx>,
        arguments: &[Origin],
    ) -> Option<Origin> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                self.place_origin(method, body, place, arguments)
            }
            Operand::Constant(constant) => {
                let value_type = method.instantiate_ty(self.tcx, operand.ty(body, self.tcx));
                match value_type.kind() {
                    ty::FnDef(definition, _) => Some(Origin::Callback(*definition)),
                    ty::FnPtr(..) | ty::Dynamic(..) | ty::Infer(_) | ty::Error(_) => None,
                    _ if matches!(constant.const_, mir::Const::Unevaluated(..)) => {
                        let mir::Const::Unevaluated(value, _) = constant.const_ else {
                            return None;
                        };
                        let promoted = value.promoted?;
                        let body = self.tcx.promoted_mir(value.def).get(promoted)?;
                        let mut trace = Trace {
                            tcx: self.tcx,
                            active_methods: vec![value.def],
                            active_locals: HashSet::new(),
                            delegate: None,
                        };
                        let method = Method {
                            definition: value.def,
                            instance: Some(Instance::new_raw(value.def, value.args)),
                        };
                        if !body_is_bounded(&mut trace, method, body, &[]) {
                            return None;
                        }
                        match trace.local_origin(method, body, RETURN_PLACE, &[])? {
                            Origin::Data {
                                source: false,
                                fixed: true,
                            } => Some(Origin::Data {
                                source: false,
                                fixed: true,
                            }),
                            _ => None,
                        }
                    }
                    _ if fixed_constant_type(self.tcx, value_type) => Some(Origin::Data {
                        source: false,
                        fixed: true,
                    }),
                    _ => None,
                }
            }
            Operand::RuntimeChecks(_) => None,
        }
    }

    fn place_origin(
        &mut self,
        method: Method<'tcx>,
        body: &mir::Body<'tcx>,
        place: &Place<'tcx>,
        arguments: &[Origin],
    ) -> Option<Origin> {
        let bounded_projection =
            place
                .as_ref()
                .iter_projections()
                .all(|(base, projection)| match projection {
                    mir::ProjectionElem::Deref => matches!(
                    method.instantiate_ty(
                        self.tcx,
                        base.to_place(self.tcx).ty(body, self.tcx).ty,
                    )
                    .kind(),
                    ty::Ref(..)
                ),
                    mir::ProjectionElem::Field(..) | mir::ProjectionElem::Downcast(..) => true,
                    _ => false,
                });
        if !bounded_projection {
            return None;
        }
        self.local_origin(method, body, place.local, arguments)
    }

    fn rvalue_origin(
        &mut self,
        method: Method<'tcx>,
        body: &mir::Body<'tcx>,
        value: &Rvalue<'tcx>,
        arguments: &[Origin],
    ) -> Option<Origin> {
        match value {
            Rvalue::Use(operand, _) => self.operand_origin(method, body, operand, arguments),
            Rvalue::Ref(_, BorrowKind::Shared, place) => {
                self.place_origin(method, body, place, arguments)
            }
            Rvalue::Cast(
                mir::CastKind::PointerCoercion(ty::adjustment::PointerCoercion::Unsize, _),
                operand,
                _,
            ) => self.operand_origin(method, body, operand, arguments),
            Rvalue::Aggregate(kind, operands) => {
                if matches!(**kind, mir::AggregateKind::Closure(..)) {
                    return None;
                }
                let mut combined = None;
                for operand in operands {
                    let value = self.operand_origin(method, body, operand, arguments)?;
                    combined = Some(merge(combined, value)?);
                }
                Some(combined.unwrap_or(Origin::Data {
                    source: false,
                    fixed: true,
                }))
            }
            Rvalue::Discriminant(place) => self.place_origin(method, body, place, arguments),
            Rvalue::CopyForDeref(place) => self.place_origin(method, body, place, arguments),
            Rvalue::UnaryOp(_, operand) => {
                let origin = self.operand_origin(method, body, operand, arguments)?;
                data_origin(origin)
            }
            Rvalue::BinaryOp(_, operands) => {
                let (left, right) = &**operands;
                let left = data_origin(self.operand_origin(method, body, left, arguments)?)?;
                let right = data_origin(self.operand_origin(method, body, right, arguments)?)?;
                Some(combine_data(left, right))
            }
            _ => None,
        }
    }

    fn call_origin(
        &mut self,
        caller: Method<'tcx>,
        block: BasicBlock,
        body: &mir::Body<'tcx>,
        function: &Operand<'tcx>,
        arguments: &[Spanned<Operand<'tcx>>],
        caller_arguments: &[Origin],
    ) -> Option<Origin> {
        let function_ty = caller.instantiate_ty(self.tcx, function.ty(body, self.tcx));
        let ty::FnDef(definition, generic_arguments) = function_ty.kind() else {
            return None;
        };
        if let Some(delegate) = self.delegate {
            if caller.definition == self.active_methods[0]
                && block == delegate.block
                && *definition == delegate.method
            {
                if !serde_serialize_call(self.tcx, *definition) || arguments.len() != 2 {
                    return None;
                }
                let value =
                    self.operand_origin(caller, body, &arguments[0].node, caller_arguments)?;
                let Origin::Data { source, fixed } = value else {
                    return None;
                };
                if !source && !fixed {
                    return None;
                }
                if self.operand_origin(caller, body, &arguments[1].node, caller_arguments)?
                    != Origin::Serializer
                {
                    return None;
                }
                return Some(Origin::Result);
            }
        }
        if serde_serialize_call(self.tcx, *definition) {
            return None;
        }
        if is_option_map(self.tcx, *definition) {
            return self.option_map_origin(caller, body, arguments, caller_arguments);
        }
        if is_option_as_ref(self.tcx, *definition) {
            if arguments.len() != 1 {
                return None;
            }
            return self.operand_origin(caller, body, &arguments[0].node, caller_arguments);
        }
        if let [argument] = arguments {
            let TerminatorKind::Call { destination, .. } =
                &body.basic_blocks[block].terminator().kind
            else {
                return None;
            };
            let receiver = caller.instantiate_ty(self.tcx, argument.node.ty(body, self.tcx));
            let output = caller.instantiate_ty(self.tcx, destination.ty(body, self.tcx).ty);
            if crate::structural_scalar::fixed_string_borrow(
                self.tcx,
                *definition,
                receiver,
                output,
            ) {
                return data_origin(self.operand_origin(
                    caller,
                    body,
                    &argument.node,
                    caller_arguments,
                )?);
            }
        }
        let generic_arguments = generic_arguments.no_bound_vars()?;
        let instance = resolve_instance(self.tcx, *definition, generic_arguments)?;
        if !crate::structural_fixed::fixed_callback_body(self.tcx, instance) {
            return None;
        }
        let actual: Option<Vec<_>> = arguments
            .iter()
            .map(|argument| self.operand_origin(caller, body, &argument.node, caller_arguments))
            .collect();
        let actual = actual?;
        if actual
            .iter()
            .any(|origin| !matches!(origin, Origin::Data { .. }))
        {
            return None;
        }
        self.method_result_origin(instance, &actual)
    }

    fn option_map_origin(
        &mut self,
        caller: Method<'tcx>,
        body: &mir::Body<'tcx>,
        arguments: &[Spanned<Operand<'tcx>>],
        caller_arguments: &[Origin],
    ) -> Option<Origin> {
        if arguments.len() != 2 {
            return None;
        }
        let receiver_ty = caller
            .instantiate_ty(self.tcx, arguments[0].node.ty(body, self.tcx))
            .peel_refs();
        let ty::Adt(owner, _) = receiver_ty.kind() else {
            return None;
        };
        if !types::physical_item_path(self.tcx, owner.did(), "core", &["option", "Option"]) {
            return None;
        }
        let Origin::Data {
            source: input_source,
            fixed: input_fixed,
        } = data_origin(self.operand_origin(
            caller,
            body,
            &arguments[0].node,
            caller_arguments,
        )?)?
        else {
            return None;
        };
        let Origin::Callback(callback) =
            self.operand_origin(caller, body, &arguments[1].node, caller_arguments)?
        else {
            return None;
        };
        let callback_ty = caller.instantiate_ty(self.tcx, arguments[1].node.ty(body, self.tcx));
        let ty::FnDef(callback_definition, callback_arguments) = callback_ty.kind() else {
            return None;
        };
        if *callback_definition != callback
            || !matches!(
                self.tcx.def_kind(*callback_definition),
                rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
            )
        {
            return None;
        }
        let callback_arguments = callback_arguments.no_bound_vars()?;
        let callback_instance =
            resolve_instance(self.tcx, *callback_definition, callback_arguments)?;
        if !fixed_string_instance(self.tcx, callback_instance)
            && !crate::structural_fixed::fixed_callback_body(self.tcx, callback_instance)
        {
            return None;
        }
        let callback_input = Origin::Data {
            source: input_source,
            fixed: input_fixed,
        };
        let Origin::Data {
            source: output_source,
            fixed: output_fixed,
        } = data_origin(self.method_result_origin(callback_instance, &[callback_input])?)?
        else {
            return None;
        };
        Some(Origin::Data {
            source: input_source || output_source,
            fixed: input_fixed || output_fixed,
        })
    }

    fn method_result_origin(
        &mut self,
        instance: Instance<'tcx>,
        arguments: &[Origin],
    ) -> Option<Origin> {
        if fixed_string_instance(self.tcx, instance) {
            let [argument] = arguments else {
                return None;
            };
            return data_origin(*argument);
        }
        let method = instance.def_id();
        if arguments
            .iter()
            .any(|origin| !matches!(origin, Origin::Data { .. }))
            || self.active_methods.contains(&method)
            || self.active_methods.len() >= self.tcx.recursion_limit().0
            || !self.tcx.is_mir_available(method)
        {
            return None;
        }
        let body = self.tcx.instance_mir(instance.def);
        if body.arg_count != arguments.len() || crate::conversion::cyclic(body) {
            return None;
        }
        self.active_methods.push(method);
        let previous_delegate = self.delegate;
        self.delegate = None;
        let method = Method {
            definition: method,
            instance: Some(instance),
        };
        let bounded = body_is_bounded(self, method, body, arguments);
        let result = if bounded {
            self.local_origin(method, body, RETURN_PLACE, arguments)
        } else {
            None
        };
        self.delegate = previous_delegate;
        self.active_methods.pop();
        match result? {
            origin @ Origin::Data { .. } => Some(origin),
            _ => None,
        }
    }
}

fn body_is_bounded<'tcx>(
    trace: &mut Trace<'tcx>,
    method: Method<'tcx>,
    body: &mir::Body<'tcx>,
    arguments: &[Origin],
) -> bool {
    if body.arg_count != arguments.len()
        || crate::conversion::cyclic(body)
        || (1..=body.arg_count).any(|index| {
            matches!(
                method
                    .instantiate_ty(trace.tcx, body.local_decls[Local::from_usize(index)].ty,)
                    .kind(),
                ty::Ref(_, _, ty::Mutability::Mut)
            )
        })
    {
        return false;
    }
    let reachable = reachable_blocks(body);
    for block in &reachable {
        let data = &body.basic_blocks[*block];
        for statement in &data.statements {
            match &statement.kind {
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => (),
                StatementKind::Assign(assignment) => {
                    let (target, value) = &**assignment;
                    if !target.projection.is_empty()
                        || target.local.as_usize() > 0 && target.local.as_usize() <= body.arg_count
                        || trace
                            .rvalue_origin(method, body, value, arguments)
                            .is_none()
                    {
                        return false;
                    }
                }
                StatementKind::SetDiscriminant { place, .. }
                    if place.projection.is_empty() && place.local.as_usize() > body.arg_count =>
                {
                    ()
                }
                StatementKind::FakeRead(fake_read) => {
                    let (_, place) = &**fake_read;
                    if trace.place_origin(method, body, place, arguments).is_none() {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        match &data.terminator().kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                target: Some(_),
                ..
            } => {
                if !destination.projection.is_empty()
                    || trace
                        .call_origin(method, *block, body, func, args, arguments)
                        .is_none()
                {
                    return false;
                }
            }
            TerminatorKind::SwitchInt { discr, .. } => {
                if trace
                    .operand_origin(method, body, discr, arguments)
                    .is_none()
                {
                    return false;
                }
            }
            TerminatorKind::Drop { place, .. } => {
                let origin = trace.place_origin(method, body, place, arguments);
                let serializer = place.local.as_usize() > 0
                    && place.local.as_usize() <= body.arg_count
                    && arguments.get(place.local.as_usize() - 1) == Some(&Origin::Serializer)
                    && origin == Some(Origin::Serializer);
                if !place.projection.is_empty()
                    || !(serializer
                        || crate::structural_fixed::drop_is_fixed(
                            trace.tcx,
                            method.instantiate_ty(trace.tcx, place.ty(body, trace.tcx).ty),
                        ))
                {
                    return false;
                }
            }
            TerminatorKind::Goto { .. }
            | TerminatorKind::Return
            | TerminatorKind::UnwindResume
            | TerminatorKind::UnwindTerminate(_)
            | TerminatorKind::Unreachable => (),
            _ => return false,
        }
    }
    true
}

fn reachable_blocks(body: &mir::Body<'_>) -> HashSet<BasicBlock> {
    let mut reachable = HashSet::new();
    let mut pending = vec![mir::START_BLOCK];
    while let Some(block) = pending.pop() {
        if reachable.insert(block) {
            pending.extend(body.basic_blocks[block].terminator().successors());
        }
    }
    reachable
}

fn merge(current: Option<Origin>, next: Origin) -> Option<Origin> {
    match (current, next) {
        (None, value) => Some(value),
        (
            Some(Origin::Data { source, fixed }),
            Origin::Data {
                source: next_source,
                fixed: next_fixed,
            },
        ) => Some(Origin::Data {
            source: source || next_source,
            fixed: fixed || next_fixed,
        }),
        (Some(Origin::Serializer), Origin::Serializer) => Some(Origin::Serializer),
        (Some(Origin::Result), Origin::Result) => Some(Origin::Result),
        (Some(Origin::Callback(left)), Origin::Callback(right)) if left == right => {
            Some(Origin::Callback(left))
        }
        _ => None,
    }
}

fn combine_data(left: Origin, right: Origin) -> Origin {
    match (left, right) {
        (
            Origin::Data { source, fixed },
            Origin::Data {
                source: next_source,
                fixed: next_fixed,
            },
        ) => Origin::Data {
            source: source || next_source,
            fixed: fixed || next_fixed,
        },
        _ => Origin::Data {
            source: false,
            fixed: false,
        },
    }
}

fn data_origin(origin: Origin) -> Option<Origin> {
    matches!(origin, Origin::Data { .. }).then_some(origin)
}

fn fixed_constant_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) => true,
        ty::Ref(_, referent, ty::Mutability::Not) => match referent.kind() {
            ty::Str => true,
            ty::Slice(element) | ty::Array(element, _) => fixed_constant_type(tcx, *element),
            _ => false,
        },
        ty::Array(element, _) => fixed_constant_type(tcx, *element),
        ty::Tuple(fields) => fields.iter().all(|field| fixed_constant_type(tcx, field)),
        ty::Adt(owner, arguments)
            if types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"]) =>
        {
            arguments
                .types()
                .next()
                .is_some_and(|element| fixed_constant_type(tcx, element))
        }
        _ => false,
    }
}

fn serde_serialize_call(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    tcx.opt_item_name(definition)
        .is_some_and(|name| name.as_str() == "serialize")
        && tcx.trait_of_assoc(definition).is_some_and(|trait_id| {
            ["serde", "serde_core"].iter().any(|crate_name| {
                types::physical_item_path(tcx, trait_id, crate_name, &["ser", "Serialize"])
            })
        })
}

fn is_option_map(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    types::physical_inherent_method(tcx, definition, "core", &["option", "Option"], "map")
}

fn is_option_as_ref(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    types::physical_inherent_method(tcx, definition, "core", &["option", "Option"], "as_ref")
        && external::summary(tcx, definition, None).is_some_and(|summary| {
            summary.allocation == external::Allocation::None
                && summary.work == external::Work::Fixed
        })
}

fn fixed_string_instance<'tcx>(tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
    if !matches!(
        tcx.def_kind(instance.def_id()),
        rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
    ) {
        return false;
    }
    let signature = tcx
        .fn_sig(instance.def_id())
        .instantiate(tcx, instance.args)
        .skip_binder();
    let [receiver] = signature.inputs() else {
        return false;
    };
    crate::structural_scalar::fixed_string_borrow(
        tcx,
        instance.def_id(),
        *receiver,
        signature.output(),
    )
}

fn resolve_instance<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
) -> Option<Instance<'tcx>> {
    let environment = ty::TypingEnv::fully_monomorphized();
    let arguments = tcx
        .try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(arguments))
        .ok()?;
    Instance::try_resolve(tcx, environment, definition, arguments)
        .ok()
        .flatten()
        .or_else(|| {
            tcx.is_mir_available(definition).then_some(Instance {
                def: ty::InstanceKind::Item(definition),
                args: arguments,
            })
        })
}
