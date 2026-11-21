use super::{scope_bytes, Notes};
use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::tessellate::OWNER_ROLES;
use crate::design::decode::mesh_operations::{MeshOperationFraming, MeshOperationRecords};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

pub(in crate::decode) struct TessellateInputs<'a> {
    pub(in crate::decode) scopes: &'a [DesignParameterScope],
    pub(in crate::decode) owners: &'a [DesignParameterOwner],
    pub(in crate::decode) parameters: &'a [DesignParameter],
}

fn parameter_name<'a>(
    inputs: &TessellateInputs<'a>,
    scope: &DesignParameterScope,
    owner: u32,
) -> Option<&'a str> {
    let stream = crate::ids::native_stream(&scope.id);
    let owner = inputs.owners.iter().find(|candidate| {
        crate::ids::native_stream(candidate.id()) == stream
            && candidate.record_index() == owner
            && candidate.scope_record_index() == scope.record_index
    })?;
    inputs
        .parameters
        .iter()
        .find(|candidate| {
            crate::ids::native_stream(&candidate.id) == stream
                && candidate.record_index == owner.parameter_record_index()
        })
        .map(DesignParameter::name)
}

pub(in crate::decode) fn bind_features(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    inputs: &TessellateInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut records = HashMap::<String, Option<MeshOperationRecords<'_>>>::new();
    for feature in features {
        if feature.source_tag.as_deref() != Some("MeshTessellate") {
            continue;
        }
        let mut candidates = inputs
            .scopes
            .iter()
            .filter(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()));
        let scope = candidates.next().filter(|_| candidates.next().is_none());
        let source = scope.map_or_else(|| feature.id.to_string(), |scope| scope.id.clone());
        let mut notes = Notes {
            source: &source,
            properties: Vec::new(),
            losses: Vec::new(),
        };
        let parsed = if let Some((scope, (entry, bytes))) =
            scope.and_then(|scope| Some((scope, scope_bytes(scan, scope)?)))
        {
            let indexed = match records.entry(entry) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let meta = match crate::design::decode::meta::metadata_for_bulk_stream(
                        scan,
                        entry.key(),
                    ) {
                        Ok(meta) => meta,
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => None,
                    };
                    entry.insert(meta.map(|meta| MeshOperationRecords::index(bytes, meta)))
                }
            };
            match indexed.as_ref() {
                Some(records) => Some((
                    scope,
                    records,
                    records.tessellate_scope(ctx, scope.record_index)?,
                )),
                None => None,
            }
        } else {
            None
        };
        match parsed {
            Some((scope, records, Ok(parsed))) => {
                for (role, owner) in OWNER_ROLES.iter().zip(parsed.owners) {
                    let Some(owner) = owner else {
                        continue;
                    };
                    notes.property(format!("mesh-tessellate-owner:{role}"), owner);
                    if let Some(name) = parameter_name(inputs, scope, owner) {
                        notes.property(format!("mesh-tessellate-parameter:{role}"), name);
                    }
                }
                if let Some(count) = parsed.operand_count {
                    notes.property("mesh-tessellate-operand-count", count);
                    notes.losses.push(F3dLossCode::FeatureMeshTessellateOperandsAssumed.note(format!(
                        "Mesh tessellation {source} reads the u32 {count} before its operand references as their count."
                    )));
                }
                let mut deviations = parsed.deviations;
                for (ordinal, operand) in parsed.operands.into_iter().enumerate() {
                    notes.property(format!("mesh-tessellate-operand:{ordinal}"), operand);
                    let reason = match records.tessellate_operand(operand) {
                        Ok(read) => {
                            notes.property(format!("mesh-tessellate-group:{ordinal}"), read.group);
                            notes.property(format!("mesh-tessellate-body:{ordinal}"), read.body);
                            deviations.extend(read.deviations);
                            format!(
                                "group record {} and produced mesh body {} stay in its source properties; no neutral operation states a tessellation of a body",
                                read.group, read.body
                            )
                        }
                        Err(error) => format!("its {error} is unresolved"),
                    };
                    notes.losses.push(F3dLossCode::FeatureMeshTessellateSourceUnresolved.note(format!(
                        "Mesh tessellation {source} retains operand {operand} in position {ordinal}: {reason}."
                    )));
                }
                for (field, value) in &deviations {
                    notes.property(format!("mesh-field:{field}"), value);
                }
                if !deviations.is_empty() {
                    notes.losses.push(F3dLossCode::FeatureMeshTessellateFieldsUninterpreted.note(format!(
                        "Mesh tessellation {source} retains uninterpreted field values {deviations:?} in its source properties."
                    )));
                }
                if let Some(error) = parsed.tail_error {
                    notes.losses.push(F3dLossCode::FeatureMeshTessellateScopeUnparsed.note(format!(
                        "Mesh tessellation {source} retains the owner references and operands before its unresolved {error}."
                    )));
                }
            }
            other => {
                let reason = match other {
                    Some((.., Err(error))) => error,
                    _ => MeshOperationFraming("tessellate scope or metadata"),
                };
                notes.losses.push(F3dLossCode::FeatureMeshTessellateScopeUnparsed.note(format!(
                    "Mesh tessellation {source} keeps its native parameters because its {reason} is unresolved."
                )));
            }
        }
        for (key, value) in notes.properties {
            let key = NonBlankString::new(key)
                .ok_or_else(|| CodecError::malformed("blank mesh tessellation property key"))?;
            feature.source_properties.insert(key, value);
        }
        losses.append(&mut notes.losses);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
