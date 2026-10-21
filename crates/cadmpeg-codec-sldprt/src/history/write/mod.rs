// SPDX-License-Identifier: Apache-2.0
//! Neutral-to-native history write preparation.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition,
    FeatureOperation, LinearTermination, PlanarProfileRef, VertexSelection,
};
use std::collections::{HashMap, HashSet};

use crate::history::configuration::{
    enrich_history_semantic, project_compact_and_generated, HistoryEnrichment,
};
use crate::history::project::{project_feature_model, project_features, FeatureProjection};

pub(crate) mod configurations;
pub(crate) mod features;
pub(crate) mod parameters;
pub(crate) mod xml;

use self::features::sync_neutral_features;

/// Regenerate native feature fields from current neutral content.
pub(crate) fn synchronize_feature_records(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<Vec<features::FeatureInputRename>, CodecError> {
    if !ir.scope.design {
        return Ok(Vec::new());
    }
    validate_compact_body_selection_edits(&ir.model.features, native.as_ref())?;
    validate_compact_edge_selection_edits(&ir.model.features, native.as_ref())?;
    validate_compact_surface_selection_edits(&ir.model.features, native.as_ref())?;
    validate_surface_sweep_profile_edits(&ir.model.features, native.as_ref())?;
    validate_embedded_helix_edits(ctx, &ir.model.features, native.as_ref())?;
    sync_neutral_features(&ir.model, &ir.model.parameters, &ir.model.bodies, native)
}

fn validate_embedded_helix_edits(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let embedded = project_features(ctx, &native.feature_histories)?
        .into_iter()
        .filter_map(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::HelixNativeAxis { .. })
            )
            .then_some(feature.id)
        })
        .collect::<HashSet<_>>();
    let expected = project_features_with_native_inputs(native)?
        .into_iter()
        .filter_map(|feature| {
            (embedded.contains(&feature.id)
                && matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Helix { .. })
                ))
            .then_some((feature.id, feature.evaluation.definition().clone()))
        })
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(expected) = expected.get(&feature.id) else {
            continue;
        };
        if feature.evaluation.definition() != expected {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes embedded helix geometry",
                feature.id
            )));
        }
    }
    Ok(())
}

fn validate_surface_sweep_profile_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let expected = project_features_with_native_inputs(native)?
        .into_iter()
        .filter_map(|feature| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) =
                feature.evaluation.definition()
            else {
                return None;
            };
            let profile @ (PlanarProfileRef::Feature(_) | PlanarProfileRef::Generated { .. }) =
                shape.referenced_profile()?
            else {
                return None;
            };
            (matches!(profile, PlanarProfileRef::Generated { .. })
                || !feature.source_properties.contains_key("Profile"))
            .then_some((feature.id, profile.clone()))
        })
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(expected) = expected.get(&feature.id) else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) =
            feature.evaluation.definition()
        else {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        };
        let Some(profile) = shape.referenced_profile() else {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        };
        if profile != expected {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        }
    }
    Ok(())
}

fn project_features_with_native_inputs(
    native: &crate::native::SldprtNative,
) -> Result<Vec<cadmpeg_ir::features::Feature>, cadmpeg_core::CodecError> {
    Ok(project_feature_model_with_native_inputs(native)?.features)
}

fn project_feature_model_with_native_inputs(
    native: &crate::native::SldprtNative,
) -> Result<FeatureProjection, cadmpeg_core::CodecError> {
    let projection_bytes = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| lane.native_payload.iter().copied())
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&projection_bytes, &arena, &DecodePolicy::service())?;
    let mut histories = native.feature_histories.clone();
    enrich_history_semantic(
        &ctx,
        &mut histories,
        &native.feature_input_lanes,
        &native.pmi_dimensions,
        HistoryEnrichment::Write,
    )?;
    let mut projection = project_feature_model(&ctx, &histories)?;
    let features = &mut projection.features;
    crate::resolved_features::bindings::bind_pattern_inputs(
        features,
        &histories,
        &native.feature_input_lanes,
    )?;
    crate::resolved_features::operations::bind_sweep_operations(
        features,
        &histories,
        &native.feature_input_lanes,
        None,
    );
    project_compact_and_generated(&ctx, features, &histories, &native.feature_input_lanes)?;
    crate::resolved_features::operations::bind_revolution_operations(
        features,
        &histories,
        &native.feature_input_lanes,
        None,
    );
    // discarded-value: marking the features is the whole effect on the write route; the collected sketches have a reader only on the decode route
    let _ = crate::resolved_features::markers::spatial_sketches(
        features,
        &histories,
        &native.feature_input_lanes,
    );
    // Later bindings replace provisional feature operands with their actual
    // sketch or spatial carriers. Dependencies follow the final definition.
    crate::history::project::refresh_definition_dependencies(&ctx, features)?;
    Ok(projection)
}

fn validate_compact_body_selection_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let mut selections = HashMap::<&str, Vec<&crate::records::FeatureInputBodySelection>>::new();
    for selection in native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.body_selections)
    {
        selections
            .entry(selection.feature_ref.as_str())
            .or_default()
            .push(selection);
    }
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some([selection]) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, mode }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let expected = BodySelection::local(
            selection
                .local_body_ids
                .iter()
                .map(u32::to_string)
                .collect(),
            crate::resolved_features::component_paths::compact_body_selection_value(
                &selection.local_body_ids,
            ),
        )
        .map_err(|error| CodecError::UnsupportedContent(error.to_string()))?;
        if bodies != &expected {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes a compact body selection",
                feature.id
            )));
        }
        if selection
            .mode
            .as_ref()
            .is_some_and(|expected| mode != expected)
        {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes a compact body retention mode",
                feature.id
            )));
        }
    }
    Ok(())
}

fn validate_compact_edge_selection_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let mut selections = HashMap::<&str, Vec<&crate::records::FeatureInputEdgeSelection>>::new();
    for selection in native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.edge_selections)
    {
        selections
            .entry(selection.feature_ref.as_str())
            .or_default()
            .push(selection);
    }
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(edge_selections) = selections
            .get(native_ref)
            .filter(|selections| !selections.is_empty())
        else {
            continue;
        };
        let groups = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
            }
            _ => continue,
        };
        let [edges] = groups.as_slice() else {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} requires exactly one compact edge group",
                feature.id
            )));
        };
        let native = crate::resolved_features::component_paths::compact_edge_selection_set_value(
            edge_selections,
        );
        let generated = edge_selections
            .iter()
            .map(|selection| -> Result<Option<_>, CodecError> {
                let Some(native_feature) = selection.terminal_feature_ref.as_deref() else {
                    return Ok(None);
                };
                let Some(feature) = feature_ids_by_native.get(native_feature) else {
                    return Ok(None);
                };
                let feature = feature.clone();
                let local_id = selection
                    .local_edge_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                Ok(cadmpeg_ir::features::GeneratedEdgeRef::new(feature, local_id).ok())
            })
            .collect::<Result<Option<Vec<_>>, CodecError>>()?;
        let expected = match generated.filter(|edges| !edges.is_empty()) {
            Some(edges) => EdgeSelection::generated(edges, native.clone())
                .unwrap_or(EdgeSelection::Native(native)),
            None => EdgeSelection::Native(native),
        };
        if *edges != &expected {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes a compact edge selection",
                feature.id
            )));
        }
    }
    Ok(())
}

fn validate_compact_surface_selection_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    enum SelectionSlot<'a> {
        Face(&'a FaceSelection),
        Vertex(&'a VertexSelection),
    }
    let Some(native) = native else { return Ok(()) };
    let mut selections = HashMap::<&str, Vec<&crate::records::FeatureInputSurfaceSelection>>::new();
    for selection in native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.surface_selections)
    {
        selections
            .entry(selection.feature_ref.as_str())
            .or_default()
            .push(selection);
    }
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some([selection]) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        let first_component = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread { .. })
        );
        let slot = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Thicken { faces, .. }) => {
                SelectionSlot::Face(faces)
            }
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }) => {
                SelectionSlot::Face(face)
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent:
                    ExtrudeExtent::OneSided {
                        side:
                            ExtrudeSide {
                                termination:
                                    LinearTermination::ToFace { face, .. }
                                    | LinearTermination::OffsetFromFace { face, .. },
                                ..
                            },
                    },
                ..
            }) => SelectionSlot::Face(face),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent:
                    ExtrudeExtent::OneSided {
                        side:
                            ExtrudeSide {
                                termination: LinearTermination::ToVertex { vertex },
                                ..
                            },
                    },
                ..
            }) => SelectionSlot::Vertex(vertex),
            _ => continue,
        };
        let native = crate::resolved_features::terminations::compact_surface_selection_value(
            &selection.components,
        );
        let producer = if first_component {
            selection.producer_feature_refs.first().map(String::as_str)
        } else {
            selection.terminal_feature_ref.as_deref()
        };
        let component = if first_component {
            selection.components.first()
        } else {
            selection.components.last()
        };
        let generated = producer
            .and_then(|producer| feature_ids_by_native.get(producer))
            .zip(component)
            .and_then(|(feature, component)| Some((feature, component.local_id?)));
        let changed = match slot {
            SelectionSlot::Face(faces) => {
                let expected = match generated {
                    Some((feature, local_id)) => match cadmpeg_ir::features::GeneratedFaceRef::new(
                        feature.clone(),
                        local_id.to_string(),
                    ) {
                        Ok(face) => FaceSelection::generated(vec![face], native.clone())
                            .unwrap_or(FaceSelection::Native(native)),
                        Err(_) => FaceSelection::Native(native),
                    },
                    None => FaceSelection::Native(native),
                };
                faces != &expected
            }
            // Edge-endpoint references keep the endpoint selector native.
            SelectionSlot::Vertex(VertexSelection::Native(value))
                if value.starts_with("sldprt:feature-input:edge-endpoint-ref:") =>
            {
                false
            }
            SelectionSlot::Vertex(vertex) => {
                let expected = match generated {
                    Some((feature, local_id)) => {
                        match cadmpeg_ir::features::GeneratedVertexRef::new(
                            feature.clone(),
                            local_id.to_string(),
                        ) {
                            Ok(vertex) => {
                                match VertexSelection::generated(vertex, native.clone()) {
                                    Ok(selection) => selection,
                                    Err(_) => VertexSelection::native(native)
                                        .unwrap_or(VertexSelection::Unresolved),
                                }
                            }
                            Err(_) => VertexSelection::native(native)
                                .unwrap_or(VertexSelection::Unresolved),
                        }
                    }
                    None => VertexSelection::native(native).unwrap_or(VertexSelection::Unresolved),
                };
                vertex != &expected
            }
        };
        if changed {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} changes a compact surface selection",
                feature.id
            )));
        }
    }
    Ok(())
}

mod input_classes;
