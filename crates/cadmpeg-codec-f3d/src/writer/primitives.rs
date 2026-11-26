// SPDX-License-Identifier: Apache-2.0
//! Byte and predicate helpers shared by the source-less generator and the
//! edit-and-patch engine.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::configurations::{ConfigurationTargets, Projection};
use crate::native::F3dNative;
use crate::records::configuration::table::{ColumnTarget, ConfigurationTable};
use crate::records::configuration::DesignConfiguration;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, DesignConfiguration as Configuration, ParameterValue,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::Sense;

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

/// The configurations the decoder states for `native`, and the table whose
/// rows they project: the projection of its configuration tables, with the
/// active configuration reconciled with the features of `target`. A
/// projection the decoder recovers from states no configuration and no
/// table, and a reconciliation that fails leaves the projection as it is.
pub(super) fn decoded_configurations(
    target: &CadIr,
    native: &F3dNative,
) -> Result<Projection, CodecError> {
    use crate::design::decode::record::is_fatal;
    let projection = match crate::design::configurations::project(
        &native.design_configurations,
        &configuration_targets(target, native),
    ) {
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
        &mut target.model.features.clone(),
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

/// The table at `table`, the position of a document of `documents` and of
/// the table in the document's `tables` array.
pub(super) fn table_at(
    documents: &[DesignConfiguration],
    table: Option<(usize, usize)>,
) -> Option<&ConfigurationTable> {
    let (document, position) = table?;
    documents
        .get(document)?
        .tables()
        .iter()
        .find(|candidate| candidate.position == position)
}

/// The features that a column of `table` suppresses.
pub(super) fn columned_features(
    table: Option<&ConfigurationTable>,
    targets: &ConfigurationTargets<'_>,
) -> BTreeSet<FeatureId> {
    table
        .into_iter()
        .flat_map(|table| &table.columns)
        .filter_map(|column| match column.target {
            ColumnTarget::Feature(record) => Some(targets.feature(record)?.id.clone()),
            _ => None,
        })
        .collect()
}

/// The property prefix of a suppressed state that the decoder infers in an
/// active configuration.
pub(super) const INFERRED: &str = "suppression_inferred:";

/// What a configuration states of its table row: every field but its
/// identity, position, activity, source slot and native record, with each
/// feature state reduced to its evaluation. The projection copies the
/// dependencies and the definition of a state from the feature, so an edit
/// of the feature changes them in every configuration. Of an active
/// configuration, it leaves out what the decoder derives from the features
/// (`reconcile_active_configurations`): the state of each feature that no
/// column of the table names, and each [`INFERRED`] property.
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
    pub(super) fn of(configuration: &'a Configuration, columned: &BTreeSet<FeatureId>) -> Self {
        let Configuration {
            id: _,
            ordinal: _,
            active,
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
                .filter(|(key, _)| !(*active && key.as_str().starts_with(INFERRED)))
                .collect(),
            parameter_overrides,
            parameter_values,
            feature_states: feature_states
                .iter()
                .filter(|(feature, _)| !*active || columned.contains(*feature))
                .map(|(feature, state)| (feature, &state.evaluation))
                .collect(),
            bodies: bodies.as_ref(),
        }
    }
}

/// Whether `left` and `right` state the same configurations: the same ids,
/// positions, activity, source slots, native records and [`Cells`].
fn same_configurations(
    left: &[Configuration],
    right: &[Configuration],
    columned: &BTreeSet<FeatureId>,
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
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
            ) && Cells::of(left, columned) == Cells::of(right, columned)
        })
}

/// Refuse a patch of `target` whose configurations are other than the ones
/// the decoder states for `native`.
pub(super) fn validate_configuration_projection(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let decoded = decoded_configurations(target, native)?;
    let columned = columned_features(
        table_at(&native.design_configurations, decoded.table),
        &configuration_targets(target, native),
    );
    if !same_configurations(
        &target.model.configurations,
        &decoded.configurations,
        &columned,
    ) {
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
