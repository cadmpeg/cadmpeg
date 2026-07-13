// SPDX-License-Identifier: Apache-2.0
//! SOLIDWORKS native feature-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::records::{
    FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName, FeatureInputReference,
    FeatureInputRelationBinding, FeatureInputRelationInstance, FeatureInputScalar, PmiDimension,
};

/// Current schema version for the SOLIDWORKS native namespace.
pub const SLDPRT_NATIVE_VERSION: u32 = 1;

pub(crate) const SLDPRT_ARENA_NAMES: &[&str] = &[
    "configurations",
    "feature_histories",
    "feature_input_classes",
    "feature_input_lanes",
    "feature_input_names",
    "feature_input_references",
    "feature_input_relation_bindings",
    "feature_input_relation_instances",
    "feature_input_scalars",
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
        let mut native = Self {
            version: namespace.version,
            feature_histories: namespace.arena_as("feature_histories")?,
            feature_input_lanes: namespace.arena_as("feature_input_lanes")?,
            pmi_dimensions: namespace.arena_as("pmi_dimensions")?,
        };
        let configurations: Vec<crate::records::Configuration> =
            namespace.arena_as("configurations")?;
        let features: Vec<crate::records::Feature> = namespace.arena_as("features")?;
        let entities: Vec<crate::records::SketchInputEntity> =
            namespace.arena_as("sketch_input_entities")?;
        let classes: Vec<FeatureInputClass> = namespace.arena_as("feature_input_classes")?;
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
                || record
                    .scalar_refs
                    .iter()
                    .any(|scalar| !scalar_ids.contains(scalar.as_str()))
                || record
                    .parameter_scalar_ref
                    .as_deref()
                    .is_some_and(|scalar| !record.scalar_refs.iter().any(|value| value == scalar))
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
        let feature_ids = self
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
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
            for scalar in &lane.scalars {
                for operand in &scalar.operands {
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
                }
            }
            if let Some(record) = lane
                .sketch_entities
                .iter()
                .find(|record| record.parent != lane.id)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "sketch input entity {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
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
                lane.references.clear();
                lane.sketch_entities.clear();
                lane
            })
            .collect::<Vec<_>>();
        namespace.set_arena("feature_input_lanes", &lanes)?;
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
