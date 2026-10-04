// SPDX-License-Identifier: Apache-2.0
//! Proves bounded fieldwise bodies for imported built-in derive candidates.

use crate::{conversion, types};
use rustc_hir::def::DefKind;
use rustc_middle::mir::{
    self, BinOp, Body, BorrowKind, Local, Operand, Place, Rvalue, StatementKind, TerminatorKind,
    UnwindAction,
};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;
use rustc_span::Spanned;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CallbackKind {
    Hash,
    PartialEq,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FieldPath {
    variant: usize,
    field: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CallbackSite {
    Field(FieldPath),
    Discriminant,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Root {
    Argument(usize),
    Discriminant(usize),
    Compared(FieldPath),
    ComparedDiscriminant,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Projection {
    Deref,
    Downcast(usize),
    Field(usize),
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Origin {
    root: Root,
    projection: Vec<Projection>,
}

/// Proves an imported Hash or PartialEq implementation by inspecting the
/// concrete MIR body. The compiler's `automatically_derived` marker only
/// selects candidates; it does not establish this proof.
pub(crate) fn bounded_imported_derive<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    implementation: DefId,
    derive_name: &str,
) -> bool {
    if implementation.is_local() || !tcx.is_automatically_derived(implementation) {
        return false;
    }
    let kind = match derive_name {
        "Hash" => CallbackKind::Hash,
        "PartialEq" => CallbackKind::PartialEq,
        _ => return false,
    };
    let value = types::reveal_opaque(tcx, value).peel_refs();
    let ty::Adt(owner, owner_arguments) = value.kind() else {
        return false;
    };
    if owner.is_union()
        || !matches!(tcx.def_kind(implementation), DefKind::Impl { of_trait: true })
    {
        return false;
    }
    let expected_trait = match kind {
        CallbackKind::Hash => ("core", &["hash", "Hash"][..]),
        CallbackKind::PartialEq => ("core", &["cmp", "PartialEq"][..]),
    };
    let method_name = match kind {
        CallbackKind::Hash => "hash",
        CallbackKind::PartialEq => "eq",
    };
    let Some(trait_ref) = tcx.impl_opt_trait_ref(implementation) else {
        return false;
    };
    let trait_ref = trait_ref
        .instantiate(tcx, owner_arguments)
        .skip_norm_wip();
    let trait_id = trait_ref.def_id;
    if trait_ref.self_ty() != value
        || !types::physical_item_path(tcx, trait_id, expected_trait.0, expected_trait.1)
    {
        return false;
    }
    if kind == CallbackKind::PartialEq
        && !trait_ref
            .args
            .types()
            .nth(1)
            .is_some_and(|right| types::reveal_opaque(tcx, right) == value)
    {
        return false;
    }
    let Some(trait_method) = tcx
        .associated_items(trait_id)
        .in_definition_order()
        .find(|item| item.name().as_str() == method_name)
        .map(|item| item.def_id)
    else {
        return false;
    };
    let trait_method_path = if kind == CallbackKind::Hash {
        &["hash", "Hash", "hash"][..]
    } else {
        &["cmp", "PartialEq", "eq"][..]
    };
    if !types::physical_item_path(tcx, trait_method, expected_trait.0, trait_method_path) {
        return false;
    }
    let Some(method) = tcx
        .associated_items(implementation)
        .in_definition_order()
        .find(|item| item.name().as_str() == method_name)
        .map(|item| item.def_id)
    else {
        return false;
    };
    if tcx.parent(method) != implementation
        || !tcx.is_mir_available(method)
        || tcx.generics_of(implementation).count() != owner_arguments.len()
    {
        return false;
    }

    let implementation_self = tcx
        .type_of(implementation)
        .instantiate(tcx, owner_arguments)
        .skip_norm_wip();
    if implementation_self != value
        || !matches!(implementation_self.kind(), ty::Adt(definition, _) if definition.did() == owner.did())
    {
        return false;
    }
    let method_arguments = ty::GenericArgs::identity_for_item(tcx, method)
        .rebase_onto(tcx, implementation, owner_arguments);
    let instance = Instance::new_raw(method, method_arguments);
    let body = tcx.instance_mir(instance.def);
    if body.arg_count != 2 || conversion::cyclic(body) {
        return false;
    }

    let mut proof = CallbackBody {
        tcx,
        value,
        owner: *owner,
        owner_arguments,
        kind,
        instance,
        body,
        active: HashSet::new(),
    };
    proof.body_is_bounded()
}

struct CallbackBody<'tcx, 'body> {
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    owner: ty::AdtDef<'tcx>,
    owner_arguments: ty::GenericArgsRef<'tcx>,
    kind: CallbackKind,
    instance: Instance<'tcx>,
    body: &'body Body<'tcx>,
    active: HashSet<Local>,
}

impl<'tcx, 'body> CallbackBody<'tcx, 'body> {
    fn body_is_bounded(&mut self) -> bool {
        let mut events = Vec::<(mir::BasicBlock, CallbackSite)>::new();
        let body = self.body;
        for (block_index, block) in body.basic_blocks.iter_enumerated() {
            for statement in &block.statements {
                match &statement.kind {
                    StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
                    | StatementKind::Nop => (),
                    StatementKind::Assign(assignment) => {
                        let (target, value) = &**assignment;
                        if !target.projection.is_empty()
                            || target.local.as_usize() > 0
                                && target.local.as_usize() <= self.body.arg_count
                            || self.rvalue_origin(value).is_none()
                        {
                            return false;
                        }
                    }
                    StatementKind::FakeRead(fake_read) => {
                        let (_, place) = &**fake_read;
                        if self.place_origin(place).is_none() {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }

            match &block.terminator().kind {
                TerminatorKind::Call {
                    func,
                    args,
                    destination,
                    target: Some(_),
                    unwind,
                    ..
                } if !matches!(unwind, UnwindAction::Cleanup(_))
                    && destination.projection.is_empty() =>
                {
                    let Some(site) = self.callback_site(func, args) else {
                        return false;
                    };
                    events.push((block_index, site));
                }
                TerminatorKind::Call { .. } | TerminatorKind::Drop { .. } => return false,
                TerminatorKind::SwitchInt { discr, .. } => {
                    if !matches!(
                        self.operand_origin(discr).map(|origin| origin.root),
                        Some(
                            Root::Discriminant(1 | 2)
                                | Root::Compared(_)
                                | Root::ComparedDiscriminant,
                        )
                    ) {
                        return false;
                    }
                }
                TerminatorKind::Goto { .. }
                | TerminatorKind::Return
                | TerminatorKind::Unreachable => (),
                _ => return false,
            }
        }

        self.no_repeated_callback_on_path(&events)
    }

    fn callback_site(
        &mut self,
        function: &Operand<'tcx>,
        arguments: &[Spanned<Operand<'tcx>>],
    ) -> Option<CallbackSite> {
        if arguments.len() != 2 {
            return None;
        }
        let function = self.instance.instantiate_mir(
            self.tcx,
            ty::EarlyBinder::bind(self.tcx, function.ty(self.body, self.tcx)),
        );
        let ty::FnDef(definition, generic_arguments) = function.kind() else {
            return None;
        };
        let callback_matches = callback_trait(self.tcx, *definition, self.kind);
        if !callback_matches {
            return None;
        }
        let generic_arguments = (*generic_arguments).no_bound_vars()?;
        // An impl method's generic arguments contain parent impl parameters
        // before method parameters; the first type argument is not `Self`.
        // Read the concrete receiver from the instantiated function signature
        // so both trait items and impl methods are checked the same way.
        let signature = self
            .tcx
            .fn_sig(*definition)
            .instantiate(self.tcx, generic_arguments)
            .skip_binder();
        let ty::Ref(_, callback_self, _) = signature.inputs().first()?.kind() else {
            return None;
        };
        let first_type = self
            .instance
            .instantiate_mir(
                self.tcx,
                ty::EarlyBinder::bind(self.tcx, arguments[0].node.ty(self.body, self.tcx)),
            );
        let ty::Ref(_, first_type, _) = first_type.kind() else {
            return None;
        };
        if types::reveal_opaque(self.tcx, *first_type)
            != types::reveal_opaque(self.tcx, *callback_self)
        {
            return None;
        }

        match self.kind {
            CallbackKind::Hash => {
                let state = self.operand_origin(&arguments[1].node)?;
                if state.root != Root::Argument(2)
                    || !state
                        .projection
                        .iter()
                        .all(|projection| matches!(projection, Projection::Deref))
                {
                    return None;
                }
                let value = self.operand_origin(&arguments[0].node)?;
                match value.root {
                    Root::Argument(1) => {
                        let path = self.field_path(value, 1)?;
                        self.field_type_matches(path, *first_type)
                            .then_some(CallbackSite::Field(path))
                    }
                    Root::Discriminant(1) if value.projection.is_empty() => {
                        discriminant_type(self.tcx, self.value, *first_type)
                            .then_some(CallbackSite::Discriminant)
                    }
                    _ => None,
                }
            }
            CallbackKind::PartialEq => {
                let second_type = self.instance.instantiate_mir(
                    self.tcx,
                    ty::EarlyBinder::bind(self.tcx, arguments[1].node.ty(self.body, self.tcx)),
                );
                let ty::Ref(_, second_type, _) = second_type.kind() else {
                    return None;
                };
                if types::reveal_opaque(self.tcx, *second_type)
                    != types::reveal_opaque(self.tcx, *first_type)
                {
                    return None;
                }
                let left = self.operand_origin(&arguments[0].node)?;
                let right = self.operand_origin(&arguments[1].node)?;
                if left.root != Root::Argument(1) || right.root != Root::Argument(2) {
                    return None;
                }
                let left_path = self.field_path(left, 1)?;
                let right_path = self.field_path(right, 2)?;
                (left_path == right_path
                    && self.field_type_matches(left_path, *first_type))
                    .then_some(CallbackSite::Field(left_path))
            }
        }
    }

    fn field_path(&self, origin: Origin, argument: usize) -> Option<FieldPath> {
        if origin.root != Root::Argument(argument) {
            return None;
        }
        let mut variant = None;
        let mut field = None;
        let mut field_seen = false;
        for projection in origin.projection {
            match projection {
                Projection::Deref if !field_seen => (),
                Projection::Downcast(index) if !field_seen && variant.is_none() => {
                    variant = Some(index)
                }
                Projection::Field(index) if !field_seen => {
                    field = Some(index);
                    field_seen = true;
                }
                _ => return None,
            }
        }
        let field = field?;
        if self.owner.is_enum() {
            Some(FieldPath {
                variant: variant?,
                field,
            })
        } else if variant.is_none() {
            Some(FieldPath { variant: 0, field })
        } else {
            None
        }
    }

    fn field_type_matches(&self, path: FieldPath, callback_type: Ty<'tcx>) -> bool {
        let Some((_, variant)) = self
            .owner
            .variants()
            .iter_enumerated()
            .find(|(index, _)| index.as_usize() == path.variant)
        else {
            return false;
        };
        let Some((_, field)) = variant
            .fields
            .iter_enumerated()
            .find(|(index, _)| index.as_usize() == path.field)
        else {
            return false;
        };
        let field_type = field
            .ty(self.tcx, self.owner_arguments)
            .skip_norm_wip();
        !field_type.has_aliases()
            && !field_type.has_non_region_param()
            && !field_type.has_escaping_bound_vars()
            && types::reveal_opaque(self.tcx, field_type)
                == types::reveal_opaque(self.tcx, callback_type)
    }

    fn no_repeated_callback_on_path(
        &self,
        events: &[(mir::BasicBlock, CallbackSite)],
    ) -> bool {
        let mut reachable = HashMap::<mir::BasicBlock, HashSet<mir::BasicBlock>>::new();
        for (start, _) in events {
            reachable
                .entry(*start)
                .or_insert_with(|| block_reachable(self.body, *start));
        }
        for (index, (left_block, left_site)) in events.iter().enumerate() {
            for (right_block, right_site) in &events[index + 1..] {
                if left_site == right_site
                    && (left_block == right_block
                        || reachable
                            .get(left_block)
                            .is_some_and(|blocks| blocks.contains(right_block))
                        || reachable
                            .get(right_block)
                            .is_some_and(|blocks| blocks.contains(left_block)))
                {
                    return false;
                }
            }
        }
        true
    }

    fn operand_origin(&mut self, operand: &Operand<'tcx>) -> Option<Origin> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_origin(place),
            Operand::Constant(_) => Some(Origin {
                root: Root::Other,
                projection: Vec::new(),
            }),
            Operand::RuntimeChecks(_) => None,
        }
    }

    fn place_origin(&mut self, place: &Place<'tcx>) -> Option<Origin> {
        let mut origin = self.local_origin(place.local)?;
        origin.projection.extend(place.projection.iter().map(|projection| match projection {
            mir::ProjectionElem::Deref => Projection::Deref,
            mir::ProjectionElem::Downcast(_, variant) => {
                Projection::Downcast(variant.as_usize())
            }
            mir::ProjectionElem::Field(field, _) => Projection::Field(field.as_usize()),
            _ => Projection::Other,
        }));
        Some(origin)
    }

    fn local_origin(&mut self, local: Local) -> Option<Origin> {
        let index = local.as_usize();
        if index > 0 && index <= self.body.arg_count {
            return Some(Origin {
                root: Root::Argument(index),
                projection: Vec::new(),
            });
        }
        if self.active.len() >= self.tcx.recursion_limit().0 || !self.active.insert(local) {
            return None;
        }

        let body = self.body;
        let mut definition = None;
        for block in body.basic_blocks.iter() {
            for statement in &block.statements {
                let StatementKind::Assign(assignment) = &statement.kind else {
                    continue;
                };
                let (target, value) = &**assignment;
                if target.local != local {
                    continue;
                }
                if !target.projection.is_empty() || definition.is_some() {
                    self.active.remove(&local);
                    return None;
                }
                definition = Some(LocalDefinition::Assignment(value));
            }
            if let TerminatorKind::Call { destination, .. } = &block.terminator().kind {
                if destination.local == local {
                    if !destination.projection.is_empty() || definition.is_some() {
                        self.active.remove(&local);
                        return None;
                    }
                    definition = Some(LocalDefinition::Call(block));
                }
            }
        }
        let Some(definition) = definition else {
            self.active.remove(&local);
            return None;
        };
        let result = match definition {
            LocalDefinition::Assignment(value) => self.rvalue_origin(value),
            LocalDefinition::Call(block) => match &block.terminator().kind {
                TerminatorKind::Call { func, args, .. } => {
                    self.callback_site(func, args).map(|site| match (self.kind, site) {
                        (CallbackKind::PartialEq, CallbackSite::Field(path)) => Origin {
                            root: Root::Compared(path),
                            projection: Vec::new(),
                        },
                        _ => Origin {
                            root: Root::Other,
                            projection: Vec::new(),
                        },
                    })
                }
                _ => None,
            },
        };
        self.active.remove(&local);
        result
    }

    fn rvalue_origin(&mut self, value: &Rvalue<'tcx>) -> Option<Origin> {
        match value {
            Rvalue::Use(operand, _) => self.operand_origin(operand),
            Rvalue::Ref(_, BorrowKind::Shared | BorrowKind::Mut { .. }, place)
            | Rvalue::CopyForDeref(place) => self.place_origin(place),
            Rvalue::Discriminant(place) => {
                let origin = self.place_origin(place)?;
                if !matches!(origin.root, Root::Argument(1 | 2))
                    || !origin
                        .projection
                        .iter()
                        .all(|projection| matches!(projection, Projection::Deref))
                {
                    return None;
                }
                let Root::Argument(argument) = origin.root else {
                    return None;
                };
                Some(Origin {
                    root: Root::Discriminant(argument),
                    projection: Vec::new(),
                })
            }
            Rvalue::BinaryOp(BinOp::Eq, operands)
                if self.kind == CallbackKind::PartialEq =>
            {
                let (left, right) = &**operands;
                let left_origin = self.operand_origin(left)?;
                let right_origin = self.operand_origin(right)?;
                let left_type = self.instance.instantiate_mir(
                    self.tcx,
                    ty::EarlyBinder::bind(self.tcx, left.ty(self.body, self.tcx)),
                );
                let right_type = self.instance.instantiate_mir(
                    self.tcx,
                    ty::EarlyBinder::bind(self.tcx, right.ty(self.body, self.tcx)),
                );
                if left_origin.root == Root::Discriminant(1)
                    && left_origin.projection.is_empty()
                    && right_origin.root == Root::Discriminant(2)
                    && right_origin.projection.is_empty()
                    && left_type == right_type
                {
                    Some(Origin {
                        root: Root::ComparedDiscriminant,
                        projection: Vec::new(),
                    })
                } else if left_origin.root == Root::Argument(1)
                    && right_origin.root == Root::Argument(2)
                {
                    let left_path = self.field_path(left_origin, 1)?;
                    let right_path = self.field_path(right_origin, 2)?;
                    (left_path == right_path
                        && self.field_type_matches(left_path, left_type)
                        && left_type == right_type)
                        .then_some(Origin {
                            root: Root::Compared(left_path),
                            projection: Vec::new(),
                        })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

enum LocalDefinition<'body, 'tcx> {
    Assignment(&'body Rvalue<'tcx>),
    Call(&'body mir::BasicBlockData<'tcx>),
}

fn block_reachable<'tcx>(
    body: &Body<'tcx>,
    start: mir::BasicBlock,
) -> HashSet<mir::BasicBlock> {
    let mut pending = vec![start];
    let mut visited = HashSet::new();
    while let Some(block) = pending.pop() {
        if !visited.insert(block) {
            continue;
        }
        pending.extend(body.basic_blocks[block].terminator().successors());
    }
    visited.remove(&start);
    visited
}

fn discriminant_type<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, candidate: Ty<'tcx>) -> bool {
    if matches!(candidate.kind(), ty::Int(_) | ty::Uint(_)) {
        return true;
    }
    let ty::Adt(owner, arguments) = candidate.kind() else {
        return false;
    };
    types::physical_item_path(tcx, owner.did(), "core", &["mem", "Discriminant"])
        && arguments.types().next().is_some_and(|inner| {
            types::reveal_opaque(tcx, inner) == types::reveal_opaque(tcx, value)
        })
}

fn callback_trait(tcx: TyCtxt<'_>, definition: DefId, kind: CallbackKind) -> bool {
    let (crate_name, trait_parts, method) = match kind {
        CallbackKind::Hash => ("core", &["hash", "Hash"][..], "hash"),
        CallbackKind::PartialEq => ("core", &["cmp", "PartialEq"][..], "eq"),
    };
    let trait_item = match tcx.trait_item_of(definition) {
        Some(trait_item) => trait_item,
        None if matches!(tcx.def_kind(tcx.parent(definition)), DefKind::Trait) => definition,
        // `trait_of_assoc` only accepts an item owned by the trait. An impl
        // method must first resolve to its trait item above; passing the impl
        // method directly loses that association.
        None => return false,
    };
    let trait_id = tcx.parent(trait_item);
    let trait_method = match kind {
        CallbackKind::Hash => &["hash", "Hash", "hash"][..],
        CallbackKind::PartialEq => &["cmp", "PartialEq", "eq"][..],
    };
    tcx.opt_item_name(trait_item)
        .is_some_and(|name| name.as_str() == method)
        && types::physical_item_path(tcx, trait_id, crate_name, trait_parts)
        && types::physical_item_path(tcx, trait_item, crate_name, trait_method)
}
