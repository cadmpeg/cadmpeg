use super::{merge_edge_selections, native_stream, unique_feature_match, ProjectInputs};
use crate::design::edge_resolve::resolved_edge_flange_group;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::sheet_metal::{DesignBendPosition, DesignSheetMetalHeightDatum};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::sheet_metal::{SheetMetalEdgeFlangeConstruction, SheetMetalFlangeOffset};
use cadmpeg_ir::features::{
    EdgeSelection, FeatureDefinition, FeatureOperation, NonEmptyMembers, SelectionReference,
    SheetMetalBendPosition, SheetMetalFlangeHeight, SheetMetalFlangeWidth, SheetMetalHeightDatum,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Length, NonNegativeLength, PositiveLength};

mod legacy_inputs;

fn role_owner(scope: &DesignParameterScope, kind: &str, inputs: &ProjectInputs<'_>) -> Option<u32> {
    let stream = native_stream(&scope.id)?;
    let owners = inputs
        .owners
        .iter()
        .filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
                && inputs.native.iter().any(|parameter| {
                    native_stream(&parameter.id) == Some(stream)
                        && parameter.record_index == owner.parameter_record_index()
                        && parameter.source_kind() == kind
                })
        })
        .map(DesignParameterOwner::record_index);
    let backlinks = inputs
        .native
        .iter()
        .filter(|parameter| {
            native_stream(&parameter.id) == Some(stream) && parameter.source_kind() == kind
        })
        .filter_map(DesignParameter::owner_record_index)
        .filter(|index| {
            scope
                .reference_members()
                .values()
                .any(|value| value == index)
                && !inputs.owners.iter().any(|owner| {
                    native_stream(owner.id()) == Some(stream) && owner.record_index() == *index
                })
        });
    let mut found = None;
    for index in owners.chain(backlinks) {
        if found.is_some_and(|previous| previous != index) {
            return None;
        }
        found = Some(index);
    }
    found
}

fn scalar(
    scope: &DesignParameterScope,
    index: Option<u32>,
    kind: &str,
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Option<f64> {
    let index = index?;
    let stream = native_stream(&scope.id)?;
    let mut owners = inputs.owners.iter().filter(|owner| {
        native_stream(owner.id()) == Some(stream)
            && owner.scope_record_index() == scope.record_index
            && owner.record_index() == index
    });
    let owner = owners.next();
    if owners.next().is_some() {
        return None;
    }
    let mut parameters = inputs.native.iter().filter(|parameter| {
        native_stream(&parameter.id) == Some(stream)
            && parameter.source_kind() == kind
            && match owner {
                Some(owner) => parameter.record_index == owner.parameter_record_index(),
                None => parameter.owner_record_index() == Some(index),
            }
    });
    let parameter = parameters.next();
    if parameters.next().is_some() {
        return None;
    }
    let angular = kind == "FlangeAngle";
    let value = if let Some(parameter) = parameter {
        if owner.is_none() {
            losses.push(F3dLossCode::EdgeParameterOwnerBacklinkAssumed.note(format!(
                "EdgeFlange scope {} joins missing owner {} to its unique {} definition backlink.",
                scope.id, index, kind
            )));
        }
        let unit = parameter.unit().map(|field| field.value.as_str());
        if !unit.is_some_and(if angular {
            super::design_angle_unit
        } else {
            super::design_length_unit
        }) {
            losses.push(F3dLossCode::EdgeParameterUnitAssumed.note(format!(
                "EdgeFlange scope {} reads {} owner {} in database {} units despite its unresolved display unit.",
                scope.id, kind, index, if angular { "radian" } else { "centimetre" }
            )));
        }
        parameter.evaluated_value().get()
    } else {
        let owner = owner?;
        if inputs.native.iter().any(|parameter| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.record_index == owner.parameter_record_index()
        }) {
            return None;
        }
        losses.push(F3dLossCode::EdgeCachedParameterAssumed.note(format!(
            "EdgeFlange scope {} reads owner {} cached scalar as its missing {} definition in database units.", scope.id, index, kind
        )));
        owner.evaluated_value().get()
    };
    let value = value * if angular { 1.0 } else { 10.0 };
    value.is_finite().then_some(value)
}

fn boundary(
    scope: &DesignParameterScope,
    index: Option<u32>,
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Option<SelectionReference> {
    let group = index.and_then(|index| {
        unique_feature_match(inputs.construction_groups.iter().filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && group.record_index == index
        }))
    });
    let (code, source) = match group {
        Some(group) => (F3dLossCode::EdgeBoundaryReferenceNative, group.id.clone()),
        None => (
            F3dLossCode::EdgeBoundaryScopeReferenceAssumed,
            scope.id.clone(),
        ),
    };
    losses.push(code.note(format!(
        "EdgeFlange scope {} retains boundary {:?} through source reference {}.",
        scope.id, index, source
    )));
    SelectionReference::new(source).ok()
}

fn unresolved(
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    field: &str,
) {
    losses.push(code.note(format!(
        "EdgeFlange scope {} retains an unresolved {}.",
        scope.id, field
    )));
}

pub(super) fn project(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    let mut explicit_radii = inputs.native.iter().filter(|parameter| {
        native_stream(&parameter.id) == native_stream(&scope.id)
            && parameter.source_kind() == "BendRadius"
            && parameter.owner_record_index().is_some_and(|index| {
                scope
                    .reference_members()
                    .values()
                    .any(|value| *value == index)
            })
    });
    let explicit_radius = explicit_radii.next();
    let preferred_radius = if explicit_radii.next().is_none() {
        explicit_radius
            .and_then(|parameter| {
                scalar(
                    scope,
                    parameter.owner_record_index(),
                    "BendRadius",
                    inputs,
                    losses,
                )
            })
            .and_then(NonNegativeLength::new)
    } else {
        None
    };
    if let Some(mut definition) =
        crate::design::feature_project::sheet_metal::project_edge_flange(scope, inputs, ctx)?
    {
        if let (
            Some(radius),
            FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange(construction)),
        ) = (preferred_radius, &mut definition)
        {
            construction.bend_radius = Some(radius);
        }
        if matches!(&definition, FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange(value))
            if matches!(value.width, Some(SheetMetalFlangeWidth::TwoOffsetsPerEdge { .. })))
        {
            losses.push(F3dLossCode::EdgeSignedOffsetsAssumed.note(format!(
                "EdgeFlange scope {} reads signed EdgeOffset pairs in source edge order.",
                scope.id
            )));
            losses.push(F3dLossCode::EdgeBoundaryScopeReferenceAssumed.note(format!("EdgeFlange scope {} retains legacy offset boundary programs through its scope reference.", scope.id)));
        }
        return Ok(definition);
    }
    let reference = scope.edge_flange_construction();
    let legacy = scope.edge_flange_operation();
    let mut construction = SheetMetalEdgeFlangeConstruction::default();
    if reference.is_none() && legacy.is_none() {
        unresolved(
            scope,
            losses,
            F3dLossCode::EdgeReferenceEnvelopeUnresolved,
            "construction frame",
        );
    }
    let position = reference
        .and_then(|value| value.bend_position.as_ref())
        .map(|value| DesignBendPosition::from_code(value.value))
        .or_else(|| legacy.map(|value| value.bend_position));
    construction.bend_position = match position {
        Some(DesignBendPosition::Outside) => Some(SheetMetalBendPosition::Outside),
        Some(DesignBendPosition::Inside) => Some(SheetMetalBendPosition::Inside),
        Some(DesignBendPosition::Adjacent) => Some(SheetMetalBendPosition::Adjacent),
        Some(DesignBendPosition::TangentToSide) => Some(SheetMetalBendPosition::TangentToSide),
        _ => None,
    };
    let datum = reference
        .and_then(|value| value.height_datum.as_ref())
        .map(|value| DesignSheetMetalHeightDatum::from_code(value.value))
        .or_else(|| legacy.map(|value| value.height_datum));
    construction.height_datum = match datum {
        Some(DesignSheetMetalHeightDatum::InnerFaces) => Some(SheetMetalHeightDatum::InnerFaces),
        Some(DesignSheetMetalHeightDatum::OuterFaces) => Some(SheetMetalHeightDatum::OuterFaces),
        _ => None,
    };
    let height_owner = reference
        .and_then(|value| value.height_owner.as_ref())
        .map(|value| value.value)
        .or_else(|| legacy.map(|value| value.height_owner_record_index))
        .or_else(|| role_owner(scope, "FlangeHeight", inputs));
    let angle_owner = reference
        .and_then(|value| value.angle_owner.as_ref())
        .map(|value| value.value)
        .or_else(|| legacy.map(|value| value.angle_owner_record_index))
        .or_else(|| role_owner(scope, "FlangeAngle", inputs));
    construction.height = if legacy.is_some() {
        legacy_inputs::height(scope, inputs, losses)
    } else {
        scalar(scope, height_owner, "FlangeHeight", inputs, losses)
            .and_then(PositiveLength::new)
            .map(SheetMetalFlangeHeight::Distance)
    };
    construction.angle =
        scalar(scope, angle_owner, "FlangeAngle", inputs, losses).and_then(Angle::new);
    construction.bend_radius = preferred_radius;
    if construction.bend_radius.is_none() {
        construction.bend_radius = reference
            .and_then(|value| value.radius_control.as_ref())
            .map(|value| value.value.get())
            .or_else(|| legacy.map(|value| value.bend_radius.get()))
            .and_then(|value| NonNegativeLength::new(value * 10.0));
        if construction.bend_radius.is_some() {
            losses.push(F3dLossCode::EdgeStoredRadiusInsideAssumed.note(format!(
                "EdgeFlange scope {} reads its positive scope control as the inside bend radius in database centimetres (DR-161).", scope.id
            )));
        }
    }
    let mut selections = Vec::new();
    let indices: Vec<_> = match reference {
        Some(value) => value
            .edges
            .iter()
            .map(|edge| edge.group.as_ref().map(|group| group.value))
            .collect(),
        None => legacy
            .map(|value| {
                value
                    .selection
                    .shape()
                    .edges()
                    .map(|edge| Some(edge.group_record_index.get()))
                    .collect()
            })
            .unwrap_or_default(),
    };
    for index in indices {
        let group = index.and_then(|index| {
            unique_feature_match(inputs.construction_groups.iter().filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
                    && group.record_index == index
                    && group.role()
                        == crate::records::topology::extrude_selection::DesignOperandRole::BODIES_B
            }))
        });
        let selection = match group {
            Some(group) => resolved_edge_flange_group(
                group,
                inputs.construction_groups,
                inputs.edge_operands,
                inputs.edge_identity_operands,
                scope.previous_history_state_id(),
                &crate::ids::neutral_feature_id(scope),
                ctx,
            )?,
            None => EdgeSelection::Native(scope.id.clone()),
        };
        if matches!(
            selection,
            EdgeSelection::Native(_) | EdgeSelection::Unresolved
        ) {
            losses.push(F3dLossCode::EdgeSelectionNative.note(format!(
                "EdgeFlange scope {} retains edge group {:?} through its source selection.",
                scope.id, index
            )));
        }
        selections.push(selection);
    }
    construction.edges = match selections.len() {
        0 => None,
        1 => selections.pop(),
        _ => Some(merge_edge_selections(scope, &selections)),
    };
    if let Some(reference) = reference {
        if reference.edges.iter().any(|edge| {
            edge.wrapper_member_count
                .as_ref()
                .is_some_and(|count| count.value == 5)
        }) {
            let mut offsets = ctx.alloc_filled(
                reference.edges.len(),
                SheetMetalFlangeOffset::default(),
                "projecting EdgeFlange offsets",
            )?;
            for (row, edge) in offsets.iter_mut().zip(&reference.edges) {
                row.first = scalar(
                    scope,
                    edge.first_owner.as_ref().map(|value| value.value),
                    "EdgeOffset_1",
                    inputs,
                    losses,
                )
                .and_then(Length::new);
                row.second = scalar(
                    scope,
                    edge.second_owner.as_ref().map(|value| value.value),
                    "EdgeOffset_2",
                    inputs,
                    losses,
                )
                .and_then(Length::new);
                row.first_reference = boundary(
                    scope,
                    edge.first_boundary.as_ref().map(|value| value.value),
                    inputs,
                    losses,
                );
                row.second_reference = boundary(
                    scope,
                    edge.second_boundary.as_ref().map(|value| value.value),
                    inputs,
                    losses,
                );
                if row.first.is_none() || row.second.is_none() {
                    unresolved(
                        scope,
                        losses,
                        F3dLossCode::EdgeParameterUnresolved,
                        "edge offset",
                    );
                }
            }
            construction.width = NonEmptyMembers::try_from(offsets)
                .ok()
                .map(|offsets| SheetMetalFlangeWidth::TwoOffsetsPerEdge { offsets });
            losses.push(F3dLossCode::EdgeSignedOffsetsAssumed.note(format!("EdgeFlange scope {} reads wrapper-linked signed EdgeOffset pairs in main-edge order.", scope.id)));
        } else if !reference.edges.is_empty() {
            construction.width = Some(SheetMetalFlangeWidth::FullEdge);
            losses.push(F3dLossCode::EdgeFullWidthAssumed.note(format!("EdgeFlange scope {} reads reference-framed wrappers without a decoded offset pair as full-edge width.", scope.id)));
        }
    }
    if reference.is_none() {
        construction.width = legacy_inputs::width(scope, inputs, ctx, losses)?;
    }
    for (missing, code, field) in [
        (
            construction.bend_position.is_none(),
            F3dLossCode::EdgePositionUnresolved,
            "bend position",
        ),
        (
            construction.height_datum.is_none(),
            F3dLossCode::EdgeDatumUnresolved,
            "height datum",
        ),
        (
            construction.height.is_none(),
            F3dLossCode::EdgeParameterUnresolved,
            "height",
        ),
        (
            construction.angle.is_none(),
            F3dLossCode::EdgeParameterUnresolved,
            "angle",
        ),
        (
            construction.bend_radius.is_none(),
            F3dLossCode::EdgeRadiusUnresolved,
            "bend radius",
        ),
        (
            construction.edges.is_none(),
            F3dLossCode::EdgeSelectionNative,
            "edge selection",
        ),
        (
            construction.width.is_none(),
            F3dLossCode::EdgeWidthUnresolved,
            "width law",
        ),
    ] {
        if missing {
            unresolved(scope, losses, code, field);
        }
    }
    Ok(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalEdgeFlange(construction),
    ))
}

#[cfg(test)]
mod tests;
