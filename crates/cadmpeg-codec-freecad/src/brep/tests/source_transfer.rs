// SPDX-License-Identifier: Apache-2.0
//! Source association ownership during geometry transfer.

use super::allocation_tests::{
    extrusion_surface, shape_payload, shape_property, trimmed_curve, trimmed_surface,
};
use crate::brep::{transfer_text_geometry, BinaryTopologyVersion, ShapePayload};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn geometry_without_curve_or_surface_consumers_skips_owner_storage() {
    let property = shape_property("<Property/>");
    let mut empty = shape_payload();
    empty.payload = ShapePayload::Empty;
    let mut topology_only = shape_payload();
    let ShapePayload::Text { facts, .. } = &mut topology_only.payload else {
        panic!("text payload fixture")
    };
    facts.curves.clear();
    facts.surfaces.clear();
    facts.triangulations.push(crate::brep::triangulation::TextTriangulation::try_new(
        0.0, vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
        ], None, vec![[1, 2, 3]], None,
    ).expect("display triangulation"));
    let mut binary_topology_only = shape_payload();
    binary_topology_only.payload = ShapePayload::Binary {
        version: BinaryTopologyVersion::V1, facts: facts.clone(),
    };
    for payloads in [
        &[][..], std::slice::from_ref(&empty), std::slice::from_ref(&topology_only),
        std::slice::from_ref(&binary_topology_only),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(payloads.len());
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (curves, surfaces) = transfer_text_geometry(
            &ctx, payloads, std::slice::from_ref(&property),
        ).expect("no geometry consumer");
        assert!(curves.curves.is_empty());
        assert!(curves.procedural.is_empty());
        assert!(surfaces.surfaces.is_empty());
        assert!(surfaces.procedural.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
        let CodecError::ResourceLimit(original) = ctx.charge_work(
            policy.limits.max_work_units + 1, "prior geometry refusal",
        ).expect_err("work limit") else {
            panic!("resource refusal")
        };
        let Err(CodecError::ResourceLimit(repeated)) = transfer_text_geometry(
            &ctx, payloads, std::slice::from_ref(&property),
        ) else {
            panic!("original refusal must propagate")
        };
        assert_eq!(repeated, original);
    }
}

#[test]
fn recursive_geometry_preserves_mapped_and_fallback_source_objects() {
    let mut payload = shape_payload();
    let ShapePayload::Text { facts, .. } = &mut payload.payload else {
        panic!("text payload fixture")
    };
    facts.curves = vec![trimmed_curve()];
    facts.surfaces = vec![trimmed_surface(), extrusion_surface()];
    let property = shape_property("<Property/>");
    let mut conflicting = property.clone();
    conflicting.owner = "fcstd:native:object#Other".into();
    let duplicate = [property.clone(), conflicting];
    for (properties, expected) in [
        (std::slice::from_ref(&property), property.owner.as_str()),
        (&[][..], payload.property.as_str()),
        (duplicate.as_slice(), payload.property.as_str()),
    ] {
        crate::test_support::with_service_context(&[], |ctx| {
            let (curves, surfaces) = transfer_text_geometry(
                ctx, std::slice::from_ref(&payload), properties,
            ).expect("recursive transfer");
            assert_eq!(curves.curves.len(), 3);
            assert_eq!(surfaces.surfaces.len(), 3);
            for association in curves.curves.iter().map(|curve| curve.source_object.as_ref())
                .chain(surfaces.surfaces.iter().map(|surface| surface.source_object.as_ref()))
            {
                let association = association.expect("source object on every geometry record");
                assert_eq!(association.format, cadmpeg_ir::CodecFormat::Fcstd);
                assert_eq!(association.object_id.as_str(), expected);
                assert_eq!(association.name, None);
                assert_eq!(association.color, None);
                assert_eq!(association.visible, None);
                assert_eq!(association.layer, None);
                assert!(association.instance_path.is_empty());
            }
        });
    }
}

#[test]
fn geometry_source_template_uses_scoped_storage() {
    let payload = shape_payload();
    let property = shape_property("<Property/>");
    crate::test_support::refusal_at(
        ResourceDimension::MaterializedBytes, &[], "FreeCAD geometry source object",
        |ctx| transfer_text_geometry(ctx, std::slice::from_ref(&payload), std::slice::from_ref(&property)),
    );
}

#[test]
fn geometry_transfer_stops_before_unvisited_curve_and_payload_suffixes() {
    let mut first = shape_payload();
    let ShapePayload::Text { facts, .. } = &mut first.payload else {
        panic!("text payload fixture")
    };
    facts.curves = vec![crate::brep::TextCurve::Line {
        origin: cadmpeg_ir::features::FinitePoint3::ZERO,
        direction: cadmpeg_ir::features::FiniteVector3::ZERO,
    }];
    facts.surfaces.clear();
    let property = shape_property("<Property/>");
    let assert_first_error = |error| {
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "LineCurve.direction must have unit length"));
    };
    let visited_work = crate::test_support::with_service_context(&[], |ctx| {
        assert_first_error(transfer_text_geometry(
            ctx, std::slice::from_ref(&first), std::slice::from_ref(&property),
        ).err().expect("nonunit direction"));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(
            u64::MAX, "measure visited geometry work",
        ).expect_err("work overflow") else {
            panic!("work refusal")
        };
        limit.used
    });
    let mut extended = first;
    let ShapePayload::Text { facts, .. } = &mut extended.payload else {
        panic!("text payload fixture")
    };
    facts.curves.extend((0..128).map(|_| trimmed_curve()));
    let mut payloads = vec![extended];
    payloads.extend((0..128).map(|_| shape_payload()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = visited_work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert_first_error(transfer_text_geometry(
        &ctx, &payloads, std::slice::from_ref(&property),
    ).err().expect("first malformed curve"));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn shape_payloads_without_exact_shape_consumers_skip_entry_storage() {
    let mut property = shape_property("<Property/>");
    property.type_name = "App::PropertyString".into();
    let entry = crate::test_support::entry_record(
        "fcstd:native:entry#Unused.brp".into(), "Unused.brp".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary, Vec::new(), vec![1; 4096],
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(crate::brep::parse_payloads(
        &ctx, std::slice::from_ref(&property), std::slice::from_ref(&entry),
    ).expect("no exact-shape consumer").is_empty());
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "prior shape payload refusal")
        .expect_err("work limit") else {
            panic!("resource refusal")
        };
    assert!(matches!(crate::brep::parse_payloads(&ctx, &[], std::slice::from_ref(&entry)),
        Err(CodecError::ResourceLimit(repeated)) if repeated == original));
}

#[test]
fn text_token_allocation_refuses_before_unvisited_bytes() {
    let text = format!("a {}", "unvisited".repeat(4096));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 2;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let Err(CodecError::ResourceLimit(limit)) = crate::brep::text_tokens(&ctx, &text) else {
        panic!("first token allocation must refuse")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "FreeCAD text B-rep tokens");
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(crate::brep::text_tokens(&ctx, ""),
        Err(CodecError::ResourceLimit(repeated)) if repeated == limit));
}

#[test]
fn binary_root_record_admits_its_collection_slot_and_storage() {
    let mut bytes = b"Open CASCADE Topology V1\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\n".to_vec();
    bytes.extend_from_slice(&[0, 1, 0, 0, 1, 0, 0, 0, b'*']);
    for value in [1_i32, 0, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for dimension in [ResourceDimension::CollectionItems, ResourceDimension::RetainedBytes] {
        crate::test_support::refusal_at(dimension, &bytes, "FreeCAD binary root records", |ctx| {
            crate::brep::parse_binary_prefix(ctx, &bytes)
        });
    }
    crate::test_support::with_service_context(&bytes, |ctx| {
        let (facts, version) = crate::brep::parse_binary_prefix(ctx, &bytes).expect("compound root");
        assert_eq!(version, BinaryTopologyVersion::V1);
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(facts.roots[0].shape, 1);
        assert_eq!(facts.roots[0].location, crate::brep::LocationRef::Identity);
        assert_eq!(facts.roots[0].orientation, crate::brep::TextOrientation::Forward);
    });
    let root_start = bytes.len() - 12;
    for (chunk, value) in bytes[root_start..].chunks_exact_mut(4).zip([-1_i32; 3]) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let (facts, _) = crate::brep::parse_binary_prefix(&ctx, &bytes).expect("sentinel root");
    assert!(facts.roots.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn shape_payload_consumer_admits_entry_index_before_entry_access() {
    let property = shape_property("<Property><Part file=\"empty.brp\"/></Property>");
    let entry = crate::test_support::entry_record(
        "fcstd:native:entry#empty.brp".into(), "empty.brp".into(),
        cadmpeg_core::container::ContainerRole::Brep, Vec::new(), Vec::new(),
    );
    crate::test_support::assert_collection_refusal_at(
        &[], "FreeCAD shape entry index", |ctx| crate::brep::parse_payloads(
            ctx, std::slice::from_ref(&property), std::slice::from_ref(&entry),
        ),
    );
    crate::test_support::with_service_context(&[], |ctx| {
        let payloads = crate::brep::parse_payloads(
            ctx, std::slice::from_ref(&property), std::slice::from_ref(&entry),
        ).expect("shape-entry consumer");
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].property, property.id);
        assert_eq!(payloads[0].entry, entry.id());
        assert!(matches!(payloads[0].payload, crate::brep::ShapePayload::Empty));
    });
}
