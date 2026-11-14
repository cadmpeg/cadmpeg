// SPDX-License-Identifier: Apache-2.0
use super::{unique_feature_match, ProjectInputs};
use crate::design::decode::scopes::chamfer_set as set_layout;
use crate::design::edge_resolve::resolved_edge_treatment_group_with_corners;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::chamfer_set::{DesignChamferSet, DesignChamferSetSpec};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    edge_treatments::{ChamferForm, ChamferGroup, ChamferSpec},
    EdgeSelection, FeatureDefinition, FeatureOperation,
};
use cadmpeg_ir::report::loss::LossNote;

type Specification = (ChamferSpec, Vec<LossNote>);

pub(super) struct Projection<'p, 'data> {
    legacy_compatible: bool,
    scope: &'p DesignParameterScope,
    inputs: &'p ProjectInputs<'data>,
    specifications: Option<Vec<Option<Specification>>>,
}

pub(super) fn prepare<'p, 'data>(
    scope: &'p DesignParameterScope,
    inputs: &'p ProjectInputs<'data>,
) -> Projection<'p, 'data> {
    let specifications = scope.chamfer_sets().map(|sets| {
        sets.sets
            .iter()
            .map(|set| set_spec(scope, set.spec, inputs))
            .collect::<Vec<_>>()
    });
    let legacy_compatible = scope.chamfer_sets().is_none_or(|sets| {
        sets.sets.first().is_some_and(|first| {
            sets.sets.iter().all(|set| {
                set_form(set.spec) == set_form(first.spec)
                    && unique_feature_match(inputs.construction_groups.iter().filter(|group| {
                        native_stream(&group.id) == native_stream(&scope.id)
                            && group.scope_record_index == scope.record_index
                            && group.record_index == set.group_record_index
                    }))
                    .is_some()
            })
        }) && set_layout::agrees_with_owner_order(scope, sets, inputs.owners) == Some(true)
            && specifications.as_ref().is_some_and(|specs| {
                specs
                    .iter()
                    .all(|spec| spec.as_ref().is_some_and(|(_, notes)| notes.is_empty()))
            })
    });
    Projection {
        legacy_compatible,
        scope,
        inputs,
        specifications,
    }
}

impl Projection<'_, '_> {
    pub(super) fn legacy_compatible(&self) -> bool {
        self.legacy_compatible
    }
    pub(super) fn project(
        self,
        parameters: &[(u32, &DesignParameter)],
        ctx: &DecodeContext<'_>,
        losses: &mut Vec<LossNote>,
    ) -> Result<FeatureDefinition, CodecError> {
        let Self {
            scope,
            inputs,
            specifications,
            ..
        } = self;
        if let (Some(sets), Some(specifications)) = (scope.chamfer_sets(), specifications) {
            let claimed = sets
                .sets
                .iter()
                .flat_map(|set| set_layout::spec_owners(set.spec))
                .flatten()
                .collect::<std::collections::HashSet<_>>();
            let claimed_parameters = inputs
                .owners
                .iter()
                .filter(|owner| {
                    native_stream(owner.id()) == native_stream(&scope.id)
                        && owner.scope_record_index() == scope.record_index
                        && claimed.contains(&owner.record_index())
                })
                .map(DesignParameterOwner::parameter_record_index)
                .collect::<std::collections::HashSet<_>>();
            let extra = parameters
                .iter()
                .filter(|(_, parameter)| {
                    !claimed_parameters.contains(&parameter.record_index)
                        && parameter
                            .owner_record_index()
                            .is_none_or(|owner| !claimed.contains(&owner))
                })
                .count();
            if extra != 0 {
                losses.push(F3dLossCode::ChamferDefinitionUnresolved.note(format!(
                "Chamfer {} retains {} additional parameter(s) in native metadata because its set slots do not assign their behavior.", scope.id, extra
            )));
            }
            if sets.corner_type
                != crate::records::feature::chamfer_set::DesignChamferCornerType::Chamfer
            {
                losses.push(F3dLossCode::ChamferCornerTreatmentUnresolved.note(format!(
                "Chamfer {} retains {:?} corner treatment in its native set metadata; neutral corner construction remains unresolved.",
                scope.id, sets.corner_type
            )));
            }
            let mut groups = Vec::new();
            for (set, specification) in sets.sets.iter().zip(specifications) {
                if set.tangent_chain {
                    losses.push(F3dLossCode::ChamferTangentChainAssumed.note(format!(
                    "Chamfer set {} of {} uses its retained selector as the complete propagated selection for its tangent-chain flag (DR-460).",
                    set.record_index, scope.id
                )));
                }
                let spec = match specification {
                    Some((spec, notes)) => {
                        losses.extend(notes);
                        spec
                    }
                    None => {
                        losses.push(F3dLossCode::ChamferDefinitionUnresolved.note(format!(
                        "Chamfer set {} of {} retains its form with unresolved dimensions; its parameter ownership or scalar is incomplete.",
                        set.record_index, scope.id
                    )));
                        ChamferSpec::Unresolved {
                            form: Some(set_form(set.spec)),
                        }
                    }
                };
                if !spec.is_unresolved()
                    && !matches!(set.spec, DesignChamferSetSpec::EqualDistance { .. })
                {
                    losses.push(F3dLossCode::ChamferReferenceFaceAssumed.note(format!(
                    "Chamfer set {} of {} applies its owner-slot order with forward direction; the input reference-face assignment is assumed (DR-341).",
                    set.record_index, scope.id
                )));
                    if matches!(set.spec, DesignChamferSetSpec::DistanceAngle { .. }) {
                        losses.push(F3dLossCode::ChamferAngleReferenceAssumed.note(format!(
                        "Chamfer set {} of {} reads the stored radian angle as the neutral angle from the distance reference face (DR-343).",
                        set.record_index, scope.id
                    )));
                    }
                }
                let group =
                    unique_feature_match(inputs.construction_groups.iter().filter(|group| {
                        native_stream(&group.id) == native_stream(&scope.id)
                            && group.scope_record_index == scope.record_index
                            && group.record_index == set.group_record_index
                    }));
                let edges = match group {
                    Some(group) => selection(scope, group, inputs, ctx, losses)?,
                    None => unresolved_selection(scope, Some(set), losses),
                };
                groups.push(ChamferGroup { edges, spec });
            }
            if let Ok(groups) = groups.try_into() {
                return Ok(FeatureDefinition::Operation(FeatureOperation::Chamfer {
                    groups,
                    flip_direction: false,
                }));
            }
        }
        losses.push(F3dLossCode::ChamferDefinitionUnresolved.note(format!(
        "Chamfer {} retains its selectors and parameter expressions with an unresolved specification; the dimensional lanes do not establish a complete pairing.",
        scope.id
    )));
        let mut groups = Vec::new();
        let mut native_groups = inputs
            .construction_groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
                    && group.extrude_role().is_none()
            })
            .collect::<Vec<_>>();
        native_groups.sort_by_key(|group| group.scope_reference_ordinal);
        for group in native_groups {
            groups.push(ChamferGroup {
                edges: selection(scope, group, inputs, ctx, losses)?,
                spec: ChamferSpec::Unresolved { form: None },
            });
        }
        let groups = groups.try_into().unwrap_or_else(|_| {
            cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                edges: unresolved_selection(scope, None, losses),
                spec: ChamferSpec::Unresolved { form: None },
            })
        });
        Ok(FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            flip_direction: false,
        }))
    }
}

fn set_form(spec: DesignChamferSetSpec) -> ChamferForm {
    match spec {
        DesignChamferSetSpec::EqualDistance { .. } => ChamferForm::Distance,
        DesignChamferSetSpec::TwoDistances { .. } => ChamferForm::TwoDistances,
        DesignChamferSetSpec::DistanceAngle { .. } => ChamferForm::DistanceAngle,
    }
}

#[derive(Clone, Copy, Debug)]
enum Slot {
    Distance,
    FirstDistance,
    SecondDistance,
    Angle,
}

fn source_scalar<'a>(
    scope: &DesignParameterScope,
    record_index: u32,
    slot: Slot,
    inputs: &ProjectInputs<'a>,
    losses: &mut Vec<LossNote>,
) -> Option<(f64, Option<&'a str>)> {
    let stream = native_stream(&scope.id)?;
    let mut owners = inputs.owners.iter().filter(|owner| {
        native_stream(owner.id()) == Some(stream) && owner.record_index() == record_index
    });
    let owner = owners.next();
    if owners.next().is_some()
        || owner.is_some_and(|owner| owner.scope_record_index() != scope.record_index)
    {
        return None;
    }
    let mut parameters = inputs.native.iter().filter(|parameter| {
        native_stream(&parameter.id) == Some(stream)
            && owner.map_or_else(
                || parameter.owner_record_index() == Some(record_index),
                |owner| parameter.record_index == owner.parameter_record_index(),
            )
    });
    let parameter = parameters.next();
    if parameters.next().is_some() {
        return None;
    }
    let Some(parameter) = parameter else {
        let value = match owner {
            Some(owner) => owner.evaluated_value().get(),
            None => {
                use crate::records::feature::fixed_parameters::DesignFixedChamferParameters;
                match scope.fixed_chamfer_parameters()? {
                    DesignFixedChamferParameters::EqualDistance { distance }
                        if distance.record_index == record_index =>
                    {
                        distance.value.get()
                    }
                    DesignFixedChamferParameters::TwoDistances { first, .. }
                        if first.record_index == record_index =>
                    {
                        first.value.get()
                    }
                    DesignFixedChamferParameters::TwoDistances { second, .. }
                        if second.record_index == record_index =>
                    {
                        second.value.get()
                    }
                    _ => return None,
                }
            }
        };
        losses.push(F3dLossCode::ChamferParameterValueAssumed.note(format!(
            "Chamfer {} reads owner {}'s stored scalar as its {:?} value because the parameter definition is absent.",
            scope.id, record_index, slot
        )));
        return Some((value, None));
    };
    if parameter.owner_record_index() != Some(record_index) {
        return None;
    }
    let unit = parameter.unit().map(|field| field.value.as_str());
    let valid_unit = unit.is_some_and(|unit| match slot {
        Slot::Angle => super::design_angle_unit(unit),
        _ => super::design_length_unit(unit),
    });
    if !valid_unit {
        losses.push(F3dLossCode::ChamferParameterUnitAssumed.note(format!(
            "Chamfer {} reads parameter {}'s canonical scalar as {:?} despite unit {:?}.",
            scope.id, parameter.record_index, slot, unit
        )));
    }
    if owner.is_none() {
        losses.push(F3dLossCode::ChamferParameterOwnerAssumed.note(format!(
            "Chamfer {} binds parameter {} to owner {} through its backlink because the owner definition is absent.",
            scope.id, parameter.record_index, record_index
        )));
    }
    let kinds = match slot {
        Slot::Distance => set_layout::DISTANCE_KINDS,
        Slot::FirstDistance => set_layout::FIRST_DISTANCE_KINDS,
        Slot::SecondDistance => set_layout::SECOND_DISTANCE_KINDS,
        Slot::Angle => set_layout::ANGLE_KINDS,
    };
    let kind = parameter.source_kind_str();
    if !kinds.contains(&kind) {
        losses.push(F3dLossCode::ChamferParameterKindAssumed.note(format!(
            "Chamfer {} reads parameter {} as the {:?} slot of its explicit set despite source kind {}.",
            scope.id, parameter.record_index, slot, kind
        )));
    }
    Some((parameter.evaluated_value().get(), Some(kind)))
}

fn set_spec(
    scope: &DesignParameterScope,
    spec: DesignChamferSetSpec,
    inputs: &ProjectInputs<'_>,
) -> Option<Specification> {
    let mut losses = Vec::new();
    let distance = |record_index, slot, losses: &mut Vec<LossNote>| {
        let (value, kind) = source_scalar(scope, record_index, slot, inputs, losses)?;
        Some((cadmpeg_ir::scalar::PositiveLength::new(value * 10.0)?, kind))
    };
    let spec = match spec {
        DesignChamferSetSpec::EqualDistance {
            distance_owner_record_index,
        } => ChamferSpec::Distance {
            distance: distance(distance_owner_record_index, Slot::Distance, &mut losses)?.0,
        },
        DesignChamferSetSpec::TwoDistances {
            first_owner_record_index,
            second_owner_record_index,
        } => {
            let (first, first_kind) =
                distance(first_owner_record_index, Slot::FirstDistance, &mut losses)?;
            let (second, second_kind) =
                distance(second_owner_record_index, Slot::SecondDistance, &mut losses)?;
            if matches!((first_kind, second_kind), (Some(first), Some(second))
                if set_layout::FIRST_DISTANCE_KINDS.contains(&first)
                    && set_layout::SECOND_DISTANCE_KINDS.contains(&second)
                    && !set_layout::two_distance_kinds_pair(first, second))
            {
                losses.push(F3dLossCode::ChamferParameterKindAssumed.note(format!(
                    "Chamfer {} pairs distance owners {} and {} by their explicit set slots across parameter namespaces.",
                    scope.id, first_owner_record_index, second_owner_record_index
                )));
            }
            ChamferSpec::TwoDistances { first, second }
        }
        DesignChamferSetSpec::DistanceAngle {
            distance_owner_record_index,
            angle_owner_record_index,
        } => {
            let distance = distance(distance_owner_record_index, Slot::Distance, &mut losses)?.0;
            let (angle, _) = source_scalar(
                scope,
                angle_owner_record_index,
                Slot::Angle,
                inputs,
                &mut losses,
            )?;
            let angle =
                cadmpeg_ir::scalar::InteriorAngle::try_from(cadmpeg_ir::scalar::Angle::new(angle)?)
                    .ok()?;
            ChamferSpec::DistanceAngle { distance, angle }
        }
    };
    Some((spec, losses))
}

fn selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<EdgeSelection, CodecError> {
    resolved_edge_treatment_group_with_corners(
        group,
        crate::design::edge_resolve::EdgeTreatmentInputs {
            groups: inputs.construction_groups,
            operands: inputs.edge_operands,
            identity_operands: inputs.edge_identity_operands,
            vertex_operands: inputs.edge_treatment_vertex_operands,
            histories: inputs.histories,
            previous_state_id: scope.previous_history_state_id(),
            feature_id: &crate::ids::neutral_feature_id(scope),
            treatment_radius: None,
        },
        ctx,
        losses,
    )
}

pub(super) fn unresolved_selection(
    scope: &DesignParameterScope,
    set: Option<&DesignChamferSet>,
    losses: &mut Vec<LossNote>,
) -> EdgeSelection {
    let selection = match set.zip(native_stream(&scope.id)) {
        Some((set, stream)) => format!("{stream}:design-record-header#{}", set.byte_offset),
        None => scope.id.clone(),
    };
    losses.push(F3dLossCode::ChamferSelectionUnresolved.note(format!(
        "Chamfer {} retains native selection {} because its construction group is missing or ambiguous.",
        scope.id, selection
    )));
    EdgeSelection::Native(selection)
}

pub(super) fn retained_parameters<'a>(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'a>,
) -> Vec<&'a DesignParameter> {
    let stream = native_stream(&scope.id);
    let mut references = scope
        .reference_members()
        .values()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    if let Some(sets) = scope.chamfer_sets() {
        references.extend(
            sets.sets
                .iter()
                .flat_map(|set| set_layout::spec_owners(set.spec))
                .flatten(),
        );
    }
    let owned = inputs
        .owners
        .iter()
        .filter(|owner| {
            native_stream(owner.id()) == stream && owner.scope_record_index() == scope.record_index
        })
        .map(DesignParameterOwner::parameter_record_index)
        .collect::<std::collections::HashSet<_>>();
    inputs
        .native
        .iter()
        .filter(|parameter| {
            native_stream(&parameter.id) == stream
                && (owned.contains(&parameter.record_index)
                    || parameter
                        .owner_record_index()
                        .is_some_and(|owner| references.contains(&owner)))
        })
        .collect()
}
