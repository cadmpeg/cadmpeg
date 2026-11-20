// SPDX-License-Identifier: Apache-2.0
//! The part's configurations (§5.11).
//!
//! `SolidWorks` configurations are peers, so the base is the configuration
//! active at decode, which states no overrides. Another configuration states
//! the material it assigns where that differs from the active one's. Its
//! parameter and suppression differences live in its own feature-input lane,
//! which this projection does not read, so it states none.

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::{
    DesignConfiguration, MaterialOverride, MaterialRef, Overrides,
};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::ids::ProductDefinitionId;

use crate::history::project::neutral_configuration_id;
use crate::records::FeatureHistory;

/// The configurations of a scope and the active one.
#[derive(Default)]
pub(super) struct Configurations {
    pub(super) records: Vec<DesignConfiguration>,
    pub(super) active: Option<ConfigurationId>,
}

/// The configurations of the histories, or none where the document's active
/// configuration does not resolve to one of them: a scope with
/// configurations names its active one (part 10).
pub(super) fn configurations(
    histories: &[FeatureHistory],
    source: Option<&SourceMeta>,
    scope: &ProductDefinitionId,
) -> Result<Configurations, CodecError> {
    let native = histories
        .iter()
        .flat_map(|history| &history.configurations)
        .collect::<Vec<_>>();
    let selectors = native
        .iter()
        .map(|configuration| {
            (
                Some(configuration.name.as_str()),
                configuration.source_index,
            )
        })
        .collect::<Vec<_>>();
    let Some(active) = crate::decode::active_configuration_position(source, &selectors) else {
        return Ok(Configurations::default());
    };
    let base_material = native[active].material.as_deref();
    let mut records = Vec::with_capacity(native.len());
    for configuration in &native {
        let materials = configuration
            .material
            .as_deref()
            .filter(|material| Some(*material) != base_material)
            .and_then(|material| NonBlankString::new(material.to_owned()))
            .map(|material| MaterialOverride {
                target: None,
                material: MaterialRef::Named(material),
            })
            .into_iter()
            .collect();
        records.push(DesignConfiguration {
            id: neutral_configuration_id(&configuration.id)?,
            scope: scope.clone(),
            name: configuration.name.clone(),
            parent: None,
            comment: None,
            overrides: Overrides::default(),
            active_arrangements: BTreeSet::new(),
            materials,
            native_ref: Some(configuration.id.clone()),
        });
    }
    let active = records.get(active).map(|record| record.id.clone());
    Ok(Configurations { records, active })
}
