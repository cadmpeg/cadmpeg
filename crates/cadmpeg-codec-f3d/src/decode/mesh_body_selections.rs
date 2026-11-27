// SPDX-License-Identifier: Apache-2.0
//! Whole-body selections of `Modify`, `Move`, and `RemoveBody` scopes whose
//! construction groups list mesh operation members.

use super::MeshProjection;
use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::MeshOperationRecords;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::selections::{BodySelection, NativeSelections, SelectionReference};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

/// The mesh bodies that one group selects: their tessellations in member
/// order and the scopes that own them.
struct MeshGroupSelection {
    tessellations: Vec<String>,
    scopes: Vec<u32>,
}

/// Bind each native whole-body selection whose construction group lists only
/// mesh operation members to the tessellations of the mesh bodies those
/// members name. The mesh feature that owns each selected body becomes a
/// dependency of the selecting feature.
pub(super) fn bind(
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    groups: &[DesignConstructionOperandGroup],
    scopes: &[DesignParameterScope],
    projection: &MeshProjection,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if projection.tessellations_by_body.is_empty() {
        return Ok(());
    }
    let producers = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let mut records = HashMap::<String, Option<MeshOperationRecords<'_>>>::new();
    let mut moved = std::collections::BTreeMap::<String, Vec<String>>::new();
    for feature in features.iter_mut() {
        let (bodies, moves) = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) => {
                (bodies, feature.suppressed != Some(true))
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) => {
                (bodies, false)
            }
            _ => continue,
        };
        let BodySelection::Native(native) = bodies else {
            continue;
        };
        let Some(group) = groups.iter().find(|group| &group.id == native) else {
            continue;
        };
        let Some(stream) = crate::ids::native_stream(&group.id) else {
            continue;
        };
        let indexed = match records.entry(stream.to_owned()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let indexed = stream_records(scan, entry.key())?;
                entry.insert(indexed)
            }
        };
        let Some(selected) = indexed
            .as_ref()
            .and_then(|records| group_selection(records, group, stream, projection))
        else {
            continue;
        };
        let Ok(members) = NativeSelections::try_from(selected.tessellations) else {
            continue;
        };
        let reference =
            SelectionReference::try_from(native.clone()).map_err(CodecError::malformed)?;
        let mut dependencies = Vec::new();
        for scope_record in selected.scopes {
            let producer = scopes
                .iter()
                .find(|scope| {
                    crate::ids::native_stream(&scope.id) == Some(stream)
                        && scope.record_index == scope_record
                })
                .and_then(|scope| producers.get(&scope.id));
            if let Some(producer) = producer {
                dependencies.push(producer.clone());
            }
        }
        losses.push(F3dLossCode::FeatureMeshBodySelectionAssumed.note(format!(
            "Feature {} reads the mesh operation members of whole-body group {native} as whole mesh-body selections; member-tail selection controls remain unresolved.",
            feature.id
        )));
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(
                FeatureOperation::MoveBody { bodies, .. }
                | FeatureOperation::DeleteBody { bodies, .. },
            ) = definition
            {
                *bodies = BodySelection::Local {
                    bodies: members,
                    native: reference,
                };
            }
        });
        for dependency in dependencies {
            if moves {
                moved
                    .entry(dependency.as_str().to_owned())
                    .or_default()
                    .push(feature.id.as_str().to_owned());
            }
            add_dependency(feature, dependency);
        }
    }
    for (import, motions) in moved {
        let motions = motions.join(", ");
        losses.push(F3dLossCode::FeatureMeshImportPlacementCombined.note(format!(
            "Mesh import {import} states its mesh at the current placement, which includes the motions of {motions}; its import placement is that placement premultiplied by the inverse motions, the last first."
        )));
    }
    Ok(())
}

fn add_dependency(feature: &mut Feature, dependency: FeatureId) {
    if dependency != feature.id {
        feature.dependencies.insert(dependency);
    }
}

/// The record index of the Design stream `stream`, or `None` when the stream
/// or its metadata is unavailable.
fn stream_records<'a>(
    scan: &ContainerScan<'a>,
    stream: &str,
) -> Result<Option<MeshOperationRecords<'a>>, CodecError> {
    let Some(entry) = stream
        .strip_prefix("f3d:")
        .and_then(crate::ids::decode_identity_key_component)
    else {
        return Ok(None);
    };
    let Some(bytes) = scan.entry_view(&entry) else {
        return Ok(None);
    };
    let meta = match crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry) {
        Ok(meta) => meta,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => None,
    };
    Ok(meta.map(|meta| MeshOperationRecords::index(bytes, meta)))
}

/// The mesh bodies of `group`, when every member is a mesh operation member
/// whose mesh body joins a tessellation.
fn group_selection(
    records: &MeshOperationRecords<'_>,
    group: &DesignConstructionOperandGroup,
    stream: &str,
    projection: &MeshProjection,
) -> Option<MeshGroupSelection> {
    let members = group.members();
    if members.is_empty() {
        return None;
    }
    let mut selected = MeshGroupSelection {
        tessellations: Vec::with_capacity(members.len()),
        scopes: Vec::new(),
    };
    for member in members {
        let body = records.member_body(member.value).ok()?;
        let joined = projection
            .tessellations_by_body
            .get(&(stream.to_owned(), body))?
            .as_ref()?;
        selected.tessellations.push(joined.tessellation.clone());
        if let Some(scope) = joined.scope_record_index {
            if !selected.scopes.contains(&scope) {
                selected.scopes.push(scope);
            }
        }
    }
    Some(selected)
}
