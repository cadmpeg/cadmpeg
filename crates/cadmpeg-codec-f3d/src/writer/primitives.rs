// SPDX-License-Identifier: Apache-2.0
//! Byte and predicate helpers shared by the source-less generator and the
//! edit-and-patch engine.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use crate::design::configurations::{table_at, ConfigurationTargets, Projection};
use crate::ids::neutral_configuration_id;
use crate::native::F3dNative;
use crate::records::configuration::table::ConfiguredValue;
use crate::records::configuration::DesignConfiguration;
use crate::F3dCodec;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, DesignConfiguration as Configuration, ParameterValue,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::Sense;
use serde::Serialize;

pub(super) fn f3d_native(ir: &CadIr) -> Result<Option<F3dNative>, CodecError> {
    ir.native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(Into::into)
}

/// The records that the configured objects of `native` name, in `native`
/// and in `target`.
pub(super) fn configuration_targets<'a>(
    target: &'a CadIr,
    native: &'a F3dNative,
) -> ConfigurationTargets<'a> {
    ConfigurationTargets {
        parameters: &native.design_parameters,
        owners: &native.design_parameter_owners,
        scopes: &native.design_parameter_scopes,
        neutral_parameters: &target.model.parameters,
        neutral_features: &target.model.features,
    }
}

/// The configurations the decoder states for `documents`, and the table
/// whose rows they project: the projection of the configuration tables,
/// with the active configuration reconciled with the features of
/// `targets`. A projection the decoder recovers from states no configuration
/// and no table, and a reconciliation that fails leaves the projection as it
/// is.
pub(super) fn decoded_configurations(
    documents: &[DesignConfiguration],
    targets: &ConfigurationTargets<'_>,
) -> Result<Projection, CodecError> {
    use crate::design::decode::record::is_fatal;
    let projection = match crate::design::configurations::project(documents, targets) {
        Ok(projection) => projection,
        Err(error) if is_fatal(&error) => return Err(error),
        Err(_) => return Ok(Projection::default()),
    };
    if !projection
        .configurations
        .iter()
        .any(|configuration| configuration.active)
    {
        return Ok(projection);
    }
    let mut reconciled = projection.configurations.clone();
    match crate::design::configurations::reconcile_active_configurations(
        &mut targets.neutral_features.to_vec(),
        &mut reconciled,
    ) {
        Ok(()) => Ok(Projection {
            configurations: reconciled,
            ..projection
        }),
        Err(error) if is_fatal(&error) => Err(error),
        Err(_) => Ok(projection),
    }
}

/// The features whose state each row of a table stores in a cell, keyed by
/// the configuration id of the row. Every other feature state of a
/// configuration is one the decoder derives from the features
/// (`reconcile_active_configurations`).
#[derive(Default)]
pub(super) struct StoredStates(BTreeMap<String, BTreeSet<FeatureId>>);

impl StoredStates {
    /// The stored states of the rows of the table at `table` of `documents`.
    pub(super) fn of(
        documents: &[DesignConfiguration],
        table: Option<(usize, usize)>,
        targets: &ConfigurationTargets<'_>,
    ) -> Self {
        let Some((index, table)) = table_at(documents, table) else {
            return Self::default();
        };
        let entry = documents[index].entry_name();
        Self(
            table
                .rows
                .iter()
                .map(|row| {
                    let features = row
                        .cells
                        .iter()
                        .filter_map(|cell| match cell.value {
                            ConfiguredValue::Feature { record_index, .. } => {
                                Some(targets.feature(record_index)?.id.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    (
                        neutral_configuration_id(entry, &row.id).to_string(),
                        features,
                    )
                })
                .collect(),
        )
    }

    /// The features whose state the row of configuration `id` stores in a
    /// cell; none where no row reads back as `id`.
    pub(super) fn row(&self, id: &str) -> &BTreeSet<FeatureId> {
        static NONE: BTreeSet<FeatureId> = BTreeSet::new();
        self.0.get(id).unwrap_or(&NONE)
    }
}

/// The property prefix of a suppressed state that the decoder infers in an
/// active configuration.
pub(super) const INFERRED: &str = "suppression_inferred:";

/// The configurations that the source of a write decodes as: the decode
/// of its retained image, made when first asked for. A write with no
/// source, or whose source does not decode, has none.
pub(super) struct SourceConfigurations<'a> {
    image: Option<&'a [u8]>,
    decoded: OnceCell<Option<Cow<'a, [Configuration]>>>,
}

impl<'a> SourceConfigurations<'a> {
    pub(super) fn new(image: Option<&'a [u8]>) -> Self {
        Self {
            image,
            decoded: OnceCell::new(),
        }
    }

    /// The source whose decode states `configurations`.
    pub(super) fn decoded(configurations: &'a [Configuration]) -> Self {
        Self {
            image: None,
            decoded: OnceCell::from(Some(Cow::Borrowed(configurations))),
        }
    }

    fn get(&self) -> Option<&[Configuration]> {
        self.decoded
            .get_or_init(|| {
                let decoded = F3dCodec
                    .decode(&mut Cursor::new(self.image?), &DecodeOptions::default())
                    .ok()?;
                Some(Cow::Owned(decoded.ir().model.configurations.clone()))
            })
            .as_deref()
    }
}

/// A field of a configuration that the written file reads back otherwise.
#[derive(Debug, PartialEq)]
pub(super) struct Difference {
    pub(super) field: String,
    pub(super) stated: Option<String>,
    pub(super) read: Option<String>,
}

impl Difference {
    /// The field `field`, whose values `stated` and `read` serialize.
    pub(super) fn of<T: Serialize>(field: String, stated: Option<&T>, read: Option<&T>) -> Self {
        let shown = |value: Option<&T>| value.and_then(|value| serde_json::to_string(value).ok());
        Self {
            field,
            stated: shown(stated),
            read: shown(read),
        }
    }
}

impl std::fmt::Display for Difference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shown = |value: &Option<String>| value.clone().unwrap_or_else(|| "absent".into());
        write!(
            formatter,
            "{} stated {}, reads back {}",
            self.field,
            shown(&self.stated),
            shown(&self.read)
        )
    }
}

/// A part of a configuration that its table row does not store.
#[derive(PartialEq, Serialize)]
#[serde(untagged)]
enum Derived<'a> {
    State(&'a ConfigurationEvaluation),
    Property(&'a str),
}

/// The parts of `configuration` that no cell of its row stores, keyed by
/// field path: the state of each feature not in `stored`, and each
/// [`INFERRED`] property.
fn derived<'a>(
    configuration: &'a Configuration,
    stored: &BTreeSet<FeatureId>,
) -> BTreeMap<String, Derived<'a>> {
    let states = configuration
        .feature_states
        .iter()
        .filter(|(feature, _)| !stored.contains(*feature))
        .map(|(feature, state)| {
            (
                format!("feature_states.{feature}"),
                Derived::State(&state.evaluation),
            )
        });
    let properties = configuration
        .properties
        .iter()
        .filter(|(key, _)| key.as_str().starts_with(INFERRED))
        .map(|(key, value)| (format!("properties.{key}"), Derived::Property(value)));
    states.chain(properties).collect()
}

/// The parts that no cell of the row stores (`stored` names the feature
/// states it does store) which `stated` states and `read`, what the
/// written file decodes as, states otherwise. A part that the source of the
/// write states as `stated` does is one the decoder derived, and not an
/// edit; without a source, every such part is listed.
pub(super) fn unheld(
    stated: &Configuration,
    read: &Configuration,
    stored: &BTreeSet<FeatureId>,
    source: &SourceConfigurations<'_>,
) -> Vec<Difference> {
    let (stated_parts, read_parts) = (derived(stated, stored), derived(read, stored));
    stated_parts
        .keys()
        .chain(read_parts.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| stated_parts.get(*key) != read_parts.get(*key))
        .filter(|key| {
            let Some(source) = source.get() else {
                return true;
            };
            let before = source
                .iter()
                .find(|configuration| configuration.id == stated.id)
                .map(|configuration| derived(configuration, stored));
            before.as_ref().and_then(|parts| parts.get(*key)) != stated_parts.get(*key)
        })
        .map(|key| Difference::of(key.clone(), stated_parts.get(key), read_parts.get(key)))
        .collect()
}

/// What a configuration states in the cells of its table row: every field
/// but its identity, position, activity, source slot and native record,
/// with each feature state reduced to its evaluation, and without the parts
/// no cell stores (`stored` names the feature states the row stores; see
/// [`unheld`]). The projection copies the dependencies and the definition
/// of a state from the feature, so an edit of the feature changes them in
/// every configuration.
#[derive(PartialEq)]
pub(super) struct Cells<'a> {
    name: Option<&'a str>,
    material: Option<&'a str>,
    properties: BTreeMap<&'a NonBlankString, &'a String>,
    parameter_overrides: &'a BTreeMap<ParameterId, String>,
    parameter_values: &'a BTreeMap<ParameterId, ParameterValue>,
    feature_states: BTreeMap<&'a FeatureId, &'a ConfigurationEvaluation>,
    bodies: Option<&'a DistinctMembers<BodyId>>,
}

impl<'a> Cells<'a> {
    pub(super) fn of(configuration: &'a Configuration, stored: &BTreeSet<FeatureId>) -> Self {
        let Configuration {
            id: _,
            ordinal: _,
            active: _,
            source_index: _,
            name,
            material,
            properties,
            parameter_overrides,
            bodies,
            parameter_values,
            feature_states,
            native_ref: _,
        } = configuration;
        Self {
            name: name.as_deref(),
            material: material.as_deref(),
            properties: properties
                .iter()
                .filter(|(key, _)| !key.as_str().starts_with(INFERRED))
                .collect(),
            parameter_overrides,
            parameter_values,
            feature_states: feature_states
                .iter()
                .filter(|(feature, _)| stored.contains(*feature))
                .map(|(feature, state)| (feature, &state.evaluation))
                .collect(),
            bodies: bodies.as_ref(),
        }
    }
}

/// Whether `left` and `right` state the same configurations: the same ids,
/// positions, activity, source slots, native records and [`Cells`], and no
/// [`unheld`] part of `left`.
fn same_configurations(
    left: &[Configuration],
    right: &[Configuration],
    stored: &StoredStates,
    source: &SourceConfigurations<'_>,
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            let stored = stored.row(left.id.as_str());
            (
                &left.id,
                left.ordinal,
                left.active,
                left.source_index,
                &left.native_ref,
            ) == (
                &right.id,
                right.ordinal,
                right.active,
                right.source_index,
                &right.native_ref,
            ) && Cells::of(left, stored) == Cells::of(right, stored)
                && unheld(left, right, stored, source).is_empty()
        })
}

/// Whether `configurations` are the ones the decoder states for
/// `documents`, where `source` is the patched source.
pub(super) fn projects(
    configurations: &[Configuration],
    documents: &[DesignConfiguration],
    targets: &ConfigurationTargets<'_>,
    source: &SourceConfigurations<'_>,
) -> Result<bool, CodecError> {
    let decoded = decoded_configurations(documents, targets)?;
    Ok(same_configurations(
        configurations,
        &decoded.configurations,
        &StoredStates::of(documents, decoded.table, targets),
        source,
    ))
}

/// Refuse a patch of `target` whose configurations are other than the ones
/// the decoder states for `native`, where `source` is the patched source.
pub(super) fn validate_configuration_projection(
    target: &CadIr,
    native: &F3dNative,
    source: &SourceConfigurations<'_>,
) -> Result<(), CodecError> {
    if !projects(
        &target.model.configurations,
        &native.design_configurations,
        &configuration_targets(target, native),
        source,
    )? {
        return Err(CodecError::Malformed(
            "neutral F3D configurations must equal the projection of native configuration tables"
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_assembly_projection(
    target: &CadIr,
    native: Option<&F3dNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return target
            .model
            .assembly_joints
            .is_empty()
            .then_some(())
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "source-less F3D generation does not support assembly joints".into(),
                )
            });
    };
    let expanded = crate::tree::expand(
        &cadmpeg_core::decode::ResourceLimits::default(),
        &target.model.product_definitions,
        &target.model.occurrences,
        &std::collections::BTreeMap::new(),
    )?;
    let mut inserts = crate::design::components::ComponentInserts::decide(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
        &native.xref_references,
    );
    inserts.bind_external_paths(&native.xref_references, &expanded.occurrences);
    let mut projected = crate::design::assembly::project_assembly_joints_with_paths(
        &native.design_parameter_scopes,
        &native.design_component_occurrences,
        &inserts,
        &native.design_joint_motions,
        &crate::design::components::paths::LocalOccurrencePaths::new(
            &native.design_component_occurrences,
            &native.xref_references,
            &expanded.definitions,
            &expanded.occurrences,
            &native.design_parameter_scopes,
        ),
    );
    crate::f3z::merge::rebind_standalone_projected_joints(
        native,
        &expanded.definitions,
        &expanded.occurrences,
        &mut projected.joints,
    );
    let mut selected = target.model.assembly_joints.clone();
    let parents = crate::tree::parent_ids(&expanded.definitions, &expanded.occurrences);
    let keys = native
        .design_component_occurrences
        .iter()
        .map(|carrier| (carrier.id.as_str(), carrier.occurrence_guid.as_str()))
        .chain(native.xref_references.iter().filter_map(|reference| {
            Some((
                reference.id.as_str(),
                reference
                    .placement_source
                    .as_ref()?
                    .occurrence_guid
                    .as_ref()?
                    .as_str(),
            ))
        }))
        .chain(native.design_parameter_scopes.iter().filter_map(|scope| {
            let key = scope
                .component_insert_construction()?
                .occurrence_key
                .as_ref()?;
            Some((scope.id.as_str(), key.value.as_str()))
        }))
        .collect::<std::collections::BTreeMap<_, _>>();
    for connector in selected
        .iter_mut()
        .flat_map(cadmpeg_ir::products::AssemblyJoint::connectors_mut)
    {
        if let cadmpeg_ir::products::OperandContainer::Occurrence {
            occurrence,
            occurrence_path,
        } = &mut connector.operand.container
        {
            let mut consumed = 0;
            for key in occurrence_path.iter() {
                let mut children = expanded.occurrences.iter().filter(|child| {
                    parents.get(&child.id) == Some(occurrence)
                        && child
                            .native_ref
                            .as_deref()
                            .and_then(|id| keys.get(id))
                            .is_some_and(|native_key| native_key.eq_ignore_ascii_case(key))
                });
                match (children.next(), children.next()) {
                    (Some(child), None) => {
                        *occurrence = child.id.clone();
                        consumed += 1;
                    }
                    _ => break,
                }
            }
            occurrence_path.drain(..consumed);
        }
    }
    if selected != projected.joints {
        return Err(CodecError::NotImplemented(
            "editing F3D assembly joints is not supported".into(),
        ));
    }
    Ok(())
}

pub(super) fn normalized_face_sense_to_native(desired: Sense, carrier_flipped: bool) -> Sense {
    if carrier_flipped {
        match desired {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        }
    } else {
        desired
    }
}

/// Zero-fill `bytes` to `length`, or truncate it when it is longer.
pub(crate) fn zero_fill_to(bytes: &mut Vec<u8>, length: usize) {
    if let Some(additional) = length.checked_sub(bytes.len()) {
        for _ in 0..additional {
            bytes.push(0);
        }
    } else {
        bytes.truncate(length);
    }
}

pub(super) fn native_bool(value: bool) -> u8 {
    if value {
        0x0a
    } else {
        0x0b
    }
}

pub(super) fn unique_knot_count(knots: &[f64]) -> usize {
    knots
        .iter()
        .enumerate()
        .filter(|(index, value)| *index == 0 || knots[*index - 1] != **value)
        .count()
}

#[cfg(test)]
mod tests;
