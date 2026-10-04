// SPDX-License-Identifier: Apache-2.0
//! Feature edge selection and generated result-edge identity.

use super::axes::model_feature_ids;
use super::dependencies::{
    agreed_feature_replay_edge_ids, agreed_feature_replay_geometry_ids, has_feature_affected_ids,
};
use super::outputs::CommaList;
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
            let native =
                edge_selection_native(ctx, "creo:allfeatur:edgs_affected", feature_id, ids)?;
            return Ok(Some(EdgeSelection::Resolved {
                edges: Vec::new(),
                native,
            }));
        }
        let native = edge_selection_native(ctx, "creo:allfeatur:edgs_affected", feature_id, ids)?;
        (ids, native)
    } else {
        if has_feature_affected_ids(
            ctx,
            &scan.features.affected_ids,
            feature_id,
            crate::feature::rows::AffectedIdKind::Edges,
        )? {
            return Ok(None);
        }
        if let Some(ids) = agreed_feature_replay_edge_ids(
            ctx,
            &scan.features.replay_affected_ids,
            feature_id,
        )?
        {
            if ids.is_empty() {
                let native = edge_selection_native(
                    ctx,
                    "creo:allfeatur:replay_edgs_affected",
                    feature_id,
                    ids,
                )?;
                return Ok(Some(EdgeSelection::Resolved {
                    edges: Vec::new(),
                    native,
                }));
            }
            let native =
                edge_selection_native(ctx, "creo:allfeatur:replay_edgs_affected", feature_id, ids)?;
            (ids, native)
        } else {
            let Some(round) = ctx
                .admit_iter(&scan.features.legacy_rounds, "creo legacy round records")?
                .find(|round| round.feature_id == feature_id)
            else {
                return Ok(None);
            };
            let Some(ids) = round.edge_ids.as_deref() else {
                return Ok(None);
            };
            let native =
                edge_selection_native(ctx, "creo:legacy_ascii:feature_edges", feature_id, ids)?;
            (ids, native)
        }
    };
    let result_edge_ids = feature_result_edge_ids_by_feature(ctx, &scan.curves.topology_rows)?;
    let mut edges = Vec::new();
    let mut seen = BTreeSet::new();
    let mut unique = true;
    for id in ctx.admit_iter(ids, "creo feature selection IDs")? {
        let text = ctx.format_retained(
            format_args!("creo:visibgeom:edge#{id}"),
            "creo selected edge IDs",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(text.len()),
            "creo selected edge identity validation",
        )?;
        let edge = EdgeId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo edge ID is invalid".into()))?;
        ctx.reserve_vec(&mut edges, 1, "creo selected edge identities")?;
        edges.push(edge);
        if seen.contains(id) {
            unique = false;
        } else {
            ctx.insert_btree_set(&mut seen, *id, "creo selected edge identity nodes")?;
        }
    }
    let all_model_edges_present = if unique {
        let mut all_present = true;
        for edge in ctx.admit_iter(&edges, "creo feature selection edge references")? {
            let mut found = false;
            for candidate in ctx.admit_iter(&ir.model.edges, "creo model edge lookup")? {
                if ctx.equal(
                    &candidate.id,
                    edge,
                    "creo selected model edge identity comparison",
                )? {
                    found = true;
                    break;
                }
            }
            if !found {
                all_present = false;
                break;
            }
        }
        all_present
    } else {
        false
    };
    if unique && all_model_edges_present {
        Ok(Some(EdgeSelection::Resolved { edges, native }))
    } else {
        let mut any_model_edge_present = false;
        for edge in ctx.admit_iter(&edges, "creo feature selection edge references")? {
            for candidate in ctx.admit_iter(&ir.model.edges, "creo model edge lookup")? {
                if ctx.equal(
                    &candidate.id,
                    edge,
                    "creo selected model edge identity comparison",
                )? {
                    any_model_edge_present = true;
                    break;
                }
            }
            if any_model_edge_present {
                break;
            }
        }
        if any_model_edge_present {
            // A typed generated selection names one result namespace. A roster
            // that mixes current B-rep edges with absent edges has no neutral
            // mixed identity, so retain the exact native selection.
            Ok(Some(EdgeSelection::Native(native)))
        } else if let Some(edges) = generated_curve_edge_refs(
            ctx,
            ids,
            &scan.curves.topology_rows,
            &model_feature_ids(ctx, scan)?,
            &result_edge_ids,
        )? {
            Ok(Some(
                EdgeSelection::generated(
                    edges,
                    ctx.copy_retained_text(&native, "creo generated edge selection native")?,
                    ctx,
                )?
                .unwrap_or(EdgeSelection::Native(native)),
            ))
        } else {
            Ok(Some(EdgeSelection::Native(native)))
        }
    }
}

pub(in super::super) fn generated_curve_edge_refs(
    ctx: &DecodeContext<'_>,
    curve_ids: &[u32],
    rows: &[crate::curve::CurveTopologyRow],
    available_features: &BTreeSet<IrFeatureId>,
    result_edge_ids: &BTreeMap<u32, Vec<u32>>,
) -> Result<Option<Vec<GeneratedEdgeRef>>, CodecError> {
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut unique_curve_ids = BTreeSet::new();
    for &curve_id in ctx.admit_iter(curve_ids, "creo selected curve IDs")? {
        if unique_curve_ids.contains(&curve_id) {
            return Ok(None);
        }
        local_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut unique_curve_ids,
                curve_id,
                "creo generated curve identity nodes",
            )
        })?;
    }
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in ctx.admit_iter(rows, "creo curve topology rows")? {
        local_storage.with_storage(|| {
            ctx.admit_btree_entry(&counts, &row.id, "creo generated curve count nodes")
        })?;
        let count = counts.entry(row.id).or_default();
        *count = count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("creo generated curve row counts", u64::MAX, u64::MAX)
        })?;
    }
    let mut unique_rows = BTreeMap::new();
    for row in ctx.admit_iter(rows, "creo curve topology rows")? {
        if counts.get(&row.id) == Some(&1) {
            local_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut unique_rows,
                    row.id,
                    row,
                    "creo generated unique curve row nodes",
                )
            })?;
        }
    }
    let mut generated = Vec::new();
    for &curve_id in ctx.admit_iter(curve_ids, "creo selected curve IDs")? {
        let Some(row) = unique_rows.get(&curve_id) else {
            return Ok(None);
        };
        let feature_text = ctx.format_retained(
            format_args!("creo:model:feature#{}", row.feature_id),
            "creo generated curve feature IDs",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(feature_text.len()),
            "creo generated curve feature identity validation",
        )?;
        let feature = IrFeatureId::mint(feature_text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        if !ctx.contains_btree_set(
            available_features,
            &feature,
            "creo generated curve feature lookup",
        )?
            || !result_edge_ids
                .get(&row.feature_id)
                .is_some_and(|ids| ids.contains(&curve_id))
        {
            return Ok(None);
        }
        let local_id = ctx.format_retained(
            format_args!("curve#{curve_id}"),
            "creo generated curve local IDs",
        )?;
        let Some(edge) = GeneratedEdgeRef::new(feature, local_id, ctx)?.ok() else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut generated, 1, "creo generated curve edge references")?;
        generated.push(edge);
    }
    Ok(Some(generated))
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
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in ctx.admit_iter(rows, "creo curve topology rows")? {
        local_storage.with_storage(|| {
            ctx.admit_btree_entry(&counts, &row.id, "creo feature result edge count nodes")
        })?;
        let count = counts.entry(row.id).or_default();
        *count = count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("creo feature result edge row counts", u64::MAX, u64::MAX)
        })?;
    }
    let mut edge_ids = Vec::new();
    for row in ctx
        .admit_iter(rows, "creo curve topology rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        if counts.get(&row.id) != Some(&1) {
            return Ok(None);
        }
        ctx.reserve_vec(&mut edge_ids, 1, "creo feature result edge IDs")?;
        edge_ids.push(row.id);
    }
    Ok((!edge_ids.is_empty()).then_some(edge_ids))
}

fn feature_result_edge_ids_by_feature(
    ctx: &DecodeContext<'_>,
    rows: &[crate::curve::CurveTopologyRow],
) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let mut feature_ids = BTreeSet::new();
    for row in ctx.admit_iter(rows, "creo feature result edge source rows")? {
        ctx.insert_btree_set(
            &mut feature_ids,
            row.feature_id,
            "creo feature result edge feature nodes",
        )?;
    }
    let mut by_feature = BTreeMap::new();
    for feature_id in ctx
        .admit_iter(&feature_ids, "creo feature result edge feature IDs")?
        .copied()
    {
        if let Some(edge_ids) = feature_result_edge_ids(ctx, rows, feature_id)? {
            ctx.insert_btree_map(
                &mut by_feature,
                feature_id,
                edge_ids,
                "creo feature result edge map nodes",
            )?;
        }
    }
    Ok(by_feature)
}

pub(in super::super) fn agreed_feature_geometry_ids<'a>(
    ctx: &DecodeContext<'_>,
    affected_ids: &'a [crate::feature::rows::FeatureAffectedIds],
    replay_affected_ids: &'a [crate::feature::rows::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Result<Option<&'a [u32]>, CodecError> {
    let named = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Geometry,
    );
    if named.is_some() {
        return Ok(named);
    }
    if has_feature_affected_ids(
        ctx,
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Geometry,
    )? {
        return Ok(None);
    }
    agreed_feature_replay_geometry_ids(ctx, replay_affected_ids, feature_id)
}

#[cfg(test)]
mod tests {
    use super::{
        feature_edge_selection, feature_result_edge_ids, feature_result_edge_ids_by_feature,
        generated_curve_edge_refs,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeSet;

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

    fn generated_reference_error(
        collection: Option<u64>,
        retained: Option<u64>,
        operation: &'static str,
    ) {
        let rows = one_edge();
        let available =
            BTreeSet::from([
                cadmpeg_ir::features::FeatureId::mint("creo:model:feature#97")
                    .expect("fixture feature ID"),
            ]);
        let results = std::collections::BTreeMap::from([(97, vec![77])]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if let Some(limit) = collection {
            policy.limits.max_collection_items = limit;
        }
        if retained.is_some() {
            policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some(operation),
                |cap| {
                    let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut trial_policy = policy;
                    trial_policy.limits.max_retained_bytes = cap;
                    let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &trial_arena,
                        &trial_policy,
                    )
                    .expect("root");
                    generated_curve_edge_refs(&trial_ctx, &[77], &rows, &available, &results)
                },
            );
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty source is admitted");
        let error = generated_curve_edge_refs(&ctx, &[77], &rows, &available, &results)
            .expect_err("one generated reference exceeds the resource limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn generated_curve_identity_nodes_refuse_collection_limit() {
        generated_reference_error(Some(0), None, "creo generated curve identity nodes");
    }

    #[test]
    fn generated_curve_count_nodes_refuse_collection_limit() {
        generated_reference_error(Some(1), None, "creo generated curve count nodes");
    }

    #[test]
    fn generated_unique_curve_row_nodes_refuse_collection_limit() {
        generated_reference_error(Some(2), None, "creo generated unique curve row nodes");
    }

    #[test]
    fn generated_curve_edge_references_refuse_collection_limit() {
        generated_reference_error(Some(3), None, "creo generated curve edge references");
    }

    #[test]
    fn generated_curve_feature_id_refuses_retained_limit() {
        generated_reference_error(
            None,
            Some(cadmpeg_core::decode::u64_from_index("creo:model:feature#97".len()) - 1),
            "creo generated curve feature IDs",
        );
    }

    #[test]
    fn generated_curve_local_id_refuses_retained_limit() {
        generated_reference_error(
            None,
            Some(cadmpeg_core::decode::u64_from_index(
                "creo:model:feature#97".len(),
            )),
            "creo generated curve local IDs",
        );
    }

    fn one_selected_edge() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features
            .affected_ids
            .push(crate::feature::rows::FeatureAffectedIds {
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
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
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
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::RetainedBytes,
            "creo generated edge selection native",
            |ctx| feature_edge_selection(ctx, &scan, &cadmpeg_ir::document::CadIr::empty(), 10),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo generated edge selection native"),
            "{error:?}"
        );
    }

    fn selection_limit_error(
        collection: Option<u64>,
        retained: Option<u64>,
        operation: &'static str,
    ) {
        let scan = one_selected_edge();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if let Some(limit) = collection {
            policy.limits.max_collection_items = limit;
        }
        if retained.is_some() {
            policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some(operation),
                |cap| {
                    let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut trial_policy = policy;
                    trial_policy.limits.max_retained_bytes = cap;
                    let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &trial_arena,
                        &trial_policy,
                    )
                    .expect("root");
                    feature_edge_selection(
                        &trial_ctx,
                        &scan,
                        &cadmpeg_ir::document::CadIr::empty(),
                        10,
                    )
                },
            );
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let error = feature_edge_selection(&ctx, &scan, &cadmpeg_ir::document::CadIr::empty(), 10)
            .expect_err("selected edge exceeds the resource limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn feature_edge_native_text_refuses_retained_limit() {
        selection_limit_error(
            None,
            Some(
                cadmpeg_core::decode::u64_from_index("creo:allfeatur:edgs_affected#10:45".len())
                    - 1,
            ),
            "creo feature edge selection native",
        );
    }

    #[test]
    fn feature_edge_identity_refuses_retained_limit() {
        selection_limit_error(
            None,
            Some(cadmpeg_core::decode::u64_from_index(
                "creo:allfeatur:edgs_affected#10:45".len(),
            )),
            "creo selected edge IDs",
        );
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
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty source is admitted");
        let error = if by_feature {
            feature_result_edge_ids_by_feature(&ctx, &rows).map(|_| ())
        } else {
            feature_result_edge_ids(&ctx, &rows, 97).map(|_| ())
        }
        .expect_err("one edge exceeds the collection limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation),
            "{error:?}"
        );
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
    fn feature_result_edge_source_scan_refuses_work_limit() {
        let rows = one_edge();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty source is admitted");

        let error = feature_result_edge_ids_by_feature(&ctx, &rows)
            .expect_err("the source row scan exceeds the work limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo feature result edge source rows"),
            "{error:?}"
        );
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
            assert_eq!(
                feature_result_edge_ids_by_feature(ctx, &rows)?.get(&97),
                Some(&vec![77])
            );
            Ok::<(), cadmpeg_core::CodecError>(())
        })
        .expect("service profile admits one result edge");
    }

    #[test]
    fn feature_edge_identity_validation_refuses_at_work_boundary() {
        let scan = one_selected_edge();
        let selection = crate::test_support::assert_work_boundaries(
            &["creo selected edge identity validation"],
            |ctx| {
                feature_edge_selection(
                    ctx,
                    &scan,
                    &cadmpeg_ir::document::CadIr::empty(),
                    10,
                )
            },
        );
        assert!(matches!(
            selection,
            Some(cadmpeg_ir::features::EdgeSelection::Native(native))
                if native == "creo:allfeatur:edgs_affected#10:45"
        ));
    }


    #[test]
    fn generated_curve_feature_membership_miss_preserves_result_id_laziness() {
        let rows = one_edge();
        let available = BTreeSet::from([
            cadmpeg_ir::features::FeatureId::mint("creo:model:feature#98")
                .expect("nonmatching feature ID"),
        ]);
        let results = std::collections::BTreeMap::from([(97, vec![77])]);
        let refusal = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo generated curve feature lookup",
            |ctx| generated_curve_edge_refs(ctx, &[77], &rows, &available, &results),
        );
        let limit = match refusal {
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "creo generated curve feature lookup" => limit,
            error => panic!("expected generated feature membership refusal, got {error:?}"),
        };
        let cap = limit.used.checked_add(limit.additional).expect("work cap");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty source is admitted");
        assert!(generated_curve_edge_refs(&ctx, &[77], &rows, &available, &results)
            .expect("a feature miss returns before result-ID membership")
            .is_none());
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            generated_curve_edge_refs(ctx, &[77], &rows, &available, &results)
        })
        .expect("service profile preserves the feature-miss result")
        .is_none());
    }

    #[test]
    fn generated_curve_feature_identity_validation_refuses_at_work_boundary() {
        let rows = one_edge();
        let available = BTreeSet::from([
            cadmpeg_ir::features::FeatureId::mint("creo:model:feature#97")
                .expect("fixture feature ID"),
        ]);
        let results = std::collections::BTreeMap::from([(97, vec![77])]);
        let generated = crate::test_support::assert_work_boundaries(
            &[
                "creo generated curve feature identity validation",
                "creo generated curve feature lookup",
            ],
            |ctx| generated_curve_edge_refs(ctx, &[77], &rows, &available, &results),
        )
        .expect("the feature result contains the selected curve");
        assert_eq!(generated.len(), 1);
        assert_eq!(generated[0].feature.as_str(), "creo:model:feature#97");
        assert_eq!(generated[0].local_id.as_str(), "curve#77");
    }

}
