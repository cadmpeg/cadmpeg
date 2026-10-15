// SPDX-License-Identifier: Apache-2.0
//! Feature edge selection and generated result-edge identity.

use super::axes::model_feature_ids;
use super::dependencies::{
    agreed_feature_replay_edge_ids, agreed_feature_replay_geometry_ids, has_feature_affected_ids,
};
use super::outputs::CommaList;
use crate::container::ContainerScan;
use crate::feature::rows::agreed_feature_affected_ids;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{EdgeSelection, FeatureId as IrFeatureId, GeneratedEdgeRef};
use cadmpeg_ir::ids::EdgeId;
use std::collections::{BTreeMap, BTreeSet};

fn edge_selection_native(namespace: &'static str, feature_id: u32, ids: &[u32]) -> String {
    format!("{namespace}#{feature_id}:{}", CommaList(ids))
}

pub(in super::super) fn feature_edge_selection(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Option<EdgeSelection>, CodecError> {
    let (ids, native) = if let Some(ids) = agreed_feature_affected_ids(
        &scan.features.affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Edges,
    ) {
        if ids.is_empty() {
            let native = edge_selection_native("creo:allfeatur:edgs_affected", feature_id, ids);
            return Ok(Some(EdgeSelection::Resolved {
                edges: Vec::new(),
                native,
            }));
        }
        let native = edge_selection_native("creo:allfeatur:edgs_affected", feature_id, ids);
        (ids, native)
    } else {
        if has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::rows::AffectedIdKind::Edges,
        ) {
            return Ok(None);
        }
        if let Some(ids) =
            agreed_feature_replay_edge_ids(&scan.features.replay_affected_ids, feature_id)
        {
            if ids.is_empty() {
                let native =
                    edge_selection_native("creo:allfeatur:replay_edgs_affected", feature_id, ids);
                return Ok(Some(EdgeSelection::Resolved {
                    edges: Vec::new(),
                    native,
                }));
            }
            let native =
                edge_selection_native("creo:allfeatur:replay_edgs_affected", feature_id, ids);
            (ids, native)
        } else {
            let Some(round) = scan
                .features
                .legacy_rounds
                .iter()
                .find(|round| round.feature_id == feature_id)
            else {
                return Ok(None);
            };
            let Some(ids) = round.edge_ids.as_deref() else {
                return Ok(None);
            };
            let native = edge_selection_native("creo:legacy_ascii:feature_edges", feature_id, ids);
            (ids, native)
        }
    };
    let mut edges = Vec::new();
    let mut seen = BTreeSet::new();
    let mut unique = true;
    for id in ids {
        let text = format!("creo:visibgeom:edge#{id}");
        let edge = EdgeId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo edge ID is invalid".into()))?;
        edges.push(edge);
        if seen.contains(id) {
            unique = false;
        } else {
            seen.insert(*id);
        }
    }
    if unique
        && edges
            .iter()
            .all(|edge| ir.model.edges.iter().any(|candidate| candidate.id == *edge))
    {
        Ok(Some(EdgeSelection::Resolved { edges, native }))
    } else if edges
        .iter()
        .any(|edge| ir.model.edges.iter().any(|candidate| candidate.id == *edge))
    {
        // A typed generated selection names one result namespace. A roster
        // that mixes current B-rep edges with absent edges has no neutral
        // mixed identity, so retain the exact native selection.
        Ok(Some(EdgeSelection::Native(native)))
    } else if let Some(edges) = generated_curve_edge_refs(
        ids,
        &scan.curves.topology_rows,
        &model_feature_ids(scan)?,
        &feature_result_edge_ids_by_feature(&scan.curves.topology_rows),
    )? {
        Ok(Some(
            EdgeSelection::generated(edges, native.clone())
                .unwrap_or(EdgeSelection::Native(native)),
        ))
    } else {
        Ok(Some(EdgeSelection::Native(native)))
    }
}

pub(in super::super) fn generated_curve_edge_refs(
    curve_ids: &[u32],
    rows: &[crate::curve::CurveTopologyRow],
    available_features: &BTreeSet<IrFeatureId>,
    result_edge_ids: &BTreeMap<u32, Vec<u32>>,
) -> Result<Option<Vec<GeneratedEdgeRef>>, CodecError> {
    let mut unique_curve_ids = BTreeSet::new();
    for &curve_id in curve_ids {
        if unique_curve_ids.contains(&curve_id) {
            return Ok(None);
        }
        unique_curve_ids.insert(curve_id);
    }
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows.iter().filter(|row| unique_curve_ids.contains(&row.id)) {
        *counts.entry(row.id).or_default() += 1;
    }
    let mut unique_rows = BTreeMap::new();
    for row in rows {
        if counts.get(&row.id) == Some(&1) {
            unique_rows.insert(row.id, row);
        }
    }
    let mut generated = Vec::new();
    for &curve_id in curve_ids {
        let Some(row) = unique_rows.get(&curve_id) else {
            return Ok(None);
        };
        let feature_text = format!("creo:model:feature#{}", row.feature_id);
        let feature = IrFeatureId::mint(feature_text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        if !available_features.contains(&feature)
            || !result_edge_ids
                .get(&row.feature_id)
                .is_some_and(|ids| ids.contains(&curve_id))
        {
            return Ok(None);
        }
        let local_id = format!("curve#{curve_id}");
        let Some(edge) = GeneratedEdgeRef::new(feature, local_id).ok() else {
            return Ok(None);
        };
        generated.push(edge);
    }
    Ok(Some(generated))
}

/// Group source-ordered feature rosters after one global uniqueness count.
/// A duplicate curve identifier invalidates every roster that contains it.
pub(super) fn feature_result_edge_ids_by_feature(
    rows: &[crate::curve::CurveTopologyRow],
) -> BTreeMap<u32, Vec<u32>> {
    let mut feature_ids = BTreeSet::new();
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        feature_ids.insert(row.feature_id);
        *counts.entry(row.id).or_default() += 1;
    }
    // A duplicate in the global namespace invalidates its owner's whole roster.
    for row in rows {
        if counts.get(&row.id) != Some(&1) {
            feature_ids.remove(&row.feature_id);
        }
    }
    let mut by_feature = BTreeMap::<u32, Vec<u32>>::new();
    for row in rows
        .iter()
        .filter(|row| feature_ids.contains(&row.feature_id))
    {
        let edge_ids = by_feature.entry(row.feature_id).or_default();
        edge_ids.push(row.id);
    }
    by_feature
}

pub(in super::super) fn agreed_feature_geometry_ids<'a>(
    affected_ids: &'a [crate::feature::rows::FeatureAffectedIds],
    replay_affected_ids: &'a [crate::feature::rows::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&'a [u32]> {
    let named = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Geometry,
    );
    if named.is_some() {
        return named;
    }
    if has_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Geometry,
    ) {
        return None;
    }
    agreed_feature_replay_geometry_ids(replay_affected_ids, feature_id)
}

#[cfg(test)]
mod tests {
    use super::feature_result_edge_ids_by_feature;
    use std::collections::BTreeMap;

    fn one_edge() -> Vec<crate::curve::CurveTopologyRow> {
        vec![crate::curve::CurveTopologyRow {
            id: 77,
            type_byte: 8,
            feature_id: 97,
            directions: [1, 0xf6],
            faces: [
                std::num::NonZeroU32::new(98),
                std::num::NonZeroU32::new(145),
            ],
            next_edges: [77, 77],
            offset: 0,
        }]
    }

    #[test]
    fn feature_result_edge_roster_keeps_order() {
        let rows = one_edge();
        assert_eq!(
            feature_result_edge_ids_by_feature(&rows).remove(&97),
            Some(vec![77])
        );
        assert_eq!(
            feature_result_edge_ids_by_feature(&rows).get(&97),
            Some(&vec![77])
        );
    }
    #[test]
    fn result_edge_rosters_count_global_identifiers_once() {
        let row = one_edge().remove(0);
        let rows = (0..100)
            .map(|id| crate::curve::CurveTopologyRow {
                id,
                feature_id: id,
                ..row.clone()
            })
            .collect::<Vec<_>>();
        let rosters = feature_result_edge_ids_by_feature(&rows);
        assert_eq!(rosters.len(), 100);
        for id in 0..100 {
            assert_eq!(rosters.get(&id), Some(&vec![id]));
        }
    }

    #[test]
    fn duplicate_global_curve_ids_invalidate_whole_owner_rosters() {
        let row = one_edge().remove(0);
        let rows =
            [(4, 17), (3, 17), (5, 18), (4, 19), (8, 20), (7, 20)].map(|(id, feature_id)| {
                crate::curve::CurveTopologyRow {
                    id,
                    feature_id,
                    ..row.clone()
                }
            });
        let rosters = feature_result_edge_ids_by_feature(&rows);
        assert_eq!(rosters, BTreeMap::from([(18, vec![5]), (20, vec![8, 7])]));
    }
}
