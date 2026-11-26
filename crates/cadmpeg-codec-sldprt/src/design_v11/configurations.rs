// SPDX-License-Identifier: Apache-2.0
//! The part's configurations (§5.11).
//!
//! `SolidWorks` configurations are peers, so the base is the configuration
//! active at decode, which states no overrides. Another configuration states
//! the material it assigns where that differs from the active one's. Its
//! parameter and suppression differences live in its own feature-input lane,
//! which this projection does not read, so it states none.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::{
    DesignConfiguration, MaterialOverride, MaterialRef, Overrides,
};
use cadmpeg_ir::design::projection::restated::Restater;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::features::parameters::DesignConfiguration as ModelConfiguration;
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
                configuration.partition_index(),
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

/// States the model configuration fields each configuration leaves out: the
/// custom properties, which no configuration field holds, and the parameter
/// values, parameter expressions and feature suppressions of a configuration
/// other than the active one where they differ from the active one's and its
/// overrides do not state them (§5.11).
pub(super) fn restate(ir: &mut CadIr, configurations: &Configurations) {
    let model = ir
        .model
        .configurations
        .iter()
        .map(|configuration| (configuration.id.as_str(), configuration))
        .collect::<HashMap<_, _>>();
    let base = configurations
        .active
        .as_ref()
        .and_then(|active| model.get(active.as_str()).copied());
    let mut restated = Vec::new();
    for design in &configurations.records {
        let Some(own) = model.get(design.id.as_str()).copied() else {
            continue;
        };
        let mut cut = Vec::new();
        if !own.properties.is_empty() {
            cut.push("properties");
        }
        if let Some(base) = base.filter(|base| base.id != own.id) {
            cut.extend(override_cut(design, own, base));
        }
        if !cut.is_empty() {
            restated.push((design.id.as_str().to_owned(), cut));
        }
    }
    for (id, cut) in restated {
        let by = Restater::Record(id.clone());
        ir.model.restated_mut().record(&id, by, cut);
    }
}

fn override_cut(
    design: &DesignConfiguration,
    own: &ModelConfiguration,
    base: &ModelConfiguration,
) -> Vec<&'static str> {
    fn unstated<K: Ord, V: PartialEq>(
        own: &BTreeMap<K, V>,
        base: &BTreeMap<K, V>,
        stated: &HashSet<&str>,
        key: impl Fn(&K) -> &str,
    ) -> bool {
        own.iter()
            .any(|(id, value)| base.get(id) != Some(value) && !stated.contains(key(id)))
    }
    let parameters = design
        .overrides
        .parameters
        .keys()
        .map(cadmpeg_ir::ParameterId::as_str)
        .collect::<HashSet<_>>();
    let features = design
        .overrides
        .features
        .keys()
        .map(cadmpeg_ir::FeatureId::as_str)
        .collect::<HashSet<_>>();
    let suppressed = |configuration: &ModelConfiguration| {
        configuration
            .feature_states
            .iter()
            .map(|(id, state)| (id.clone(), state.evaluation.is_suppressed()))
            .collect::<BTreeMap<_, _>>()
    };
    let mut cut = Vec::new();
    if unstated(
        &own.parameter_values,
        &base.parameter_values,
        &parameters,
        |id| id.as_str(),
    ) {
        cut.push("parameter_values");
    }
    if unstated(
        &own.parameter_overrides,
        &base.parameter_overrides,
        &parameters,
        |id| id.as_str(),
    ) {
        cut.push("parameter_overrides");
    }
    let base_suppressed = suppressed(base);
    let own_suppressed = suppressed(own)
        .into_iter()
        .filter(|(id, suppressed)| *suppressed || base_suppressed.contains_key(id))
        .collect::<BTreeMap<_, _>>();
    if unstated(&own_suppressed, &base_suppressed, &features, |id| {
        id.as_str()
    }) {
        cut.push("feature_states");
    }
    cut
}
