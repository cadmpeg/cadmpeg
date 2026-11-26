// SPDX-License-Identifier: Apache-2.0
//! The part's configurations (§5.11).
//!
//! `SolidWorks` configurations are peers, so the base is the configuration
//! active at decode, which states no overrides. Another configuration states
//! the material it assigns where that differs from the active one's, the
//! value of each parameter where its stored state differs from the active
//! one's (see [`super::parameters::configuration_override`]). The decoder
//! reads a feature's suppression from the history, which every
//! configuration shares, so no configuration states a suppression of its
//! own.
//!
//! Interim: the configuration's own definition of a feature (§5.11
//! `definition`) is deferred. The decoder's per-configuration definitions
//! differ from the active one's also where only a dimension differs, and
//! no reading yet separates a structural difference from that.

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::{
    DesignConfiguration, MaterialOverride, MaterialRef, Overrides,
};
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;

use crate::history::project::neutral_configuration_id;
use crate::loss::{NamedCount, SldprtLossCode};
use crate::records::FeatureHistory;

/// The configurations of a scope and the active one.
#[derive(Default)]
pub(super) struct Configurations {
    pub(super) records: Vec<DesignConfiguration>,
    pub(super) active: Option<ConfigurationId>,
}

/// The configurations of the histories, or none where the document's active
/// configuration does not resolve to one of them: a scope with
/// configurations names its active one (part 10), and a loss in `losses`
/// names them.
pub(super) fn configurations(
    histories: &[FeatureHistory],
    source: Option<&SourceMeta>,
    scope: &ProductDefinitionId,
    losses: &mut Vec<LossNote>,
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
                configuration.partition_index(),
            )
        })
        .collect::<Vec<_>>();
    let Some(active) = crate::decode::active_configuration_position(source, &selectors) else {
        let mut named = NamedCount::default();
        for configuration in &native {
            named.push(neutral_configuration_id(&configuration.id)?.as_str());
        }
        if !named.is_empty() {
            losses.push(
                SldprtLossCode::DesignConfigurationActiveUnread.note(format!(
                    "{} configurations state no design configuration: the document's active \
                 configuration resolves to none of them: {named}",
                    named.count()
                )),
            );
        }
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

/// States in each configuration that is not active the parameter values in
/// which its decoded state, the model configuration of the same id in
/// `decoded`, differs from the design's.
pub(super) fn parameter_overrides(
    configurations: &mut Configurations,
    decoded: &[cadmpeg_ir::features::parameters::DesignConfiguration],
    parameters: &[DesignParameter],
) {
    let Configurations { records, active } = configurations;
    for record in records.iter_mut() {
        if active.as_ref() == Some(&record.id) {
            continue;
        }
        let Some(state) = decoded
            .iter()
            .find(|configuration| configuration.id == record.id)
        else {
            continue;
        };
        for parameter in parameters {
            let Some(stored) = state.parameter_values.get(&parameter.id) else {
                continue;
            };
            if let Some(entry) =
                super::parameters::configuration_override(parameter, stored.clone())
            {
                record
                    .overrides
                    .parameters
                    .insert(parameter.id.clone(), entry);
            }
        }
    }
}
