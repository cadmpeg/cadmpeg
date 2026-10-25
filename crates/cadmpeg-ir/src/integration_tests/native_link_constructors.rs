// SPDX-License-Identifier: Apache-2.0
//! Neutral constructor defaults and native-link builders preserve geometry fields.

#[test]
fn native_link_builders_preserve_all_fifteen_geometry_carriers() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.pcurves.push(crate::geometry::pcurve::Pcurve::new(
        "test:model:pcurve#one".try_into().unwrap(),
        crate::geometry::pcurve::PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        Default::default(),
    ));
    ir.model.subds.push(crate::subd::SubdSurface::new(
        "test:model:subd#one".try_into().unwrap(),
        crate::subd::SubdScheme::CatmullClark,
        crate::subd::SubdCage::new(Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap(),
        None,
    ));
    ir.model
        .procedural_curves
        .push(crate::geometry::ProceduralCurve::new(
            "test:model:procedural-curve#one".try_into().unwrap(),
            crate::geometry::ProceduralCurveDefinition::Offset(
                crate::geometry::curve_payloads::OffsetCurveConstruction::try_new(
                    ir.model.curves[0].id.clone(),
                    1.0,
                    crate::geometry::OffsetSide::PlaneNormal {
                        normal: crate::math::Vector3::new(0.0, 0.0, 1.0),
                    },
                    None,
                )
                .unwrap(),
            ),
        ));
    ir.model
        .procedural_surfaces
        .push(crate::geometry::ProceduralSurface::new(
            "test:model:procedural-surface#one".try_into().unwrap(),
            crate::geometry::ProceduralSurfaceDefinition::Compound(
                crate::geometry::surface_payloads::CompoundSurfacePayload::try_new(
                    vec![crate::geometry::CompoundComponent {
                        parameter: 1.0,
                        component: ir.model.surfaces[0].id.clone(),
                    }],
                    None,
                )
                .unwrap(),
            ),
            None,
        ));
    let mut count = 0;
    macro_rules! check {
        ($($field:ident),*) => {$(
            assert!(!ir.model.$field.is_empty(), stringify!($field));
            for record in &ir.model.$field {
                let original = serde_json::to_value(record).unwrap();
                assert!(original.get("native_ref").is_none());
                let mut expected = original.clone();
                expected["native_ref"] = "test:native:record#geometry".into();
                let linked = record.clone().with_native_ref(Some("test:native:record#geometry".into()));
                assert_eq!(serde_json::to_value(&linked).unwrap(), expected);
                assert_eq!(serde_json::to_value(linked.with_native_ref(None)).unwrap(), original);
            }
            count += 1;
        )*};
    }
    check!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        curves,
        surfaces,
        pcurves,
        subds,
        procedural_curves,
        procedural_surfaces
    );
    assert_eq!(count, 15);
}
