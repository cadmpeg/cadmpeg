// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use std::mem::size_of;

#[test]
fn tabulated_geometry_transfer_admits_only_surviving_directrix_and_surface_backing() {
    let scan = super::construction_copy_scan(true);
    // Directrix: eight knots and four converted finite poles. Extrusion: four
    // retained row slots, four two-point rows with four amortized slots each,
    // eight cloned U knots and four V knots. Temporary raw controls are scoped.
    let bytes = u64::try_from(20 * size_of::<f64>() + 20 * size_of::<FinitePoint3>()
        + 4 * size_of::<Vec<FinitePoint3>>()).expect("fixed geometry backing");
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::super::transfer_tabulated_cylinder_spline_extrusions(
            &ctx, &scan, &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(), &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    let below = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes, Some("creo tabulated extrusion geometry"), run,
    );
    for cap in [below, bytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut losses = Vec::new();
        let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let error = super::super::transfer_tabulated_cylinder_spline_extrusions(
            &ctx, &scan, &mut ir, &mut annotations, &mut losses, &mut carriers,
        ).expect_err("geometry or next identity exceeds exact cap");
        let original = ctx.resource_refusal().expect("retained refusal");
        assert!(matches!(error, CodecError::ResourceLimit(actual) if actual == original));
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        if cap < bytes {
            assert_eq!((original.used, original.additional, original.operation),
                (0, bytes, "creo tabulated extrusion geometry"));
        } else {
            // Geometry promotion succeeds at its exact bound. The next
            // separately owned identity promotion needs its actual text bytes.
            let identity_bytes = u64::try_from("creo:visibgeom:tabulated_directrix#7".len())
                .expect("fixed identity");
            assert_eq!((original.used, original.additional, original.operation),
                (bytes, identity_bytes, "creo tabulated directrix identity"));
        }
        assert!(ir.model.curves.is_empty());
        assert!(ir.model.surfaces.is_empty());
        assert!(ir.model.procedural_surfaces.is_empty());
        assert!(losses.is_empty());
        assert!(matches!(super::super::transfer_tabulated_cylinder_spline_extrusions(
            &ctx, &scan, &mut ir, &mut annotations, &mut losses, &mut carriers,
        ), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}
