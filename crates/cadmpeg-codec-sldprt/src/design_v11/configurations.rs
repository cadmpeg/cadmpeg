// SPDX-License-Identifier: Apache-2.0
//! The part's configurations (§5.11).
//!
//! `SolidWorks` configurations are peers, so the base is the configuration
//! active at decode, which states no overrides. Another configuration states
//! the material it assigns where that differs from the active one's, the
//! value of each parameter where its stored state differs from the active
//! one's (see [`super::parameters::configuration_override`]).
//!
//! Interim: the decoder reads a feature's suppression from the history,
//! which every configuration shares, so no configuration states a
//! suppression of its own, and a loss names each configuration whose own
//! suppression is unread (DI-96). The configuration's own definition of a feature
//! (§5.11 `definition`) is deferred: the decoder's per-configuration
//! definitions differ from the active one's also where only a dimension
//! differs, and no reading yet separates a structural difference from that,
//! so a loss names each feature whose definition differs.

use std::collections::{BTreeSet, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::{
    DesignConfiguration, MaterialOverride, MaterialRef, Overrides,
};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Body;

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

/// The note naming the bodies of `geometry` outside `current`, the active
/// configuration's bodies, in a part whose feature tree holds no feature.
/// The scope lists them with its unproduced bodies (§5.5): a scope with no
/// root container has no position for the stored state of a configuration
/// that is not active.
pub(super) fn unplaced(geometry: &[Body], current: &[BodyId]) -> Option<LossNote> {
    let current = current.iter().collect::<HashSet<_>>();
    let mut named = NamedCount::default();
    for body in geometry.iter().filter(|body| !current.contains(&body.id)) {
        named.push(body.id.as_str());
    }
    (!named.is_empty()).then(|| {
        SldprtLossCode::DesignConfigurationStateUnplaced.note(format!(
            "{} bodies the geometry holds outside the active configuration's bodies are \
             unproduced bodies of the scope: the feature tree holds no feature, and a scope \
             with no root container has no position for the stored state of a configuration \
             that is not active: {named}",
            named.count()
        ))
    })
}

/// The notes naming what the configurations that are not active do not
/// state of `features`: their own suppression, which the decoder does not
/// read, and each feature whose definition in their decoded state, the
/// model configuration of the same id in `decoded`, differs from the active
/// configuration's.
pub(super) fn unread_states(
    configurations: &Configurations,
    decoded: &[cadmpeg_ir::features::parameters::DesignConfiguration],
    features: &[Feature],
) -> Vec<LossNote> {
    let Configurations { records, active } = configurations;
    if features.is_empty() {
        return Vec::new();
    }
    let state = |id: &ConfigurationId| decoded.iter().find(|state| state.id == *id);
    let base = active.as_ref().and_then(state);
    let mut suppression = NamedCount::default();
    let mut definitions = NamedCount::default();
    for record in records {
        if active.as_ref() == Some(&record.id) {
            continue;
        }
        suppression.push(record.id.as_str());
        let Some(state) = state(&record.id) else {
            continue;
        };
        for feature in features {
            let differs = state
                .feature_states
                .get(&feature.id)
                .zip(base.and_then(|base| base.feature_states.get(&feature.id)))
                .is_some_and(|(own, active)| own.definition != active.definition);
            if differs {
                definitions.push(&format!(
                    "{} in {}",
                    feature.id.as_str(),
                    record.id.as_str()
                ));
            }
        }
    }
    let mut notes = Vec::new();
    if !suppression.is_empty() {
        notes.push(
            SldprtLossCode::DesignConfigurationSuppressionUnread.note(format!(
                "{} configurations state no suppression of their own: the decoder reads one \
             suppression state per feature, which every configuration shares: {suppression}",
                suppression.count()
            )),
        );
    }
    if !definitions.is_empty() {
        notes.push(
            SldprtLossCode::DesignConfigurationDefinitionUnstated.note(format!(
                "{} features of a configuration that is not active state no definition of their \
             own where the decoded definition differs from the active configuration's: no \
             reading separates a structural difference from a dimension's: {definitions}",
                definitions.count()
            )),
        );
    }
    notes
}

/// States in each configuration that is not active the parameter values in
/// which its decoded state, the model configuration of the same id in
/// `decoded`, differs from the active configuration's.
pub(super) fn parameter_overrides(
    configurations: &mut Configurations,
    decoded: &[cadmpeg_ir::features::parameters::DesignConfiguration],
    parameters: &[DesignParameter],
    length_unit: Option<UnitSymbol>,
) {
    let Configurations { records, active } = configurations;
    let base = decoded
        .iter()
        .find(|configuration| active.as_ref().is_some_and(|id| configuration.id == *id));
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
            let active = base.and_then(|base| base.parameter_values.get(&parameter.id));
            if let Some(entry) = super::parameters::configuration_override(
                parameter,
                stored.clone(),
                active.cloned(),
                length_unit,
            ) {
                record
                    .overrides
                    .parameters
                    .insert(parameter.id.clone(), entry);
            }
        }
    }
}
