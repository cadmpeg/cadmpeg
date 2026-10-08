// SPDX-License-Identifier: Apache-2.0
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
