// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn topology_regeneration_references_preserve_temporary_storage_and_work_refusals() {
    let definition = crate::features::FeatureOperation::DerivedGeometry {
        source: crate::features::FeatureId::mint("test:model:feature#source").unwrap(),
    };
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = super::super::regeneration_references(&ctx, &definition) else { panic!("temporary reference collection must refuse"); };
        assert_eq!(original.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}

#[test]
fn topology_regeneration_references_keep_byte_order_uniqueness_and_release_storage() {
    use crate::features::patterns::{PatternKind, PatternSeed, PatternTransform};
    let first = crate::features::FeatureId::mint("test:model:feature#first").unwrap();
    let second = crate::features::FeatureId::mint("test:model:feature#second").unwrap();
    let definition = crate::features::FeatureOperation::Pattern {
        seeds: vec![PatternSeed::Feature(second.clone()), PatternSeed::Feature(first.clone()), PatternSeed::Feature(second.clone())],
        pattern: PatternKind::new(PatternTransform::Mirror {
            plane_origin: crate::features::FinitePoint3::new(crate::math::Point3::new(0.0, 0.0, 0.0)).unwrap(),
            plane_normal: crate::features::FeatureDirection3::new(crate::math::Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        }).unwrap(),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65536;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let references = super::super::regeneration_references(&ctx, &definition).unwrap();
        assert_eq!(&*references, [&first, &second]);
    }
    drop(ctx.reserve_scoped(65536, "temporary references released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn topology_profile_reference_borrows_the_selection_payload() {
    let profile = crate::features::ProfileRef::Planar(crate::features::PlanarProfileRef::Sketch(
        crate::sketches::SketchId::mint("test:model:sketch#profile").unwrap(),
    ));
    let crate::features::ProfileRef::Planar(original) = &profile else { panic!("planar fixture"); };
    let super::super::ProfileReference::Planar(borrowed) = super::super::ProfileReference::from(&profile) else { panic!("planar view"); };
    assert!(std::ptr::eq(original, borrowed));
}
