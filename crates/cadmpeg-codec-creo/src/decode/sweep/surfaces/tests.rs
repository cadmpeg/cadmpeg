// SPDX-License-Identifier: Apache-2.0

use super::{
    push_saved_spline_loss, transfer_saved_spline_curves, unique_feature_surface_row,
    JoinedLaneRecords,
};
use crate::decode::tests::surface_row;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn saved_spline_loss_joins_lane_records() {
    let records = ["first".to_owned(), "second".to_owned()];
    let mut losses = Vec::new();
    push_saved_spline_loss(
        &mut losses,
        format_args!(
            "Saved section spline at offset 7 cannot form a NURBS curve: {}",
            JoinedLaneRecords(&records)
        ),
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].message,
        "Saved section spline at offset 7 cannot form a NURBS curve: first; second"
    );
}

#[test]
fn revolved_nurbs_surface_sweeps_the_directrix_through_nine_columns() {
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3, RevolutionAxis};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::{Point3, Vector3};

    let directrix = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
        None,
        false,
    )
    .expect("valid directrix");
    let axis = RevolutionAxis {
        origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite origin"),
        direction: FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).expect("axis direction"),
        reference: None,
    };
    let surface = super::revolved_nurbs_surface(
        &directrix,
        &axis,
        &"revolved NURBS fixture",
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("valid revolution surface");
    assert_eq!((surface.u_count(), surface.v_count()), (2, 9));
    assert_eq!(surface.u_knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn generated_surface_binding_requires_one_matching_row() {
    let row = surface_row(31, 7, crate::surface::SurfaceKind::Plane);
    assert!(unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        8,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Cylinder,
    ));
    assert!(!unique_feature_surface_row(
        &[row.clone(), row],
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
}

fn saved_spline_definition() -> crate::feature::definitions::FeatureDefinition {
    crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: Some(40),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(crate::feature::definitions::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        }),
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Spline(
                crate::feature::definitions::FeatureSavedSpline {
                    entity_id: Some(1),
                    declared_point_count: Some(2),
                    interpolation_points: vec![[2.0, 0.0, 0.0], [2.0, 0.0, 1.0]],
                    interpolation_points_body: Vec::new(),
                    endpoint_tangents: Some(crate::feature::definitions::DecodedField {
                        value: [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
                        body: Vec::new(),
                    }),
                    parameters: Some(crate::feature::definitions::DecodedField {
                        value: Vec::new(),
                        body: Vec::new(),
                    }),
                    offset: 0,
                },
            )],
            offset: 0,
        }),
        offset: 0,
    }
}

#[test]
fn extrusion_solved_segment_ids_collect_the_solved_ids() {
    let mut definition = saved_spline_definition();
    definition.trim_entities = Some(crate::feature::definitions::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::definitions::FeatureTrimEntity {
            external_id: 7,
            mode: None,
            vertices: [1, 2],
            kind: crate::feature::definitions::TrimEntityKind::Line,
            offset: 0,
        }],
        solved_external_ids: vec![7],
        offset: 0,
    });
    let ids = super::extrusion_solved_segment_ids(&definition);
    assert_eq!(ids, std::collections::BTreeSet::from([7]));
}

#[test]
fn malformed_saved_spline_reports_transfer_loss() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.definitions.push(saved_spline_definition());
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section frame"),
    );
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    assert_eq!(
        crate::decode::with_service_context(|ctx| transfer_saved_spline_curves(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut losses,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("transfer"),
        0
    );
    assert!(ir.model.curves.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::SectionSplineUnresolved.kind()
    );
}

#[test]
fn numerical_followup_revolution_refuses_skew_line_specialization() {
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3, RevolutionAxis};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
    for radius in [2e-10, 2.] {
        let axis = RevolutionAxis {
            origin: FinitePoint3::new(Point3::new(0., 0., 0.))
                .expect("valid finite regression fixture"),
            direction: FeatureDirection3::new(Vector3::new(0., 0., 1.))
                .expect("valid finite regression fixture"),
            reference: None,
        };
        let transform = crate::placement::FeatureSectionTransform::new(
            1,
            Some(1),
            [radius, 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            0,
        )
        .expect("valid finite regression fixture");
        let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0., 0.),
            end: Point2::new(radius, radius),
        })
        .expect("valid finite regression fixture");
        assert!(super::revolved_section_surface(&transform, &line, &axis).is_none());
        let generator = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0., 0.),
            end: Point2::new(0., radius),
        })
        .expect("valid finite regression fixture");
        assert!(super::revolved_section_surface(&transform, &generator, &axis).is_some());
    }
}

#[test]
fn saved_spline_extrusion_transfers_one_surface() {
    let mut scan = crate::test_support::empty_container_scan();
    let mut definition = saved_spline_definition();
    let Some(crate::feature::definitions::FeatureSavedEntity::Spline(spline)) = definition
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .first_mut()
    else {
        panic!("saved spline");
    };
    spline.parameters.as_mut().expect("parameters").value = vec![0.0, 1.0];
    definition.order_table = Some(crate::feature::definitions::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::definitions::FeatureOrderRow {
            external_id: 7,
            internal_id: 1,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    });
    scan.features.definitions.push(definition);
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section transform"),
    );
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(surface_row(
        20,
        40,
        crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
    ));
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            40,
            29,
            vec![crate::feature::entity::FeatureEntityTableEntry {
                entity_id: 20,
                payload: crate::feature::entity::entry_payload(200, Some(7), None, None),
                prefixed: false,
                offset: 0,
                end_offset: 0,
            }],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([20]),
    );
    for (id, z) in [(21, -1.0), (22, 1.0)] {
        scan.surfaces
            .rows
            .push(surface_row(id, 40, crate::surface::SurfaceKind::Plane));
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: id,
            origin: [0.0, 0.0, z],
            normal: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .expect("normal"),
            u_axis: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(
                1.0, 0.0, 0.0,
            ))
            .expect("axis"),
            offset: 0,
        });
    }
    let count = crate::decode::with_service_context(|ctx| {
        super::transfer_feature_extrusion_surfaces(
            ctx,
            &scan,
            &mut CadIr::empty(),
            &mut AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("extrusion surface transfer");
    assert_eq!(count, 1);
}

#[test]
fn sweep_carriers_require_complete_envelopes_and_ordered_arcs() {
    use cadmpeg_ir::geometry::{
        analytic::{CylinderSurface, TorusSurface},
        SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    let circle_geometry = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
        Point3::new(0., 0., 0.),
        Vector3::new(0., 0., 1.),
        Vector3::new(1., 0., 0.),
        3.,
    )
    .expect("circle");
    let circle = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Circle(circle_geometry),
    );
    let samples = [[3., 0., 0.], [0., 3., 0.], [-3., 0., 0.], [0., -3., 0.]];
    for endpoints in [[samples[0], samples[3]], [samples[3], samples[0]]] {
        let (_, range) =
            super::orient_sampled_circle(&circle, &samples, endpoints).expect("ordered major arc");
        assert!((range[1] - 3. * std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    }
    let mut out_of_order = samples;
    out_of_order.swap(1, 2);
    assert!(
        super::orient_sampled_circle(&circle, &out_of_order, [samples[0], samples[3]]).is_none()
    );
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            3.,
        )
        .expect("cylinder"),
    ));
    let body = [
        0x18, 0xe4, 0x11, 0x2f, 0x10, 0x00, 0x48, 0x08, 0x00, 0x18, 0xe4, 0x2f, 0x08, 0x00, 0xe4,
        0x2f, 0x10, 0x00,
    ];
    crate::decode::with_service_context(|ctx| {
        assert!(super::envelope_matches(ctx, &body, &cylinder)?);
        assert!(super::circle_on_surface(&circle_geometry, &cylinder));
        let charts = vec![
            cadmpeg_ir::geometry::Surface {
                id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#7").expect("surface"),
                geometry: cylinder.clone(),
                source_object: None,
            },
            cadmpeg_ir::geometry::Surface {
                id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#8").expect("surface"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0., 0., 0.),
                        Vector3::new(0., 0., 1.),
                        Vector3::new(1., 0., 0.),
                    )
                    .expect("plane"),
                )),
                source_object: None,
            },
        ];
        let path = crate::curve::TwoChartPcurveSamples {
            curve_id: 9,
            faces: [7, 8],
            offset: 13,
            samples: vec![
                [[0., 0.], [3., 0.]],
                [[std::f64::consts::FRAC_PI_2, 0.], [0., 3.]],
                [[std::f64::consts::PI, 0.], [-3., 0.]],
            ],
        };
        let cylinder_carrier = crate::decode::analytic::carriers::surface_carrier(&cylinder)
            .expect("analytic cylinder");
        let endpoints = [[3., 0., 0.], [-3., 0., 0.]];
        let mut incident = [vec![cylinder_carrier], vec![cylinder_carrier]];
        assert!(super::endpoints_on_incident_carriers(endpoints, &incident));
        incident[0].push(crate::decode::analytic::equations::CarrierEquation::Plane(
            crate::decode::analytic::equations::PlaneEquation {
                origin: [1., 0., 0.],
                normal: [1., 0., 0.],
            },
        ));
        assert!(!super::endpoints_on_incident_carriers(endpoints, &incident));
        let source = crate::decode::source_carriers::SourceUnitCarriers::default();
        let unresolved = std::collections::BTreeSet::new();
        assert!(super::admit_angular_chart_pair(
            ctx,
            &path,
            &charts,
            &source,
            &unresolved,
            None
        )?);
        let empty = crate::container::scan_bytes_ok(crate::test_support::build_prt("angular", &[]));
        let mut binding_losses = Vec::new();
        assert!(!super::admit_angular_constraints(
            ctx,
            &empty,
            &path,
            &charts,
            &source,
            Some(&mut binding_losses)
        )?);
        assert_eq!(binding_losses.len(), 1);
        assert_eq!(
            binding_losses[0].code,
            crate::loss::CreoLossCode::AngularPcurvePairUnresolved.kind()
        );
        assert!(binding_losses[0].message.contains("Curve 9 at byte 13"));
        assert!(binding_losses[0].message.contains("endpoint binding"));
        let mut pair_losses = Vec::new();
        assert!(!super::admit_angular_chart_pair(
            ctx,
            &path,
            &charts[..1],
            &source,
            &unresolved,
            Some(&mut pair_losses)
        )?);
        assert_eq!(pair_losses.len(), 1);
        assert_eq!(
            pair_losses[0].code,
            crate::loss::CreoLossCode::AngularPcurvePairUnresolved.kind()
        );
        assert!(pair_losses[0].message.contains("Curve 9 at byte 13"));
        assert!(!super::admit_angular_chart_pair(
            ctx,
            &path,
            &charts,
            &source,
            &std::collections::BTreeSet::from([8]),
            None
        )?);
        let mut invalid = body;
        invalid[8] = 0x10;
        let mut losses = Vec::new();
        assert!(!super::sweep_envelope_replay_matches(
            ctx,
            &invalid,
            &cylinder,
            17,
            29,
            &mut losses
        )?);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::CreoLossCode::SweepEnvelopeReplayUnresolved.kind()
        );
        assert!(losses[0].message.contains("Surface 17 at byte 29"));
        let torus = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                7.,
                3.,
            )
            .expect("torus"),
        ));
        let meridian = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(7., 0., 0.),
            Vector3::new(0., 1., 0.),
            Vector3::new(1., 0., 0.),
            3.,
        )
        .expect("meridian");
        assert!(super::circle_on_surface(&meridian, &torus));
        assert!(!super::circle_on_surface(&circle_geometry, &torus));
        let torus_body = [
            0x18, 0xe4, 0x11, 0x12, 0x48, 0x24, 0, 0x18, 0xe4, 0x2f, 0x24, 0, 0xe4, 0x2f, 0x08, 0,
        ];
        assert!(super::envelope_matches(ctx, &torus_body, &torus)?);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("finite envelope evaluation");
}
