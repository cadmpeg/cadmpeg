// SPDX-License-Identifier: Apache-2.0
//! SOLIDWORKS native feature-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::records::{FeatureHistory, FeatureInputLane};

/// Current schema version for the SOLIDWORKS native namespace.
pub const SLDPRT_NATIVE_VERSION: u32 = 1;

pub(crate) const SLDPRT_ARENA_NAMES: &[&str] = &[
    "configurations",
    "feature_histories",
    "feature_input_lanes",
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
                lane.sketch_entities.clear();
                lane
            })
            .collect::<Vec<_>>();
        namespace.set_arena("feature_input_lanes", &lanes)?;
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
