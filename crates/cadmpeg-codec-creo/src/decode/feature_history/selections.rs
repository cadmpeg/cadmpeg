// SPDX-License-Identifier: Apache-2.0
//! Feature edge selection and generated result-edge identity.

use super::{
    agreed_feature_affected_ids, agreed_feature_replay_edge_ids,
    agreed_feature_replay_geometry_ids, has_feature_affected_ids, model_feature_ids,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{EdgeSelection, FeatureId as IrFeatureId, GeneratedEdgeRef};
use cadmpeg_ir::ids::EdgeId;
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn feature_edge_selection(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<EdgeSelection> {
    let (ids, native) = if let Some(ids) = agreed_feature_affected_ids(
        &scan.features.affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Edges,
    ) {
        if ids.is_empty() {
            let native = format!("creo:allfeatur:edgs_affected#{feature_id}:");
            return Some(EdgeSelection::Resolved {
                edges: Vec::new(),
                native,
            });
        }
        let native = format!(
            "creo:allfeatur:edgs_affected#{feature_id}:{}",
            ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        );
        (ids, native)
    } else {
        if has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Edges,
        ) {
            return None;
        }
        if let Some(ids) =
            agreed_feature_replay_edge_ids(&scan.features.replay_affected_ids, feature_id)
        {
            if ids.is_empty() {
                let native = format!("creo:allfeatur:replay_edgs_affected#{feature_id}:");
                return Some(EdgeSelection::Resolved {
                    edges: Vec::new(),
                    native,
                });
            }
            let native = format!(
                "creo:allfeatur:replay_edgs_affected#{feature_id}:{}",
                ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
            );
            (ids, native)
        } else {
            let round = scan
                .features
                .legacy_rounds
                .iter()
                .find(|round| round.feature_id == feature_id)?;
            let ids = round.edge_ids.as_deref()?;
            let native = format!(
                "creo:legacy_ascii:feature_edges#{feature_id}:{}",
                ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
            );
            (ids, native)
        }
    };
    let result_edge_ids = feature_result_edge_ids_by_feature(&scan.curves.topology_rows);
    let edges = ids
        .iter()
        .map(|id| EdgeId(format!("creo:visibgeom:edge#{id}")))
        .collect::<Vec<_>>();
    let unique = edges.iter().collect::<BTreeSet<_>>().len() == edges.len();
    if unique
        && edges
            .iter()
            .all(|edge| ir.model.edges.iter().any(|candidate| candidate.id == *edge))
    {
        Some(EdgeSelection::Resolved { edges, native })
    } else if edges
        .iter()
        .any(|edge| ir.model.edges.iter().any(|candidate| candidate.id == *edge))
    {
        // A typed generated selection names one result namespace. A roster
        // that mixes current B-rep edges with absent edges has no neutral
        // mixed identity, so retain the exact native selection.
        Some(EdgeSelection::Native(native))
    } else if let Some(edges) = generated_curve_edge_refs(
        ids,
        &scan.curves.topology_rows,
        &model_feature_ids(scan),
        &result_edge_ids,
    ) {
        Some(EdgeSelection::Generated { edges, native })
    } else {
        Some(EdgeSelection::Native(native))
    }
}

pub(in super::super) fn generated_curve_edge_refs(
    curve_ids: &[u32],
    rows: &[crate::curve::CurveTopologyRow],
    available_features: &BTreeSet<IrFeatureId>,
    result_edge_ids: &BTreeMap<u32, Vec<u32>>,
) -> Option<Vec<GeneratedEdgeRef>> {
    let unique_curve_ids = curve_ids.iter().copied().collect::<BTreeSet<_>>();
    (unique_curve_ids.len() == curve_ids.len()).then_some(())?;
    let unique_rows = crate::topology::uniquely_identified_rows(rows)
        .into_iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    curve_ids
        .iter()
        .map(|curve_id| {
            let row = unique_rows.get(curve_id)?;
            let feature = IrFeatureId(format!("creo:model:feature#{}", row.feature_id));
            (available_features.contains(&feature)
                && result_edge_ids
                    .get(&row.feature_id)
                    .is_some_and(|ids| ids.contains(curve_id)))
            .then_some(GeneratedEdgeRef {
                feature,
                local_id: format!("curve#{curve_id}"),
            })
        })
        .collect()
}

/// Return the complete feature-local edge roster proven by unique topology rows.
///
/// A decoded `crv_array` topology row is one materialized edge identity. The
/// global curve namespace must contain that identifier exactly once before the
/// row can be exposed in a feature result state.
pub(in super::super) fn feature_result_edge_ids(
    rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Option<Vec<u32>> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        *counts.entry(row.id).or_default() += 1;
    }
    let feature_rows = rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!feature_rows.is_empty()).then_some(())?;
    feature_rows
        .iter()
        .all(|row| counts.get(&row.id) == Some(&1))
        .then_some(())?;
    Some(feature_rows.into_iter().map(|row| row.id).collect())
}

pub(in super::super) fn feature_result_edge_ids_by_feature(
    rows: &[crate::curve::CurveTopologyRow],
) -> BTreeMap<u32, Vec<u32>> {
    rows.iter()
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|feature_id| {
            feature_result_edge_ids(rows, feature_id).map(|edge_ids| (feature_id, edge_ids))
        })
        .collect()
}

pub(in super::super) fn agreed_feature_geometry_ids<'a>(
    affected_ids: &'a [crate::feature::FeatureAffectedIds],
    replay_affected_ids: &'a [crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&'a [u32]> {
    let named = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    if named.is_some() {
        return named;
    }
    if has_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    ) {
        return None;
    }
    agreed_feature_replay_geometry_ids(replay_affected_ids, feature_id)
}
