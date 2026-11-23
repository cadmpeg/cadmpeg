// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal edge flange and hem projection.

use crate::design::edge_resolve::resolved_edge_flange_group;
use crate::design::face_resolve::design_angle;
use crate::design::feature_project::design_length;
use crate::design::feature_project::design_positive_length;
use crate::design::feature_project::merge_edge_selections;

use crate::design::feature_project::unique_feature_match;
use crate::design::feature_project::ProjectInputs;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Project a sheet-metal `EdgeFlange` scope onto its neutral operation.
///
/// Owner parameters supply the height, angle, and width. A to-object
/// height resolves its target entity to a known neutral construction feature;
/// otherwise the source selection remains explicit in the neutral height law.
pub(super) fn project_edge_flange(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::sheet_metal::{
        DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeWidthParameterSource,
        DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::sheet_metal_forms::{
        SheetMetalBendPosition, SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget,
        SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth, SheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let ProjectInputs {
        native: parameters,
        owners,
        scopes,
        construction_groups: groups,
        edge_operands,
        edge_identity_operands,
        entity_selection_operands,
        ..
    } = inputs;
    let Some((operation, stream, height, angle, width, height_datum, bend_position)) =
        (|| -> Result<Option<_>, CodecError> {
            let operation = or_none!(scope.edge_flange_operation());
            let stream = or_none!(native_stream(&scope.id));
            let parameter = |owner_record_index, source_kind: &str| {
                let mut matching = owners.iter().filter(|owner| {
                    native_stream(owner.id()) == Some(stream)
                        && owner.scope_record_index() == scope.record_index
                        && owner.record_index() == owner_record_index
                });
                let owner = matching.next()?;
                if matching.next().is_some() {
                    return None;
                }
                parameters.iter().find(|parameter| {
                    native_stream(&parameter.id) == Some(stream)
                        && parameter.record_index == owner.parameter_record_index()
                        && parameter.source_kind() == source_kind
                })
            };

            let height = match &operation.selection.shape().height() {
                DesignEdgeFlangeHeightExtent::Distance => {
                    SheetMetalFlangeHeight::Distance(or_none!(design_positive_length(or_none!(
                        parameter(operation.height_owner_record_index, "FlangeHeight",)
                    ))))
                }
                DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    ..
                } => {
                    let mut target_groups = groups.iter().filter(|group| {
                        native_stream(&group.id) == Some(stream)
                            && group.scope_record_index == scope.record_index
                            && group.record_index == *target_group_record_index
                            && group.role() == DesignOperandRole::ROLE_0X21
                            && group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([*target_operand_record_index])
                    });
                    let target_group = or_none!(target_groups.next());
                    if target_groups.next().is_some() {
                        return Ok(None);
                    }
                    let mut target_selections =
                        entity_selection_operands.iter().filter(|operand| {
                            native_stream(&operand.id) == Some(stream)
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == target_group.record_index
                                && operand.group_member_ordinal == 0
                                && operand.record_index() == *target_operand_record_index
                        });
                    let target_selection = or_none!(target_selections.next());
                    if target_selections.next().is_some() {
                        return Ok(None);
                    }
                    let target_record_index =
                        or_none!(u32::try_from(target_selection.primary_identity)
                            .ok()
                            .and_then(|index| index.checked_add(1)));
                    let mut target_scopes = scopes.iter().filter(|candidate| {
                        native_stream(&candidate.id) == Some(stream)
                            && candidate.record_index == target_record_index
                            && matches!(
                                candidate.kind(),
                                crate::records::feature::scope::DesignFeatureKind::WorkPlane
                                    | crate::records::feature::scope::DesignFeatureKind::WorkPoint
                            )
                    });
                    let target = match target_scopes.next() {
                        Some(target_scope) if target_scopes.next().is_none() => {
                            SheetMetalFlangeHeightTarget::Feature(crate::ids::neutral_feature_id(
                                target_scope,
                            ))
                        }
                        None => SheetMetalFlangeHeightTarget::Native(target_selection.id.clone()),
                        Some(_) => return Ok(None),
                    };
                    let offset = or_none!(design_length(or_none!(parameter(
                        *offset_owner_record_index,
                        "ToObjectOffset"
                    ))));
                    SheetMetalFlangeHeight::to_object(target, offset)
                }
            };
            let angle = or_none!(design_angle(or_none!(parameter(
                operation.angle_owner_record_index,
                "FlangeAngle",
            ))));

            let width = match &operation.selection.shape() {
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::FullEdge {
                    ..
                } => SheetMetalFlangeWidth::FullEdge,
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::Symmetric {
                    owner,
                    ..
                } => SheetMetalFlangeWidth::symmetric(or_none!(design_positive_length(or_none!(
                    parameter(*owner, "EdgeWidth")
                )))),
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::SymmetricPerEdge(
                    edges,
                ) => {
                    let mut widths = edges.iter().map(|row| {
                        parameter(row.owners, "EdgeWidth").and_then(design_positive_length)
                    });
                    let first = or_none!(or_none!(widths.next()));
                    if widths.any(|width| width != Some(first)) {
                        return Ok(None);
                    }
                    SheetMetalFlangeWidth::symmetric(first)
                }
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                    edges,
                    source,
                } => {
                    if *source == DesignEdgeFlangeWidthParameterSource::EdgeOffset {
                        let mut offsets = Vec::new();
                        for row in edges {
                            let offset =
                                cadmpeg_ir::features::sheet_metal::SheetMetalFlangeOffset {
                                    first: Some(or_none!(design_length(or_none!(parameter(
                                        row.owners[0],
                                        "EdgeOffset_1"
                                    ))))),
                                    second: Some(or_none!(design_length(or_none!(parameter(
                                        row.owners[1],
                                        "EdgeOffset_2"
                                    ))))),
                                    first_reference:
                                        cadmpeg_ir::features::selections::SelectionReference::new(
                                            scope.id.clone(),
                                        )
                                        .ok(),
                                    second_reference:
                                        cadmpeg_ir::features::selections::SelectionReference::new(
                                            scope.id.clone(),
                                        )
                                        .ok(),
                                };
                            offsets.push(offset);
                        }
                        SheetMetalFlangeWidth::TwoOffsetsPerEdge {
                            offsets: or_none!(
                                cadmpeg_ir::features::selections::NonEmptyMembers::try_from(
                                    offsets
                                )
                                .ok()
                            ),
                        }
                    } else {
                        let mut widths = Vec::new();
                        for row in edges {
                            widths.push(SheetMetalFlangeTwoSidedWidth::complete(
                                or_none!(design_positive_length(or_none!(parameter(
                                    row.owners[0],
                                    "EdgeWidth_1"
                                )))),
                                or_none!(design_positive_length(or_none!(parameter(
                                    row.owners[1],
                                    "EdgeWidth_2"
                                )))),
                            ));
                        }
                        SheetMetalFlangeWidth::TwoSidesPerEdge {
                            widths: or_none!(
                                cadmpeg_ir::features::sheet_metal_forms::SheetMetalFlangeEdgeWidths::new(widths).ok()
                            ),
                        }
                    }
                }
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSides {
                    owners: [first, second],
                    ..
                } => SheetMetalFlangeWidth::two_sides(
                    or_none!(design_positive_length(or_none!(parameter(
                        *first,
                        "EdgeWidth_1"
                    )))),
                    or_none!(design_positive_length(or_none!(parameter(
                        *second,
                        "EdgeWidth_2"
                    )))),
                ),
            };

            let height_datum = match operation.height_datum {
                DesignSheetMetalHeightDatum::InnerFaces => SheetMetalHeightDatum::InnerFaces,
                DesignSheetMetalHeightDatum::OuterFaces => SheetMetalHeightDatum::OuterFaces,
                DesignSheetMetalHeightDatum::Unknown(_) => return Ok(None),
            };
            let bend_position = match operation.bend_position {
                DesignBendPosition::Outside => SheetMetalBendPosition::Outside,
                DesignBendPosition::Inside => SheetMetalBendPosition::Inside,
                DesignBendPosition::Adjacent => SheetMetalBendPosition::Adjacent,
                DesignBendPosition::TangentToSide => SheetMetalBendPosition::TangentToSide,
                DesignBendPosition::Unknown(_) => return Ok(None),
            };
            Ok(Some((
                operation,
                stream,
                height,
                angle,
                width,
                height_datum,
                bend_position,
            )))
        })()?
    else {
        return Ok(None);
    };

    // Each role-`0x08` group carries one selected edge. The aggregate role-`0x43`
    // group repeats them, so it contributes no separate selection.
    if operation.selection.shape().edges().next().is_none() {
        return Ok(None);
    }
    let bend_radius = if matches!(scope.class_tag.as_str(), "361" | "362")
        && operation.selection.shape().edges().count() > 1
    {
        let mut aggregates = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == operation.selection.aggregate_group_record_index()
        });
        let aggregate = or_none!(aggregates.next());
        if aggregates.next().is_some()
            || aggregate.role() != DesignOperandRole::ROLE_0X43
            || !aggregate
                .members()
                .iter()
                .map(|member| member.value)
                .eq(operation
                    .selection
                    .shape()
                    .edges()
                    .map(|edge| edge.aggregate_operand_record_index))
        {
            return Ok(None);
        }
        let mut matching = inputs.sheet_metal_settings.iter().filter(|settings| {
            native_stream(settings.id()) == Some(stream)
                && settings.record_index == u64::from(operation.settings_record_index)
                && settings.owner_record_index == u64::from(scope.record_index)
        });
        let settings = or_none!(matching.next());
        if matching.next().is_some() || settings.mask != Some(0) {
            return Ok(None);
        }
        let thickness = or_none!(settings.rule_thickness).get() * 10.0;
        let mut selected = operation.selection.shape().edges();
        let first = or_none!(selected.next());
        let second = or_none!(selected.next());
        if selected.next().is_some() {
            return Ok(None);
        }
        let slot = |edge: &crate::records::feature::sheet_metal::DesignEdgeFlangeEdge| {
            unique_feature_match(edge_operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.record_index() == edge.operand_record_index()
            }))?
            .resolved_edge_slot
        };
        or_none!(crate::history::edge_flange::inside_radius_for_scope(
            scope,
            [or_none!(slot(first)), or_none!(slot(second))],
            thickness,
            inputs.histories,
        ))
    } else {
        or_none!(cadmpeg_ir::scalar::PositiveLength::new(
            operation.bend_radius.get() * 10.0
        ))
    };
    let mut selections = Vec::new();
    for edge in operation.selection.shape().edges() {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == edge.group_record_index.get()
        });
        let Some(edge_group) = matching.next() else {
            return Ok(None);
        };
        if matching.next().is_some()
            || edge_group.role() != DesignOperandRole::BODIES_B
            || !edge_group
                .members()
                .iter()
                .map(|member| member.value)
                .eq([edge.operand_record_index()])
        {
            return Ok(None);
        }
        let selection = resolved_edge_flange_group(
            edge_group,
            groups,
            edge_operands,
            edge_identity_operands,
            scope.previous_history_state_id(),
            &crate::ids::neutral_feature_id(scope),
            ctx,
        )?;
        selections.push(selection);
    }
    let edges = if selections.len() == 1 {
        or_none!(selections.into_iter().next())
    } else {
        merge_edge_selections(scope, &selections)
    };

    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalEdgeFlange(
            cadmpeg_ir::features::sheet_metal::SheetMetalEdgeFlangeConstruction::complete(
                edges,
                height,
                angle,
                height_datum,
                bend_position,
                width,
                bend_radius.into(),
            ),
        ),
    )))
}

/// Project a sheet-metal `Hem` scope onto its neutral operation.
///
/// The owner layout distinguishes the rolled and teardrop forms from the
/// shared gap-and-length layout. The native Flip control supplies fold direction;
/// inserted coaxial bend carriers distinguish Flat from Open gap-and-length forms.
pub(super) fn project_hem(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::sheet_metal::DesignHemParameterOwners;
    use cadmpeg_ir::features::sheet_metal_forms::{SheetMetalHemDirection, SheetMetalHemForm};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let ProjectInputs {
        native: parameters,
        owners,
        construction_groups: groups,
        edge_operands,
        edge_identity_operands,
        histories,
        ..
    } = inputs;
    let Some((operation, form, edge_group)) = (|| {
        let operation = scope.hem_operation()?;
        let stream = native_stream(&scope.id)?;
        let parameter = |owner_record_index: u32, source_kind: &str| {
            let mut matching_owners = owners.iter().filter(|owner| {
                native_stream(owner.id()) == Some(stream)
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == owner_record_index
            });
            let owner = matching_owners.next()?;
            if matching_owners.next().is_some() {
                return None;
            }
            let mut matching_parameters = parameters.iter().filter(|parameter| {
                native_stream(&parameter.id) == Some(stream)
                    && parameter.record_index == owner.parameter_record_index()
                    && parameter.source_kind() == source_kind
            });
            let parameter = matching_parameters.next()?;
            matching_parameters.next().is_none().then_some(parameter)
        };

        let form =
            match &operation.parameter_owners {
                DesignHemParameterOwners::GapLength {
                    gap_owner_record_index,
                    length_owner_record_index,
                } => SheetMetalHemForm::GapLength {
                    gap: cadmpeg_ir::scalar::NonNegativeLength::try_from(design_length(
                        parameter(*gap_owner_record_index, "HemGap")?,
                    )?)
                    .ok()?,
                    length: design_positive_length(parameter(
                        *length_owner_record_index,
                        "HemLength",
                    )?)?,
                },
                DesignHemParameterOwners::RadiusAngle {
                    radius_owner_record_index,
                    angle_owner_record_index,
                } => SheetMetalHemForm::Rolled {
                    radius: design_positive_length(parameter(
                        *radius_owner_record_index,
                        "HemRadius",
                    )?)?,
                    angle: design_angle(parameter(*angle_owner_record_index, "HemAngle")?)?,
                },
                DesignHemParameterOwners::GapLengthRadius {
                    gap_owner_record_index,
                    length_owner_record_index,
                    radius_owner_record_index,
                } => SheetMetalHemForm::Teardrop {
                    gap: cadmpeg_ir::scalar::NonNegativeLength::try_from(design_length(
                        parameter(*gap_owner_record_index, "HemGap")?,
                    )?)
                    .ok()?,
                    length: design_positive_length(parameter(
                        *length_owner_record_index,
                        "HemLength",
                    )?)?,
                    radius: design_positive_length(parameter(
                        *radius_owner_record_index,
                        "HemRadius",
                    )?)?,
                },
            };

        let mut edge_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == operation.edge_group_record_index.get()
        });
        let edge_group = edge_groups.next()?;
        let edge_has_extra = edge_groups.next().is_some();
        let edge_role_ok = edge_group.role() == DesignOperandRole::BODIES_B;
        let edge_members_ok = edge_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.edge_operand_record_index()]);
        if edge_has_extra || !edge_role_ok || !edge_members_ok {
            return None;
        }

        let mut aggregate_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == operation.aggregate_group_record_index.get()
        });
        let aggregate_group = aggregate_groups.next()?;
        let aggregate_has_extra = aggregate_groups.next().is_some();
        let aggregate_role_ok = aggregate_group.role() == DesignOperandRole::ROLE_0X43;
        let aggregate_members_ok = aggregate_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.aggregate_operand_record_index()]);
        if aggregate_has_extra || !aggregate_role_ok || !aggregate_members_ok {
            return None;
        }
        Some((operation, form, edge_group))
    })() else {
        return Ok(None);
    };

    let edges = crate::design::edge_resolve::resolved_hem_edge_group(
        edge_group,
        groups,
        edge_operands,
        edge_identity_operands,
        crate::history::effective_scope_previous_history_state_id(scope, histories),
        &crate::ids::neutral_feature_id(scope),
        ctx,
    )?;

    let edge_slot = match unique_feature_match(edge_operands.iter().filter(|operand| {
        native_stream(&operand.id) == native_stream(&edge_group.id)
            && operand.scope_record_index == edge_group.scope_record_index
            && operand.record_index() == operation.edge_operand_record_index()
    })) {
        Some(operand) => crate::design::edge_resolve::resolved_hem_edge_slot(
            operand,
            crate::history::effective_scope_previous_history_state_id(scope, histories),
        ),
        None => None,
    };
    let gap_length_form = edge_slot.and_then(|edge_slot| {
        crate::history::hem_gap_length_form_for_scope(scope, edge_slot, histories)
    });
    let form = match (form, gap_length_form) {
        (
            SheetMetalHemForm::GapLength { gap: _, length },
            Some(crate::history::HemGapLengthForm::Flat),
        ) => SheetMetalHemForm::Flat { length },
        (
            SheetMetalHemForm::GapLength { gap, length },
            Some(crate::history::HemGapLengthForm::Open),
        ) => SheetMetalHemForm::Open { gap, length },
        (form, _) => form,
    };
    let direction = if operation.flipped {
        SheetMetalHemDirection::Reverse
    } else {
        SheetMetalHemDirection::Forward
    };

    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalHem {
            edges,
            bend_position: operation.bend_position,
            form,
            direction,
            bend_radius: or_none!(cadmpeg_ir::scalar::PositiveLength::new(
                operation.bend_radius.get() * 10.0,
            )),
        },
    )))
}
