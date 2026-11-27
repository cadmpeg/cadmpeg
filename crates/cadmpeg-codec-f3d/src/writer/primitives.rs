// SPDX-License-Identifier: Apache-2.0
//! Byte and predicate helpers shared by the source-less generator and the
//! edit-and-patch engine.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::configurations::{ConfigurationTargets, Projection};
use crate::design::decode::record::is_fatal;
use crate::native::F3dNative;
use crate::records::configuration::DesignConfiguration;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::{
    ConfigurationFeatureState, DesignConfiguration as Configuration,
};
use cadmpeg_ir::features::{Feature, FeatureId};
use cadmpeg_ir::topology::Sense;
use serde::Serialize;

pub(super) fn f3d_native(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Option<F3dNative>, CodecError> {
    ir.native
        .namespace("f3d")
        .map(|namespace| F3dNative::load_inner(ctx, namespace))
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

/// The projection of the configuration tables `documents`, as the decoder
/// makes it: a projection the decoder recovers from states no
/// configuration and no table.
pub(super) fn projection(
    documents: &[DesignConfiguration],
    targets: &ConfigurationTargets<'_>,
) -> Result<Projection, CodecError> {
    match crate::design::configurations::project(documents, targets) {
        Ok(projection) => Ok(projection),
        Err(error) if is_fatal(&error) => Err(error),
        Err(_) => Ok(Projection::default()),
    }
}

/// `projection` with its active configuration reconciled with `features`,
/// and `features` with the suppression that the active configuration gives
/// them, as the decoder reconciles both. A reconciliation that fails leaves
/// both as they are.
pub(super) fn reconciled(
    projection: Projection,
    features: &mut [Feature],
) -> Result<Projection, CodecError> {
    if !projection
        .configurations
        .iter()
        .any(|configuration| configuration.active)
    {
        return Ok(projection);
    }
    let mut configurations = projection.configurations.clone();
    let mut reconciled = features.to_vec();
    match crate::design::configurations::reconcile_active_configurations(
        &mut reconciled,
        &mut configurations,
    ) {
        Ok(()) => {
            features.clone_from_slice(&reconciled);
            Ok(Projection {
                configurations,
                ..projection
            })
        }
        Err(error) if is_fatal(&error) => Err(error),
        Err(_) => Ok(projection),
    }
}

/// The configurations the decoder states for `documents`, and the table
/// whose rows they project.
pub(super) fn decoded_configurations(
    documents: &[DesignConfiguration],
    targets: &ConfigurationTargets<'_>,
) -> Result<Projection, CodecError> {
    reconciled(
        projection(documents, targets)?,
        &mut targets.neutral_features.to_vec(),
    )
}

/// The property prefix of a suppressed state that the decoder infers in an
/// active configuration. It annotates a feature state, which
/// [`differences`] compares.
const INFERRED: &str = "suppression_inferred:";

/// The state of each of `features` in a configuration that states none:
/// the feature's own state (cadir-v11.md §5.11, Resolution).
pub(super) fn bases(features: &[Feature]) -> BTreeMap<&FeatureId, ConfigurationFeatureState> {
    features
        .iter()
        .map(|feature| (&feature.id, feature.configuration_state()))
        .collect()
}

/// The state of `feature` in `configuration`: its own, else the one in
/// `bases`.
fn resolved<'s>(
    configuration: &'s Configuration,
    feature: &FeatureId,
    bases: &'s BTreeMap<&FeatureId, ConfigurationFeatureState>,
) -> Option<&'s ConfigurationFeatureState> {
    configuration
        .feature_states
        .get(feature)
        .or_else(|| bases.get(feature))
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

/// The keys whose values `left` and `right` state otherwise.
fn differing<'k, K: Ord, V: PartialEq>(
    left: &'k BTreeMap<K, V>,
    right: &'k BTreeMap<K, V>,
) -> impl Iterator<Item = &'k K> {
    left.keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| left.get(*key) != right.get(*key))
}

/// The fields that a table row states, other than its name, which `read`
/// states otherwise than `stated`: material, bodies, properties, parameter
/// overrides and values, and the evaluation, definition and dependencies of
/// each feature state. A configuration that states no state of a feature
/// resolves it to the feature's state in `bases`.
pub(super) fn differences(
    stated: &Configuration,
    read: &Configuration,
    bases: &BTreeMap<&FeatureId, ConfigurationFeatureState>,
) -> Vec<Difference> {
    let mut fields = Vec::new();
    if stated.material != read.material {
        fields.push(Difference::of(
            "material".to_owned(),
            stated.material.as_ref(),
            read.material.as_ref(),
        ));
    }
    if stated.bodies != read.bodies {
        fields.push(Difference::of(
            "bodies".to_owned(),
            stated.bodies.as_ref(),
            read.bodies.as_ref(),
        ));
    }
    fields.extend(
        differing(&stated.properties, &read.properties)
            .filter(|key| !key.as_str().starts_with(INFERRED))
            .map(|key| {
                Difference::of(
                    format!("properties.{key}"),
                    stated.properties.get(key),
                    read.properties.get(key),
                )
            }),
    );
    fields.extend(
        differing(&stated.parameter_overrides, &read.parameter_overrides).map(|parameter| {
            Difference::of(
                format!("parameter_overrides.{parameter}"),
                stated.parameter_overrides.get(parameter),
                read.parameter_overrides.get(parameter),
            )
        }),
    );
    fields.extend(
        differing(&stated.parameter_values, &read.parameter_values).map(|parameter| {
            Difference::of(
                format!("parameter_values.{parameter}"),
                stated.parameter_values.get(parameter),
                read.parameter_values.get(parameter),
            )
        }),
    );
    let features = stated
        .feature_states
        .keys()
        .chain(read.feature_states.keys())
        .collect::<BTreeSet<_>>();
    for feature in features {
        let field = format!("feature_states.{feature}");
        match (
            resolved(stated, feature, bases),
            resolved(read, feature, bases),
        ) {
            (Some(stated), Some(read)) => {
                if stated.evaluation != read.evaluation {
                    fields.push(Difference::of(
                        field.clone(),
                        Some(&stated.evaluation),
                        Some(&read.evaluation),
                    ));
                }
                if stated.definition != read.definition {
                    fields.push(Difference::of(
                        format!("{field}.definition"),
                        Some(&stated.definition),
                        Some(&read.definition),
                    ));
                }
                if stated.dependencies != read.dependencies {
                    fields.push(Difference::of(
                        format!("{field}.dependencies"),
                        Some(&stated.dependencies),
                        Some(&read.dependencies),
                    ));
                }
            }
            (stated, read) => fields.push(Difference::of(
                field,
                stated.map(|state| &state.evaluation),
                read.map(|state| &state.evaluation),
            )),
        }
    }
    fields
}

/// Whether `left` and `right` state the same table row: the same name and
/// no [`differences`].
pub(super) fn same_row(
    left: &Configuration,
    right: &Configuration,
    bases: &BTreeMap<&FeatureId, ConfigurationFeatureState>,
) -> bool {
    left.name == right.name && differences(left, right, bases).is_empty()
}

/// Whether `configurations` are the ones the decoder states for
/// `documents`: the same ids, positions, activity, source slots and native
/// records, and the same rows ([`same_row`]).
pub(super) fn projects(
    configurations: &[Configuration],
    documents: &[DesignConfiguration],
    targets: &ConfigurationTargets<'_>,
) -> Result<bool, CodecError> {
    let decoded = decoded_configurations(documents, targets)?.configurations;
    let bases = bases(targets.neutral_features);
    let identity = |configuration: &Configuration| {
        (
            configuration.id.clone(),
            configuration.ordinal,
            configuration.active,
            configuration.source_index,
            configuration.native_ref.clone(),
        )
    };
    Ok(configurations.len() == decoded.len()
        && configurations.iter().zip(&decoded).all(|(left, right)| {
            identity(left) == identity(right) && same_row(left, right, &bases)
        }))
}

pub(crate) fn validate_assembly_projection(
    ctx: &DecodeContext<'_>,
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
        &ctx.policy().limits,
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
