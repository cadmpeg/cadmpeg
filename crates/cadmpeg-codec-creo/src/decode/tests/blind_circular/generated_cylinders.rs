// SPDX-License-Identifier: Apache-2.0

use crate::decode::holes::placement::ExtrusionSpan;
use crate::decode::sweep::extent::{
    bounded_cylinder_span, directed_blind_extrusion_span, generated_bounded_cylinder_extent,
    resolved_feature_extrusion_span,
};
use crate::decode::sweep::planes::{
    agreed_generated_cylinder_extent, unique_available_positional_cylinder_frame_records,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use std::collections::BTreeSet;

#[test]
fn agreeing_generated_cylinders_define_blind_extrusion_extent() {
    let transform = crate::placement::FeatureSectionTransform::new(
        917,
        Some(40),
        [0.0, 4.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        100,
    )
    .expect("valid section frame");
    let frame = |origin| {
        crate::surface::PositionalCylinderFrame::new(
            origin,
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            0.75,
            Some(34.0),
        )
        .expect("valid positional cylinder frame")
    };
    let frames = [frame([-12.5, 4.0, 0.0]), frame([12.5, 4.0, 0.0])];
    assert_eq!(
        agreed_generated_cylinder_extent(&transform, &frames),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(34.0)
                            .expect("nonzero length fixture")
                    },
                    draft: None,
                }
            },
            [0.0, 1.0, 0.0]
        ))
    );
    assert_eq!(
        directed_blind_extrusion_span(transform.normal(), [0.0, 1.0, 0.0], 34.0),
        Some(ExtrusionSpan::new(0.0, 34.0).expect("valid span fixture"))
    );
    assert_eq!(
        directed_blind_extrusion_span(transform.normal(), [0.0, -1.0, 0.0], 34.0),
        Some(ExtrusionSpan::new(-34.0, 0.0).expect("valid span fixture"))
    );
    assert!(directed_blind_extrusion_span(transform.normal(), [1.0, 0.0, 0.0], 34.0).is_none());

    let mut inconsistent = frames;
    inconsistent[1] = crate::surface::PositionalCylinderFrame::new(
        frames[1].frame().origin(),
        frames[1].frame().axis(),
        frames[1].frame().ref_direction(),
        frames[1].radius().get(),
        Some(33.0),
    )
    .expect("valid positional cylinder frame");
    assert!(agreed_generated_cylinder_extent(&transform, &inconsistent).is_none());
    inconsistent = frames;
    let mut origin = frames[1].frame().origin();
    origin[1] = 5.0;
    inconsistent[1] = crate::surface::PositionalCylinderFrame::new(
        origin,
        frames[1].frame().axis(),
        frames[1].frame().ref_direction(),
        frames[1].radius().get(),
        frames[1].length().map(PositiveLength::get),
    )
    .expect("valid positional cylinder frame");
    assert!(agreed_generated_cylinder_extent(&transform, &inconsistent).is_none());

    let diagonal = 0.5_f64.sqrt();
    let diagonal_transform = crate::placement::FeatureSectionTransform::new(
        transform.definition_id,
        transform.feature_id,
        transform.origin(),
        [diagonal, -diagonal, 0.0],
        [0.0, 0.0, -1.0],
        transform.offset,
    )
    .expect("valid section frame");
    let perpendicular = [crate::surface::PositionalCylinderFrame::new(
        diagonal_transform.origin(),
        [diagonal, -diagonal, 0.0],
        [0.0, 0.0, 1.0],
        frames[0].radius().get(),
        frames[0].length().map(PositiveLength::get),
    )
    .expect("valid positional cylinder frame")];
    assert!(agreed_generated_cylinder_extent(&diagonal_transform, &perpendicular).is_none());
}

#[test]
fn generated_cylinder_extent_uses_unique_available_parameter_frames() {
    let frame = crate::surface::PositionalCylinderFrame::new(
        [1.0, 2.0, 3.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        4.0,
        Some(5.0),
    )
    .expect("valid positional cylinder frame");
    let parameter =
        |surface_id, positional_cylinder_frame: Option<crate::surface::PositionalCylinderFrame>| {
            crate::surface::SurfaceParameterRecord {
                surface_id,
                body: Vec::new(),
                scalar_tokens: Vec::new(),
                opaque_spans: Vec::new(),
                scalar_frames: Vec::new(),
                carrier: positional_cylinder_frame.map_or(
                    crate::surface::SurfaceParameterCarrier::Unresolved(
                        crate::surface::SurfaceKind::Cylinder,
                    ),
                    |frame| {
                        crate::surface::SurfaceParameterCarrier::Resolved(
                            crate::surface::InlineSurfaceCarrier::Cylinder {
                                frame,
                                split_bounds: None,
                            },
                        )
                    },
                ),
                boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
                offset: 0,
                body_offset: 0,
            }
        };
    let surface_ids = BTreeSet::from([1, 2, 3]);
    let parameters = [parameter(1, Some(frame)), parameter(2, None)];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            unique_available_positional_cylinder_frame_records(ctx, &surface_ids, &parameters)
        })
        .expect("service resources"),
        Some(vec![(1, frame)])
    );

    let duplicates = [parameter(1, Some(frame)), parameter(1, Some(frame))];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        unique_available_positional_cylinder_frame_records(ctx, &surface_ids, &duplicates)
    })
    .expect("service resources")
    .is_none());
}

#[test]
fn bounded_generated_cylinders_define_a_blind_extrusion() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 7,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        };
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.extend([
            row(31, crate::surface::SurfaceKind::Plane),
            row(32, crate::surface::SurfaceKind::Plane),
            row(33, crate::surface::SurfaceKind::Cylinder),
        ]);
        let parameter = crate::surface::SurfaceParameterRecord {
            surface_id: 33,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cylinder {
                    frame: crate::surface::PositionalCylinderFrame::new(
                        [2.0, 4.0, 0.0],
                        [0.0, -1.0, 0.0],
                        [1.0, 0.0, 0.0],
                        1.0,
                        Some(8.0),
                    )
                    .expect("valid positional cylinder frame"),
                    split_bounds: None,
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 33,
            body_offset: 34,
        };
        scan.surfaces.parameters.push(parameter);
        let plane = |id, y, normal| Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, y, 0.0),
                    normal,
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        };
        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            plane(31, 4.0, Vector3::new(0.0, 1.0, 0.0)),
            plane(32, -4.0, Vector3::new(0.0, -1.0, 0.0)),
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#33".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        Point3::new(2.0, 4.0, 0.0),
                        Vector3::new(0.0, -1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        1.0,
                    )
                    .expect("valid CylinderSurface fixture"),
                )),
                source_object: None,
            },
        ]);

        let expected = Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(8.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, -1.0, 0.0],
        ));
        assert_eq!(
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent"),
            expected
        );

        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: 31,
            origin: [0.0, 5.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 31,
        });
        let conflicting_extent = generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None,
        )
        .expect("admitted extent");
        assert!(conflicting_extent.is_none());
        scan.planes.outlines[0].origin[1] = 4.0;
        assert_eq!(
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent"),
            expected
        );

        scan.surfaces
            .rows
            .push(row(34, crate::surface::SurfaceKind::Plane));
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#34".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        assert_eq!(
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent"),
            expected
        );

        scan.surfaces
            .rows
            .push(row(35, crate::surface::SurfaceKind::Cylinder));
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#35".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        assert_eq!(
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent"),
            expected
        );
        scan.surfaces.rows.truncate(3);
        ir.model.surfaces.truncate(3);

        let mut untransferred_caps = ir.clone();
        untransferred_caps.model.surfaces.retain(|surface| {
            surface.id
                == SurfaceId::mint("creo:visibgeom:surface#33".to_string())
                    .expect("identity grammar")
        });
        assert_eq!(
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &untransferred_caps,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent"),
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent")
        );

        let crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Cylinder { frame, .. },
        ) = &mut scan.surfaces.parameters[0].carrier
        else {
            panic!("cylinder frame");
        };
        *frame = crate::surface::PositionalCylinderFrame::new(
            frame.frame().origin(),
            frame.frame().axis(),
            frame.frame().ref_direction(),
            frame.radius().get(),
            None,
        )
        .expect("valid positional cylinder frame");
        assert_eq!(
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent"),
            Some((
                ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: cadmpeg_ir::scalar::NonZeroLength::new(8.0)
                                .expect("nonzero length fixture"),
                        },
                        draft: None,
                    },
                },
                [0.0, -1.0, 0.0],
            ))
        );
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &untransferred_caps,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None
        )
        .expect("admitted extent")
        .is_none());
        let lengthless = scan.surfaces.parameters[0]
            .positional_cylinder_frame()
            .expect("cylinder frame");
        assert!(bounded_cylinder_span(
            ctx,
            lengthless,
            &[
                ([0.0, -4.0, 0.0], [0.0, 1.0, 0.0]),
                ([0.0, -6.0, 0.0], [0.0, 1.0, 0.0]),
            ],
        )
        .expect("admitted extent")
        .is_none());
        let crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Cylinder { frame, .. },
        ) = &mut scan.surfaces.parameters[0].carrier
        else {
            panic!("cylinder frame");
        };
        *frame = crate::surface::PositionalCylinderFrame::new(
            frame.frame().origin(),
            frame.frame().axis(),
            frame.frame().ref_direction(),
            frame.radius().get(),
            Some(8.0),
        )
        .expect("valid positional cylinder frame");

        let transform = crate::placement::FeatureSectionTransform::new(
            7,
            Some(7),
            [0.0, 4.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            0,
        )
        .expect("valid section frame");
        assert_eq!(
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &untransferred_caps,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                Some(&transform)
            )
            .expect("admitted extent"),
            generated_bounded_cylinder_extent(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                7,
                None
            )
            .expect("admitted extent")
        );
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: Some(7),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let surface_rows = std::mem::take(&mut scan.surfaces.rows);
        scan.surfaces.rows = surface_rows
            .iter()
            .filter(|row| row.id == 33)
            .cloned()
            .collect();
        let model_surfaces = std::mem::take(&mut ir.model.surfaces);
        ir.model.surfaces = model_surfaces
            .iter()
            .filter(|surface| {
                surface.id == SurfaceId::mint("creo:visibgeom:surface#33").expect("valid identity")
            })
            .cloned()
            .collect();
        assert_eq!(
            resolved_feature_extrusion_span(
                ctx,
                &scan,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                &definition,
                &transform
            )
            .expect("admitted extent"),
            Some(ExtrusionSpan::new(0.0, 8.0).expect("valid span fixture"))
        );
        scan.surfaces.rows = surface_rows;
        ir.model.surfaces = model_surfaces;
        let displaced = crate::placement::FeatureSectionTransform::new(
            transform.definition_id,
            transform.feature_id,
            [0.0, 3.0, 0.0],
            transform.u_axis(),
            transform.v_axis(),
            transform.offset,
        )
        .expect("valid section frame");
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &untransferred_caps,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            Some(&displaced)
        )
        .expect("admitted extent")
        .is_none());
        let perpendicular = crate::placement::FeatureSectionTransform::new(
            transform.definition_id,
            transform.feature_id,
            transform.origin(),
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            transform.offset,
        )
        .expect("valid section frame");
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &untransferred_caps,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            Some(&perpendicular)
        )
        .expect("admitted extent")
        .is_none());

        let mut oblique = ir.clone();
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
            &mut oblique.model.surfaces[0].geometry
        else {
            panic!("plane");
        };
        let origin = plane_surface.origin();

        let normal = Vector3::new(
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        );
        let u_axis = Vector3::new(1.0, 0.0, 0.0);
        *plane_surface =
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(*origin, normal, u_axis)
                .expect("valid PlaneSurface fixture");
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &oblique,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None
        )
        .expect("admitted extent")
        .is_none());

        let crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Cylinder { frame, .. },
        ) = &mut scan.surfaces.parameters[0].carrier
        else {
            panic!("cylinder frame");
        };
        *frame = crate::surface::PositionalCylinderFrame::new(
            frame.frame().origin(),
            frame.frame().axis(),
            frame.frame().ref_direction(),
            frame.radius().get(),
            Some(7.0),
        )
        .expect("valid positional cylinder frame");
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None
        )
        .expect("admitted extent")
        .is_none());
        let crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Cylinder { frame, .. },
        ) = &mut scan.surfaces.parameters[0].carrier
        else {
            panic!("cylinder frame");
        };
        *frame = crate::surface::PositionalCylinderFrame::new(
            frame.frame().origin(),
            frame.frame().axis(),
            frame.frame().ref_direction(),
            frame.radius().get(),
            Some(8.0),
        )
        .expect("valid positional cylinder frame");

        scan.surfaces.rows.push(scan.surfaces.rows[0].clone());
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None
        )
        .expect("admitted extent")
        .is_none());
        scan.surfaces.rows.pop();

        scan.surfaces
            .parameters
            .push(scan.surfaces.parameters[0].clone());
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None
        )
        .expect("admitted extent")
        .is_none());
        scan.surfaces.parameters.pop();

        let mut missing_transfer = ir.clone();
        missing_transfer.model.surfaces.pop();
        assert!(generated_bounded_cylinder_extent(
            ctx,
            &scan,
            &missing_transfer,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            None
        )
        .expect("admitted extent")
        .is_none());
    });
}
