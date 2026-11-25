// SPDX-License-Identifier: Apache-2.0
//! The states of the mesh bodies that a `MeshRemove` removes.
//!
//! The collection of a removed mesh body no longer lists it, and its stored
//! mesh holds the state at the input of the `MeshRemove` that removes it
//! (`f3d.md` §3.1, the mesh-collection class). That mesh is geometry of its definition in that state
//! only (§8 of the CADIR v11 specification).

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::refs::{Evaluated, Member, Position, StateRef};
use cadmpeg_ir::document::Design;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::Meshed;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

use crate::loss::F3dLossCode;
use crate::records::mesh::DesignMeshFeature;

/// The native arena that holds the mesh features.
const MESH_FEATURES: &str = "design_mesh_features";

/// State each removed mesh body at the input of the `MeshRemove` that
/// removes it. A removed body without one such state keeps its geometry as
/// an unresolved mesh.
pub(super) fn bind(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let features = namespace.arena_as_for_decode::<DesignMeshFeature>(ctx, MESH_FEATURES)?;
    let mut states = BTreeMap::new();
    for tessellation in features
        .iter()
        .flat_map(DesignMeshFeature::removed_bodies)
        .filter_map(|body| body.tessellation_id.clone())
    {
        let removers = ir
            .model
            .features
            .iter()
            .filter(|feature| removes(feature, &tessellation))
            .map(|feature| &feature.id)
            .collect::<Vec<_>>();
        let state = match removers.as_slice() {
            [remover] => input_state(&ir.design, remover).ok_or_else(|| {
                format!("its MeshRemove {remover} has no built position in an ordered timeline")
            }),
            [] => Err("no MeshRemove removes it".to_owned()),
            _ => Err("several MeshRemove features remove it".to_owned()),
        };
        states.insert(tessellation, state);
    }
    for mesh in &mut ir.model.tessellations {
        match states.remove(mesh.id.as_str()) {
            Some(Ok(state)) => mesh.state = Some(state),
            Some(Err(reason)) => {
                mesh.of = Meshed::Unresolved;
                mesh.state = None;
                losses.push(F3dLossCode::FeatureMeshRemovedStateUnresolved.note(format!(
                    "Removed mesh body {} keeps its stored mesh as an unresolved mesh: {reason}.",
                    mesh.id.as_str()
                )));
            }
            None => {}
        }
    }
    Ok(())
}

/// Whether `feature` is a `MeshRemove` that selects `tessellation`.
fn removes(feature: &cadmpeg_ir::features::Feature, tessellation: &str) -> bool {
    feature.source_tag.as_deref() == Some("MeshRemove")
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Local { bodies, .. },
                ..
            }) if bodies.iter().any(|body| body == tessellation)
        )
}

/// The state at the input of `feature`, a built member of an ordered
/// container.
fn input_state(design: &Design, feature: &FeatureId) -> Option<StateRef> {
    let member = Member::Feature(feature.clone());
    design.containers.iter().find_map(|container| {
        let Container::Ordered {
            id,
            members,
            rollback,
            ..
        } = container
        else {
            return None;
        };
        let at = members.iter().position(|candidate| *candidate == member)?;
        if let Some(Rollback::Before(marker)) = rollback {
            let built = members.iter().position(|candidate| candidate == marker)?;
            if at >= built {
                return None;
            }
        }
        let position = match at.checked_sub(1) {
            Some(previous) => Position::After(members.get(previous)?.clone()),
            None => Position::Start(id.clone()),
        };
        Some(StateRef {
            configuration: None,
            of: Evaluated::State(position),
        })
    })
}
