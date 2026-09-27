// SPDX-License-Identifier: Apache-2.0
//! Feature edge selection and generated result-edge identity.

use super::axes::model_feature_ids;
use super::outputs::CommaList;
use super::dependencies::{
    agreed_feature_replay_edge_ids, agreed_feature_replay_geometry_ids, has_feature_affected_ids,
};
use crate::container::ContainerScan;
use crate::feature::rows::agreed_feature_affected_ids;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{EdgeSelection, FeatureId as IrFeatureId, GeneratedEdgeRef};
use cadmpeg_ir::ids::EdgeId;
use std::collections::{BTreeMap, BTreeSet};

fn edge_selection_native(
    ctx: &DecodeContext<'_>,
    namespace: &'static str,
    feature_id: u32,
    ids: &[u32],
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("{namespace}#{feature_id}:{}", CommaList(ids)),
        "creo feature edge selection native",
    )
}

pub(in super::super) fn feature_edge_selection(
    ctx: &DecodeContext<'_>,
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
            let native = edge_selection_native(ctx, "creo:allfeatur:edgs_affected", feature_id, ids)?;
            return Ok(Some(EdgeSelection::Resolved {
                edges: Vec::new(),
                native,
            }));
        }
        let native = edge_selection_native(ctx, "creo:allfeatur:edgs_affected", feature_id, ids)?;
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
                let native = edge_selection_native(ctx, "creo:allfeatur:replay_edgs_affected", feature_id, ids)?;
                return Ok(Some(EdgeSelection::Resolved {
                    edges: Vec::new(),
                    native,
                }));
            }
            let native = edge_selection_native(ctx, "creo:allfeatur:replay_edgs_affected", feature_id, ids)?;
            (ids, native)
        } else {
            let Some(round) = scan
                .features
                .legacy_rounds
                .iter()
                .find(|round| round.feature_id == feature_id) else {
                return Ok(None);
            };
            let Some(ids) = round.edge_ids.as_deref() else {
                return Ok(None);
            };
            let native = edge_selection_native(ctx, "creo:legacy_ascii:feature_edges", feature_id, ids)?;
            (ids, native)
        }
    };
    let result_edge_ids = feature_result_edge_ids_by_feature(ctx, &scan.curves.topology_rows)?;
    let mut edges = Vec::new();
    let mut seen = BTreeSet::new();
    let mut unique = true;
    for id in ids {
        let text = ctx.format_retained(
            format_args!("creo:visibgeom:edge#{id}"),
            "creo selected edge IDs",
        )?;
        let edge = EdgeId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo edge ID is invalid".into()))?;
        ctx.try_reserve_items(&mut edges, 1, "creo selected edge identities")?;
        edges.push(edge);
        if seen.contains(id) {
            unique = false;
        } else {
            ctx.charge_collection_items(1, "creo selected edge identity nodes")?;
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
        &model_feature_ids(ctx, scan)?,
        &result_edge_ids,
    ) {
        Ok(Some(
            EdgeSelection::generated(
                edges,
                ctx.copy_retained_text(&native, "creo generated edge selection native")?,
            )
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
            let feature = IrFeatureId::compose(&crate::identity::MODEL_FEATURE, row.feature_id);
            (available_features.contains(&feature)
                && result_edge_ids
                    .get(&row.feature_id)
                    .is_some_and(|ids| ids.contains(curve_id)))
            .then_some(GeneratedEdgeRef::new(feature, format!("curve#{curve_id}")).ok()?)
        })
        .collect()
}

/// Return the complete feature-local edge roster proven by unique topology rows.
///
/// A decoded `crv_array` topology row is one materialized edge identity. The
/// global curve namespace must contain that identifier exactly once before the
/// row can be exposed in a feature result state.
pub(in super::super) fn feature_result_edge_ids(
    ctx: &DecodeContext<'_>,
    rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        if !counts.contains_key(&row.id) {
            ctx.charge_collection_items(1, "creo feature result edge count nodes")?;
        }
        *counts.entry(row.id).or_default() += 1;
    }
    let mut edge_ids = Vec::new();
    for row in rows.iter().filter(|row| row.feature_id == feature_id) {
        if counts.get(&row.id) != Some(&1) {
            return Ok(None);
        }
        ctx.try_reserve_items(&mut edge_ids, 1, "creo feature result edge IDs")?;
        edge_ids.push(row.id);
    }
    Ok((!edge_ids.is_empty()).then_some(edge_ids))
}

fn feature_result_edge_ids_by_feature(
    ctx: &DecodeContext<'_>,
    rows: &[crate::curve::CurveTopologyRow],
) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let mut feature_ids = BTreeSet::new();
    for row in rows {
        if !feature_ids.contains(&row.feature_id) {
            ctx.charge_collection_items(1, "creo feature result edge feature nodes")?;
            feature_ids.insert(row.feature_id);
        }
    }
    let mut by_feature = BTreeMap::new();
    for feature_id in feature_ids {
        if let Some(edge_ids) = feature_result_edge_ids(ctx, rows, feature_id)? {
            ctx.charge_collection_items(1, "creo feature result edge map nodes")?;
            by_feature.insert(feature_id, edge_ids);
        }
    }
    Ok(by_feature)
}

#[cfg(test)]
mod tests {
    use super::{feature_edge_selection, feature_result_edge_ids, feature_result_edge_ids_by_feature};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn one_edge() -> Vec<crate::curve::CurveTopologyRow> {
        vec![crate::curve::CurveTopologyRow {
            id: 77,
            type_byte: 8,
            feature_id: 97,
            directions: [1, 0xf6],
            faces: [std::num::NonZeroU32::new(98), std::num::NonZeroU32::new(145)],
            next_edges: [77, 77],
            offset: 0,
        }]
    }

    fn one_selected_edge() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.features.affected_ids.push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::rows::AffectedIdKind::Edges,
            ids: vec![45],
            offset: 0,
        });
        scan
    }

    fn one_generated_edge() -> crate::container::ContainerScan<'static> {
        let mut scan = one_selected_edge();
        scan.features.affected_ids[0].ids[0] = 59;
        scan.features.rows.push(crate::feature::rows::FeatureRow {
            feature_id: 50,
            root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 1,
            offset: 0,
        });
        scan.curves.topology_rows.push(crate::curve::CurveTopologyRow {
            id: 59,
            type_byte: 8,
            feature_id: 50,
            directions: [1, 0xf6],
            faces: [std::num::NonZeroU32::new(61), std::num::NonZeroU32::new(62)],
            next_edges: [59, 59],
            offset: 100,
        });
        scan
    }

    #[test]
    fn generated_edge_selection_native_copy_refuses_retained_limit() {
        let scan = one_generated_edge();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = (
            "creo:allfeatur:edgs_affected#10:59".len()
                + "creo:visibgeom:edge#59".len()
        ) as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty source is admitted");
        let error = feature_edge_selection(&ctx, &scan, &cadmpeg_ir::document::CadIr::empty(), 10)
            .expect_err("the generated native copy exceeds the retained limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo generated edge selection native"), "{error:?}");
    }

    fn selection_limit_error(collection: Option<u64>, retained: Option<u64>, operation: &'static str) {
        let scan = one_selected_edge();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if let Some(limit) = collection {
            policy.limits.max_collection_items = limit;
        }
        if let Some(limit) = retained {
            policy.limits.max_retained_bytes = limit;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let error = feature_edge_selection(&ctx, &scan, &cadmpeg_ir::document::CadIr::empty(), 10)
            .expect_err("selected edge exceeds the resource limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation), "{error:?}");
    }

    #[test]
    fn feature_edge_native_text_refuses_retained_limit() {
        selection_limit_error(None, Some("creo:allfeatur:edgs_affected#10:45".len() as u64 - 1),
            "creo feature edge selection native");
    }

    #[test]
    fn feature_edge_identity_refuses_retained_limit() {
        selection_limit_error(None, Some("creo:allfeatur:edgs_affected#10:45".len() as u64),
            "creo selected edge IDs");
    }

    #[test]
    fn feature_edge_identity_vec_refuses_collection_limit() {
        selection_limit_error(Some(0), None, "creo selected edge identities");
    }

    #[test]
    fn feature_edge_identity_nodes_refuse_collection_limit() {
        selection_limit_error(Some(1), None, "creo selected edge identity nodes");
    }

    fn edge_limit_error(limit: u64, by_feature: bool, operation: &'static str) {
        let rows = one_edge();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty source is admitted");
        let error = if by_feature {
            feature_result_edge_ids_by_feature(&ctx, &rows).map(|_| ())
        } else {
            feature_result_edge_ids(&ctx, &rows, 97).map(|_| ())
        }
        .expect_err("one edge exceeds the collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation), "{error:?}");
    }

    #[test]
    fn feature_result_edge_count_nodes_refuse_collection_limit() {
        edge_limit_error(0, false, "creo feature result edge count nodes");
    }

    #[test]
    fn feature_result_edge_ids_refuse_collection_limit() {
        edge_limit_error(1, false, "creo feature result edge IDs");
    }

    #[test]
    fn feature_result_edge_feature_nodes_refuse_collection_limit() {
        edge_limit_error(0, true, "creo feature result edge feature nodes");
    }

    #[test]
    fn feature_result_edge_map_nodes_refuse_collection_limit() {
        edge_limit_error(3, true, "creo feature result edge map nodes");
    }

    #[test]
    fn feature_result_edge_roster_keeps_order() {
        let rows = one_edge();
        crate::decode::with_test_decode_ctx(|ctx| {
            assert_eq!(feature_result_edge_ids(ctx, &rows, 97)?, Some(vec![77]));
            assert_eq!(feature_result_edge_ids_by_feature(ctx, &rows)?.get(&97), Some(&vec![77]));
            Ok::<(), cadmpeg_core::CodecError>(())
        }).expect("service profile admits one result edge");
    }
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
