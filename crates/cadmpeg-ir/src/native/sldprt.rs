// SPDX-License-Identifier: Apache-2.0
//! SOLIDWORKS native feature-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::history::{FeatureHistory, FeatureInputLane};

/// Current schema version for the SOLIDWORKS native namespace.
pub const SLDPRT_NATIVE_VERSION: u32 = 1;

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
    pub(crate) fn store(&self, namespace: &mut super::NativeNamespace) {
        namespace.version = SLDPRT_NATIVE_VERSION;
        namespace
            .set_arena("feature_histories", &self.feature_histories)
            .expect("typed native records serialize");
        namespace
            .set_arena(
                "configurations",
                &self
                    .feature_histories
                    .iter()
                    .flat_map(|history| history.configurations.clone())
                    .collect::<Vec<_>>(),
            )
            .expect("typed native records serialize");
        namespace
            .set_arena(
                "features",
                &self
                    .feature_histories
                    .iter()
                    .flat_map(|history| history.features.clone())
                    .collect::<Vec<_>>(),
            )
            .expect("typed native records serialize");
        namespace
            .set_arena("feature_input_lanes", &self.feature_input_lanes)
            .expect("typed native records serialize");
        namespace
            .set_arena(
                "sketch_input_entities",
                &self
                    .feature_input_lanes
                    .iter()
                    .flat_map(|lane| lane.sketch_entities.clone())
                    .collect::<Vec<_>>(),
            )
            .expect("typed native records serialize");
    }
    /// Sort every native arena by its normative record identity.
    pub(crate) fn finalize(&mut self) {
        self.feature_histories
            .sort_by(|left, right| left.id.cmp(&right.id));
        self.feature_input_lanes
            .sort_by(|left, right| left.id.cmp(&right.id));
    }

    /// Return counts for every non-empty native arena and nested record family.
    pub(crate) fn loss_counts(&self) -> Vec<(&'static str, usize)> {
        let mut counts = Vec::new();
        let values = [
            ("feature_histories", self.feature_histories.len()),
            ("feature_input_lanes", self.feature_input_lanes.len()),
            (
                "configurations",
                self.feature_histories
                    .iter()
                    .map(|history| history.configurations.len())
                    .sum(),
            ),
            (
                "features",
                self.feature_histories
                    .iter()
                    .map(|history| history.features.len())
                    .sum(),
            ),
            (
                "sketch_input_entities",
                self.feature_input_lanes
                    .iter()
                    .map(|lane| lane.sketch_entities.len())
                    .sum(),
            ),
        ];
        counts.extend(values.into_iter().filter(|(_, count)| *count != 0));
        counts
    }
}
