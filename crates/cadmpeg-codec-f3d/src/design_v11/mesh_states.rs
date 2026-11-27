// SPDX-License-Identifier: Apache-2.0
//! The states of the mesh bodies that a `MeshRemove` removes or a
//! `Convert Mesh` consumes.
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
use cadmpeg_ir::features::mesh::MeshSelection;
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
/// removes it or the `Convert Mesh` that consumes it. A removed body without
/// one such state keeps its geometry as an unresolved mesh.
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
            .filter(|feature| removes(feature, &tessellation) || converts(feature, &tessellation))
            .collect::<Vec<_>>();
        let state = match removers.as_slice() {
            [remover] => input_state(&ir.design, &remover.id).ok_or_else(|| {
                format!(
                    "its consumer {} has no built position in an ordered timeline",
                    remover.id
                )
            }),
            [] => Err("no MeshRemove or Convert Mesh consumes it".to_owned()),
            _ => Err("several MeshRemove or Convert Mesh features consume it".to_owned()),
        };
        states.insert(tessellation, state);
    }
    for mesh in &mut ir.model.tessellations {
        match states.remove(mesh.id.as_str()) {
            Some(Ok(state)) => {
                mesh.state = Some(state);
            }
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

/// Whether `feature` is a `Convert Mesh` with an input that selects
/// `tessellation`.
fn converts(feature: &cadmpeg_ir::features::Feature, tessellation: &str) -> bool {
    feature.source_tag.as_deref() == Some("Convert Mesh")
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::MeshConvert(conversion))
                if conversion.sources.iter().any(|source| matches!(
                    source,
                    MeshSelection::Tessellations(members)
                        if members.iter().any(|member| member.as_str() == tessellation)
                ))
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

#[cfg(test)]
mod tests {
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::mesh::{MeshConversion, MeshSelection};
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::tessellation::TessellationId;

    fn conversion(tag: &str, sources: Vec<MeshSelection>) -> Feature {
        Feature {
            product_definition: None,
            id: FeatureId::mint("f3d:test:feature#convert").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: Some(tag.into()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::MeshConvert(MeshConversion::new(sources, None)),
            )),
            native_ref: None,
        }
    }

    #[test]
    fn a_convert_mesh_consumes_the_tessellations_its_inputs_select() {
        let joined = MeshSelection::Tessellations(
            vec![TessellationId::mint("f3d:test:mesh-body#1").unwrap()]
                .try_into()
                .unwrap(),
        );
        let feature = conversion("Convert Mesh", vec![joined.clone()]);
        assert!(super::converts(&feature, "f3d:test:mesh-body#1"));
        assert!(!super::converts(&feature, "f3d:test:mesh-body#2"));
        assert!(!super::removes(&feature, "f3d:test:mesh-body#1"));
        assert!(!super::converts(
            &conversion("MeshReduce", vec![joined]),
            "f3d:test:mesh-body#1"
        ));
    }
}
