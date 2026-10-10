// SPDX-License-Identifier: Apache-2.0

use super::super::{
    child_index, curve_pcurve, geometry_array_elements, integer_pair, local_system_slots,
    object_id_index, real_array_values, real_scalar_array, real_vector_array, unique_primitive,
};
use super::super::{geometry_field, UniqueRecord};
use super::*;
use crate::surface::{self, SurfaceKind, SurfaceRow};
use std::collections::{BTreeMap, HashMap};

fn work_boundaries<T>(
    operations: &[&str],
    call: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
    check: impl Fn(T),
) {
    check(crate::test_support::assert_work_boundaries(
        operations, call,
    ));
}

fn real_runs(dimensions: Vec<u32>, runs: &[(u32, f64)]) -> RealRecord {
    ValueRecord {
        name: "crv_pnt_arr".to_string(),
        attribute_id: 0,
        scope_offset: 0,
        parent: Some(fixture_offset("curve_10")),
        depth: 0,
        offset: 810,
        payload: crate::decode::with_test_decode_ctx(|ctx| {
            RealPayload::array(
                ctx,
                dimensions,
                runs.iter()
                    .map(|(count, value)| RealRun {
                        count: *count,
                        value: Real::from_bits(value.to_bits()),
                    })
                    .collect(),
            )
        })
        .expect("run admission")
        .expect("complete extent"),
    }
}

#[test]
fn local_system_visits_zero_run_prefix_and_stops_at_twelve_slots() {
    let record = real_runs(
        vec![4, 3],
        &[
            (0, 9.0),
            (0, 8.0),
            (5, 1.0),
            (0, 7.0),
            (7, 2.0),
            (0, 6.0),
            (0, 5.0),
        ],
    );
    // Two zero prefixes, five slots, one zero separator, seven slots. No suffix visit.
    work_boundaries(
        &["creo legacy local system runs"],
        |ctx| local_system_slots(ctx, &record),
        |result| {
            assert_eq!(
                result,
                Some([1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0])
            );
        },
    );
}

#[test]
fn integer_pair_admits_typed_lookup_and_only_needed_source_runs() {
    let payload = crate::decode::with_test_decode_ctx(|ctx| {
        IntegerPayload::array(
            ctx,
            vec![2],
            vec![
                IntegerRun { count: 0, value: 8 },
                IntegerRun { count: 1, value: 1 },
                IntegerRun { count: 0, value: 9 },
                IntegerRun {
                    count: 1,
                    value: -1,
                },
                IntegerRun { count: 0, value: 7 },
            ],
        )
    })
    .expect("admission")
    .expect("complete pair");
    let name = "crv_pnt_dir";
    let record = integer("curve_10", name, payload, 310);
    let parent = fixture_offset("curve_10");
    let records = HashMap::from([(
        (parent, geometry_field(name).expect("known field")),
        UniqueRecord::One(&record),
    )]);
    work_boundaries(
        &["creo legacy integer pair runs"],
        |ctx| integer_pair(ctx, &records, parent, name),
        |result| assert_eq!(result, Some([1, -1])),
    );
}

#[test]
fn pcurve_endpoints_visit_compressed_runs_without_expanding_middle_samples() {
    let record = real_runs(
        vec![100_002, 4],
        &[
            (0, 7.0),
            (0, 8.0),
            (0, 9.0),
            (4, 1.0),
            (400_000, 9.0),
            (4, 2.0),
            (0, 3.0),
        ],
    );
    let object = object("curve_10", "crv_array", None, ObjectPayload::Arrow);
    let topology = crate::curve::CurveTopologyRow {
        id: 10,
        type_byte: 0,
        feature_id: 7,
        directions: [0x01, 0xf6],
        faces: [
            std::num::NonZeroU32::new(100),
            std::num::NonZeroU32::new(200),
        ],
        next_edges: [11, 11],
        offset: 10,
    };
    let name = "crv_pnt_arr";
    let records = HashMap::from([(
        (object.offset, geometry_field(name).expect("known field")),
        UniqueRecord::One(&record),
    )]);
    work_boundaries(
        &[
            "creo legacy pcurve first endpoint runs",
            "creo legacy pcurve last endpoint runs",
        ],
        |ctx| curve_pcurve(ctx, &object, &topology, &records),
        |result| {
            assert_eq!(
                result,
                Some(crate::curve::PcurveEndpoints {
                    curve_id: 10,
                    faces: topology.faces,
                    face_0_endpoints: [[1.0, 1.0], [2.0, 2.0]],
                    face_1_endpoints: [[1.0, 1.0], [2.0, 2.0]],
                    offset: 810,
                })
            );
        },
    );
}

#[test]
fn legacy_root_uniqueness_stops_at_second_witness() {
    let objects = [
        object("other", "other", None, ObjectPayload::Arrow),
        object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("root2", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("tail", "Sld_VisGeom", None, ObjectPayload::Arrow),
    ];
    work_boundaries(
        &["creo legacy geometry root search"],
        |ctx| {
            geometry_array_elements(
                ctx,
                &objects,
                &BTreeMap::new(),
                "Sld_VisGeom",
                "active_geom",
                "srf_array",
            )
        },
        |result| assert!(result.is_none()),
    );
}

#[test]
fn legacy_branch_uniqueness_visits_root_pass_and_stops_at_second_branch() {
    let objects = [
        object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("other", "other", None, ObjectPayload::Arrow),
        object("branch", "active_geom", Some("root"), ObjectPayload::Arrow),
        object("branch2", "active_geom", Some("root"), ObjectPayload::Arrow),
        object("tail", "active_geom", Some("root"), ObjectPayload::Arrow),
    ];
    work_boundaries(
        &[
            "creo legacy geometry root search",
            "creo legacy geometry branch search",
        ],
        |ctx| {
            geometry_array_elements(
                ctx,
                &objects,
                &BTreeMap::new(),
                "Sld_VisGeom",
                "active_geom",
                "srf_array",
            )
        },
        |result| assert!(result.is_none()),
    );
}

#[test]
fn legacy_array_uniqueness_stops_after_second_complete_extent() {
    let empty_array = || ObjectPayload::Array {
        dimensions: vec![0],
        elements: vec![],
        complete: true,
    };
    let objects = [
        object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("branch", "active_geom", Some("root"), ObjectPayload::Arrow),
        object("array", "srf_array", Some("branch"), empty_array()),
        object("array2", "srf_array", Some("branch"), empty_array()),
        object("tail", "srf_array", Some("branch"), empty_array()),
    ];
    work_boundaries(
        &[
            "creo legacy geometry root search",
            "creo legacy geometry branch search",
            "creo legacy geometry array search",
        ],
        |ctx| {
            geometry_array_elements(
                ctx,
                &objects,
                &BTreeMap::new(),
                "Sld_VisGeom",
                "active_geom",
                "srf_array",
            )
        },
        |result| assert!(result.is_none()),
    );
}

#[test]
fn legacy_primitive_lookup_stops_at_second_matching_child() {
    let objects = [
        object("other", "other", Some("row"), ObjectPayload::Arrow),
        object(
            "first",
            "srf_prim_ptr(plane)",
            Some("row"),
            ObjectPayload::Arrow,
        ),
        object(
            "second",
            "srf_prim_ptr(cylinder)",
            Some("row"),
            ObjectPayload::Arrow,
        ),
        object(
            "tail",
            "srf_prim_ptr(cone)",
            Some("row"),
            ObjectPayload::Arrow,
        ),
    ];
    let row = fixture_offset("row");
    let children =
        crate::decode::with_test_decode_ctx(|ctx| child_index(ctx, &objects)).expect("child index");
    assert!(unique_primitive(&children, row).is_none());
}

#[test]
fn legacy_array_element_admits_missing_key_lookup_after_present_element_visit() {
    let id = "missing_long_element_id";
    let objects = [
        object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("branch", "active_geom", Some("root"), ObjectPayload::Arrow),
        object(
            "array",
            "srf_array",
            Some("branch"),
            ObjectPayload::Array {
                dimensions: vec![1],
                elements: vec![id.to_string()],
                complete: true,
            },
        ),
    ];
    let object_ids = BTreeMap::from([("other".to_string(), &objects[0])]);
    work_boundaries(
        &[
            "creo legacy geometry root search",
            "creo legacy geometry branch search",
            "creo legacy geometry array search",
            "creo legacy geometry element search",
            "creo legacy object ID lookup",
        ],
        |ctx| {
            geometry_array_elements(
                ctx,
                &objects,
                &object_ids,
                "Sld_VisGeom",
                "active_geom",
                "srf_array",
            )
        },
        |result| assert!(result.is_none()),
    );
}

#[test]
fn child_index_visits_present_parentless_objects_without_allocating() {
    let objects = [
        object("first", "other", None, ObjectPayload::Arrow),
        object("second", "other", None, ObjectPayload::Arrow),
        object("third", "other", None, ObjectPayload::Arrow),
    ];
    work_boundaries(
        &["creo legacy primitive index rows"],
        |ctx| child_index(ctx, &objects),
        |result| assert!(result.is_empty()),
    );
}

#[test]
fn object_index_refuses_visit_before_id_copy_and_keeps_original_retained_refusal() {
    let objects = [object("first", "other", None, ObjectPayload::Arrow)];
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo legacy object index IDs",
        |ctx| object_id_index(ctx, &objects),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(ref refusal) if refusal.operation == "creo legacy object index IDs")
    );
    crate::test_support::assert_work_boundaries(&["creo legacy object index rows"], |ctx| {
        object_id_index(ctx, &objects)
    });
}

#[test]
fn real_vector_expansion_has_one_output_buffer_and_visits_all_actual_runs_and_values() {
    let mut record = real_runs(
        vec![3, 3],
        &[(0, 9.0), (2, 1.0), (0, 8.0), (7, 2.0), (0, 7.0)],
    );
    record.name = "i_points".to_string();
    let parent = fixture_offset("curve_10");
    let name = record.name.as_str();
    let records = HashMap::from([(
        (parent, geometry_field(name).expect("known field")),
        UniqueRecord::One(&record),
    )]);
    work_boundaries(
        &[
            "creo legacy real vector run traversal",
            "creo legacy real vector element expansion",
        ],
        |ctx| real_vector_array(ctx, &records, parent, name),
        |result| assert_eq!(result, Some(vec![[1.0, 1.0, 2.0], [2.0; 3], [2.0; 3]])),
    );
}

#[test]
fn real_scalar_expansion_visits_zero_runs_and_each_produced_value() {
    let mut record = real_runs(vec![3], &[(0, 9.0), (1, 1.0), (0, 8.0), (2, 2.0), (0, 7.0)]);
    record.name = "u_params".to_string();
    let parent = fixture_offset("curve_10");
    let name = record.name.as_str();
    let records = HashMap::from([(
        (parent, geometry_field(name).expect("known field")),
        UniqueRecord::One(&record),
    )]);
    work_boundaries(
        &[
            "creo legacy real array runs",
            "creo legacy real array elements",
        ],
        |ctx| real_scalar_array(ctx, &records, parent, name),
        |result| assert_eq!(result, Some(vec![1.0, 2.0, 2.0])),
    );
}

#[test]
fn empty_and_fixed_legacy_helper_returns_are_free_and_preserve_seed_refusal() {
    let integer_records = HashMap::new();
    let real_records = HashMap::new();
    let children = HashMap::new();
    let objects = BTreeMap::new();
    let scalar = real_scalar("curve_10", "scalar", 1.0, 1);
    let row = object("row", "other", None, ObjectPayload::Arrow);

    let topology = crate::curve::CurveTopologyRow {
        id: 10,
        type_byte: 0,
        feature_id: 7,
        directions: [0x01, 0xf6],
        faces: [None; 2],
        next_edges: [0; 2],
        offset: 10,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for refused in [false, true] {
        if !refused {
            assert!(integer_pair(&ctx, &integer_records, 0, "crv_pnt_dir")
                .expect("missing field")
                .is_none());
            assert!(curve_pcurve(&ctx, &row, &topology, &real_records)
                .expect("missing field")
                .is_none());
            assert!(real_vector_array(&ctx, &real_records, 0, "i_points")
                .expect("missing field")
                .is_none());
            assert!(real_scalar_array(&ctx, &real_records, 0, "u_params")
                .expect("missing field")
                .is_none());
        }
        if refused {
            ctx.charge_work_limit(1, "empty legacy helpers seed")
                .expect_err("zero work");
        }
        assert!(super::super::integer_record(&integer_records, 0, "crv_id").is_none());
        assert!(super::super::integer_field(&integer_records, 0, "crv_id").is_none());
        assert!(super::super::real_record(&real_records, 0, "local_sys").is_none());
        assert!(super::super::real_scalar(&real_records, 0, "radius").is_none());
        assert!(super::super::surface_row(&row, &integer_records).is_none());
        assert!(super::super::curve_topology_row(&row, &integer_records, [1, -1]).is_none());
        assert!(unique_primitive(&children, 0).is_none());
        assert!(super::super::surface_carrier(
            &row,
            &SurfaceRow {
                id: 1,
                kind: SurfaceKind::Plane,
                feature_id: 7,
                reversed: false,
                boundary_type: surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            },
            [0.0; 12],
            &real_records,
            LegacySurfaceNamespace::Visible
        )
        .is_none());
        let results = [
            object_id_index(&ctx, &[]).map(|r| r.is_empty()),
            child_index(&ctx, &[]).map(|r| r.is_empty()),
            geometry_array_elements(
                &ctx,
                &[],
                &objects,
                "Sld_VisGeom",
                "active_geom",
                "srf_array",
            )
            .map(|r| r.is_none()),
            local_system_slots(&ctx, &scalar).map(|r| r.is_none()),
            real_array_values(&ctx, &scalar).map(|r| r.is_none()),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seed refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                assert!(result.expect("no input-sized work"));
            }
        }
    }
}
