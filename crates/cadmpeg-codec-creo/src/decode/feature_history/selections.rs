// SPDX-License-Identifier: Apache-2.0
//! Feature edge selection and generated result-edge identity.

use super::axes::model_feature_ids;
use crate::feature::rows::agreed_feature_affected_ids;
use super::dependencies::{
    agreed_feature_replay_edge_ids, agreed_feature_replay_geometry_ids, has_feature_affected_ids,
};
use super::outputs::CommaList;
use crate::container::ContainerScan;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{EdgeSelection, FeatureId as IrFeatureId, GeneratedEdgeRef};
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
        ctx,
        &scan.features.affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Edges,
    )? {
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
        if let Some(ids) =
            agreed_feature_replay_edge_ids(ctx, &scan.features.replay_affected_ids, feature_id)?
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
            let Some(round) = ctx.find_by(
                &scan.features.legacy_rounds,
                |round| Ok(round.feature_id == feature_id),
                "creo legacy round records",
            )?
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
    let mut scratch = ctx.reserve_scoped(0, "creo selected edge identity workspace")?;
    let mut seen = std::collections::HashSet::new();
    let mut identities = ids.iter();
    while let Some(id) = ctx.next_charged(&mut identities, "creo feature selection IDs")? {
        if !scratch.with_storage(|| {
            ctx.insert_hash_set(&mut seen, *id, "creo selected edge identity nodes")
        })? {
            return Ok(Some(EdgeSelection::Native(native)));
        }
    }
    let mut all_model_edges_present = true;
    let mut any_model_edge_present = false;
    let mut resolved = Vec::new();
    let mut selected = ids.iter();
    while let Some(id) =
        ctx.next_charged(&mut selected, "creo feature selection edge references")?
    {
        let present = ctx.find_by(
            &ir.model.edges,
            |candidate| {
                Ok(crate::identity::matches_numbered_identity(
                    candidate.id.as_str(),
                    "creo:visibgeom:edge#",
                    *id,
                ))
            },
            "creo model edge lookup",
        )?;
        all_model_edges_present &= present.is_some();
        any_model_edge_present |= present.is_some();
        if all_model_edges_present {
            if let Some(edge) = present {
                scratch.with_storage(|| {
                    ctx.push_vec(&mut resolved, &edge.id, "creo resolved edge references")
                })?;
            }
        }
        if any_model_edge_present && !all_model_edges_present {
            break;
        }
    }
    if all_model_edges_present {
        let mut edges = Vec::new();
        for source in ctx.admit_iter(&resolved, "creo resolved edge identity copies")? {
            let edge = source.try_clone_for_decode(ctx, "creo selected edge IDs")?;
            ctx.push_vec(&mut edges, edge, "creo selected edge identities")?;
        }
        Ok(Some(EdgeSelection::Resolved { edges, native }))
    } else if any_model_edge_present {
        // A typed generated selection names one result namespace. A roster
        // that mixes current B-rep edges with absent edges has no neutral
        // mixed identity, so retain the exact native selection.
        Ok(Some(EdgeSelection::Native(native)))
    } else {
        let mut lookup_storage =
            ctx.reserve_scoped(0, "creo generated edge selection lookup")?;
        let result_edge_ids = lookup_storage.with_storage(|| {
            feature_result_edge_ids_by_feature(ctx, &scan.curves.topology_rows)
        })?;
        let available_features =
            lookup_storage.with_storage(|| model_feature_ids(ctx, scan))?;
        if let Some(edges) = generated_curve_edge_refs(
            ctx,
            ids,
            &scan.curves.topology_rows,
            &available_features,
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
    if curve_ids.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut unique_curve_ids = BTreeSet::new();
    let mut curve_id_iter = curve_ids.iter();
    while let Some(&curve_id) = ctx.next_charged(&mut curve_id_iter, "creo selected curve IDs")? {
        if ctx.contains_btree_set(
            &unique_curve_ids,
            &curve_id,
            "creo selected curve identity lookup",
        )? {
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
    let mut unique_rows = std::collections::HashMap::new();
    for row in ctx.admit_iter(rows, "creo curve topology rows")? {
        match local_storage.with_storage(|| {
            ctx.entry_hash_map(
                &mut unique_rows,
                row.id,
                "creo generated curve row index nodes",
            )
        })? {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(row));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
    }
    let mut generated = Vec::new();
    let mut curve_id_iter = curve_ids.iter();
    while let Some(&curve_id) = ctx.next_charged(&mut curve_id_iter, "creo selected curve IDs")? {
        let Some(row) = unique_rows.get(&curve_id).copied().flatten() else {
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
        )? {
            return Ok(None);
        }
        let Some(ids) = ctx.get_btree_map(
            result_edge_ids,
            &row.feature_id,
            "creo generated curve result roster lookup",
        )?
        else {
            return Ok(None);
        };
        if !ctx.contains(ids, &curve_id, "creo generated curve result ID lookup")? {
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
        let count = local_storage
            .with_storage(|| {
                ctx.entry_btree_map(&mut counts, row.id, "creo feature result edge count nodes")
            })?
            .or_default();
        *count = count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("creo feature result edge row counts", u64::MAX, u64::MAX)
        })?;
    }
    let mut edge_ids = Vec::new();
    let mut row_iter = rows.iter();
    while let Some(row) = ctx.next_charged(&mut row_iter, "creo curve topology rows")? {
        if row.feature_id != feature_id {
            continue;
        }
        if ctx.get_btree_map(&counts, &row.id, "creo curve row count lookup")? != Some(&1) {
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
    let mut scratch = ctx.reserve_scoped(0, "creo feature result edge index")?;
    let mut counts = std::collections::HashMap::<u32, usize>::new();
    for row in ctx.admit_iter(rows, "creo feature result edge source rows")? {
        let count = scratch
            .with_storage(|| {
                ctx.entry_hash_map(&mut counts, row.id, "creo feature result edge count nodes")
            })?
            .or_default();
        *count += 1;
    }
    let mut invalid_features = std::collections::HashSet::new();
    for row in ctx.admit_iter(rows, "creo feature result edge validity rows")? {
        if counts.get(&row.id) != Some(&1) {
            scratch.with_storage(|| {
                ctx.insert_hash_set(
                    &mut invalid_features,
                    row.feature_id,
                    "creo feature result edge feature nodes",
                )
            })?;
        }
    }
    let mut by_feature = BTreeMap::new();
    for row in ctx.admit_iter(rows, "creo feature result edge roster rows")? {
        if invalid_features.contains(&row.feature_id) {
            continue;
        }
        let ids = ctx
            .entry_btree_map(
                &mut by_feature,
                row.feature_id,
                "creo feature result edge map nodes",
            )?
            .or_default();
        ctx.push_vec(ids, row.id, "creo feature result edge IDs")?;
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
        ctx,
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Geometry,
    )?;
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
        dimension: cadmpeg_core::decode::ResourceDimension,
        operation: &'static str,
    ) {
        let rows = one_edge();
        let available =
            BTreeSet::from([
                cadmpeg_ir::features::FeatureId::mint("creo:model:feature#97")
                    .expect("fixture feature ID"),
            ]);
        let results = std::collections::BTreeMap::from([(97, vec![77])]);
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            generated_curve_edge_refs(ctx, &[77], &rows, &available, &results)
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn generated_curve_identity_nodes_refuse_collection_limit() {
        generated_reference_error(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo generated curve identity nodes",
        );
    }

    #[test]
    fn generated_curve_row_index_nodes_refuse_collection_limit() {
        generated_reference_error(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo generated curve row index nodes",
        );
    }

    #[test]
    fn generated_curve_edge_references_refuse_collection_limit() {
        generated_reference_error(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo generated curve edge references",
        );
    }

    #[test]
    fn generated_curve_feature_id_refuses_retained_limit() {
        generated_reference_error(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "creo generated curve feature IDs",
        );
    }

    #[test]
    fn generated_curve_local_id_refuses_retained_limit() {
        generated_reference_error(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
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
        dimension: cadmpeg_core::decode::ResourceDimension,
        operation: &'static str,
    ) {
        let scan = one_selected_edge();
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            feature_edge_selection(ctx, &scan, &cadmpeg_ir::document::CadIr::empty(), 10)
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn feature_edge_native_text_refuses_retained_limit() {
        selection_limit_error(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "creo feature edge selection native",
        );
    }

    #[test]
    fn feature_edge_identity_nodes_refuse_collection_limit() {
        selection_limit_error(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo selected edge identity nodes",
        );
    }

    fn edge_limit_error(by_feature: bool, operation: &'static str) {
        let rows = one_edge();
        let error = crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |ctx| {
                if by_feature {
                    feature_result_edge_ids_by_feature(ctx, &rows).map(|_| ())
                } else {
                    feature_result_edge_ids(ctx, &rows, 97).map(|_| ())
                }
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn feature_result_edge_count_nodes_refuse_collection_limit() {
        edge_limit_error(false, "creo feature result edge count nodes");
    }

    #[test]
    fn feature_result_edge_ids_refuse_collection_limit() {
        edge_limit_error(false, "creo feature result edge IDs");
    }

    #[test]
    fn feature_result_edge_source_scan_refuses_work_limit() {
        let rows = one_edge();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            Some("creo feature result edge source rows"),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty source is admitted");

                feature_result_edge_ids_by_feature(&ctx, &rows).map(|_| ())
            },
        );
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
        edge_limit_error(true, "creo feature result edge map nodes");
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
    fn native_edge_selection_does_not_construct_unavailable_edges() {
        let scan = one_selected_edge();
        let selection = crate::decode::with_test_decode_ctx(|ctx| {
            feature_edge_selection(ctx, &scan, &cadmpeg_ir::document::CadIr::empty(), 10)
        })
        .expect("service native selection");
        assert!(matches!(
            selection,
            Some(cadmpeg_ir::features::EdgeSelection::Native(native))
                if native == "creo:allfeatur:edgs_affected#10:45"
        ));
    }

    #[test]
    fn generated_curve_feature_membership_miss_preserves_result_id_laziness() {
        let rows = one_edge();
        let available =
            BTreeSet::from([
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
                    && limit.operation == "creo generated curve feature lookup" =>
            {
                limit
            }
            error => panic!("expected generated feature membership refusal, got {error:?}"),
        };
        let cap = limit.used.checked_add(limit.additional).expect("work cap");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty source is admitted");
        assert!(
            generated_curve_edge_refs(&ctx, &[77], &rows, &available, &results)
                .expect("a feature miss returns before result-ID membership")
                .is_none()
        );
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            generated_curve_edge_refs(ctx, &[77], &rows, &available, &results)
        })
        .expect("service profile preserves the feature-miss result")
        .is_none());
    }

    #[test]
    fn generated_curve_result_id_membership_refuses_work_and_preserves_edge() {
        let rows = one_edge();
        let available =
            BTreeSet::from([
                cadmpeg_ir::features::FeatureId::mint("creo:model:feature#97")
                    .expect("fixture feature ID"),
            ]);
        let results = std::collections::BTreeMap::from([(97, vec![77])]);
        let generated = crate::test_support::assert_work_boundaries(
            &["creo generated curve result ID lookup"],
            |ctx| generated_curve_edge_refs(ctx, &[77], &rows, &available, &results),
        )
        .expect("the result roster contains the generated curve");
        assert_eq!(generated.len(), 1);
        assert_eq!(generated[0].feature.as_str(), "creo:model:feature#97");
        assert_eq!(generated[0].local_id.as_str(), "curve#77");
    }

    #[test]
    fn generated_curve_feature_identity_validation_refuses_at_work_boundary() {
        let rows = one_edge();
        let available =
            BTreeSet::from([
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

    fn resolved_selection_ir() -> cadmpeg_ir::document::CadIr {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let vertex =
            cadmpeg_ir::ids::VertexId::mint("creo:test:vertex#1").expect("fixture identity");
        for id in [45, 7] {
            ir.model.edges.push(cadmpeg_ir::topology::Edge {
                id: cadmpeg_ir::ids::EdgeId::mint(format!("creo:visibgeom:edge#{id}"))
                    .expect("fixture identity"),
                carrier: cadmpeg_ir::topology::EdgeCarrier::Free,
                start: vertex.clone(),
                end: vertex.clone(),
                tolerance: None,
            });
        }
        ir
    }

    #[test]
    fn resolved_selection_copies_identities_in_selection_order() {
        let mut scan = one_selected_edge();
        scan.features.affected_ids[0].ids = vec![7, 45];
        let ir = resolved_selection_ir();
        let selection =
            crate::test_support::assert_work_boundaries(&["creo selected edge IDs"], |ctx| {
                feature_edge_selection(ctx, &scan, &ir, 10)
            });
        let Some(cadmpeg_ir::features::EdgeSelection::Resolved { edges, native }) = selection
        else {
            panic!("resolved edge selection");
        };
        assert_eq!(
            edges
                .iter()
                .map(cadmpeg_ir::ids::EdgeId::as_str)
                .collect::<Vec<_>>(),
            vec!["creo:visibgeom:edge#7", "creo:visibgeom:edge#45"]
        );
        assert_eq!(native, "creo:allfeatur:edgs_affected#10:7,45");
        for (dimension, operation) in [
            (ResourceDimension::RetainedBytes, "creo selected edge IDs"),
            (
                ResourceDimension::CollectionItems,
                "creo selected edge identities",
            ),
        ] {
            let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
                feature_edge_selection(ctx, &scan, &ir, 10)
            });
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == dimension && resource.operation == operation
            ));
        }
    }

    #[test]
    fn duplicate_selection_skips_model_and_generated_queries() {
        let mut scan = one_generated_edge();
        scan.features.affected_ids[0].ids = vec![59, 59];
        let ir = resolved_selection_ir();
        let unnecessary_query = std::cell::Cell::new(false);
        let selection = crate::test_support::assert_work_boundaries(
            &["creo feature selection IDs"],
            |ctx| {
                let result = feature_edge_selection(ctx, &scan, &ir, 10);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(resource)) = &result {
                    if matches!(
                        resource.operation,
                        "creo model edge lookup" | "creo feature result edge source rows"
                    ) {
                        unnecessary_query.set(true);
                    }
                }
                result
            },
        );
        assert!(!unnecessary_query.get());
        assert!(matches!(
            selection,
            Some(cadmpeg_ir::features::EdgeSelection::Native(native))
                if native == "creo:allfeatur:edgs_affected#10:59,59"
        ));
    }

    #[test]
    fn mixed_and_duplicate_model_edge_selections_keep_native_identity() {
        let ir = resolved_selection_ir();
        for (ids, native) in [
            (vec![7, 46], "creo:allfeatur:edgs_affected#10:7,46"),
            (vec![7, 7], "creo:allfeatur:edgs_affected#10:7,7"),
        ] {
            let mut scan = one_selected_edge();
            scan.features.affected_ids[0].ids = ids;
            let selection = crate::decode::with_test_decode_ctx(|ctx| {
                feature_edge_selection(ctx, &scan, &ir, 10)
            })
            .expect("service selection");
            assert!(
                matches!(selection, Some(cadmpeg_ir::features::EdgeSelection::Native(value)) if value == native)
            );
        }
    }
}
