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
    let points: [FinitePoint3; 21] = std::array::from_fn(|index| {
        FinitePoint3::new(Point3::new(
            f64::from(u32::try_from(index).unwrap()) * 2.0,
            0.0,
            0.0,
        ))
        .unwrap()
    });
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
    let before_slots = u64_from_index(parents_bytes + sizes_bytes + roots_bytes + members_bytes);
    let peak = before_slots + u64_from_index(cluster_bytes);
    assert!(
        parents_bytes + sizes_bytes + members_bytes + cluster_bytes
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
