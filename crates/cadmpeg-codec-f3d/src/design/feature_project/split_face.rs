use super::project_face_selection;
use super::selected_work_planes;
use super::DesignConstructionOperandGroup;
use super::DesignFaceOperand;
use super::DesignOperandRole;
use super::DesignParameterScope;
use crate::design::feature_project::direct_face::resolved_split_face_path;
use crate::design::feature_project::draft::selected_historical_face_selection;
use cadmpeg_ir::features::surfaces::SplitFaceMethod;

pub(super) struct ProjectedSplitFace {
    pub(super) definition: cadmpeg_ir::features::FeatureDefinition,
    pub(super) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

pub(super) fn project_split_face(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
    types: &[crate::records::entity_header::SegmentType],
) -> ProjectedSplitFace {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference, DesignSplitFaceMethod,
    };
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, FiniteVector3, SplitFaceTool,
    };

    let mut losses = Vec::new();
    if scope.split_face_operation().is_some_and(
        crate::records::feature::split_face::DesignSplitFaceOperation::carrier_type_assumed,
    ) {
        losses.push(crate::loss::F3dLossCode::FeatureSplitFaceCarrierTypeAssumed.note(format!(
            "{}: the registered point-direction carrier is retained despite its conflicting inline type qualifier (DR-575).",
            scope.id
        )));
    }
    let method = match scope.split_face_operation() {
        Some(operation) => match operation.method() {
            DesignSplitFaceMethod::SurfaceIntersection { extend } => {
                SplitFaceMethod::SurfaceIntersection { extend }
            }
            DesignSplitFaceMethod::ClosestPoint => SplitFaceMethod::ClosestPoint {},
            DesignSplitFaceMethod::AlongVector => {
                let direction = operation.direction.as_ref().and_then(|value| {
                    value
                        .value
                        .map(|[x, y, z]| FiniteVector3::from_components(x, y, z))
                });
                let stream =
                    crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                let reference_label = |reference: &DesignSplitFaceDirectionReference| {
                    let inline = match reference {
                        DesignSplitFaceDirectionReference::Local {
                            inline_type_guid, ..
                        }
                        | DesignSplitFaceDirectionReference::CrossSegment {
                            inline_type_guid,
                            ..
                        }
                        | DesignSplitFaceDirectionReference::CrossDocument {
                            inline_type_guid,
                            ..
                        } => inline_type_guid.as_deref(),
                        _ => None,
                    };
                    let label = match reference {
                    DesignSplitFaceDirectionReference::Unavailable | DesignSplitFaceDirectionReference::Null => None,
                    DesignSplitFaceDirectionReference::Local { target, .. } => {
                        Some(format!("{stream}:design-record#{}", target.value))
                    }
                    DesignSplitFaceDirectionReference::CrossSegment {
                        target,
                        segment,
                        same_document_asset,
                        ..
                    } => {
                        Some(match same_document_asset {
                                Some(asset) => format!("{stream}:asset#{}:segment#{segment}:design-record#{}", crate::ids::identity_key_component(asset), target.value),
                                None => format!("{stream}:segment#{segment}:design-record#{}", target.value),
                        })
                    }
                    DesignSplitFaceDirectionReference::CrossDocument {
                        target,
                        segment,
                        asset_guid,
                        target_type_guid,
                        link_name,
                        version,
                        ..
                    } => Some(format!(
                            "{stream}:asset#{}:segment#{segment}:type#{target_type_guid}:link#{}:version#{}:record#{}",
                            crate::ids::identity_key_component(asset_guid),
                            crate::ids::identity_key_component(link_name),
                            crate::ids::identity_key_component(&format!("{version:?}")),
                            target.value
                        )) ,
                    };
                    label.map(|label| match inline {
                        Some(guid) => format!("{label}:inline-type#{guid}"),
                        None => label,
                    })
                };
                let native_carrier = reference_label(&operation.direction_reference);
                let native_inputs = operation
                    .inputs
                    .iter()
                    .map(|input| reference_label(&input.value))
                    .collect::<Vec<_>>();
                losses.push(crate::loss::F3dLossCode::FeatureSplitFaceDirectionUnresolved.note(
                    format!(
                        "{}: the decoded vector {:?} and native carrier {:?} with selector records {:?} are retained (input run complete: {}); no neutral direction source is resolved (DR-571).",
                        scope.id, direction, native_carrier, native_inputs, operation.inputs_complete
                    ),
                ));
                SplitFaceMethod::AlongVector {
                    direction,
                    native_carrier,
                    native_inputs,
                    native_input_count: operation.input_count.map(|count| count.value),
                    native_inputs_complete: operation.inputs_complete,
                }
            }
            DesignSplitFaceMethod::Unresolved => {
                losses.push(
                    crate::loss::F3dLossCode::FeatureSplitFaceMethodUnresolved.note(format!(
                        "{}: method control {:?} remains unresolved (DR-570).",
                        scope.id, operation.control
                    )),
                );
                SplitFaceMethod::Unresolved {}
            }
        },
        None => {
            losses.push(
                crate::loss::F3dLossCode::FeatureSplitFaceMethodUnresolved.note(format!(
                    "{}: method control is unavailable (DR-570).",
                    scope.id
                )),
            );
            SplitFaceMethod::Unresolved {}
        }
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            stream.is_some()
                && crate::ids::native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);

    let at_tool_ordinal = groups
        .iter()
        .copied()
        .filter(|group| group.scope_reference_ordinal == 0)
        .collect::<Vec<_>>();
    let role_tool_groups = groups
        .iter()
        .copied()
        .filter(|group| {
            matches!(
                group.role(),
                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
            )
        })
        .collect::<Vec<_>>();
    let tool_group = match at_tool_ordinal.as_slice() {
        [group] => Some(*group),
        [] => match role_tool_groups.as_slice() {
            [group] => Some(*group),
            _ => None,
        },
        _ => None,
    };
    let target_groups = groups
        .iter()
        .copied()
        .filter(|group| {
            group.role() == DesignOperandRole::ROLE_0X10
                && tool_group.is_none_or(|tool| tool.record_index != group.record_index)
        })
        .collect::<Vec<_>>();
    let target_group = match target_groups.as_slice() {
        [group] => Some(*group),
        _ => None,
    };

    let mut expected_references = Vec::new();
    if let Some(group) = tool_group {
        expected_references.push(group.record_index);
        expected_references.extend(group.members().iter().map(|member| member.value));
    }
    if let Some(group) = target_group {
        expected_references.push(group.record_index);
        expected_references.extend(group.members().iter().map(|member| member.value));
    }
    if let Some(operation) = scope.split_face_operation() {
        if let Some(reference) = operation.direction_reference.local_record_index() {
            expected_references.push(reference);
        }
        expected_references.extend(
            operation
                .inputs
                .iter()
                .filter_map(|input| input.value.local_record_index()),
        );
    }
    let auxiliary_complete = scope.split_face_operation().is_none_or(|operation| {
        operation.method() != DesignSplitFaceMethod::AlongVector || operation.inputs_complete
    });
    let complete_partition = match (tool_group, target_group) {
        (Some(tool), Some(target)) => {
            let target_ordinal = tool.members().len().checked_add(1);
            auxiliary_complete
                && tool.scope_reference_ordinal == 0
                && target_ordinal.and_then(|ordinal| u32::try_from(ordinal).ok())
                    == Some(target.scope_reference_ordinal)
                && !tool.members().is_empty()
                && !target.members().is_empty()
                && matches!(
                    tool.role(),
                    DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
                )
                && target.role() == DesignOperandRole::ROLE_0X10
                && groups.len() == 2
                && expected_references
                    .iter()
                    .copied()
                    .eq(scope.reference_members().values().copied())
        }
        _ => false,
    };
    if !complete_partition {
        losses.push(crate::loss::F3dLossCode::FeatureSplitFaceGroupsUnresolved.note(
            format!(
                "{}: the available tool, target, and direction references do not prove a complete ordered partition (DR-572).",
                scope.id
            ),
        ));
    }

    let (targets, tool, face_errors) = {
        let mut face_errors = Vec::new();
        let mut project_faces =
            |group: &DesignConstructionOperandGroup| match project_face_selection(
                scope,
                group,
                face_operands,
                histories,
            ) {
                Ok(selection) => selection,
                Err(error) => {
                    face_errors.push((group.record_index, error.to_string()));
                    FaceSelection::Native(group.id.clone())
                }
            };
        let targets = match target_group {
            Some(group) => project_faces(group),
            None => FaceSelection::Native(scope.id.clone()),
        };
        let tool = match tool_group {
            Some(group) if group.role() == DesignOperandRole::ROLE_0X9 => {
                SplitFaceTool::Faces(project_faces(group))
            }
            Some(group) if group.role() == DesignOperandRole::ROLE_0X21 => {
                let resolved =
                    resolved_split_face_path(scope, group, entity_selection_operands, histories)
                        .map(SplitFaceTool::Path)
                        .or_else(|| {
                            selected_work_planes(scope, group, entity_selection_operands, scopes)
                                .map(|planes| {
                                    let planes = planes
                                        .into_iter()
                                        .map(crate::ids::neutral_feature_id)
                                        .collect::<Vec<_>>();
                                    match planes.as_slice() {
                                        [plane] => SplitFaceTool::Plane {
                                            plane: plane.clone(),
                                        },
                                        _ => match planes.try_into() {
                                            Ok(planes) => SplitFaceTool::Planes { planes },
                                            Err(_) => SplitFaceTool::Native(group.id.clone()),
                                        },
                                    }
                                })
                        })
                        .or_else(|| {
                            selected_historical_face_selection(
                                scope,
                                group,
                                entity_selection_operands,
                                histories,
                                types,
                            )
                            .map(SplitFaceTool::Faces)
                        });
                resolved.unwrap_or_else(|| SplitFaceTool::Native(group.id.clone()))
            }
            Some(group) => SplitFaceTool::Native(group.id.clone()),
            None => SplitFaceTool::Native(scope.id.clone()),
        };
        (targets, tool, face_errors)
    };
    for (record_index, error) in face_errors {
        losses.push(
            crate::loss::F3dLossCode::FeatureSplitFaceSelectionProjectionFailed.note(format!(
                "{}: initial projection of face group {record_index} failed: {error} (DR-574).",
                scope.id
            )),
        );
    }
    ProjectedSplitFace {
        definition: FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets,
            tool,
            method,
        }),
        losses,
    }
}
