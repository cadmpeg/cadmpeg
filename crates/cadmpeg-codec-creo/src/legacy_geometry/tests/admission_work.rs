// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::{
    child_index, curve_pcurve, geometry_array_elements, integer_pair, local_system_slots,
    object_id_index, real_array_values, real_scalar_array, real_vector_array, unique_primitive,
};
use std::collections::BTreeMap;
use crate::surface::{self, SurfaceKind, SurfaceRow};

/// Each step states the admitted operation before the corresponding source work.
fn work_boundaries<T: std::fmt::Debug>(
    steps: &[(&'static str, u64)],
    collection_items: u64,
    call: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
    check: impl Fn(T),
) {
    let required: u64 = steps.iter().map(|(_, cost)| cost).sum();
    for allowed in 0..=required {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowed;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = collection_items;
        if collection_items == 0 { policy.limits.max_retained_bytes = 0; }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = call(&ctx);
        let original = if allowed < required {
            let mut used = 0;
            let (operation, additional) = steps.iter().find_map(|(operation, cost)| {
                if used + cost > allowed { Some((*operation, *cost)) }
                else { used += cost; None }
            }).expect("next admitted step exceeds cap");
            let CodecError::ResourceLimit(original) = result.expect_err("present work exceeds cap")
                else { panic!("resource refusal"); };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.operation, operation);
            assert_eq!((original.used, original.additional), (used, additional));
            original
        } else {
            check(result.expect("exact present work"));
            let original = ctx.charge_work_limit(1, "after exact legacy input work")
                .expect_err("work cap consumed");
            assert_eq!((original.used, original.additional), (required, 1));
            original
        };
        assert!(matches!(call(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

fn real_runs(dimensions: Vec<u32>, runs: &[(u32, f64)]) -> RealRecord {
    ValueRecord {
        name: "crv_pnt_arr".to_string(), attribute_id: 0, scope_offset: 0,
        parent: Some(fixture_offset("curve_10")), depth: 0, offset: 810,
        payload: crate::decode::with_test_decode_ctx(|ctx| RealPayload::array(ctx, dimensions,
            runs.iter().map(|(count, value)| RealRun {
                count: *count, value: Real::from_bits(value.to_bits()),
            }).collect())).expect("run admission").expect("complete extent"),
    }
}

#[test]
fn local_system_visits_zero_run_prefix_and_stops_at_twelve_slots() {
    let record = real_runs(vec![4, 3], &[(0, 9.0), (0, 8.0), (5, 1.0), (0, 7.0),
        (7, 2.0), (0, 6.0), (0, 5.0)]);
    // Two zero prefixes, five slots, one zero separator, seven slots. No suffix visit.
    work_boundaries(&[("creo legacy local system run traversal", 1); 5], 0,
        |ctx| local_system_slots(ctx, &record),
        |result| assert_eq!(result, Some([1.0, 1.0, 1.0, 1.0, 1.0,
            2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0])));
}

#[test]
fn integer_pair_admits_typed_lookup_and_only_needed_source_runs() {
    let payload = crate::decode::with_test_decode_ctx(|ctx| IntegerPayload::array(ctx, vec![2],
        vec![IntegerRun { count: 0, value: 8 }, IntegerRun { count: 1, value: 1 },
            IntegerRun { count: 0, value: 9 }, IntegerRun { count: 1, value: -1 },
            IntegerRun { count: 0, value: 7 }])).expect("admission").expect("complete pair");
    let name = "crv_pnt_dir";
    let record = integer("curve_10", name, payload, 310);
    let parent = fixture_offset("curve_10");
    let records = BTreeMap::from([((parent, name), vec![&record])]);
    // A one-key tree compares at most one (usize, str) query.
    let lookup = u64::try_from(std::mem::size_of::<usize>() + name.len()).expect("key cost");
    let steps = [("creo legacy integer field lookup", lookup),
        ("creo legacy integer pair run traversal", 1),
        ("creo legacy integer pair run traversal", 1),
        ("creo legacy integer pair run traversal", 1),
        ("creo legacy integer pair run traversal", 1)];
    work_boundaries(&steps, 0, |ctx| integer_pair(ctx, &records, parent, name),
        |result| assert_eq!(result, Some([1, -1])));
}

#[test]
fn pcurve_endpoints_visit_compressed_runs_without_expanding_middle_samples() {
    let record = real_runs(vec![100_002, 4], &[(0, 7.0), (0, 8.0), (0, 9.0),
        (4, 1.0), (400_000, 9.0), (4, 2.0), (0, 3.0)]);
    let object = object("curve_10", "crv_array", None, ObjectPayload::Arrow);
    let topology = crate::curve::CurveTopologyRow {
        id: 10, type_byte: 0, feature_id: 7, directions: [0x01, 0xf6],
        faces: [std::num::NonZeroU32::new(100), std::num::NonZeroU32::new(200)],
        next_edges: [11, 11], offset: 10,
    };
    let name = "crv_pnt_arr";
    let records = BTreeMap::from([((object.offset, name), vec![&record])]);
    let lookup = u64::try_from(std::mem::size_of::<usize>() + name.len()).expect("key cost");
    let mut steps = vec![("creo legacy real field lookup", lookup)];
    steps.extend([("creo legacy pcurve run traversal", 1); 6]);
    work_boundaries(&steps, 0, |ctx| curve_pcurve(ctx, &object, &topology, &records),
        |result| assert_eq!(result, Some(crate::curve::PcurveEndpoints {
            curve_id: 10, faces: topology.faces,
            face_0_endpoints: [[1.0, 1.0], [2.0, 2.0]],
            face_1_endpoints: [[1.0, 1.0], [2.0, 2.0]], offset: 810,
        })));
}

#[test]
fn legacy_root_uniqueness_stops_at_second_witness() {
    let objects = [object("other", "other", None, ObjectPayload::Arrow),
        object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("root2", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("tail", "Sld_VisGeom", None, ObjectPayload::Arrow)];
    work_boundaries(&[("creo legacy geometry root selection", 1); 3], 0,
        |ctx| geometry_array_elements(ctx, &objects, &BTreeMap::new(),
            "Sld_VisGeom", "active_geom", "srf_array"), |result| assert!(result.is_none()));
}

#[test]
fn legacy_branch_uniqueness_visits_root_pass_and_stops_at_second_branch() {
    let objects = [object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("other", "other", None, ObjectPayload::Arrow),
        object("branch", "active_geom", Some("root"), ObjectPayload::Arrow),
        object("branch2", "active_geom", Some("root"), ObjectPayload::Arrow),
        object("tail", "active_geom", Some("root"), ObjectPayload::Arrow)];
    let mut steps = vec![("creo legacy geometry root selection", 1); objects.len()];
    steps.extend([("creo legacy geometry branch selection", 1); 4]);
    work_boundaries(&steps, 0, |ctx| geometry_array_elements(ctx, &objects, &BTreeMap::new(),
        "Sld_VisGeom", "active_geom", "srf_array"), |result| assert!(result.is_none()));
}

#[test]
fn legacy_array_uniqueness_stops_after_second_complete_extent() {
    let empty_array = || ObjectPayload::Array { dimensions: vec![0], elements: vec![], complete: true };
    let objects = [object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("branch", "active_geom", Some("root"), ObjectPayload::Arrow),
        object("array", "srf_array", Some("branch"), empty_array()),
        object("array2", "srf_array", Some("branch"), empty_array()),
        object("tail", "srf_array", Some("branch"), empty_array())];
    let mut steps = vec![("creo legacy geometry root selection", 1); objects.len()];
    steps.extend(vec![("creo legacy geometry branch selection", 1); objects.len()]);
    steps.extend([("creo legacy geometry complete array selection", 1); 3]);
    steps.push(("creo object array extent traversal", 1));
    steps.push(("creo legacy geometry complete array selection", 1));
    steps.push(("creo object array extent traversal", 1));
    work_boundaries(&steps, 0, |ctx| geometry_array_elements(ctx, &objects, &BTreeMap::new(),
        "Sld_VisGeom", "active_geom", "srf_array"), |result| assert!(result.is_none()));
}

#[test]
fn legacy_primitive_lookup_stops_at_second_matching_child() {
    let objects = [object("other", "other", Some("row"), ObjectPayload::Arrow),
        object("first", "srf_prim_ptr(plane)", Some("row"), ObjectPayload::Arrow),
        object("second", "srf_prim_ptr(cylinder)", Some("row"), ObjectPayload::Arrow),
        object("tail", "srf_prim_ptr(cone)", Some("row"), ObjectPayload::Arrow)];
    let row = fixture_offset("row");
    let children = BTreeMap::from([(row, objects.iter().collect::<Vec<_>>())]);
    let steps = [("creo legacy primitive child lookup",
        u64::try_from(std::mem::size_of::<usize>()).expect("fixed key cost")),
        ("creo legacy primitive child traversal", 1),
        ("creo legacy primitive child traversal", 1),
        ("creo legacy primitive child traversal", 1)];
    work_boundaries(&steps, 0, |ctx| unique_primitive(ctx, &children, row),
        |result| assert!(result.is_none()));
}

#[test]
fn legacy_array_element_admits_missing_key_lookup_after_present_element_visit() {
    let id = "missing_long_element_id";
    let objects = [object("root", "Sld_VisGeom", None, ObjectPayload::Arrow),
        object("branch", "active_geom", Some("root"), ObjectPayload::Arrow),
        object("array", "srf_array", Some("branch"), ObjectPayload::Array {
            dimensions: vec![1], elements: vec![id.to_string()], complete: true,
        })];
    let object_ids = BTreeMap::from([("other".to_string(), &objects[0])]);
    let mut steps = vec![("creo legacy geometry root selection", 1); objects.len()];
    steps.extend(vec![("creo legacy geometry branch selection", 1); objects.len()]);
    steps.extend([("creo legacy geometry complete array selection", 1); 3]);
    steps.push(("creo object array extent traversal", 1));
    steps.push(("creo legacy array element traversal", 1));
    // The object fixture converts element labels to their native node IDs.
    let ObjectPayload::Array { elements, .. } = &objects[2].payload else {
        panic!("array fixture");
    };
    steps.push(("creo legacy array element lookup",
        u64::try_from(elements[0].len()).expect("native query key bytes")));
    work_boundaries(&steps, 1, |ctx| geometry_array_elements(ctx, &objects, &object_ids,
        "Sld_VisGeom", "active_geom", "srf_array"), |result| assert!(result.is_none()));
}

#[test]
fn child_index_visits_present_parentless_objects_without_allocating() {
    let objects = [object("first", "other", None, ObjectPayload::Arrow),
        object("second", "other", None, ObjectPayload::Arrow),
        object("third", "other", None, ObjectPayload::Arrow)];
    work_boundaries(&[("creo legacy child index traversal", 1); 3], 0,
        |ctx| child_index(ctx, &objects), |result| assert!(result.is_empty()));
}

#[test]
fn object_index_refuses_visit_before_id_copy_and_keeps_original_retained_refusal() {
    let objects = [object("first", "other", None, ObjectPayload::Arrow)];
    for allowed in [0, 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowed;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let CodecError::ResourceLimit(original) = object_id_index(&ctx, &objects)
            .expect_err("visit or ID retained storage exceeds cap") else { panic!("resource refusal"); };
        assert_eq!(original.operation, if allowed == 0 { "creo legacy object index traversal" }
            else { "creo legacy object index IDs" });
        assert_eq!(original.dimension, if allowed == 0 { ResourceDimension::WorkUnits }
            else { ResourceDimension::RetainedBytes });
        assert_eq!(original.used, 0);
        if allowed == 0 { assert_eq!(original.additional, 1); }
        assert!(matches!(object_id_index(&ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn real_vector_expansion_has_one_output_buffer_and_visits_all_actual_runs_and_values() {
    let mut record = real_runs(vec![3, 3], &[(0, 9.0), (2, 1.0), (0, 8.0), (7, 2.0), (0, 7.0)]);
    record.name = "i_points".to_string();
    let parent = fixture_offset("curve_10");
    let name = record.name.as_str();
    let records = BTreeMap::from([((parent, name), vec![&record])]);
    let lookup = u64::try_from(std::mem::size_of::<usize>() + name.len()).expect("key bytes");
    let mut steps = vec![("creo legacy real field lookup", lookup),
        ("creo legacy real vector run traversal", 1),
        ("creo legacy real vector run traversal", 1)];
    steps.extend([("creo legacy real vector element expansion", 1); 2]);
    steps.extend([("creo legacy real vector run traversal", 1); 2]);
    steps.extend([("creo legacy real vector element expansion", 1); 7]);
    steps.push(("creo legacy real vector run traversal", 1));
    // Three output vectors; no nine-element intermediate scalar allocation.
    work_boundaries(&steps, 3, |ctx| real_vector_array(ctx, &records, parent, name),
        |result| assert_eq!(result, Some(vec![[1.0, 1.0, 2.0], [2.0; 3], [2.0; 3]])));
}

#[test]
fn real_scalar_expansion_visits_zero_runs_and_each_produced_value() {
    let mut record = real_runs(vec![3], &[(0, 9.0), (1, 1.0), (0, 8.0), (2, 2.0), (0, 7.0)]);
    record.name = "u_params".to_string();
    let parent = fixture_offset("curve_10");
    let name = record.name.as_str();
    let records = BTreeMap::from([((parent, name), vec![&record])]);
    let lookup = u64::try_from(std::mem::size_of::<usize>() + name.len()).expect("key bytes");
    let steps = [("creo legacy real field lookup", lookup),
        ("creo legacy real array run traversal", 1), ("creo legacy real array run traversal", 1),
        ("creo legacy real array element expansion", 1),
        ("creo legacy real array run traversal", 1), ("creo legacy real array run traversal", 1),
        ("creo legacy real array element expansion", 1), ("creo legacy real array element expansion", 1),
        ("creo legacy real array run traversal", 1)];
    work_boundaries(&steps, 3, |ctx| real_scalar_array(ctx, &records, parent, name),
        |result| assert_eq!(result, Some(vec![1.0, 2.0, 2.0])));
}

#[test]
fn empty_and_fixed_legacy_helper_returns_are_free_and_preserve_seed_refusal() {
    let integer_records = BTreeMap::new();
    let real_records = BTreeMap::new();
    let children = BTreeMap::new();
    let objects = BTreeMap::new();
    let scalar = real_scalar("curve_10", "scalar", 1.0, 1);
    let row = object("row", "other", None, ObjectPayload::Arrow);
    let topology = crate::curve::CurveTopologyRow {
        id: 10, type_byte: 0, feature_id: 7, directions: [0x01, 0xf6], faces: [None; 2],
        next_edges: [0; 2], offset: 10,
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
        if refused { ctx.charge_work_limit(1, "empty legacy helpers seed").expect_err("zero work"); }
        let results = [
            object_id_index(&ctx, &[]).map(|r| r.is_empty()),
            child_index(&ctx, &[]).map(|r| r.is_empty()),
            geometry_array_elements(&ctx, &[], &objects, "Sld_VisGeom", "active_geom", "srf_array").map(|r| r.is_none()),
            super::super::integer_record(&ctx, &integer_records, 0, "crv_id").map(|r| r.is_none()),
            super::super::integer_field(&ctx, &integer_records, 0, "crv_id").map(|r| r.is_none()),
            integer_pair(&ctx, &integer_records, 0, "crv_pnt_dir").map(|r| r.is_none()),
            super::super::real_record(&ctx, &real_records, 0, "local_sys").map(|r| r.is_none()),
            super::super::real_scalar(&ctx, &real_records, 0, "radius").map(|r| r.is_none()),
            super::super::curve_topology_row(&ctx, &row, &integer_records).map(|r| r.is_none()),
            super::super::surface_row(&ctx, &row, &integer_records).map(|r| r.is_none()),
            curve_pcurve(&ctx, &row, &topology, &real_records).map(|r| r.is_none()),
            unique_primitive(&ctx, &children, 0).map(|r| r.is_none()),
            super::super::surface_carrier(&ctx, &row, &SurfaceRow {
                id: 1, kind: SurfaceKind::Plane, feature_id: 7, reversed: false,
                boundary_type: surface::BoundaryType::Code00, next_surface: 0, offset: 0,
            }, &children, &real_records, LegacySurfaceNamespace::Visible).map(|r| r.is_none()),
            local_system_slots(&ctx, &scalar).map(|r| r.is_none()),
            real_array_values(&ctx, &scalar).map(|r| r.is_none()),
            real_vector_array(&ctx, &real_records, 0, "i_points").map(|r| r.is_none()),
            real_scalar_array(&ctx, &real_records, 0, "u_params").map(|r| r.is_none()),
        ];
        for result in results {
            if refused { let original = ctx.resource_refusal().expect("seed refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else { assert!(result.expect("no input-sized work")); }
        }
    }
}
