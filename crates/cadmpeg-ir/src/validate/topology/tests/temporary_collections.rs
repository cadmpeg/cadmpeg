// SPDX-License-Identifier: Apache-2.0

#[test]
fn topology_regeneration_references_keep_byte_order_and_uniqueness() {
    use crate::features::patterns::{PatternKind, PatternSeed, PatternTransform};
    let first = crate::features::FeatureId::mint("test:model:feature#first").unwrap();
    let second = crate::features::FeatureId::mint("test:model:feature#second").unwrap();
    let definition = crate::features::FeatureOperation::Pattern {
        seeds: vec![
            PatternSeed::Feature(second.clone()),
            PatternSeed::Feature(first.clone()),
            PatternSeed::Feature(second.clone()),
        ],
        pattern: PatternKind::new(PatternTransform::Mirror {
            plane_origin: crate::features::FinitePoint3::new(crate::math::Point3::new(
                0.0, 0.0, 0.0,
            ))
            .unwrap(),
            plane_normal: crate::features::FeatureDirection3::new(crate::math::Vector3::new(
                1.0, 0.0, 0.0,
            ))
            .unwrap(),
        })
        .unwrap(),
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    let references = super::super::regeneration_references(&ctx, &definition).unwrap();
    assert_eq!(&*references, [&first, &second]);
}

#[test]
fn topology_profile_reference_borrows_the_selection_payload() {
    let profile = crate::features::ProfileRef::Planar(crate::features::PlanarProfileRef::Sketch(
        crate::sketches::SketchId::mint("test:model:sketch#profile").unwrap(),
    ));
    let crate::features::ProfileRef::Planar(original) = &profile else {
        panic!("planar fixture");
    };
    let super::super::ProfileReference::Planar(borrowed) =
        super::super::ProfileReference::from(&profile)
    else {
        panic!("planar view");
    };
    assert!(std::ptr::eq(original, borrowed));
}
