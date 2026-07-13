// SPDX-License-Identifier: Apache-2.0
//! SOLIDWORKS native feature-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::records::{
    FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName, FeatureInputScalar,
};

/// Current schema version for the SOLIDWORKS native namespace.
pub const SLDPRT_NATIVE_VERSION: u32 = 1;

pub(crate) const SLDPRT_ARENA_NAMES: &[&str] = &[
    "configurations",
    "feature_histories",
    "feature_input_classes",
    "feature_input_lanes",
    "feature_input_names",
    "feature_input_scalars",
    "features",
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
}

impl Default for SldprtNative {
    fn default() -> Self {
        Self {
            version: SLDPRT_NATIVE_VERSION,
            feature_histories: Vec::new(),
            feature_input_lanes: Vec::new(),
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
        };
        let configurations: Vec<crate::records::Configuration> =
            namespace.arena_as("configurations")?;
        let features: Vec<crate::records::Feature> = namespace.arena_as("features")?;
        let entities: Vec<crate::records::SketchInputEntity> =
            namespace.arena_as("sketch_input_entities")?;
        let classes: Vec<FeatureInputClass> = namespace.arena_as("feature_input_classes")?;
        let names: Vec<FeatureInputName> = namespace.arena_as("feature_input_names")?;
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
        for lane in &self.feature_input_lanes {
            let name_ids = lane
                .names
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
