use super::{
    map_two_chart_endpoint_sets, mapped_two_chart_endpoint_sets,
    pcurve_edge_endpoint_evidence_with_diagnostics, transfer_analytic_pcurve_carriers,
    ModelSurfaceIndex, TwoChartEndpointSets, TwoChartMapping,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn cap_ruling_case() -> (crate::container::ContainerScan<'static>, CadIr) {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.withheld_cap_curve_ids.insert(9);
    scan.surfaces.withheld_cap_surface_ids.insert(7);
    scan.curves
        .two_chart_pcurves
        .push(crate::curve::TwoChartPcurveSamples {
            curve_id: 9,
            faces: [7, 8],
            samples: vec![[[0.0, 7.0], [0.0, 0.0]], [[1.0, 8.0], [0.0, 99.0]]],
            offset: 20,
        });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#8".to_owned()).expect("surface identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(2.0, 3.0, 4.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("cylinder"),
        )),
        source_object: None,
    });
    (scan, ir)
}

#[test]
fn cap_ruling_restores_infinite_line_without_nonzero_axial_endpoints() {
    let (scan, mut ir) = cap_ruling_case();
    crate::decode::with_service_context(|ctx| {
        let transferred = transfer_analytic_pcurve_carriers(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
        .expect("ruling");
        assert_eq!(transferred.len(), 1);
        assert_eq!(ir.model.curves.len(), 1);
        assert!(ir.model.curves[0].parameter_range.is_none());
        assert!(ir.model.points.is_empty());
        let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = &ir.model.curves[0].geometry
        else {
            panic!("infinite line")
        };
        assert_eq!(line.origin().get(), Point3::new(7.0, 3.0, 4.0));
    });
}

#[test]
fn count_framed_curve_transfer_reports_its_unverified_placement() {
    for (count_framed, expected) in [(true, 1), (false, 0)] {
        let (mut scan, mut ir) = cap_ruling_case();
        for curve_id in [9, 10] {
            scan.curves
                .parameters
                .push(crate::curve::CurveParameterRecord {
                    curve_id,
                    type_byte: 0,
                    body: Vec::new(),
                    scalar_tokens: Vec::new(),
                    references: Vec::new(),
                    opaque_spans: Vec::new(),
                    reference_geometry: [0, 0],
                    offset: 20,
                    body_offset: 20,
                    suffix_offset: 20,
                    count_framed,
                });
        }
        let mut losses = Vec::new();
        crate::decode::with_service_context(|ctx| {
            transfer_analytic_pcurve_carriers(
                ctx,
                &scan,
                &mut ir,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut losses,
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
            .expect("ruling")
        });
        assert_eq!(ir.model.curves.len(), 1);
        let reported = losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::CreoLossCode::CountFramedCurveCarrierUnverified.kind()
            })
            .collect::<Vec<_>>();
        assert_eq!(reported.len(), expected);
        assert!(reported.iter().all(|loss| loss.code.taxonomy()
            == cadmpeg_ir::report::loss::LossTaxonomy::CarrierAxisInferred));
    }
}

#[test]
fn cap_ruling_requires_constant_angle_and_one_zero_station() {
    let (mut scan, ir) = cap_ruling_case();
    for samples in [
        vec![[[0.0, 0.0], [0.0, 0.0]], [[0.0, 0.0], [1.0, 1.0]]],
        vec![[[0.0, 0.0], [0.0, 2.0]], [[0.0, 0.0], [0.0, 3.0]]],
        vec![[[0.0, 0.0], [0.0, 0.0]], [[0.0, 0.0], [0.0, 0.0]]],
    ] {
        scan.curves.two_chart_pcurves[0].samples = samples;
        crate::decode::with_service_context(|ctx| {
            assert!(super::cap_adjacent_cylinder_ruling(
                ctx,
                &scan,
                &ModelSurfaceIndex::new(&ir.model.surfaces),
                &scan.curves.two_chart_pcurves[0],
                &crate::decode::source_carriers::SourceUnitCarriers::default()
            )
            .expect("bounded ruling")
            .is_none());
        });
    }
}

#[test]
fn cap_ruling_rejects_duplicate_curve_identity() {
    let (mut scan, mut ir) = cap_ruling_case();
    scan.curves
        .two_chart_pcurves
        .push(scan.curves.two_chart_pcurves[0].clone());
    crate::decode::with_service_context(|ctx| {
        assert!(transfer_analytic_pcurve_carriers(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default()
        )
        .expect("duplicate ruling")
        .is_empty());
    });
}

#[test]
fn cap_envelope_withholding_blocks_single_chart_pcurve_promotion() {
    let mut scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#8".to_owned()).expect("surface identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane chart"),
        )),
        source_object: None,
    });
    let pcurve = crate::curve::TwoChartPcurveSamples {
        curve_id: 9,
        faces: [7, 8],
        samples: vec![[[0.0, 0.0], [0.0, 0.0]], [[0.0, 1.0], [0.0, 1.0]]],
        offset: 0,
    };
    crate::decode::with_service_context(|ctx| {
        let surfaces = ModelSurfaceIndex::new(&ir.model.surfaces);
        let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        assert!(matches!(
            mapped_two_chart_endpoint_sets(ctx, &scan, &surfaces, &pcurve, &carriers)
                .expect("ordinary missing chart"),
            Some(TwoChartEndpointSets::Second(_))
        ));
        scan.surfaces.withheld_cap_curve_ids.insert(9);
        assert!(matches!(
            map_two_chart_endpoint_sets(ctx, &scan, &surfaces, &pcurve, &carriers)
                .expect("explicitly withheld chart"),
            TwoChartMapping::Withheld
        ));
        assert!(
            mapped_two_chart_endpoint_sets(ctx, &scan, &surfaces, &pcurve, &carriers)
                .expect("withheld endpoint sets")
                .is_none()
        );
        scan.curves.two_chart_pcurves.push(pcurve);
        let (evidence, _) =
            pcurve_edge_endpoint_evidence_with_diagnostics(ctx, &scan, &ir, &carriers)
                .expect("bounded pcurve evidence");
        assert!(evidence.is_empty());
        assert!(transfer_analytic_pcurve_carriers(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default()
        )
        .expect("withheld carrier")
        .is_empty());
    });
    assert!(ir.model.curves.is_empty());
    assert!(ir.model.points.is_empty());
}

#[test]
fn compact_cone_origin_withholding_blocks_single_chart_pcurve_promotion() {
    let mut scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#8".to_owned()).expect("surface identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane chart"),
        )),
        source_object: None,
    });
    let pcurve = crate::curve::TwoChartPcurveSamples {
        curve_id: 9,
        faces: [7, 8],
        samples: vec![[[0.0, 0.0], [0.0, 0.0]], [[0.0, 1.0], [0.0, 1.0]]],
        offset: 0,
    };
    crate::decode::with_service_context(|ctx| {
        let surfaces = ModelSurfaceIndex::new(&ir.model.surfaces);
        let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        assert!(matches!(
            mapped_two_chart_endpoint_sets(ctx, &scan, &surfaces, &pcurve, &carriers)
                .expect("ordinary missing chart"),
            Some(TwoChartEndpointSets::Second(_))
        ));
        scan.surfaces.withheld_cone_origin_curve_ids.insert(9);
        assert!(matches!(
            map_two_chart_endpoint_sets(ctx, &scan, &surfaces, &pcurve, &carriers)
                .expect("explicitly withheld chart"),
            TwoChartMapping::Withheld
        ));
        assert!(
            mapped_two_chart_endpoint_sets(ctx, &scan, &surfaces, &pcurve, &carriers)
                .expect("withheld endpoint sets")
                .is_none()
        );
        scan.curves.two_chart_pcurves.push(pcurve);
        let (evidence, _) =
            pcurve_edge_endpoint_evidence_with_diagnostics(ctx, &scan, &ir, &carriers)
                .expect("bounded pcurve evidence");
        assert!(evidence.is_empty());
        assert!(transfer_analytic_pcurve_carriers(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default()
        )
        .expect("withheld carrier")
        .is_empty());
    });
    assert!(ir.model.curves.is_empty());
    assert!(ir.model.points.is_empty());
}
