// SPDX-License-Identifier: Apache-2.0
//! Neutral-to-native history write preparation.

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, DesignParameter, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection,
    FeatureDefinition, FeatureId, ProfileRef, Termination, VertexSelection,
};
use std::collections::{HashMap, HashSet};

use crate::history::configuration::{
    enrich_history_semantic, project_compact_and_generated, HistoryEnrichment,
};
use crate::history::hash::{feature_hash, history_hash, native_parameter_hash};
use crate::history::parameters::expression_identifier_tokens;
use crate::history::project::project_features;

mod configurations;
mod features;
mod parameters;
mod xml;

pub(crate) use configurations::*;
pub(crate) use features::*;
pub(crate) use parameters::*;

/// Collect retained feature names changed by the neutral model.
pub(crate) fn feature_name_changes(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
) -> HashMap<FeatureId, (String, String)> {
    native.map_or_else(HashMap::new, |native| {
        ir.model
            .features
            .iter()
            .filter_map(|feature| {
                let record = native
                    .feature_histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|record| feature.native_ref.as_deref() == Some(record.id.as_str()))?;
                let new_name = feature.name.as_deref().unwrap_or_default();
                (record.name != new_name).then(|| {
                    (
                        feature.id.clone(),
                        (record.name.clone(), new_name.to_string()),
                    )
                })
            })
            .collect()
    })
}

pub(crate) fn native_parameters_match_source(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
) -> bool {
    native
        .map(|native| native_parameter_hash(&native.feature_histories))
        .zip(
            ir.source
                .as_ref()
                .and_then(|source| source.attributes.get("sldprt_native_parameter_sha256")),
        )
        .is_some_and(|(current, baseline)| &current == baseline)
}

pub(crate) fn apply_feature_name_changes(
    parameters: &mut [DesignParameter],
    changes: &HashMap<FeatureId, (String, String)>,
) {
    let owners = parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<HashMap<_, _>>();
    for parameter in parameters {
        if let Some((old_owner, new_owner)) = parameter
            .owner
            .as_ref()
            .and_then(|owner| changes.get(owner))
        {
            if let Some(equation_id) = parameter.properties.get_mut("EquationId") {
                if let Some(base) = equation_id.strip_suffix(&format!("@{old_owner}")) {
                    *equation_id = format!("{base}@{new_owner}");
                }
            }
        }
        let dependency_changes = parameter
            .dependencies
            .iter()
            .filter_map(|dependency| owners.get(dependency))
            .filter_map(|owner| owner.as_ref().and_then(|owner| changes.get(owner)))
            .collect::<Vec<_>>();
        let aliases = expression_identifier_tokens(&parameter.expression)
            .identifiers
            .into_iter()
            .filter_map(|token| {
                dependency_changes
                    .iter()
                    .find_map(|(old_owner, new_owner)| {
                        token
                            .value
                            .strip_suffix(&format!("@{old_owner}"))
                            .map(|base| (token.value.clone(), format!("{base}@{new_owner}")))
                    })
            })
            .collect::<HashMap<_, _>>();
        if let Some(rewritten) = rewrite_parameter_expression(&parameter.expression, &aliases) {
            parameter.expression = rewritten;
        }
    }
}

/// Resolve neutral/native feature edit authority and update the write history.
///
/// Bitwise comparison against the machine-local document baseline; see
/// [`cadmpeg_ir::hash::document_local_sha256`]. Absent baseline: sync lanes from
/// the neutral side.
pub fn prepare_features_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let neutral_hash = feature_hash(&ir.model.features);
    let native_hash = native
        .as_ref()
        .map(|value| history_hash(&value.feature_histories));
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_feature_local_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_history_sha256"));
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &neutral_hash);
    let native_changed = match (&native_hash, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) => true,
        (None, Some(_)) => true,
        (None, None) => false,
    };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        validate_compact_body_selection_edits(&ir.model.features, native.as_ref())?;
        validate_compact_edge_selection_edits(&ir.model.features, native.as_ref())?;
        validate_compact_surface_selection_edits(&ir.model.features, native.as_ref())?;
        validate_surface_sweep_profile_edits(&ir.model.features, native.as_ref())?;
        validate_embedded_helix_edits(&ir.model.features, native.as_ref())?;
        return sync_neutral_features(
            &ir.model.features,
            &ir.model.parameters,
            &ir.model.bodies,
            native,
        );
    }
    match (neutral_changed, native_changed) {
        (false, _) => Ok(()),
        (true, true) => {
            let projected = native
                .as_ref()
                .map(project_features_with_native_inputs)
                .unwrap_or_default();
            if feature_hash(&projected) == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT feature edits".into(),
                ))
            }
        }
        (true, false) => {
            validate_compact_body_selection_edits(&ir.model.features, native.as_ref())?;
            validate_compact_edge_selection_edits(&ir.model.features, native.as_ref())?;
            validate_compact_surface_selection_edits(&ir.model.features, native.as_ref())?;
            validate_surface_sweep_profile_edits(&ir.model.features, native.as_ref())?;
            validate_embedded_helix_edits(&ir.model.features, native.as_ref())?;
            sync_neutral_features(
                &ir.model.features,
                &ir.model.parameters,
                &ir.model.bodies,
                native,
            )
        }
    }
}

pub(crate) fn validate_embedded_helix_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let embedded = project_features(&native.feature_histories)
        .into_iter()
        .filter_map(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::HelixNativeAxis { .. }
            )
            .then_some(feature.id)
        })
        .collect::<HashSet<_>>();
    let expected = project_features_with_native_inputs(native)
        .into_iter()
        .filter_map(|feature| {
            (embedded.contains(&feature.id)
                && matches!(feature.definition, FeatureDefinition::Helix { .. }))
            .then_some((feature.id, feature.definition))
        })
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(expected) = expected.get(&feature.id) else {
            continue;
        };
        if &feature.definition != expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes embedded helix geometry",
                feature.id
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_surface_sweep_profile_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let expected = project_features_with_native_inputs(native)
        .into_iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sweep { section, .. } = feature.definition else {
                return None;
            };
            let cadmpeg_ir::features::SweepSection::Profile(
                profile @ (ProfileRef::Feature(_) | ProfileRef::Generated { .. }),
            ) = section
            else {
                return None;
            };
            (matches!(profile, ProfileRef::Generated { .. })
                || !feature.source_properties.contains_key("Profile"))
            .then_some((feature.id, profile))
        })
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(expected) = expected.get(&feature.id) else {
            continue;
        };
        let FeatureDefinition::Sweep { section, .. } = &feature.definition else {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        };
        let Some(profile) = section.referenced_profile() else {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        };
        if profile != expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        }
    }
    Ok(())
}

pub(crate) fn project_features_with_native_inputs(
    native: &crate::native::SldprtNative,
) -> Vec<cadmpeg_ir::features::Feature> {
    let mut histories = native.feature_histories.clone();
    enrich_history_semantic(
        &mut histories,
        &native.feature_input_lanes,
        &native.pmi_dimensions,
        HistoryEnrichment::Write,
    );
    let mut features = project_features(&histories);
    crate::resolved_features::bindings::bind_pattern_inputs(
        &mut features,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::operations::bind_sweep_operations(
        &mut features,
        &histories,
        &native.feature_input_lanes,
        None,
    );
    project_compact_and_generated(&mut features, &histories, &native.feature_input_lanes);
    crate::resolved_features::operations::bind_revolution_operations(
        &mut features,
        &histories,
        &native.feature_input_lanes,
        None,
    );
    let _ = crate::resolved_features::markers::spatial_sketches(
        &mut features,
        &histories,
        &native.feature_input_lanes,
    );
    features
}

pub(crate) fn validate_compact_body_selection_edits(
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
        let FeatureDefinition::DeleteBody { bodies, mode } = &feature.definition else {
            continue;
        };
        let expected = BodySelection::Local {
            bodies: selection
                .local_body_ids
                .iter()
                .map(u32::to_string)
                .collect(),
            native: crate::resolved_features::component_paths::compact_body_selection_value(
                &selection.local_body_ids,
            ),
        };
        if bodies != &expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact body selection",
                feature.id
            )));
        }
        if selection
            .mode
            .as_ref()
            .is_some_and(|expected| mode != expected)
        {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact body retention mode",
                feature.id
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_compact_edge_selection_edits(
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
        let groups = match &feature.definition {
            FeatureDefinition::Fillet { groups } => {
                groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
            }
            FeatureDefinition::Chamfer { groups, .. } => {
                groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
            }
            _ => continue,
        };
        let [edges] = groups.as_slice() else {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} requires exactly one compact edge group",
                feature.id
            )));
        };
        let native = crate::resolved_features::component_paths::compact_edge_selection_set_value(
            edge_selections,
        );
        let generated = edge_selections
            .iter()
            .map(|selection| {
                let native_feature = selection.terminal_feature_ref.as_deref()?;
                let feature = feature_ids_by_native.get(native_feature)?.clone();
                let local_id = selection
                    .local_edge_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                Some(cadmpeg_ir::features::GeneratedEdgeRef { feature, local_id })
            })
            .collect::<Option<Vec<_>>>();
        let expected = match generated.filter(|edges| !edges.is_empty()) {
            Some(edges) => EdgeSelection::Generated { edges, native },
            None => EdgeSelection::Native(native),
        };
        if *edges != &expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact edge selection",
                feature.id
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_compact_surface_selection_edits(
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
        let first_component =
            matches!(feature.definition, FeatureDefinition::CosmeticThread { .. });
        let slot = match &feature.definition {
            FeatureDefinition::Thicken { faces, .. } => SelectionSlot::Face(faces),
            FeatureDefinition::CosmeticThread { face, .. } => SelectionSlot::Face(face),
            FeatureDefinition::Extrude {
                extent:
                    ExtrudeExtent::OneSided {
                        side:
                            ExtrudeSide {
                                termination:
                                    Termination::ToFace { face, .. }
                                    | Termination::OffsetFromFace { face, .. },
                                ..
                            },
                    },
                ..
            } => SelectionSlot::Face(face),
            FeatureDefinition::Extrude {
                extent:
                    ExtrudeExtent::OneSided {
                        side:
                            ExtrudeSide {
                                termination: Termination::ToVertex { vertex },
                                ..
                            },
                    },
                ..
            } => SelectionSlot::Vertex(vertex),
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
                    Some((feature, local_id)) => FaceSelection::Generated {
                        faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                            feature: feature.clone(),
                            local_id: local_id.to_string(),
                        }],
                        native,
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
                    Some((feature, local_id)) => VertexSelection::Generated {
                        vertex: cadmpeg_ir::features::GeneratedVertexRef {
                            feature: feature.clone(),
                            local_id: local_id.to_string(),
                        },
                        native,
                    },
                    None => VertexSelection::Native(native),
                };
                vertex != &expected
            }
        };
        if changed {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact surface selection",
                feature.id
            )));
        }
    }
    Ok(())
}
