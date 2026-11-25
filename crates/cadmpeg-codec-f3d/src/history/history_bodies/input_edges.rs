// SPDX-License-Identifier: Apache-2.0
//! Scoped native-slot joins for consumed fillet and chamfer input bodies.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::{Inputs, Want};
use crate::loss::F3dLossCode;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::evaluation::{
    FeatureInputTopology, HistoricalEdgePath, HistoricalEdgeTopologyRef, HistoricalFaceTopologyRef,
};
use cadmpeg_ir::features::selections::NonEmptyMembers;
use cadmpeg_ir::features::{Feature, FeatureOperation};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::history_body::BodyTopology;
use cadmpeg_ir::ids::{HistoricalEdgeId, HistoryBodyId};

pub(super) fn is_edge_treatment(feature: &Feature) -> bool {
    matches!(
        feature.evaluation.definition().operation(),
        FeatureOperation::Fillet { .. } | FeatureOperation::Chamfer { .. }
    )
}

pub(super) fn input_edge_bodies<'a>(
    feature: &'a Feature,
    inputs: Inputs<'a>,
    topologies: &'a [FeatureInputTopology],
) -> Vec<(usize, Want<'a>)> {
    if feature.suppressed == Some(true) || !is_edge_treatment(feature) {
        return Vec::new();
    }
    let mut matching = topologies
        .iter()
        .filter(|input| input.input_of == feature.id);
    let (Some(input), None) = (matching.next(), matching.next()) else {
        return Vec::new();
    };
    if input.edge_geometry.is_empty() || input.bodies.is_empty() {
        return Vec::new();
    }
    let Some(selected) = super::super::face_selections::face_selection_state(
        feature,
        inputs.scopes,
        inputs.bindings,
        inputs.histories,
    ) else {
        return Vec::new();
    };
    let history = &inputs.histories[selected.history];
    let state = &history.states()[selected.previous];
    if input.native_ref.as_ref() != Some(&state.id) {
        return Vec::new();
    }
    let Some(topology) = history.topology(state) else {
        return Vec::new();
    };
    let prefix = crate::ids::feature_input_prefix(&feature.id, state.state_id);
    let edge_slots = input
        .edge_geometry
        .iter()
        .filter_map(|geometry| {
            crate::ids::feature_input_edge_slot(&feature.id, state.state_id, &geometry.edge)
        })
        .collect::<BTreeSet<_>>();
    if edge_slots.is_empty() {
        return Vec::new();
    }
    let mut slots = BTreeSet::new();
    let complete = super::super::body_closures::walk_body_closures(topology, |body, entity| {
        if edge_slots.contains(&entity) {
            slots.insert(body);
        }
    });
    if complete.is_none() {
        return Vec::new();
    }
    slots
        .into_iter()
        .filter(|slot| {
            input
                .bodies
                .contains(&crate::ids::history_input_body_id(&prefix, *slot))
        })
        .map(|slot| {
            (
                selected.history,
                Want {
                    origin: feature,
                    state: state.state_id,
                    slot,
                    consumed: true,
                    capture_current: false,
                    input: Some(input),
                    identity: None,
                    bindings: RefCell::default(),
                    producer: None,
                    consumer: Some(feature),
                },
            )
        })
        .collect()
}

pub(super) fn input_bindings(
    want: &Want<'_>,
    nest: &BodyTopology,
    nest_index: usize,
) -> Vec<(HistoricalEdgeId, HistoricalEdgeTopologyRef)> {
    let Some(input) = want.input else {
        return Vec::new();
    };
    let prefix = crate::ids::feature_input_prefix(&want.origin.id, want.state);
    let mut edges = BTreeMap::<i64, Option<&cadmpeg_ir::topology::Edge>>::new();
    for edge in &nest.edges {
        if let Some(slot) = super::super::body_closures::stable_ref(edge.id.as_str()) {
            edges
                .entry(slot)
                .and_modify(|candidate| *candidate = None)
                .or_insert(Some(edge));
        }
    }
    let mut bindings = Vec::new();
    for geometry in &input.edge_geometry {
        let Some(slot) =
            crate::ids::feature_input_edge_slot(&want.origin.id, want.state, &geometry.edge)
        else {
            continue;
        };
        let Some(Some(edge)) = edges.get(&slot) else {
            continue;
        };
        let adjacent = nest
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .filter_map(|coedge| nest.loops.iter().find(|ring| ring.id == coedge.owner_loop))
            .map(|ring| ring.face.clone())
            .collect::<BTreeSet<_>>();
        let mut faces = Vec::new();
        for face in &geometry.faces {
            let targets = nest
                .faces
                .iter()
                .filter(|target| {
                    adjacent.contains(&target.id)
                        && super::super::body_closures::stable_ref(target.id.as_str()).is_some_and(
                            |slot| crate::ids::history_input_face_id(&prefix, slot) == *face,
                        )
                })
                .map(|target| target.id.clone())
                .collect::<Vec<_>>();
            let Ok(topology_faces) = NonEmptyMembers::try_from(targets) else {
                continue;
            };
            faces.push(HistoricalFaceTopologyRef {
                face: face.clone(),
                topology_faces,
            });
        }
        let Ok(history_body) =
            HistoryBodyId::mint(format!("f3d:history-body:nest#h{nest_index:04}"))
        else {
            continue;
        };
        bindings.push((
            geometry.edge.clone(),
            HistoricalEdgeTopologyRef {
                history_body,
                path: HistoricalEdgePath::Edge {
                    edge: edge.id.clone(),
                },
                faces,
            },
        ));
    }
    bindings
}

pub(super) fn source_carriers_solved(
    source: &Topology,
    reference: &HistoricalEdgeTopologyRef,
) -> bool {
    let HistoricalEdgePath::Edge { edge } = &reference.path else {
        return false;
    };
    let Some(edge) = source.edges.iter().find(|candidate| candidate.id == *edge) else {
        return false;
    };
    let cadmpeg_ir::topology::EdgeCarrier::Bounded(curve, _) = &edge.carrier else {
        return false;
    };
    if !source.curves.iter().any(|candidate| {
        candidate.id == *curve
            && matches!(&candidate.geometry,
        CurveGeometry::Solved(geometry) if !matches!(geometry, SolvedCurveGeometry::Unknown { .. }))
    }) {
        return false;
    }
    reference
        .faces
        .iter()
        .flat_map(|binding| &binding.topology_faces)
        .all(|face| {
            source
                .faces
                .iter()
                .find(|candidate| candidate.id == *face)
                .and_then(|face| {
                    source
                        .surfaces
                        .iter()
                        .find(|surface| surface.id == face.surface)
                })
                .is_some_and(|surface| {
                    matches!(&surface.geometry, SurfaceGeometry::Solved(geometry)
                if !matches!(geometry, SolvedSurfaceGeometry::Unknown { .. }))
                })
        })
}

pub(super) fn binding_findings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    model: &cadmpeg_ir::document::Model,
    states: &[FeatureInputTopology],
    tolerances: cadmpeg_ir::units::Tolerances,
) -> Result<Vec<cadmpeg_ir::report::check::Finding>, cadmpeg_core::CodecError> {
    let mut findings = Vec::new();
    for state in states {
        let bodies = state
            .edge_geometry
            .iter()
            .filter_map(|geometry| geometry.topology.as_ref())
            .map(|reference| &reference.history_body)
            .collect::<BTreeSet<_>>();
        if bodies.is_empty() {
            continue;
        }
        let Some(origin) = model
            .features
            .iter()
            .find(|feature| feature.id == state.input_of)
        else {
            continue;
        };
        let mut scratch = cadmpeg_ir::document::CadIr::empty();
        scratch.tolerances = tolerances;
        scratch.model.features.push(super::origin_stub(origin));
        scratch.model.feature_input_topologies.push(state.clone());
        scratch.model.history_bodies.extend(
            model
                .history_bodies
                .iter()
                .filter(|body| bodies.contains(&body.id))
                .cloned(),
        );
        findings.extend(
            cadmpeg_ir::validate::validate_neutral_for_decode(ctx, &scratch, Vec::new())?.findings,
        );
    }
    Ok(findings)
}

pub(super) fn admit_bound_edges(
    features: &[Feature],
    states: &mut [FeatureInputTopology],
    findings: &[cadmpeg_ir::report::check::Finding],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    for state in states {
        let treatment = features
            .iter()
            .any(|feature| feature.id == state.input_of && is_edge_treatment(feature));
        if !treatment {
            continue;
        }
        for geometry in &mut state.edge_geometry {
            let failure = findings.iter().find(|finding| {
                let state_error = finding.entity.as_deref() == Some(state.id.as_str())
                    && (finding
                        .related
                        .iter()
                        .any(|id| id == geometry.edge.as_str())
                        || finding.message.contains(geometry.edge.as_str())
                        || geometry
                            .faces
                            .iter()
                            .any(|face| finding.message.contains(face.as_str())));
                let nest_error = geometry.topology.as_ref().is_some_and(|reference| {
                    finding
                        .related
                        .iter()
                        .any(|id| id == reference.history_body.as_str())
                });
                finding.severity == cadmpeg_ir::report::Severity::Error
                    && cadmpeg_ir::DRAFT_CORE.admits(finding)
                    && (state_error || nest_error)
            });
            if failure.is_some() {
                geometry.topology = None;
            }
            if geometry.topology.is_none() {
                losses.push(F3dLossCode::FeatureHistoricalEdgeTopologyUnbound.note(format!(
                    "{} state {} edge {}: {}",
                    state.input_of, state.native_ref.as_deref().unwrap_or("unresolved"), geometry.edge,
                    failure.map_or("scoped previous-state target, solved carrier, finite domain and snapshot incidence proofs did not produce one binding", |finding| finding.message.as_str())
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests;
