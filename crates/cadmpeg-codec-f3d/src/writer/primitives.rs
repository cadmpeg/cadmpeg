// SPDX-License-Identifier: Apache-2.0
//! Byte and predicate helpers shared by the source-less generator and the
//! edit-and-patch engine.

use crate::native::F3dNative;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::Sense;

pub(super) fn f3d_native(ir: &CadIr) -> Result<Option<F3dNative>, CodecError> {
    ir.native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(Into::into)
}

/// The configurations the decoder states for `native`: the projection of its
/// configuration tables, with the active configuration reconciled with the
/// features of `target`. A projection the decoder recovers from states none,
/// and a reconciliation that fails leaves the projection as it is.
pub(super) fn decoded_configurations(
    target: &CadIr,
    native: &F3dNative,
) -> Result<Vec<cadmpeg_ir::features::parameters::DesignConfiguration>, CodecError> {
    use crate::design::decode::record::is_fatal;
    let projected = crate::design::configurations::project_configurations(
        &native.design_configurations,
        &crate::design::configurations::ConfigurationTargets {
            parameters: &native.design_parameters,
            owners: &native.design_parameter_owners,
            scopes: &native.design_parameter_scopes,
            neutral_parameters: &target.model.parameters,
            neutral_features: &target.model.features,
        },
    );
    let configurations = match projected {
        Ok(configurations) => configurations,
        Err(error) if is_fatal(&error) => return Err(error),
        Err(_) => return Ok(Vec::new()),
    };
    if !configurations
        .iter()
        .any(|configuration| configuration.active)
    {
        return Ok(configurations);
    }
    let mut reconciled = configurations.clone();
    match crate::design::configurations::reconcile_active_configurations(
        &mut target.model.features.clone(),
        &mut reconciled,
    ) {
        Ok(()) => Ok(reconciled),
        Err(error) if is_fatal(&error) => Err(error),
        Err(_) => Ok(configurations),
    }
}

/// Refuse a patch of `target` whose configurations are other than the ones
/// the decoder states for `native`.
pub(super) fn validate_configuration_projection(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    if target.model.configurations != decoded_configurations(target, native)? {
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
