// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::sketches::{SketchConstraintDefinitionInput, SketchLocus};

#[test]
fn sketch_constraint_loci_preserve_borrowed_order_and_release_storage() {
    let id = "test:model:entity#loci".try_into().unwrap();
    let definition = SketchConstraintDefinitionInput::Group { elements: vec![SketchLocus::Start(id), SketchLocus::End("test:model:entity#end".try_into().unwrap()), SketchLocus::Center("test:model:entity#center".try_into().unwrap())] };
    let SketchConstraintDefinitionInput::Group { elements } = &definition else { panic!("group fixture"); };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 256;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let loci = super::super::constraint_loci(&ctx, &definition).unwrap();
        assert_eq!(loci.len(), elements.len());
        for (actual, expected) in loci.iter().zip(elements) { assert!(std::ptr::eq(*actual, expected)); }
    }
    drop(ctx.reserve_scoped(256, "locus scopes released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_constraint_loci_preserve_first_and_later_original_refusals() {
    let definition = SketchConstraintDefinitionInput::Group { elements: vec![SketchLocus::Entity("test:model:entity#first".try_into().unwrap()), SketchLocus::Entity("test:model:entity#last".try_into().unwrap())] };
    for (dimension, cap) in [(ResourceDimension::MaterializedBytes, 0), (ResourceDimension::CollectionItems, 0), (ResourceDimension::CollectionItems, 1), (ResourceDimension::WorkUnits, 0), (ResourceDimension::WorkUnits, 2)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = super::super::constraint_loci(&ctx, &definition) else { panic!("locus storage must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn sketch_nurbs_endpoints_read_rational_poles_without_copying_all_rows() {
    let curve = crate::geometry::pcurve::PcurveNurbs::from_lanes(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], vec![crate::math::Point2::new(0.0, 0.0), crate::math::Point2::new(2.0, 8.0), crate::math::Point2::new(3.0, 4.0)], Some(vec![1.0, 2.0, 1.0]), false).unwrap();
    let geometry = crate::sketches::SketchGeometry::nurbs(curve);
    let endpoints = (crate::math::Point2::new(0.0, 0.0), crate::math::Point2::new(3.0, 4.0));
    assert_eq!(super::super::oriented_endpoints(&geometry, false), Some(endpoints));
    assert_eq!(super::super::oriented_endpoints(&geometry, true), Some((endpoints.1, endpoints.0)));
}
