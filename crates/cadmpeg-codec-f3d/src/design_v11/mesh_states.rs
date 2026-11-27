// SPDX-License-Identifier: Apache-2.0
//! The states of the mesh bodies that a `MeshRemove` removes or a
//! `Convert Mesh` consumes.
//!
//! The collection of a removed mesh body no longer lists it, and its stored
//! mesh holds the state at the input of the `MeshRemove` that removes it
//! (`f3d.md` §3.1, the mesh-collection class). That mesh is geometry of its definition in that state
//! only (§8 of the CADIR v11 specification). A removed body without one such
//! state, and every removed body of a decode that leaves out the design
//! history, stays in its native record and has no tessellation (§8, part 11).

use cadmpeg_core::decode::{DecodeContext, DecodeDepth};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::refs::{Evaluated, Member, Position, StateRef};
use cadmpeg_ir::document::Design;
use cadmpeg_ir::features::mesh::MeshSelection;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::TessellationId;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

use crate::loss::F3dLossCode;
use crate::records::mesh::DesignMeshFeature;

/// The native arena that holds the mesh features.
const MESH_FEATURES: &str = "design_mesh_features";

/// State each removed mesh body at the input of the `MeshRemove` that
/// removes it or the `Convert Mesh` that consumes it. A removed body without
/// one such state, and every removed body below [`DecodeDepth::Design`],
/// leaves the neutral model and stays in its native record.
pub(super) fn bind(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    depth: DecodeDepth,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let mut features = namespace.arena_as_for_decode::<DesignMeshFeature>(ctx, MESH_FEATURES)?;
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
    if depth < DecodeDepth::Design {
        for state in states.values_mut() {
            *state = Err(UNREAD_HISTORY.to_owned());
        }
    }
    let unstated = states
        .iter()
        .filter_map(|(tessellation, state)| Some((tessellation.clone(), state.clone().err()?)))
        .collect::<BTreeMap<_, _>>();
    if !unstated.is_empty() {
        for body in features
            .iter_mut()
            .flat_map(DesignMeshFeature::removed_bodies_mut)
        {
            if body
                .tessellation_id
                .as_deref()
                .is_some_and(|id| unstated.contains_key(id))
            {
                body.tessellation_id = None;
            }
        }
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
            .set_arena(ctx, MESH_FEATURES, &features)?;
    }
    for mesh in &mut ir.model.tessellations {
        if let Some(Ok(state)) = states.remove(mesh.id.as_str()) {
            mesh.state = Some(state);
        }
    }
    drop_unstated(ir, &unstated, losses);
    Ok(())
}

/// The reason that no removed mesh body has a state in a decode that leaves
/// out the design history.
const UNREAD_HISTORY: &str =
    "the decode depth leaves out the design history, so the state of the body is not established";

/// Remove each tessellation of `unstated` from the model and from every
/// feature selection that names it, and charge one loss per tessellation
/// with its reason.
fn drop_unstated(ir: &mut CadIr, unstated: &BTreeMap<String, String>, losses: &mut Vec<LossNote>) {
    if unstated.is_empty() {
        return;
    }
    ir.model
        .tessellations
        .retain(|mesh| !unstated.contains_key(mesh.id.as_str()));
    for (tessellation, reason) in unstated {
        let mut selectors = Vec::new();
        let mut subjects = Vec::new();
        for feature in &mut ir.model.features {
            if deselect(feature, tessellation) {
                selectors.push(feature.id.as_str().to_owned());
                subjects.push(format!("{} ", feature.id));
                subjects.extend(feature.native_ref.as_ref().map(|scope| format!("{scope} ")));
            }
        }
        losses.retain(|loss| {
            loss.code != F3dLossCode::FeatureMeshBodySelectionAssumed.kind()
                || !subjects
                    .iter()
                    .any(|subject| loss.message.contains(subject.as_str()))
        });
        let selectors = if selectors.is_empty() {
            String::new()
        } else {
            format!(
                " The selections of {} no longer name it.",
                selectors.join(", ")
            )
        };
        losses.push(F3dLossCode::FeatureMeshRemovedStateUnresolved.note(format!(
            "Removed mesh body {tessellation} stays in its native record and has no tessellation: {reason}.{selectors}"
        )));
    }
}

/// Remove `tessellation` from the selections of `feature`, and return whether
/// a selection named it. A mesh import keeps its other tessellations. A whole-body
/// selection becomes its native selection, and any other mesh source becomes
/// unresolved at `feature`.
fn deselect(feature: &mut cadmpeg_ir::features::Feature, tessellation: &str) -> bool {
    let id = feature.id.clone();
    let mut named = false;
    feature.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(operation) = definition else {
            return;
        };
        match operation {
            FeatureOperation::MeshImport { tessellations } => {
                named = shorten(tessellations, tessellation, &id);
            }
            FeatureOperation::MeshGenerateFaceGroups { source, .. }
            | FeatureOperation::MeshCombineFaceGroups { source }
            | FeatureOperation::MeshSection { source, .. }
            | FeatureOperation::MeshModify { source, .. } => {
                named = release(source, tessellation, &id);
            }
            FeatureOperation::MeshConvert(conversion) => {
                for source in &mut conversion.sources {
                    named |= release(source, tessellation, &id);
                }
            }
            FeatureOperation::MoveBody { bodies, .. }
            | FeatureOperation::DeleteBody { bodies, .. } => {
                if let BodySelection::Local {
                    bodies: members,
                    native,
                } = bodies
                {
                    if members.iter().any(|member| member == tessellation) {
                        *bodies = BodySelection::Native(native.as_str().to_owned());
                        named = true;
                    }
                }
            }
            _ => {}
        }
    });
    named
}

/// Remove `tessellation` from `selection`; a selection left empty is
/// unresolved at `feature`.
fn shorten(selection: &mut MeshSelection, tessellation: &str, feature: &FeatureId) -> bool {
    let MeshSelection::Tessellations(members) = selection else {
        return false;
    };
    if !members.iter().any(|member| member.as_str() == tessellation) {
        return false;
    }
    let kept = members
        .iter()
        .filter(|member| member.as_str() != tessellation)
        .cloned()
        .collect::<Vec<TessellationId>>();
    *selection = match kept.try_into() {
        Ok(kept) => MeshSelection::Tessellations(kept),
        Err(_) => MeshSelection::Unresolved {
            feature: feature.clone(),
        },
    };
    true
}

/// Make `selection` unresolved at `feature` when it names `tessellation`.
fn release(selection: &mut MeshSelection, tessellation: &str, feature: &FeatureId) -> bool {
    let named = matches!(
        selection,
        MeshSelection::Tessellations(members)
            if members.iter().any(|member| member.as_str() == tessellation)
    );
    if named {
        *selection = MeshSelection::Unresolved {
            feature: feature.clone(),
        };
    }
    named
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
    use cadmpeg_ir::tessellation::TessellationId;
    use std::io::Cursor;

    use cadmpeg_core::decode::DecodeDepth;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::report::loss::LossNote;
    use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

    use crate::loss::F3dLossCode;

    /// The decoded design of a mesh import, a move of its mesh body, and a
    /// `MeshRemove` of that body, with its losses.
    fn moved_and_removed() -> (CadIr, Vec<LossNote>) {
        let result = crate::F3dCodec
            .decode(
                &mut Cursor::new(
                    &include_bytes!("../../../../corpus/f3d/mesh_import_move_remove.f3d")[..],
                ),
                &DecodeOptions::default(),
            )
            .expect("decode the corpus file");
        let losses = result.report().losses.clone();
        (result.into_parts().0, losses)
    }

    fn feature<'a>(ir: &'a CadIr, tag: &str) -> &'a Feature {
        ir.model
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(tag))
            .unwrap_or_else(|| panic!("feature {tag}"))
    }

    fn rebind(ir: &mut CadIr, losses: &mut Vec<LossNote>) {
        crate::test_support::with_decode_context(|ctx| {
            super::bind(ctx, ir, DecodeDepth::Design, losses)
        })
        .expect("bind the removed mesh states");
    }

    fn unstated(losses: &[LossNote]) -> Vec<&str> {
        losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::FeatureMeshRemovedStateUnresolved.kind())
            .map(|loss| loss.message.as_str())
            .collect()
    }

    fn whole_body_selection(feature: &Feature) -> &BodySelection {
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(
                FeatureOperation::MoveBody { bodies, .. }
                | FeatureOperation::DeleteBody { bodies, .. },
            ) => bodies,
            other => panic!("a whole-body operation: {other:?}"),
        }
    }

    #[test]
    fn a_removed_body_that_no_mesh_remove_removes_leaves_the_model_and_every_selection() {
        let (mut ir, mut losses) = moved_and_removed();
        let remove = feature(&ir, "MeshRemove").id.clone();
        let tessellation = ir.model.tessellations[0].id.as_str().to_owned();
        for feature in &mut ir.model.features {
            if feature.id == remove {
                feature.evaluation.edit(|definition, _| {
                    if let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                        bodies,
                        ..
                    }) = definition
                    {
                        *bodies = BodySelection::Unresolved;
                    }
                });
            }
        }
        rebind(&mut ir, &mut losses);

        assert!(ir.model.tessellations.is_empty());
        let import = feature(&ir, "Base Mesh Feature");
        assert!(matches!(
            import.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::MeshImport {
                tessellations: MeshSelection::Unresolved { feature },
            }) if *feature == import.id
        ));
        let modify = feature(&ir, "Modify");
        assert!(matches!(
            whole_body_selection(modify),
            BodySelection::Native(_)
        ));
        let native = crate::test_support::native_test::f3d_native(&ir);
        let removed = native
            .design_mesh_features
            .iter()
            .flat_map(crate::records::mesh::DesignMeshFeature::removed_bodies)
            .collect::<Vec<_>>();
        assert_eq!(removed.len(), 1, "the native record keeps the removed body");
        assert_eq!(removed[0].tessellation_id, None);

        let [message] = unstated(&losses)[..] else {
            panic!("one removed-state loss: {losses:#?}");
        };
        assert!(message.contains(&tessellation));
        assert!(message.contains("no MeshRemove removes it"));
        assert!(message.contains(import.id.as_str()) && message.contains(modify.id.as_str()));
        assert!(!losses.iter().any(|loss| {
            loss.code == F3dLossCode::FeatureMeshBodySelectionAssumed.kind()
                && loss.message.contains(&format!("{} ", modify.id))
        }));

        let validation = cadmpeg_ir::validate_neutral(&ir, losses).unwrap();
        assert!(validation.is_ok(), "{validation:#?}");
        let findings = crate::test_support::with_decode_context(|ctx| {
            crate::validate::validate_native(ctx, &ir).unwrap()
        });
        assert!(
            !findings.iter().any(|finding| finding.message
                == "Fusion Design mesh feature has an invalid frame or object graph"),
            "{findings:#?}"
        );
    }

    #[test]
    fn a_removed_body_that_two_mesh_removes_remove_leaves_both_selections() {
        let (mut ir, mut losses) = moved_and_removed();
        let mut copy = feature(&ir, "MeshRemove").clone();
        copy.id = FeatureId::mint("f3d:test:feature#remove-copy").unwrap();
        ir.model.features.push(copy);
        rebind(&mut ir, &mut losses);

        assert!(ir.model.tessellations.is_empty());
        let [message] = unstated(&losses)[..] else {
            panic!("one removed-state loss: {losses:#?}");
        };
        assert!(message.contains("several MeshRemove features remove it"));
        for feature in ir
            .model
            .features
            .iter()
            .filter(|feature| feature.source_tag.as_deref() == Some("MeshRemove"))
        {
            assert!(matches!(
                whole_body_selection(feature),
                BodySelection::Native(_)
            ));
        }
    }

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
