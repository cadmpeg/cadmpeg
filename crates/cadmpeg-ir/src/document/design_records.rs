// SPDX-License-Identifier: Apache-2.0
//! The configurations, parameters, features, and sketches a reference can
//! name.

use std::collections::BTreeSet;

use super::CadIr;
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::sketches::SketchId;

/// The ids of the configurations, parameters, features, and sketches a
/// document holds, in the design root and in the v10 `model` arenas.
/// Decoders write the v10 arenas until their codec port; XF deletes the v10
/// lookups with them.
pub(crate) struct DesignRecords<'a> {
    configurations: BTreeSet<&'a str>,
    parameters: BTreeSet<&'a str>,
    features: BTreeSet<&'a str>,
    sketches: BTreeSet<&'a str>,
}

impl<'a> DesignRecords<'a> {
    pub(crate) fn of(ir: &'a CadIr) -> Self {
        let design = &ir.design;
        let mut records = Self {
            configurations: design
                .configurations
                .iter()
                .map(|record| record.id.as_str())
                .collect(),
            parameters: design
                .parameters
                .iter()
                .map(|record| record.id.as_str())
                .collect(),
            features: design
                .features
                .iter()
                .map(|record| record.id.as_str())
                .collect(),
            sketches: design
                .sketches
                .iter()
                .map(|record| record.id.as_str())
                .collect(),
        };
        let model = &ir.model;
        records
            .configurations
            .extend(model.configurations.iter().map(|record| record.id.as_str()));
        records
            .parameters
            .extend(model.parameters.iter().map(|record| record.id.as_str()));
        records
            .features
            .extend(model.features.iter().map(|record| record.id.as_str()));
        records
            .sketches
            .extend(model.sketches.iter().map(|record| record.id.as_str()));
        records
    }

    pub(crate) fn holds_configuration(&self, id: &ConfigurationId) -> bool {
        self.configurations.contains(id.as_str())
    }

    pub(crate) fn holds_parameter(&self, id: &ParameterId) -> bool {
        self.parameters.contains(id.as_str())
    }

    pub(crate) fn holds_feature(&self, id: &FeatureId) -> bool {
        self.features.contains(id.as_str())
    }

    pub(crate) fn holds_sketch(&self, id: &SketchId) -> bool {
        self.sketches.contains(id.as_str())
    }
}
