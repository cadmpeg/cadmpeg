// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::{LegacyGeometryIndex, LegacyGeometryScan};
use std::collections::BTreeMap;

#[test]
fn empty_legacy_geometry_routes_are_free_and_keep_original_refusal() {
    let persistence = Persistence::default();
    let object_ids = BTreeMap::new();
    let children = BTreeMap::new();
    let integer_fields = BTreeMap::new();
    let real_fields = BTreeMap::new();
    let index = LegacyGeometryIndex {
        objects: &[], object_ids: &object_ids, children: &children,
        integer_fields: &integer_fields, real_fields: &real_fields,
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
        if refused { ctx.charge_work_limit(1, "legacy geometry seed").expect_err("zero cap"); }
        let results = [
            super::super::scan(&ctx, &persistence).map(|v| v == LegacyGeometryScan::default()),
            super::super::namespace(&ctx, &index, "Sld_VisGeom", "active_geom",
                LegacySurfaceNamespace::Visible).map(|v| v.0.is_empty() && v.1.is_empty()),
            super::super::curve_namespace(&ctx, &[], &object_ids, &integer_fields, &real_fields)
                .map(|v| v.0.is_empty() && v.1.is_empty()),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seed refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else { assert!(result.expect("no input traversal or ordering")); }
        }
        assert_eq!(ctx.resource_refusal().is_some(), refused);
    }
}

#[test]
fn singleton_legacy_surface_namespace_uses_no_ordering_scratch() {
    let persistence = cylinder_persistence(false);
    let (object_ids, children, integer_fields, real_fields) =
        crate::decode::with_test_decode_ctx(|ctx| {
            let object_ids = super::super::object_id_index(ctx, &persistence.objects)?;
            let children = super::super::child_index(ctx, &persistence.objects)?;
            let mut integer_fields = BTreeMap::new();
            let mut real_fields = BTreeMap::new();
            crate::legacy::value_index(ctx, &persistence.integer_values.rows, &mut integer_fields)?;
            crate::legacy::value_index(ctx, &persistence.real_values.rows, &mut real_fields)?;
            Ok::<_, CodecError>((object_ids, children, integer_fields, real_fields))
        }).expect("fixture indexes");
    let index = LegacyGeometryIndex {
        objects: &persistence.objects, object_ids: &object_ids, children: &children,
        integer_fields: &integer_fields, real_fields: &real_fields,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    // One element reference, one row and one carrier. No sort index slots.
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (rows, carriers) = super::super::namespace(&ctx, &index,
        "Sld_VisGeom", "active_geom", LegacySurfaceNamespace::Visible)
        .expect("one surface needs no ordering scratch");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 42);
    assert_eq!(carriers.len(), 1);
    assert_eq!(carriers[0].namespace, LegacySurfaceNamespace::Visible);
    assert_eq!(carriers[0].surface_id, 42);
    assert_eq!(carriers[0].geometry, LegacySurfaceGeometry::Cylinder {
        frame: frame([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        radius: PositiveLength::new(2.0).expect("positive radius"),
    });
    let original = ctx.reserve_scoped_limit(1, "after singleton legacy surface")
        .expect_err("no materialized storage remains");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::MaterializedBytes, 0, 1));
    assert!(matches!(super::super::namespace(&ctx, &index,
        "Sld_VisGeom", "active_geom", LegacySurfaceNamespace::Visible),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn singleton_legacy_curve_namespace_uses_no_ordering_or_dedup_scratch() {
    let mut persistence = topology_persistence();
    persistence.objects.retain(|object| object.offset != fixture_offset("curve_11"));
    let array = persistence.objects.iter_mut().find(|object| object.name == "crv_array"
        && matches!(object.payload, ObjectPayload::Array { .. })).expect("curve array");
    let ObjectPayload::Array { dimensions, elements, .. } = &mut array.payload else {
        panic!("array fixture");
    };
    dimensions[0] = 1;
    elements.truncate(1);
    let (object_ids, integer_fields, real_fields) = crate::decode::with_test_decode_ctx(|ctx| {
        let object_ids = super::super::object_id_index(ctx, &persistence.objects)?;
        let mut integer_fields = BTreeMap::new();
        let mut real_fields = BTreeMap::new();
        crate::legacy::value_index(ctx, &persistence.integer_values.rows, &mut integer_fields)?;
        crate::legacy::value_index(ctx, &persistence.real_values.rows, &mut real_fields)?;
        Ok::<_, CodecError>((object_ids, integer_fields, real_fields))
    }).expect("fixture indexes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (rows, pcurves) = super::super::curve_namespace(&ctx, &persistence.objects,
        &object_ids, &integer_fields, &real_fields).expect("single curve has no ordering scratch");
    assert_eq!(rows, [crate::curve::CurveTopologyRow {
        id: 10, type_byte: 0, feature_id: 7, directions: [0x01, 0xf6],
        faces: [std::num::NonZeroU32::new(100), std::num::NonZeroU32::new(200)],
        next_edges: [11, 11], offset: 10,
    }]);
    assert_eq!(pcurves, [crate::curve::PcurveEndpoints {
        curve_id: 10,
        faces: [std::num::NonZeroU32::new(100), std::num::NonZeroU32::new(200)],
        face_0_endpoints: [[0.0, 1.0], [4.0, 5.0]],
        face_1_endpoints: [[2.0, 3.0], [6.0, 7.0]], offset: 810,
    }]);
    let original = ctx.reserve_scoped_limit(1, "after singleton legacy curve")
        .expect_err("no ordering storage remains");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::MaterializedBytes, 0, 1));
    assert!(matches!(super::super::curve_namespace(&ctx, &[],
        &object_ids, &integer_fields, &real_fields),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}
