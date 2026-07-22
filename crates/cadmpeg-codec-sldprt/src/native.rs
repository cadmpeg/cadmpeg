// SPDX-License-Identifier: Apache-2.0
//! SOLIDWORKS native feature-history records.
#![deny(clippy::disallowed_methods)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::records::{
    FeatureHistory, FeatureInputBodySelection, FeatureInputClass, FeatureInputEdgeSelection,
    FeatureInputGeneratedSurfaceIdentity, FeatureInputLane, FeatureInputName,
    FeatureInputReference, FeatureInputRelationBinding, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputSurfaceSelection, PmiDimension,
};

/// Current schema version for the SOLIDWORKS native namespace.
pub const SLDPRT_NATIVE_VERSION: u32 = 13;
pub const SLDPRT_MIN_NATIVE_VERSION: u32 = 1;

pub(crate) fn native_version_supported(version: u32) -> bool {
    (SLDPRT_MIN_NATIVE_VERSION..=SLDPRT_NATIVE_VERSION).contains(&version)
}

pub(crate) const SLDPRT_ARENA_NAMES: &[&str] = &[
    "configurations",
    "feature_histories",
    "feature_input_body_selections",
    "feature_input_classes",
    "feature_input_edge_selections",
    "feature_input_generated_surface_identities",
    "feature_input_lanes",
    "feature_input_names",
    "feature_input_references",
    "feature_input_relation_bindings",
    "feature_input_relation_instances",
    "feature_input_scalars",
    "feature_input_surface_selections",
    "features",
    "pmi_dimensions",
    "sketch_input_entities",
];

/// SOLIDWORKS records retained outside the format-neutral model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SldprtNative {
    /// Schema version this namespace was written under; see [`SLDPRT_NATIVE_VERSION`].
    pub version: u32,
    /// Parametric construction-history timelines decoded from the source part.
    #[serde(default)]
    pub feature_histories: Vec<FeatureHistory>,
    /// Native feature-input byte streams retained for parametric replay and rewrite.
    #[serde(default)]
    pub feature_input_lanes: Vec<FeatureInputLane>,
    /// Semantic dimensions decoded from `PMISemanticDataDB`.
    #[serde(default)]
    pub pmi_dimensions: Vec<PmiDimension>,
}

impl Default for SldprtNative {
    fn default() -> Self {
        Self {
            version: SLDPRT_NATIVE_VERSION,
            feature_histories: Vec::new(),
            feature_input_lanes: Vec::new(),
            pmi_dimensions: Vec::new(),
        }
    }
}

impl SldprtNative {
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        if !native_version_supported(namespace.version) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "unsupported SLDPRT native namespace version {}",
                namespace.version
            )));
        }
        let mut native = Self {
            version: SLDPRT_NATIVE_VERSION,
            feature_histories: namespace.arena_as("feature_histories")?,
            feature_input_lanes: namespace.arena_as("feature_input_lanes")?,
            pmi_dimensions: namespace.arena_as("pmi_dimensions")?,
        };
        let configurations: Vec<crate::records::Configuration> =
            namespace.arena_as("configurations")?;
        let features: Vec<crate::records::Feature> = namespace.arena_as("features")?;
        let mut entities: Vec<crate::records::SketchInputEntity> =
            namespace.arena_as("sketch_input_entities")?;
        let classes: Vec<FeatureInputClass> = namespace.arena_as("feature_input_classes")?;
        let body_selections: Vec<FeatureInputBodySelection> = if namespace.version == 1
            && !namespace
                .arenas
                .contains_key("feature_input_body_selections")
        {
            Vec::new()
        } else {
            namespace.arena_as("feature_input_body_selections")?
        };
        let edge_selections: Vec<FeatureInputEdgeSelection> = if namespace.version <= 2
            && !namespace
                .arenas
                .contains_key("feature_input_edge_selections")
        {
            Vec::new()
        } else {
            namespace.arena_as("feature_input_edge_selections")?
        };
        let surface_selections: Vec<FeatureInputSurfaceSelection> = if namespace.version <= 3
            && !namespace
                .arenas
                .contains_key("feature_input_surface_selections")
        {
            Vec::new()
        } else {
            namespace.arena_as("feature_input_surface_selections")?
        };
        let generated_surface_identities: Vec<FeatureInputGeneratedSurfaceIdentity> =
            if namespace.version <= 12
                && !namespace
                    .arenas
                    .contains_key("feature_input_generated_surface_identities")
            {
                Vec::new()
            } else {
                namespace.arena_as("feature_input_generated_surface_identities")?
            };
        let names: Vec<FeatureInputName> = namespace.arena_as("feature_input_names")?;
        let references: Vec<FeatureInputReference> =
            namespace.arena_as("feature_input_references")?;
        let relation_bindings: Vec<FeatureInputRelationBinding> =
            namespace.arena_as("feature_input_relation_bindings")?;
        let relation_instances: Vec<FeatureInputRelationInstance> =
            namespace.arena_as("feature_input_relation_instances")?;
        let scalars: Vec<FeatureInputScalar> = namespace.arena_as("feature_input_scalars")?;
        let history_ids = native
            .feature_histories
            .iter()
            .map(|history| history.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if let Some(record) = configurations
            .iter()
            .find(|record| !history_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "configuration {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = features
            .iter()
            .find(|record| !history_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature {} references {}",
                record.id, record.parent
            )));
        }
        let feature_ids = features
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let lane_ids = native
            .feature_input_lanes
            .iter()
            .map(|lane| lane.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if let Some(record) = entities
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "sketch input entity {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = classes
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input class {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = body_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input body selection {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = edge_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input edge selection {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = surface_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input surface selection {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = generated_surface_identities
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input generated surface identity {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = names
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input name {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = scalars
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input scalar {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = scalars.iter().find(|record| {
            record
                .feature_ref
                .as_deref()
                .is_some_and(|feature| !feature_ids.contains(feature))
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input scalar {} references missing feature {}",
                record.id,
                record.feature_ref.as_deref().unwrap_or_default()
            )));
        }
        if let Some(record) = references
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input reference {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = relation_bindings
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation binding {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = relation_instances
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation instance {} references {}",
                record.id, record.parent
            )));
        }
        let name_ids = names
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if let Some(record) = body_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.local_body_ids.is_empty()
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input body selection {} has unresolved ownership",
                record.id
            )));
        }
        if let Some(record) = edge_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.local_edge_ids.is_empty()
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input edge selection {} has unresolved ownership",
                record.id
            )));
        }
        if let Some(record) = surface_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || (namespace.version >= 8 && record.components.is_empty())
                || (namespace.version >= 9
                    && record
                        .producer_feature_refs
                        .iter()
                        .any(|producer| !feature_ids.contains(producer.as_str())))
                || (namespace.version >= 10
                    && record
                        .terminal_feature_ref
                        .as_deref()
                        .is_none_or(|feature| !feature_ids.contains(feature)))
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input surface selection {} has unresolved ownership",
                record.id
            )));
        }
        if let Some(record) = scalars
            .iter()
            .find(|record| !name_ids.contains(record.name.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input scalar {} references name {}",
                record.id, record.name
            )));
        }
        let references_by_id = references
            .iter()
            .map(|record| (record.id.as_str(), record))
            .collect::<std::collections::HashMap<_, _>>();
        let class_ids = classes
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let scalar_ids = scalars
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if let Some(record) = relation_bindings.iter().find(|record| {
            !class_ids.contains(record.class_ref.as_str())
                || !scalar_ids.contains(record.scalar_ref.as_str())
                || record
                    .feature_ref
                    .as_deref()
                    .is_some_and(|feature| !feature_ids.contains(feature))
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation binding {} has an unresolved class or scalar",
                record.id
            )));
        }
        if let Some(record) = relation_instances.iter().find(|record| {
            !class_ids.contains(record.class_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.scalar_refs.is_empty()
                || record.scalar_refs.len() > 3
                || record
                    .scalar_refs
                    .iter()
                    .enumerate()
                    .any(|(index, scalar)| record.scalar_refs[..index].contains(scalar))
                || classes
                    .iter()
                    .find(|class| class.id == record.class_ref)
                    .is_none_or(|class| {
                        !matches!(
                            crate::classification::native_object_class(&class.name).kind,
                            crate::classification::NativeClassKind::SketchRelation(family)
                                if family == record.family
                        )
                    })
                || record
                    .scalar_refs
                    .iter()
                    .any(|scalar| !scalar_ids.contains(scalar.as_str()))
                || record
                    .parameter_scalar_ref
                    .as_deref()
                    .is_some_and(|scalar| !record.scalar_refs.iter().any(|value| value == scalar))
                || record
                    .display_scalar_ref
                    .as_deref()
                    .is_some_and(|scalar| !record.scalar_refs.iter().any(|value| value == scalar))
                || record.parameter_scalar_ref.as_deref().is_some_and(|id| {
                    scalars
                        .iter()
                        .find(|scalar| scalar.id == id)
                        .is_none_or(|scalar| {
                            scalar.role != crate::records::FeatureInputScalarRole::Driving
                        })
                })
                || record.display_scalar_ref.as_deref().is_some_and(|id| {
                    scalars
                        .iter()
                        .find(|scalar| scalar.id == id)
                        .is_none_or(|scalar| {
                            scalar.role != crate::records::FeatureInputScalarRole::Display
                        })
                })
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation instance {} has an unresolved class, feature, or scalar",
                record.id
            )));
        }
        for scalar in &scalars {
            for operand in &scalar.operands {
                let Some(reference) = references_by_id.get(operand.reference_ref.as_str()) else {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input scalar {} references missing cell {}",
                        scalar.id, operand.reference_ref
                    )));
                };
                if reference.offset != operand.offset
                    || reference.kind != operand.kind
                    || reference.object_index != operand.entity_index
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input scalar {} has inconsistent cell {}",
                        scalar.id, operand.reference_ref
                    )));
                }
            }
        }
        for history in &mut native.feature_histories {
            history.configurations = configurations
                .iter()
                .filter(|record| record.parent == history.id)
                .cloned()
                .collect();
            history.configurations.sort_by_key(|record| record.ordinal);
            history.features = features
                .iter()
                .filter(|record| record.parent == history.id)
                .cloned()
                .collect();
            history.features.sort_by_key(|record| record.ordinal);
        }
        for lane in &mut native.feature_input_lanes {
            if namespace.version <= 4 {
                for entity in entities
                    .iter_mut()
                    .filter(|record| record.parent == lane.id)
                {
                    entity.object_index = usize::try_from(entity.offset).ok().and_then(|offset| {
                        crate::resolved_features::marker_object_index(&lane.native_payload, offset)
                    });
                }
            } else if namespace.version <= 6 {
                for entity in entities
                    .iter_mut()
                    .filter(|record| record.parent == lane.id)
                {
                    if entity.object_index == Some(u32::MAX) {
                        entity.object_index = None;
                    }
                }
            }
            lane.classes = classes
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.classes.sort_by_key(|record| record.ordinal);
            lane.names = names
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.names.sort_by_key(|record| record.ordinal);
            lane.scalars = scalars
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.scalars.sort_by_key(|record| record.ordinal);
            lane.references = references
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.references.sort_by_key(|record| record.ordinal);
            lane.relation_bindings = relation_bindings
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.relation_bindings.sort_by_key(|record| record.ordinal);
            lane.relation_instances = relation_instances
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.relation_instances.sort_by_key(|record| record.ordinal);
            lane.body_selections = body_selections
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.body_selections.sort_by_key(|record| record.ordinal);
            if namespace.version <= 5 {
                let modes = lane
                    .body_selections
                    .iter()
                    .map(|selection| {
                        crate::resolved_features::compact_body_retention_mode_for_selection(
                            lane, selection,
                        )
                    })
                    .collect::<Vec<_>>();
                for (selection, mode) in lane.body_selections.iter_mut().zip(modes) {
                    selection.mode = mode;
                }
            }
            if let Some(record) = lane.body_selections.iter().find(|record| {
                usize::try_from(record.offset).ok().and_then(|offset| {
                    crate::resolved_features::compact_body_selection_at(
                        &lane.native_payload,
                        offset,
                    )
                }) != Some(record.local_body_ids.clone())
                    || crate::resolved_features::compact_body_retention_mode_for_selection(
                        lane, record,
                    ) != record.mode
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input body selection {} disagrees with its payload",
                    record.id
                )));
            }
            lane.edge_selections = edge_selections
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.edge_selections.sort_by_key(|record| record.ordinal);
            if namespace.version <= 11 {
                for record in &mut lane.edge_selections {
                    if let Some(local_edge_ids) =
                        usize::try_from(record.offset).ok().and_then(|offset| {
                            crate::resolved_features::compact_edge_selection_at(
                                &lane.native_payload,
                                offset,
                            )
                        })
                    {
                        record.local_edge_ids = local_edge_ids;
                    }
                    record.components = usize::try_from(record.offset)
                        .ok()
                        .and_then(|offset| {
                            crate::resolved_features::compact_edge_component_path_at(
                                &lane.native_payload,
                                offset,
                            )
                        })
                        .unwrap_or_default();
                    record.producer_feature_refs = usize::try_from(record.offset)
                        .ok()
                        .map(|offset| {
                            crate::resolved_features::compact_edge_producer_features_at(
                                &lane.native_payload,
                                offset,
                                &record.components,
                                &features,
                            )
                        })
                        .unwrap_or_default();
                    record.terminal_feature_ref =
                        usize::try_from(record.offset).ok().and_then(|offset| {
                            crate::resolved_features::compact_edge_owner_feature_at(
                                &lane.native_payload,
                                offset,
                                &record.components,
                                &features,
                            )
                        });
                }
            }
            if let Some(record) = lane.edge_selections.iter().find(|record| {
                usize::try_from(record.offset).ok().and_then(|offset| {
                    crate::resolved_features::compact_edge_selection_at(
                        &lane.native_payload,
                        offset,
                    )
                }) != Some(record.local_edge_ids.clone())
                    || usize::try_from(record.offset)
                        .ok()
                        .and_then(|offset| {
                            crate::resolved_features::compact_edge_component_path_at(
                                &lane.native_payload,
                                offset,
                            )
                        })
                        .unwrap_or_default()
                        != record.components
                    || usize::try_from(record.offset)
                        .ok()
                        .map(|offset| {
                            crate::resolved_features::compact_edge_producer_features_at(
                                &lane.native_payload,
                                offset,
                                &record.components,
                                &features,
                            )
                        })
                        .unwrap_or_default()
                        != record.producer_feature_refs
                    || usize::try_from(record.offset).ok().and_then(|offset| {
                        crate::resolved_features::compact_edge_owner_feature_at(
                            &lane.native_payload,
                            offset,
                            &record.components,
                            &features,
                        )
                    }) != record.terminal_feature_ref
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input edge selection {} disagrees with its payload",
                    record.id
                )));
            }
            lane.surface_selections = surface_selections
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.surface_selections.sort_by_key(|record| record.ordinal);
            if namespace.version <= 9 {
                for record in &mut lane.surface_selections {
                    if namespace.version <= 7 {
                        record.components = usize::try_from(record.offset)
                            .ok()
                            .and_then(|offset| {
                                crate::resolved_features::compact_surface_reference_at(
                                    &lane.native_payload,
                                    offset,
                                )
                            })
                            .unwrap_or_default();
                    }
                    record.producer_feature_refs =
                        crate::resolved_features::component_path_features(
                            &record.components,
                            &features,
                        );
                    record.terminal_feature_ref =
                        crate::resolved_features::component_path_terminal_feature(
                            &record.components,
                            &features,
                        );
                }
            }
            if let Some(record) = lane.surface_selections.iter().find(|record| {
                usize::try_from(record.offset).ok().and_then(|offset| {
                    crate::resolved_features::compact_surface_reference_at(
                        &lane.native_payload,
                        offset,
                    )
                }) != Some(record.components.clone())
                    || crate::resolved_features::component_path_features(
                        &record.components,
                        &features,
                    ) != record.producer_feature_refs
                    || crate::resolved_features::component_path_terminal_feature(
                        &record.components,
                        &features,
                    ) != record.terminal_feature_ref
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input surface selection {} disagrees with its payload",
                    record.id
                )));
            }
            lane.generated_surface_identities = if namespace.version <= 12 {
                crate::resolved_features::generated_surface_identities(lane)
            } else {
                let mut records = generated_surface_identities
                    .iter()
                    .filter(|record| record.parent == lane.id)
                    .cloned()
                    .collect::<Vec<_>>();
                records.sort_by_key(|record| record.ordinal);
                records
            };
            if lane.generated_surface_identities
                != crate::resolved_features::generated_surface_identities(lane)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input lane {} generated surface identities disagree with its payload",
                    lane.id
                )));
            }
            lane.sketch_entities = entities
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.sketch_entities.sort_by_key(|record| record.ordinal);
        }
        Ok(native)
    }

    pub fn store(
        &self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        for history in &self.feature_histories {
            if let Some(record) = history
                .configurations
                .iter()
                .find(|record| record.parent != history.id)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "configuration {} references {} instead of {}",
                    record.id, record.parent, history.id
                )));
            }
            if let Some(record) = history
                .features
                .iter()
                .find(|record| record.parent != history.id)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature {} references {} instead of {}",
                    record.id, record.parent, history.id
                )));
            }
        }
        let features = self
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .cloned()
            .collect::<Vec<_>>();
        let feature_ids = features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        for lane in &self.feature_input_lanes {
            let name_ids = lane
                .names
                .iter()
                .map(|record| record.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let references_by_id = lane
                .references
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<std::collections::HashMap<_, _>>();
            let class_ids = lane
                .classes
                .iter()
                .map(|record| record.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let scalar_ids = lane
                .scalars
                .iter()
                .map(|record| record.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            if let Some(record) = lane.classes.iter().find(|record| record.parent != lane.id) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input class {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
            if let Some(record) = lane.names.iter().find(|record| record.parent != lane.id) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input name {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
            if let Some(record) = lane.scalars.iter().find(|record| record.parent != lane.id) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input scalar {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
            if let Some(record) = lane.body_selections.iter().find(|record| {
                record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.local_body_ids.is_empty()
                    || usize::try_from(record.offset).ok().and_then(|offset| {
                        crate::resolved_features::compact_body_selection_at(
                            &lane.native_payload,
                            offset,
                        )
                    }) != Some(record.local_body_ids.clone())
                    || crate::resolved_features::compact_body_state_ids_for_selection(lane, record)
                        != record.body_state_ids
                    || crate::resolved_features::compact_body_retention_mode_for_selection(
                        lane, record,
                    ) != record.mode
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input body selection {} has inconsistent ownership",
                    record.id
                )));
            }
            if let Some(record) = lane.edge_selections.iter().find(|record| {
                record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.local_edge_ids.is_empty()
                    || usize::try_from(record.offset).ok().and_then(|offset| {
                        crate::resolved_features::compact_edge_selection_at(
                            &lane.native_payload,
                            offset,
                        )
                    }) != Some(record.local_edge_ids.clone())
                    || usize::try_from(record.offset)
                        .ok()
                        .and_then(|offset| {
                            crate::resolved_features::compact_edge_component_path_at(
                                &lane.native_payload,
                                offset,
                            )
                        })
                        .unwrap_or_default()
                        != record.components
                    || usize::try_from(record.offset)
                        .ok()
                        .map(|offset| {
                            crate::resolved_features::compact_edge_producer_features_at(
                                &lane.native_payload,
                                offset,
                                &record.components,
                                &features,
                            )
                        })
                        .unwrap_or_default()
                        != record.producer_feature_refs
                    || usize::try_from(record.offset).ok().and_then(|offset| {
                        crate::resolved_features::compact_edge_owner_feature_at(
                            &lane.native_payload,
                            offset,
                            &record.components,
                            &features,
                        )
                    }) != record.terminal_feature_ref
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input edge selection {} has inconsistent ownership",
                    record.id
                )));
            }
            if let Some(record) = lane.surface_selections.iter().find(|record| {
                record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.components.is_empty()
                    || crate::resolved_features::component_path_features(
                        &record.components,
                        &features,
                    ) != record.producer_feature_refs
                    || crate::resolved_features::component_path_terminal_feature(
                        &record.components,
                        &features,
                    ) != record.terminal_feature_ref
                    || usize::try_from(record.offset).ok().and_then(|offset| {
                        crate::resolved_features::compact_surface_reference_at(
                            &lane.native_payload,
                            offset,
                        )
                    }) != Some(record.components.clone())
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input surface selection {} has inconsistent ownership",
                    record.id
                )));
            }
            if let Some(record) = lane.scalars.iter().find(|record| {
                record
                    .feature_ref
                    .as_deref()
                    .is_some_and(|feature| !feature_ids.contains(feature))
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input scalar {} references missing feature {}",
                    record.id,
                    record.feature_ref.as_deref().unwrap_or_default()
                )));
            }
            if let Some(record) = lane
                .references
                .iter()
                .find(|record| record.parent != lane.id)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input reference {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
            if let Some(record) = lane.relation_bindings.iter().find(|record| {
                record.parent != lane.id
                    || !class_ids.contains(record.class_ref.as_str())
                    || !scalar_ids.contains(record.scalar_ref.as_str())
                    || record
                        .feature_ref
                        .as_deref()
                        .is_some_and(|feature| !feature_ids.contains(feature))
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input relation binding {} has inconsistent ownership",
                    record.id
                )));
            }
            if let Some(record) = lane.relation_instances.iter().find(|record| {
                record.parent != lane.id
                    || !class_ids.contains(record.class_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || !relation_instance_shape_valid(record, lane)
                    || record
                        .scalar_refs
                        .iter()
                        .any(|scalar| !scalar_ids.contains(scalar.as_str()))
                    || record
                        .parameter_scalar_ref
                        .as_deref()
                        .is_some_and(|scalar| {
                            !record.scalar_refs.iter().any(|value| value == scalar)
                        })
                    || record.display_scalar_ref.as_deref().is_some_and(|scalar| {
                        !record.scalar_refs.iter().any(|value| value == scalar)
                    })
                    || record.parameter_scalar_ref.as_deref().is_some_and(|id| {
                        lane.scalars
                            .iter()
                            .find(|scalar| scalar.id == id)
                            .is_none_or(|scalar| {
                                scalar.role != crate::records::FeatureInputScalarRole::Driving
                            })
                    })
                    || record.display_scalar_ref.as_deref().is_some_and(|id| {
                        lane.scalars
                            .iter()
                            .find(|scalar| scalar.id == id)
                            .is_none_or(|scalar| {
                                scalar.role != crate::records::FeatureInputScalarRole::Display
                            })
                    })
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input relation instance {} has inconsistent ownership",
                    record.id
                )));
            }
            if let Some(record) = lane.relation_bindings.iter().find(|record| {
                lane.scalars
                    .iter()
                    .find(|scalar| scalar.id == record.scalar_ref)
                    .is_some_and(|scalar| scalar.feature_ref != record.feature_ref)
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input relation binding {} disagrees with its scalar owner",
                    record.id
                )));
            }
            if let Some(record) = lane
                .scalars
                .iter()
                .find(|record| !name_ids.contains(record.name.as_str()))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input scalar {} references name {}",
                    record.id, record.name
                )));
            }
            let sketch_entities = lane
                .sketch_entities
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<std::collections::HashMap<_, _>>();
            for scalar in &lane.scalars {
                let resolved_operands = crate::resolved_features::resolve_scalar_operand_markers(
                    lane.sketch_entities
                        .iter()
                        .filter(|candidate| candidate.feature_ref == scalar.feature_ref),
                    &scalar.operands,
                );
                for (operand, resolved) in scalar.operands.iter().zip(resolved_operands) {
                    let Some(reference) = references_by_id.get(operand.reference_ref.as_str())
                    else {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "feature-input scalar {} references missing cell {}",
                            scalar.id, operand.reference_ref
                        )));
                    };
                    if reference.offset != operand.offset
                        || reference.kind != operand.kind
                        || reference.object_index != operand.entity_index
                    {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "feature-input scalar {} has inconsistent cell {}",
                            scalar.id, operand.reference_ref
                        )));
                    }
                    if let Some(entity_ref) = operand.entity_ref.as_deref() {
                        let Some(target) = sketch_entities.get(entity_ref) else {
                            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                                "feature-input scalar {} references missing sketch marker {}",
                                scalar.id, entity_ref
                            )));
                        };
                        if resolved != Some(*target) {
                            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                                "feature-input scalar {} has inconsistent sketch marker {}",
                                scalar.id, entity_ref
                            )));
                        }
                    }
                }
            }
            if let Some(record) = lane.sketch_entities.iter().find(|record| {
                record.parent != lane.id
                    || record
                        .feature_ref
                        .as_deref()
                        .is_some_and(|feature| !feature_ids.contains(feature))
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "sketch input entity {} has inconsistent lane or feature ownership",
                    record.id
                )));
            }
            if let Some(record) = lane.sketch_entities.iter().find(|record| {
                record
                    .coordinates_m
                    .is_some_and(|values| !values.iter().all(|value| value.is_finite()))
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "sketch input entity {} has non-finite coordinates",
                    record.id
                )));
            }
            for record in &lane.sketch_entities {
                if record.links.is_empty() != record.link_selector.is_none() {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "sketch input entity {} has inconsistent local-link selector",
                        record.id
                    )));
                }
                for link in &record.links {
                    let Some(target) = sketch_entities.get(link.entity_ref.as_str()) else {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "sketch input entity {} references missing local-link target {}",
                            record.id, link.entity_ref
                        )));
                    };
                    if target.feature_ref != record.feature_ref
                        || target.local_id != Some(u32::from(link.local_id))
                    {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "sketch input entity {} has inconsistent local-link target {}",
                            record.id, link.entity_ref
                        )));
                    }
                }
            }
        }
        let mut expected_histories = self.feature_histories.clone();
        crate::resolved_features::bind_history_classes(
            &mut expected_histories,
            &self.feature_input_lanes,
        );
        if self
            .feature_histories
            .iter()
            .zip(&expected_histories)
            .flat_map(|(history, expected)| history.features.iter().zip(&expected.features))
            .any(|(feature, expected)| feature.input_class != expected.input_class)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "history feature classes do not match the feature-input index".into(),
            ));
        }
        namespace.version = SLDPRT_NATIVE_VERSION;
        let histories = self
            .feature_histories
            .iter()
            .cloned()
            .map(|mut history| {
                history.configurations.clear();
                history.features.clear();
                history
            })
            .collect::<Vec<_>>();
        namespace.set_arena("feature_histories", &histories)?;
        namespace.set_arena("pmi_dimensions", &self.pmi_dimensions)?;
        namespace.set_arena(
            "configurations",
            &self
                .feature_histories
                .iter()
                .flat_map(|history| history.configurations.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "features",
            &self
                .feature_histories
                .iter()
                .flat_map(|history| history.features.clone())
                .collect::<Vec<_>>(),
        )?;
        let lanes = self
            .feature_input_lanes
            .iter()
            .cloned()
            .map(|mut lane| {
                lane.classes.clear();
                lane.names.clear();
                lane.scalars.clear();
                lane.relation_bindings.clear();
                lane.relation_instances.clear();
                lane.body_selections.clear();
                lane.edge_selections.clear();
                lane.surface_selections.clear();
                lane.generated_surface_identities.clear();
                lane.references.clear();
                lane.sketch_entities.clear();
                lane
            })
            .collect::<Vec<_>>();
        namespace.set_arena("feature_input_lanes", &lanes)?;
        namespace.set_arena(
            "feature_input_body_selections",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.body_selections.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_edge_selections",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.edge_selections.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_surface_selections",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.surface_selections.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_generated_surface_identities",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.generated_surface_identities.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_classes",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.classes.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_names",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.names.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_scalars",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.scalars.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_references",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.references.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_relation_bindings",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.relation_bindings.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "feature_input_relation_instances",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.relation_instances.clone())
                .collect::<Vec<_>>(),
        )?;
        namespace.set_arena(
            "sketch_input_entities",
            &self
                .feature_input_lanes
                .iter()
                .flat_map(|lane| lane.sketch_entities.clone())
                .collect::<Vec<_>>(),
        )?;
        debug_assert!(SLDPRT_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SldprtNative, SLDPRT_NATIVE_VERSION};

    #[test]
    fn version_twelve_adds_generated_surface_identity_arena() {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        SldprtNative::default().store(&mut namespace).unwrap();
        namespace.version = 12;
        namespace
            .arenas
            .remove("feature_input_generated_surface_identities");

        let migrated = SldprtNative::load(&namespace).unwrap();
        let mut current = cadmpeg_ir::NativeNamespace::default();
        migrated.store(&mut current).unwrap();

        assert_eq!(current.version, SLDPRT_NATIVE_VERSION);
        assert!(current
            .arenas
            .contains_key("feature_input_generated_surface_identities"));
    }
}

fn relation_instance_shape_valid(
    record: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
) -> bool {
    if record.scalar_refs.is_empty() || record.scalar_refs.len() > 3 {
        return false;
    }
    let Some(class) = lane
        .classes
        .iter()
        .find(|class| class.id == record.class_ref)
    else {
        return false;
    };
    if !matches!(
        crate::classification::native_object_class(&class.name).kind,
        crate::classification::NativeClassKind::SketchRelation(family)
            if family == record.family
    ) {
        return false;
    }
    let mut positions = Vec::new();
    for scalar_ref in &record.scalar_refs {
        let Some((position, scalar)) = lane
            .scalars
            .iter()
            .enumerate()
            .find(|(_, scalar)| scalar.id == *scalar_ref)
        else {
            return false;
        };
        if scalar.feature_ref.as_deref() != Some(record.feature_ref.as_str()) {
            return false;
        }
        positions.push((position, scalar));
    }
    let scalar_operands_match = |scalar: &crate::records::FeatureInputScalar| {
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .eq(record
                .operands
                .iter()
                .map(|operand| (operand.kind, operand.entity_index)))
            || (record.family == crate::records::FeatureInputRelationFamily::CircleDiameter
                && matches!(record.operands.as_slice(), [first, _]
                    if matches!(scalar.operands.as_slice(), [candidate]
                        if candidate.kind == first.kind
                            && candidate.entity_index == first.entity_index)))
    };
    if positions[0].1.offset != record.offset || !scalar_operands_match(positions[0].1) {
        return false;
    }
    let operand_scalars = positions
        .iter()
        .filter(|(_, scalar)| !scalar.operands.is_empty())
        .collect::<Vec<_>>();
    if operand_scalars.is_empty()
        || operand_scalars
            .windows(2)
            .any(|pair| pair[1].0 != pair[0].0 + 1)
        || operand_scalars
            .iter()
            .any(|(_, scalar)| !scalar_operands_match(scalar))
    {
        return false;
    }
    let detached = positions
        .iter()
        .filter(|(_, scalar)| scalar.operands.is_empty())
        .collect::<Vec<_>>();
    match detached.as_slice() {
        [] => true,
        [(position, scalar)] => {
            record.parameter_scalar_ref.as_deref() == Some(scalar.id.as_str())
                && *position > operand_scalars.last().expect("nonempty operand scalars").0
        }
        _ => false,
    }
}
