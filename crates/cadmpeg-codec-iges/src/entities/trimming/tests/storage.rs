// SPDX-License-Identifier: Apache-2.0
use std::mem::{align_of, size_of};

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::PositiveReal;

use super::*;

#[test]
fn trimmed_topology_identity_copies_refuse_before_retaining_text() {
    let bytes = bounded_plane_file();
    assert_trimming_retained_refusal(&bytes, "iges trimming identity copy");
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
}

#[test]
fn trimmed_support_nurbs_copy_refuses_nested_storage() {
    let bytes = subrange_nurbs_surface_boundary_file(2);
    IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    // The copy visits u knots, v knots, outer pole rows, then the first inner row.
    for occurrence in 0..4 {
        assert_trimming_collection_refusal(&bytes, "iges copied support surface", occurrence);
    }
}

#[test]
fn trimming_projection_refuses_counted_boundary_vectors() {
    for (bytes, operation) in [
        (bounded_plane_file(), "iges Type141 boundary segments"),
        (multi_pcurve_boundary_file(), "iges Type141 segment pcurves"),
        (trimmed_plane_file(), "iges Type144 boundary sequences"),
        (bounded_plane_file(), "iges Type143 boundary sequences"),
        (bounded_plane_file(), "iges trimming linear candidates"),
        (bounded_plane_file(), "iges trimming boundary items"),
        (
            multi_pcurve_boundary_file(),
            "iges trimming segment pcurves",
        ),
        (bounded_plane_file(), "iges trimming coedge ids"),
        (bounded_plane_file(), "iges trimming source endpoints"),
        (
            bounded_plane_file(),
            "iges trimming candidate vertex derivations",
        ),
        (bounded_plane_file(), "loop ring members"),
    ] {
        assert_trimming_collection_refusal(&bytes, operation, 0);
    }
}

#[test]
fn type142_boundary_creation_refuses_nested_slots_and_index_node() {
    let bytes = subrange_nurbs_surface_boundary_file_with_source_precision();
    for operation in [
        "iges Type142 boundary pcurve pointers",
        "iges Type142 boundary segments",
        "iges trimming boundary index nodes",
    ] {
        assert_trimming_collection_refusal(&bytes, operation, 0);
    }
    assert!(IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .is_ok());
}

#[test]
fn boundary_carrier_index_and_selected_edge_refuse_unadmitted_storage() {
    let bytes = bounded_plane_file();
    for operation in [
        "iges boundary carrier index nodes",
        "iges boundary carrier edge references",
    ] {
        assert_trimming_collection_refusal(&bytes, operation, 0);
    }
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    for operation in [
        "iges selected edge curve ID",
        "iges selected edge ID",
        "iges selected edge start ID",
        "iges selected edge end ID",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    let mut storage = ctx.reserve_scoped(0, "test selected edge scratch")?;
                    storage.with_storage(|| {
                        super::super::clone_boundary_edge(&decoded.ir().model.edges[0], ctx)
                    })
                })
            },
        );
    }
}

#[test]
fn trimming_model_index_refuses_identity_storage_before_lookup() {
    let bytes = bounded_plane_file();
    assert_trimming_collection_refusal(&bytes, "model identity universe slots", 0);
    assert_trimming_collection_refusal(&bytes, "model identity index slots", 0);
    assert_trimming_materialized_refusal(&bytes, "model identity universe slots");
    assert!(IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .is_ok());
}

#[test]
fn support_bound_walk_refuses_surface_identity_and_node() {
    let bytes = bounded_plane_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let index =
        cadmpeg_ir::index::ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges support-bound visiting surface ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                super::super::surface_parameter_bounds(
                    &index,
                    &decoded.ir().model.surfaces[0].id,
                    ctx,
                )
            })
        },
    );
    assert_trimming_collection_refusal(&bytes, "iges support-bound visiting surface nodes", 0);
    assert!(IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .is_ok());
}

#[test]
fn trimming_source_text_refuses_scoped_storage_and_work() {
    let bytes = bounded_plane_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    for operation in [
        "iges trimming source endpoint edge text",
        "iges trimming source entity text",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                IgesCodec
                    .decode(
                        &mut Cursor::new(&bytes),
                        &DecodeOptions {
                            policy,
                            ..DecodeOptions::default()
                        },
                    )
                    .map(|_| ())
                    .map_err(|error| match error {
                        cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
                        other => panic!("unexpected text refusal: {other:?}"),
                    })
            },
        );
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    let mut storage = ctx.reserve_scoped(0, "test boundary source text")?;
                    storage.with_storage(|| {
                        if operation == "iges trimming source endpoint edge text" {
                            ctx.format_retained(
                                format_args!("{}", decoded.ir().model.edges[0].id),
                                "iges trimming source endpoint edge text",
                            )
                        } else {
                            ctx.format_retained(
                                format_args!("iges:entity:directory#{}", 13),
                                "iges trimming source entity text",
                            )
                        }
                    })
                })
            },
        );
    }
}

#[test]
fn implicit_outer_surface_attachment_refuses_procedural_slot() {
    let bytes = trimmed_plane_with_boundaries("106,1,5,0,0,0,1,0,1,1,0,1,0,0;", "144,1,0,1,,13;");
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(!result.ir().model.procedural_surfaces.is_empty());
    assert_trimming_collection_refusal(&bytes, "store procedural surface constructions", 0);
}

#[test]
fn implicit_outer_boundary_refuses_curve_id_storage() {
    let bytes = trimmed_plane_with_boundaries("106,1,5,0,0,0,1,0,1,1,0,1,0,0;", "144,1,0,1,,13;");
    assert_trimming_collection_refusal(&bytes, "iges implicit boundary curve IDs", 0);
    assert_trimming_collection_refusal(&bytes, "iges trimming surfaces slots", 0);
    assert_trimming_retained_refusal(&bytes, "iges implicit boundary curve ID text");
    assert_trimming_collection_refusal(&bytes, "iges implicit boundary pcurve IDs", 0);
    assert_trimming_retained_refusal(&bytes, "iges implicit boundary pcurve ID text");
    assert!(IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .is_ok());
}

#[test]
fn trimmed_pcurve_uses_refuse_nested_storage() {
    let bytes = trimmed_plane_with_inner_loop_file();
    for operation in [
        "iges trimming coedge pcurve uses",
        "iges trimming pcurve slots",
    ] {
        assert_trimming_collection_refusal(&bytes, operation, 0);
    }
    assert_trimming_retained_refusal(&bytes, "iges trimming pcurve ID copy");
    assert!(IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .is_ok());
}

#[test]
fn trimmed_face_refuses_nested_topology_lanes_and_staging() {
    let bytes = bounded_plane_file();
    for operation in [
        "iges trimming edges slots",
        "iges trimming coedges slots",
        "iges trimming loops slots",
        "iges trimming faces slots",
        "iges trimming shells slots",
        "iges trimming regions slots",
        "iges trimming bodies slots",
        "iges trimming face loop IDs",
        "iges trimming shell face IDs",
        "iges trimming region shell IDs",
        "iges trimming body region IDs",
        "iges trimming staged candidates",
        "iges trimming committed vertex derivations",
    ] {
        assert_trimming_collection_refusal(&bytes, operation, 0);
    }
    assert!(IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .is_ok());
}

#[test]
fn linear_boundary_path_refuses_collection_limit_before_append() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges linear boundary path",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut target = Vec::new();
            let result = append_path(&mut target, &[1_u8, 2, 3], &ctx);
            assert!(target.is_empty());
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == 0 && limit.additional == 3));
    let mut target = Vec::new();

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(append_path(&mut target, &[1_u8, 2, 3], &ctx).unwrap());
    assert_eq!(target, [1, 2, 3]);
}

#[test]
fn boundary_clustering_releases_consumed_root_nodes_before_the_allocating_sort() {
    // The core stable sorter allocates two index lanes above twenty values.
    for count in [1, 21, 64] {
        let points: Vec<FinitePoint3> = (0..count).map(|index| {
            FinitePoint3::new(Point3::new(
                f64::from(u32::try_from(index).unwrap()) * 2.0,
                0.0,
                0.0,
            ))
            .unwrap()
        }).collect();
        let parents_bytes = points.len() * size_of::<usize>();
        let sizes_bytes = points.len() * size_of::<usize>();
        // Core charges the insertion-built node bound: ceil(n/5) nodes, each
        // with eleven key/value lanes, sixteen pointers and both alignments.
        let node_bytes = 11 * (size_of::<usize>() + size_of::<Vec<usize>>())
            + 16 * size_of::<usize>()
            + 2 * align_of::<usize>().max(align_of::<Vec<usize>>());
        let roots_bytes = ((points.len() - 1) / 5 + 1) * node_bytes;
        // Each singleton member vector uses core's four-slot usize minimum.
        let members_bytes = points.len() * 4 * size_of::<usize>();
        let cluster_bytes = points.len() * size_of::<super::super::BoundaryVertexCluster>();
        let before_slots = u64_from_index(roots_bytes + members_bytes);
        let peak = before_slots + u64_from_index(cluster_bytes);
        // Union-find scratch has no survivor after membership construction.
        assert!(parents_bytes + sizes_bytes + roots_bytes + members_bytes
            < usize::try_from(peak - 1).unwrap());
        assert!(
            members_bytes + cluster_bytes
                + 2 * points.len() * size_of::<usize>()
                < usize::try_from(peak).unwrap()
        );
        for cap in [peak - 1, peak] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = ctx.with_scoped_storage("discarded boundary clusters", || {
                cluster_boundary_positions(&points, PositiveReal::new(1.0).unwrap(), &ctx)
            });
            if cap < peak {
                let first = match result.as_ref() {
                    Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(first))) => {
                        *first
                    }
                    _ => panic!("expected actual cluster-slot allocation refusal"),
                };
                drop(result);
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(first.operation, "iges boundary cluster slots");
                assert_eq!(
                    (first.limit, first.used, first.additional),
                    (cap, before_slots, u64_from_index(cluster_bytes))
                );
                for _ in 0..64 {
                    for source in [points.as_slice(), &[]] {
                        assert!(matches!(cluster_boundary_positions(source, PositiveReal::ONE, &ctx),
                            Err(super::super::BoundaryVertexCreationError::Resource(
                                CodecError::ResourceLimit(last))) if last == first));
                    }
                }
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
                );
            } else {
                let (clusters, storage) = result.unwrap();
                assert_eq!(clusters.len(), points.len());
                for (index, cluster) in clusters.iter().enumerate() {
                    assert_eq!(cluster.members, [index]);
                    assert_eq!(cluster.representative, points[index]);
                }
                drop(clusters);
                drop(storage);
                let released = ctx
                    .reserve_scoped(peak, "all boundary cluster scratch released")
                    .unwrap();
                drop(released);
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[derive(Clone, Copy)]
enum RejectedBoundary {
    ModelCurve,
    Sense,
    Count,
    Cardinality,
    Pcurve,
    PcurveLate,
    UseFlag,
    UseFlagLate,
}

fn assert_rejected_type141_storage(kind: RejectedBoundary) {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    use cadmpeg_ir::report::loss::LossNote;
    const ITEMS: usize = 4096;
    const ATTEMPTS: usize = 16;
    let pcurves = matches!(kind, RejectedBoundary::Pcurve | RejectedBoundary::PcurveLate | RejectedBoundary::UseFlag | RejectedBoundary::UseFlagLate);
    let segment_count = if pcurves { 1 } else { ITEMS };
    let mut values = vec![141, i64::from(pcurves), 1, 1, i64::try_from(segment_count).unwrap()];
    let reason = match kind {
        RejectedBoundary::ModelCurve => "boundary model-curve pointer is invalid",
        RejectedBoundary::Sense => "boundary segment sense is not 1 or 2",
        RejectedBoundary::Count => "boundary pcurve count is invalid",
        RejectedBoundary::Cardinality => "boundary pcurve collection cardinality disagrees with its representation type",
        RejectedBoundary::Pcurve | RejectedBoundary::PcurveLate | RejectedBoundary::UseFlag | RejectedBoundary::UseFlagLate => "boundary pcurve pointer is invalid",
    };
    if pcurves {
        values.extend([1, 1, i64::try_from(ITEMS).unwrap()]);
        if matches!(kind, RejectedBoundary::PcurveLate | RejectedBoundary::UseFlagLate) {
            values.extend(std::iter::repeat_n(33, ITEMS - 1));
            values.push(i64::from(matches!(kind, RejectedBoundary::UseFlagLate)));
        } else {
            values.extend(std::iter::repeat_n(i64::from(matches!(kind, RejectedBoundary::UseFlag)), ITEMS));
        }
    } else {
        let tuple = match kind {
            RejectedBoundary::ModelCurve => [0, 1, 0],
            RejectedBoundary::Sense => [1, 0, 0],
            RejectedBoundary::Count => [1, 1, -1],
            RejectedBoundary::Cardinality => [1, 1, 1],
            RejectedBoundary::Pcurve | RejectedBoundary::PcurveLate | RejectedBoundary::UseFlag | RejectedBoundary::UseFlagLate => unreachable!(),
        };
        for _ in 0..ITEMS { values.extend(tuple); }
    }
    let mut directory: Vec<_> = (0..ATTEMPTS).map(|i| {
        crate::test_support::directory_target(u32::try_from(2 * i + 1).unwrap(), 141)
    }).collect();
    if matches!(kind, RejectedBoundary::PcurveLate | RejectedBoundary::UseFlagLate) {
        let mut parametric = crate::test_support::directory_target(33, 110);
        parametric.status = crate::directory::SourceStatus::from_codes([0, 0, 5, 0]);
        directory.push(parametric);
    }
    let records: Vec<_> = directory[..ATTEMPTS].iter().map(|entry| {
        ParameterRecord::from_test_tokens(entry.sequence, 0..0, Vec::new(), values.len(),
            values.iter().map(|value| Token { value: TokenValue::Integer(*value), span: 0..0 }).collect(),
            Vec::new())
    }).collect();
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let segments = u64_from_index(segment_count * size_of::<super::super::BoundarySegment>());
    let pointers = if pcurves { u64_from_index(ITEMS * size_of::<u32>()) } else { 0 };
    let allocation = if pcurves { pointers } else { segments };
    let prior = if pcurves { segments } else { 0 };
    let losses_per_attempt = if matches!(kind, RejectedBoundary::UseFlag | RejectedBoundary::UseFlagLate) { 2 } else { 1 };
    let loss_count = ATTEMPTS * losses_per_attempt;
    let slots = u64_from_index(loss_count * size_of::<LossNote>());
    let peak = segments + pointers + slots;
    // Exact rejected candidate capacity covers the old/new slot overlap of
    // the power-of-two loss vector, without holding earlier dead candidates.
    assert!(segments + pointers > slots);
    for cap in [prior + allocation - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut derivation_storage = ctx.reserve_scoped(0, "test boundary derivation storage").unwrap();
        let mut sequences = crate::entities::geometry::SourceSequences::new(&ctx).unwrap();
        let mut ir = CadIr::empty();
        let result = super::super::project(&mut ir, &directory, &records, &global,
            (&ctx, &mut derivation_storage), &mut sequences);
        if cap < peak {
            let first = match result.err().expect("expected actual candidate allocation refusal") {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected resource refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, if pcurves { "iges Type141 segment pcurves" } else { "iges Type141 boundary segments" });
            assert_eq!((first.limit, first.used, first.additional), (cap, prior, allocation));
            drop(sequences);
            drop(derivation_storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let (outcome, derivations) = result.unwrap();
            assert!(outcome.decoded.is_empty());
            assert!(derivations.is_empty());
            assert_eq!(outcome.losses.len(), loss_count);
            if losses_per_attempt == 2 {
                for pair in outcome.losses.chunks_exact(2) {
                    assert!(pair[0].message.ends_with("boundary pcurve does not have entity-use flag 05"));
                    assert!(pair[1].message.ends_with(reason));
                }
            } else { for loss in &outcome.losses { assert!(loss.message.ends_with(reason)); } }
            assert_eq!(ir, CadIr::empty());
            let released = ctx.reserve_scoped(cap - slots, "test rejected Type141 backing destroyed").unwrap();
            drop(released);
            drop(outcome);
            drop(derivations);
            drop(sequences);
            drop(derivation_storage);
            let released = ctx.reserve_scoped(cap, "test boundary outcome destroyed").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn rejected_boundary_model_curve_releases_segments_per_attempt() {
    assert_rejected_type141_storage(RejectedBoundary::ModelCurve);
}
#[test]
fn rejected_boundary_sense_releases_segments_per_attempt() {
    assert_rejected_type141_storage(RejectedBoundary::Sense);
}
#[test]
fn rejected_boundary_count_releases_segments_per_attempt() {
    assert_rejected_type141_storage(RejectedBoundary::Count);
}
#[test]
fn rejected_boundary_cardinality_releases_segments_per_attempt() {
    assert_rejected_type141_storage(RejectedBoundary::Cardinality);
}
#[test]
fn rejected_boundary_pcurve_releases_nested_storage_per_attempt() {
    assert_rejected_type141_storage(RejectedBoundary::Pcurve);
}
#[test]
fn rejected_boundary_use_flag_preserves_two_losses_and_releases_nested_storage() {
    assert_rejected_type141_storage(RejectedBoundary::UseFlag);
}

#[test]
fn rejected_boundary_use_flag_after_valid_prefix_releases_nested_storage() {
    assert_rejected_type141_storage(RejectedBoundary::UseFlagLate);
}

#[test]
fn rejected_boundary_pointer_after_valid_prefix_releases_nested_storage() {
    assert_rejected_type141_storage(RejectedBoundary::PcurveLate);
}

fn assert_boundary_vertex_output_custody(positions: &[Point3], expected: &[&str]) {
    use cadmpeg_ir::ids::VertexId;
    const CAP: u64 = 65536;
    let endpoints: Vec<_> = positions.iter().enumerate().map(|(index, position)| {
        BoundaryVertexSourceEndpoint {
            edge: format!("test:model:edge#{index}"),
            endpoint: BoundaryEndpoint::Start,
            position: FinitePoint3::new(*position).unwrap(),
        }
    }).collect();
    let live = u64_from_index(expected.len() * size_of::<VertexId>()
        + expected.iter().map(|id| id.len()).sum::<usize>());
    for refuse_extra in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = CAP;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut candidate_storage = ctx.reserve_scoped(0, "test boundary candidate storage").unwrap();
        let mut candidate = ModelDraft::new();
        let mut sequences = crate::entities::geometry::SourceSequences::new(&ctx).unwrap();
        let mut derivation_storage = ctx.reserve_scoped(0, "test boundary derivation storage").unwrap();
        let super::super::BoundaryVertices { ids, derivations, _storage: output_storage } =
            candidate_storage.with_storage(|| create_boundary_vertices(
                &mut candidate,
                &crate::ids::Stem::directory(9_u32),
                ("iges:entity:directory#9", 0),
                &endpoints,
                PositiveReal::new(1.0).unwrap(),
                (&mut sequences, &mut derivation_storage),
                &ctx,
            )).unwrap();
        assert_eq!(ids.iter().map(VertexId::as_str).collect::<Vec<_>>(), expected);
        let cluster_count = expected.iter().enumerate().filter(|(index, id)| {
            !expected[..*index].contains(id)
        }).count();
        assert_eq!(derivations.len(), cluster_count);
        assert_eq!(candidate.model().points.len(), cluster_count);
        assert_eq!(candidate.model().vertices.len(), cluster_count);
        drop(derivations);
        drop(derivation_storage);
        drop(candidate);
        drop(candidate_storage);
        drop(sequences);
        let free = ctx.reserve_scoped(CAP - live, "test exact live boundary vertex output").unwrap();
        drop(free);
        if refuse_extra {
            let first = match ctx.reserve_scoped(CAP - live + 1, "test boundary output remains live").err().unwrap() {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected exact materialized refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "test boundary output remains live");
            assert_eq!((first.limit, first.used, first.additional), (CAP, live, CAP - live + 1));
            drop(ids);
            drop(output_storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            drop(ids);
            drop(output_storage);
            let free = ctx.reserve_scoped(CAP, "test boundary output backing destroyed").unwrap();
            drop(free);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn empty_boundary_vertex_output_retains_no_temporary_storage() {
    assert_boundary_vertex_output_custody(&[], &[]);
}

#[test]
fn sewn_boundary_vertex_output_retains_only_result_ids_and_slots() {
    assert_boundary_vertex_output_custody(
        &[Point3::new(0.0, 0.0, 0.0), Point3::new(0.5, 0.0, 0.0)],
        &["iges:model:vertex#D9:0:0", "iges:model:vertex#D9:0:0"],
    );
}

#[test]
fn separate_boundary_vertex_output_releases_each_cluster_identity() {
    assert_boundary_vertex_output_custody(
        &[Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        &["iges:model:vertex#D9:0:0", "iges:model:vertex#D9:0:1"],
    );
}

fn source_path_node_bytes() -> u64 {
    u64_from_index(11 * size_of::<CurveId>() + 16 * size_of::<usize>()
        + 2 * align_of::<CurveId>().max(align_of::<usize>()))
}

fn source_intervals<'ctx>(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    id: &CurveId,
    active: &mut std::collections::BTreeSet<CurveId>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<Option<super::super::SourceCurveControls<'ctx>>, CodecError> {
    super::super::source_curve_control_intervals(
        index, id, (&[], &[]),
        crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
        1.0, active, ctx,
    )
}

#[test]
fn repeated_absent_source_curves_release_each_actual_path_root() {
    const ATTEMPTS: u64 = 16;
    let ir = CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let id = CurveId::mint("test:model:curve#absent").unwrap();
    let key = u64_from_index(id.as_str().len());
    let node = source_path_node_bytes();
    for cap in [node + key - 1, node + key] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = ATTEMPTS;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut outer = ctx.reserve_scoped(0, "test repeated source scratch").unwrap();
        let mut active = std::collections::BTreeSet::new();
        if cap < node + key {
            let first = match outer.with_storage(|| source_intervals(&index, &id, &mut active, &ctx)).err().expect("expected original resource refusal") {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected first active node refusal"),
            };
            assert!(active.is_empty());
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges source active curve nodes");
            assert_eq!((first.limit, first.used, first.additional), (cap, key, node));
            drop(active);
            drop(outer);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            for _ in 0..ATTEMPTS {
                assert!(outer.with_storage(|| source_intervals(&index, &id, &mut active, &ctx)).unwrap().is_none());
                assert!(active.is_empty());
                let free = ctx.reserve_scoped(cap, "test destroyed source path root").unwrap();
                drop(free);
            }
            drop(active);
            drop(outer);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn source_curve_removal_preserves_seeded_ancestor_storage() {
    let ir = CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let seed = CurveId::mint("test:model:curve#ancestor").unwrap();
    let child = CurveId::mint("test:model:curve#absent").unwrap();
    let node = source_path_node_bytes();
    let seed_bytes = u64_from_index(seed.as_str().len());
    let child_bytes = u64_from_index(child.as_str().len());
    let cap = node + seed_bytes + child_bytes;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut seed_storage = ctx.reserve_scoped(0, "test source ancestor storage").unwrap();
    let mut active = std::collections::BTreeSet::new();
    seed_storage.with_storage(|| {
        let seed = seed.try_clone_for_decode(&ctx, "test source ancestor ID")?;
        ctx.insert_btree_set(&mut active, seed, "test source ancestor node")
    }).unwrap();
    assert!(source_intervals(&index, &child, &mut active, &ctx).unwrap().is_none());
    assert_eq!(active.iter().collect::<Vec<_>>(), [&seed]);
    let free = ctx.reserve_scoped(child_bytes, "test source child identity destroyed").unwrap();
    drop(free);
    drop(active);
    drop(seed_storage);
    let free = ctx.reserve_scoped(cap, "test source ancestor backing destroyed").unwrap();
    drop(free);
    ctx.finish_session().unwrap();
}

#[test]
fn source_curve_node_growth_refund_preserves_nonempty_ancestor_bound() {
    let ir = CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let child = CurveId::mint("test:model:curve#absent").unwrap();
    let child_bytes = u64_from_index(child.as_str().len());
    let node = source_path_node_bytes();
    for count in [5_usize, 10, 15, 55] {
        let seeds: Vec<_> = (0..count).map(|i| {
            CurveId::mint(format!("test:model:curve#ancestor{i:03}")).unwrap()
        }).collect();
        // Non-root nodes have at least five keys. Existing frame receipts
        // sum to ceil(count / 5) nodes; this insertion adds one node bound.
        let seed_bytes = u64_from_index(seeds.iter().map(|id| id.as_str().len()).sum::<usize>());
        let seed_live = seed_bytes + u64_from_index((count - 1) / 5 + 1) * node;
        let peak = seed_live + child_bytes + node;
        for cap in [peak - 1, peak] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut seed_storage = ctx.reserve_scoped(0, "test multiple ancestor storage").unwrap();
            let mut active = std::collections::BTreeSet::new();
            seed_storage.with_storage(|| {
                for seed in &seeds {
                    let seed = seed.try_clone_for_decode(&ctx, "test multiple ancestor ID")?;
                    ctx.insert_btree_set(&mut active, seed, "test multiple ancestor node")?;
                }
                Ok::<_, CodecError>(())
            }).unwrap();
            let result = source_intervals(&index, &child, &mut active, &ctx);
            assert_eq!(active.len(), seeds.len());
            assert!(seeds.iter().all(|seed| active.contains(seed)));
            if cap < peak {
                let first = match result.err().expect("expected source node growth refusal") {
                    CodecError::ResourceLimit(first) => first,
                    _ => panic!("expected original resource refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(first.operation, "iges source active curve nodes");
                assert_eq!((first.limit, first.used, first.additional), (cap, seed_live + child_bytes, node));
                drop(active);
                drop(seed_storage);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert!(result.unwrap().is_none());
                let free = ctx.reserve_scoped(child_bytes + node, "test multiple ancestor bound remains live").unwrap();
                drop(free);
                drop(active);
                drop(seed_storage);
                let free = ctx.reserve_scoped(cap, "test multiple ancestor backing destroyed").unwrap();
                drop(free);
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn source_curve_depth_refusal_destroys_path_and_cycle_keeps_original_absence() {
    use cadmpeg_ir::geometry::{CompositeCurveSegment, CompositeCurveTransition};
    let id = CurveId::mint("test:model:curve#cycle").unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: id.clone(), same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }].try_into().unwrap(),
            self_intersect: None,
        }),
        source_object: None,
    });
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let cap = source_path_node_bytes() + u64_from_index(id.as_str().len());
    for depth in [1, 2] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = depth;
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut active = std::collections::BTreeSet::new();
        let result = source_intervals(&index, &id, &mut active, &ctx);
        assert!(active.is_empty());
        if depth == 1 {
            let first = match result.err().expect("expected original resource refusal") {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected original child depth refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
            assert_eq!(first.operation, "iges source curve intervals");
            assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert!(result.unwrap().is_none());
            let free = ctx.reserve_scoped(cap, "test source cycle root destroyed").unwrap();
            drop(free);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn source_curve_removal_refusal_destroys_path_before_its_receipt() {
    let ir = CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let id = CurveId::mint("test:model:curve#absent").unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "iges source active curve removal", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut active = std::collections::BTreeSet::new();
            let result = source_intervals(&index, &id, &mut active, &ctx).map(|controls| {
                assert!(controls.is_none());
                None::<()>
            });
            assert!(active.is_empty());
            if let Err(CodecError::ResourceLimit(first)) = &result {
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first));
            } else {
                assert!(result.as_ref().unwrap().is_none());
                ctx.finish_session().unwrap();
            }
            result
        },
    );
}

fn native_composite_source_fixture(last_pointer: i64) -> (CadIr, CurveId, Vec<crate::directory::DirectoryEntry>, Vec<crate::parameter::ParameterRecord>) {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    use cadmpeg_ir::geometry::{CompositeCurveSegment, CompositeCurveTransition};
    const CHILDREN: usize = 64;
    let root = CurveId::mint("iges:model:curve#D1").unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: root.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: root.clone(), same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }].try_into().unwrap(),
            self_intersect: None,
        }),
        source_object: None,
    });
    let entries = vec![
        crate::test_support::directory_target(1, 102),
        crate::test_support::directory_target(3, 110),
    ];
    let mut values = vec![102, i64::try_from(CHILDREN).unwrap()];
    values.extend(std::iter::repeat_n(3, CHILDREN - 1));
    values.push(last_pointer);
    let records = vec![ParameterRecord::from_test_tokens(1, 0..0, Vec::new(), values.len(),
        values.into_iter().map(|value| Token { value: TokenValue::Integer(value), span: 0..0 }).collect(), Vec::new())];
    (ir, root, entries, records)
}

fn assert_native_composite_child_storage(last_pointer: i64, refuse: bool) {
    use cadmpeg_core::decode::ScopedReservation;
    const CHILDREN: usize = 64;
    const ATTEMPTS: usize = 16;
    let (ir, root, entries, records) = native_composite_source_fixture(last_pointer);
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let root_bytes = source_path_node_bytes() + u64_from_index(root.as_str().len());
    let slots = u64_from_index(CHILDREN * size_of::<(CurveId, ScopedReservation<'_>)>());
    let child_bytes = u64_from_index("iges:model:curve#D3".len());
    // All identities are built before the first child traversal. That child
    // adds its active identity while every original tuple remains live.
    let peak = root_bytes + slots + u64_from_index(CHILDREN) * child_bytes + child_bytes;
    let caps = if refuse { vec![root_bytes + slots - 1, root_bytes + slots + child_bytes - 1] } else { vec![peak] };
    for cap in caps {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut outer = ctx.reserve_scoped(0, "test native composite source scratch").unwrap();
        let mut active = std::collections::BTreeSet::new();
        let mut refusal = None;
        for _ in 0..if refuse { 1 } else { ATTEMPTS } {
            let result = outer.with_storage(|| super::super::source_curve_control_intervals(
                &index, &root, (&entries, &records),
                crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
                1.0, &mut active, &ctx,
            ));
            assert!(active.is_empty());
            if refuse {
                let first = match result.err().expect("expected native child allocation refusal") {
                    CodecError::ResourceLimit(first) => first,
                    _ => panic!("expected original resource refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                let (operation, used, additional) = if cap < root_bytes + slots {
                    ("iges source composite child IDs", root_bytes, slots)
                } else {
                    ("iges generated identity", root_bytes + slots, child_bytes)
                };
                assert_eq!(first.operation, operation);
                assert_eq!((first.limit, first.used, first.additional), (cap, used, additional));
                refusal = Some(first);
                break;
            }
            assert!(result.unwrap().is_none());
            let free = ctx.reserve_scoped(cap, "test native child IDs and slots destroyed").unwrap();
            drop(free);
        }
        drop(active);
        drop(outer);
        if let Some(first) = refusal {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn absent_native_composite_children_release_actual_ids_and_iterator_slots() {
    assert_native_composite_child_storage(3, false);
}

#[test]
fn invalid_last_native_composite_pointer_releases_earlier_ids_and_slots() {
    assert_native_composite_child_storage(-1, false);
}

#[test]
fn native_composite_child_allocation_refusals_preserve_original_error() {
    assert_native_composite_child_storage(3, true);
}

fn source_control_fixture() -> (CadIr, CurveId, Vec<crate::directory::DirectoryEntry>, Vec<crate::parameter::ParameterRecord>) {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    let (mut ir, root, entries, records) = native_composite_source_fixture(3);
    let nurbs = crate::test_support::with_service_context(&[], |setup| {
        NurbsCurve::new(setup, 1, vec![0.0, 0.0, 1.0, 1.0],
            NurbsPoles3::Polynomial {
                points: [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
                    .map(|point| FinitePoint3::new(point).unwrap()).to_vec(),
            }, false).unwrap().unwrap()
    });
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D3").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        source_object: None,
    });
    (ir, root, entries, records)
}

fn assert_composite_control_output_custody(native: bool) {
    use cadmpeg_ir::geometry::{CompositeCurveSegment, CompositeCurveTransition};
    const CAP: u64 = 65536;
    const CHILDREN: usize = 64;
    let (mut ir, mut root, entries, records) = source_control_fixture();
    if !native {
        root = CurveId::mint("test:model:curve#root").unwrap();
        ir.model.curves[0].id = root.clone();
        ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: (0..CHILDREN).map(|_| CompositeCurveSegment {
                curve: ir.model.curves[1].id.clone(), same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }).collect::<Vec<_>>().try_into().unwrap(),
            self_intersect: None,
        });
    }
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let live = u64_from_index(2 * CHILDREN * size_of::<[DeclaredInterval; 3]>());
    for refuse_extra in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = CAP;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut outer = ctx.reserve_scoped(0, "test surviving source controls ambient scope").unwrap();
        let mut active = std::collections::BTreeSet::new();
        let controls = outer.with_storage(|| super::super::source_curve_control_intervals(
            &index, &root, (&entries, &records),
            crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
            1.0, &mut active, &ctx,
        )).unwrap().unwrap();
        assert!(active.is_empty());
        assert_eq!(controls.values.len(), 2 * CHILDREN);
        for pair in controls.values.chunks_exact(2) {
            let actual: Vec<_> = pair.iter().map(|point| point.map(|value| {
                [value.lower_bound(), value.upper_bound()]
            })).collect();
            assert_eq!(actual, [[[0.0, 0.0]; 3], [[1.0, 1.0], [0.0, 0.0], [0.0, 0.0]]]);
        }
        let free = ctx.reserve_scoped(CAP - live, "test exact live source controls").unwrap();
        drop(free);
        if refuse_extra {
            let first = match ctx.reserve_scoped(CAP - live + 1, "test source controls still live").err().unwrap() {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected exact live source control refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.limit, first.used, first.additional), (CAP, live, CAP - live + 1));
            assert_eq!(first.operation, "test source controls still live");
            drop(controls);
            drop(active);
            drop(outer);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            drop(controls);
            let free = ctx.reserve_scoped(CAP, "test source controls backing destroyed").unwrap();
            drop(free);
            drop(active);
            drop(outer);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn native_composite_controls_keep_only_surviving_aggregate_storage() {
    assert_composite_control_output_custody(true);
}

#[test]
fn solved_composite_controls_release_consumed_children_before_return() {
    assert_composite_control_output_custody(false);
}

#[test]
fn exact_source_control_allocation_refusal_keeps_original_error() {
    let (ir, _, entries, records) = source_control_fixture();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let id = &ir.model.curves[1].id;
    let prior = source_path_node_bytes() + u64_from_index(id.as_str().len());
    let allocation = u64_from_index(2 * size_of::<[DeclaredInterval; 3]>());
    let cap = prior + allocation - 1;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut active = std::collections::BTreeSet::new();
    let first = match super::super::source_curve_control_intervals(
        &index, id, (&entries, &records),
        crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
        1.0, &mut active, &ctx,
    ).err().expect("expected actual exact control allocation refusal") {
        CodecError::ResourceLimit(first) => first,
        _ => panic!("expected original resource refusal"),
    };
    assert!(active.is_empty());
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges source exact controls");
    assert_eq!((first.limit, first.used, first.additional), (cap, prior, allocation));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

fn declared_source_control_record(invalid: bool) -> crate::parameter::ParameterRecord {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    // Degree one, two poles, four knots, two weights and the [0, 1] domain.
    let values = [126, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1];
    let tokens = values.into_iter().enumerate().map(|(index, value)| Token {
        value: if invalid && index == 18 { TokenValue::Omitted } else { TokenValue::Integer(value) },
        span: 0..0,
    }).collect();
    ParameterRecord::from_test_tokens(3, 0..0, Vec::new(), values.len(), tokens, Vec::new())
}

#[test]
fn declared_source_controls_keep_scaled_values_and_exact_output_storage() {
    const CAP: u64 = 65536;
    let (ir, _, mut entries, _) = source_control_fixture();
    entries[1].entity_type = 126;
    let records = [declared_source_control_record(false)];
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let id = &ir.model.curves[1].id;
    let live = u64_from_index(2 * size_of::<[DeclaredInterval; 3]>());
    for refuse_extra in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = CAP;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut active = std::collections::BTreeSet::new();
        let controls = super::super::source_curve_control_intervals(
            &index, id, (&entries, &records),
            crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
            2.0, &mut active, &ctx,
        ).unwrap().unwrap();
        assert!(active.is_empty());
        let actual: Vec<_> = controls.values.iter().map(|point| point.map(|value| {
            [value.lower_bound(), value.upper_bound()]
        })).collect();
        // Multiplication encloses each exact product with its adjacent floats.
        let zero = [0.0_f64.next_down(), 0.0_f64.next_up()];
        let two = [2.0_f64.next_down(), 2.0_f64.next_up()];
        assert_eq!(actual, [[zero; 3], [two, zero, zero]]);
        let free = ctx.reserve_scoped(CAP - live, "test exact declared source controls").unwrap();
        drop(free);
        if refuse_extra {
            let first = match ctx.reserve_scoped(CAP - live + 1, "test declared controls remain live").err().unwrap() {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected live declared control refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "test declared controls remain live");
            assert_eq!((first.limit, first.used, first.additional), (CAP, live, CAP - live + 1));
            drop(controls);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            drop(controls);
            let free = ctx.reserve_scoped(CAP, "test declared control backing destroyed").unwrap();
            drop(free);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn partial_declared_source_controls_release_before_the_next_attempt() {
    let (ir, _, mut entries, _) = source_control_fixture();
    entries[1].entity_type = 126;
    let records = [declared_source_control_record(true)];
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let id = &ir.model.curves[1].id;
    let cap = source_path_node_bytes() + u64_from_index(id.as_str().len())
        + u64_from_index(2 * size_of::<[DeclaredInterval; 3]>());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut outer = ctx.reserve_scoped(0, "test partial declared source scope").unwrap();
    let mut active = std::collections::BTreeSet::new();
    for _ in 0..16 {
        assert!(outer.with_storage(|| super::super::source_curve_control_intervals(
            &index, id, (&entries, &records),
            crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
            2.0, &mut active, &ctx,
        )).unwrap().is_none());
        assert!(active.is_empty());
        let free = ctx.reserve_scoped(cap, "test partial declared controls destroyed").unwrap();
        drop(free);
    }
    drop(active);
    drop(outer);
    ctx.finish_session().unwrap();
}

#[test]
fn declared_source_control_allocation_refusal_keeps_original_error() {
    let (ir, _, mut entries, _) = source_control_fixture();
    entries[1].entity_type = 126;
    let records = [declared_source_control_record(false)];
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let id = &ir.model.curves[1].id;
    let prior = source_path_node_bytes() + u64_from_index(id.as_str().len());
    let allocation = u64_from_index(2 * size_of::<[DeclaredInterval; 3]>());
    let cap = prior + allocation - 1;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut active = std::collections::BTreeSet::new();
    let first = match super::super::source_curve_control_intervals(
        &index, id, (&entries, &records),
        crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
        2.0, &mut active, &ctx,
    ).err().expect("expected declared control allocation refusal") {
        CodecError::ResourceLimit(first) => first,
        _ => panic!("expected original resource refusal"),
    };
    assert!(active.is_empty());
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges Type126 declared control intervals");
    assert_eq!((first.limit, first.used, first.additional), (cap, prior, allocation));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

mod linear_boundary;
