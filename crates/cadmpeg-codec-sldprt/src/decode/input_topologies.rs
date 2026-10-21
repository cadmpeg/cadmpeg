// SPDX-License-Identifier: Apache-2.0
//! Historical membership explicitly declared by feature-input selections.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureId, FeatureInputTopology};
use cadmpeg_ir::ids::{
    FeatureInputTopologyId, HistoricalBodyId, HistoricalEdgeId, HistoricalFaceId,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::loss::SldprtLossCode;
use crate::records::{FeatureInputComponentPathEntry, FeatureInputLane};

pub(super) fn project(ir: &mut CadIr, lanes: &[FeatureInputLane], losses: &mut Vec<LossNote>) {
    let owners: BTreeMap<_, _> = ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect();
    let mut states = BTreeMap::<FeatureId, FeatureInputTopology>::new();
    for lane in lanes {
        for selection in &lane.body_selections {
            let Some(owner) = owners.get(selection.feature_ref.as_str()) else {
                unprojected(losses, &selection.id, "owning feature does not resolve");
                continue;
            };
            let state = states.entry(owner.clone()).or_insert_with(|| state(owner));
            for local in &selection.local_body_ids {
                state.bodies.insert(HistoricalBodyId::compose(
                    &cadmpeg_ir::identity_namespace!("sldprt", "history", "body"),
                    owner.key().colon(*local),
                ));
            }
        }
        for selection in &lane.edge_selections {
            let Some(owner) = owners.get(selection.feature_ref.as_str()) else {
                unprojected(losses, &selection.id, "owning feature does not resolve");
                continue;
            };
            let producer = selection
                .terminal_feature_ref
                .as_deref()
                .and_then(|reference| owners.get(reference))
                .unwrap_or(owner);
            let state = states.entry(owner.clone()).or_insert_with(|| state(owner));
            let mut represented = false;
            for local in &selection.local_edge_ids {
                state.edges.insert(edge(producer, *local));
                represented = true;
            }
            for path in std::iter::once(selection.components.as_slice())
                .chain(selection.references.iter().map(Vec::as_slice))
            {
                if let Some(local) = terminal_identity(path) {
                    state.edges.insert(edge(producer, local));
                    represented = true;
                } else if !path.is_empty() {
                    unprojected(
                        losses,
                        &selection.id,
                        "persistent edge path has no terminal identity",
                    );
                }
            }
            if !represented {
                unprojected(
                    losses,
                    &selection.id,
                    "selection has no resolved historical edge identity",
                );
            }
        }
        for selection in &lane.surface_selections {
            let Some(owner) = owners.get(selection.feature_ref.as_str()) else {
                unprojected(losses, &selection.id, "owning feature does not resolve");
                continue;
            };
            let Some(local) = terminal_identity(&selection.components) else {
                unprojected(
                    losses,
                    &selection.id,
                    "persistent surface path has no terminal face identity",
                );
                continue;
            };
            let producer = selection
                .terminal_feature_ref
                .as_deref()
                .and_then(|reference| owners.get(reference))
                .unwrap_or(owner);
            let state = states.entry(owner.clone()).or_insert_with(|| state(owner));
            state.faces.insert(HistoricalFaceId::compose(
                &cadmpeg_ir::identity_namespace!("sldprt", "history", "face"),
                producer.key().colon(local),
            ));
        }
    }
    ir.model
        .feature_input_topologies
        .extend(states.into_values());
}

fn state(owner: &FeatureId) -> FeatureInputTopology {
    FeatureInputTopology {
        id: FeatureInputTopologyId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "model", "input-topology"),
            owner.key(),
        ),
        input_of: owner.clone(),
        bodies: cadmpeg_ir::features::DistinctMembers::default(),
        faces: cadmpeg_ir::features::DistinctMembers::default(),
        edges: cadmpeg_ir::features::DistinctMembers::default(),
        vertices: cadmpeg_ir::features::DistinctMembers::default(),
        native_ref: None,
    }
}

fn edge(producer: &FeatureId, local: u32) -> HistoricalEdgeId {
    HistoricalEdgeId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "history", "edge"),
        producer.key().colon(local),
    )
}

fn terminal_identity(path: &[FeatureInputComponentPathEntry]) -> Option<u32> {
    // An earlier path component may name an ancestor body or feature. Only the
    // terminal component establishes the selected topological identity.
    path.last()?.local_id
}

fn unprojected(losses: &mut Vec<LossNote>, id: &str, reason: &str) {
    losses.push(SldprtLossCode::FeatureTypedOperandIncomplete.note(format!(
        "feature-input selection {id} has no neutral input-topology projection: {reason}; its typed native record is retained"
    )));
}
