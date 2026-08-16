// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};
use cadmpeg_ir::geometry::{NurbsSurface, Pcurve, PcurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, CoedgeId, PcurveId, RegionId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Region, Vertex};
use cadmpeg_ir::units::Units;
use std::collections::HashSet;
use std::io::Cursor;

const EPS_PCURVE_LOCI_DISTANCE: f64 = 1.0e-6;

fn surface_draft(id: &str) -> ModelDraft {
    let mut draft = ModelDraft::new();
    draft
        .insert(Surface {
            id: SurfaceId(id.into()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        })
        .expect("insert surface into draft");
    draft
}

#[test]
fn cross_root_surface_filter_tracks_successful_commits_only() {
    let committed_id = "step:data:surface#implicit-face-1";
    let rejected_id = "step:data:surface#implicit-face-2";
    let mut ir = CadIr::empty(Units::default());
    let mut session = CommitSession::new(&ir);

    session
        .commit_model(surface_draft(committed_id), &mut ir)
        .expect("first root commit");
    let mut second_root = surface_draft(committed_id);
    drop_committed_surfaces(&mut second_root, &session, &ir);
    assert!(second_root.model().surfaces.is_empty());

    let mut rejected_root = surface_draft(rejected_id);
    rejected_root
        .insert(Vertex {
            id: "step:data:vertex#rejected".into(),
            point: "step:data:point#missing".into(),
            tolerance: None,
        })
        .expect("insert invalid root reference");
    assert!(session.commit_model(rejected_root, &mut ir).is_err());

    let mut later_root = surface_draft(rejected_id);
    drop_committed_surfaces(&mut later_root, &session, &ir);
    assert_eq!(later_root.model().surfaces.len(), 1);
}

#[test]
fn pcurve_fit_keeps_exact_points_at_a_degenerate_surface_boundary() {
    let surface_id = SurfaceId("step:data:surface#boundary".into());
    let surface_geometry = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    });
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });
    let index = ModelIndex::new(&ir);
    let pcurve = PcurveGeometry::Line {
        origin: cadmpeg_ir::math::Point2::new(0.0, 0.0),
        direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
    };

    let fit = pcurve_endpoint_fit(
        &index,
        &surface_id,
        &pcurve,
        &surface_geometry,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
    )
    .expect("surface points should be evaluable at the boundary");

    assert_eq!(fit.start_parameter, 0.0);
    assert_eq!(fit.end_parameter, 1.0);
    assert!(fit.score <= f64::EPSILON);
}

#[test]
fn trimmed_pcurve_fit_uses_declared_endpoints() {
    let surface_id = SurfaceId("step:data:surface#trimmed-endpoints".into());
    let surface_geometry = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });
    let pcurve = PcurveGeometry::Trimmed {
        parameter_range: [
            std::f64::consts::FRAC_PI_2,
            5.0 * std::f64::consts::FRAC_PI_2,
        ],
        same_sense: true,
        basis: Box::new(PcurveGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            radius: 1.0,
        }),
    };

    let fit = pcurve_endpoint_fit(
        &ModelIndex::new(&ir),
        &surface_id,
        &pcurve,
        &surface_geometry,
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    )
    .expect("declared pcurve endpoints should be evaluable");

    assert_eq!(fit.start_parameter, std::f64::consts::FRAC_PI_2);
    assert_eq!(fit.end_parameter, 5.0 * std::f64::consts::FRAC_PI_2);
    assert!(fit.score <= 2.0 * f64::EPSILON);
}

#[test]
fn pcurve_locus_check_rejects_a_narrow_between_sample_crossing() {
    let surface_id = SurfaceId("step:data:surface#locus-crossing".into());
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let left = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let right = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 0.415, 0.421, 0.428, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(0.415, 0.0),
            Point2::new(0.421, 1.0),
            Point2::new(0.428, 0.0),
            Point2::new(1.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };

    assert!(!pcurve_loci_equivalent(
        &ModelIndex::new(&ir),
        &surface_id,
        &left,
        [0.0, 1.0],
        &right,
        [0.0, 1.0],
        EPS_PCURVE_LOCI_DISTANCE,
    ));
}

#[test]
fn synthesized_pcurve_chart_rejects_a_collapsed_bowed_axis() {
    let bowed = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 2.0),
        ],
        weights: None,
        periodic: false,
    };

    assert!(endpoint_parameter_transform(
        &bowed,
        [Point2::new(0.0, 0.0), Point2::new(0.0, 2.0)],
        [[1.0, 0.0], [1.0, 2.0]],
        [false, false],
    )
    .is_none());

    let isoparametric = PcurveGeometry::Trimmed {
        basis: Box::new(PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(0.0, 1.0),
        }),
        parameter_range: [0.0, 1.0],
        same_sense: true,
    };
    let transform = endpoint_parameter_transform(
        &isoparametric,
        [Point2::new(0.0, 0.0), Point2::new(0.0, 1.0)],
        [[3.0, 4.0], [3.0, 5.0]],
        [false, false],
    )
    .expect("constant source axis may collapse to a constant destination");
    assert_eq!(
        transform.rows,
        [[0.0, 0.0, 3.0], [0.0, 1.0, 4.0], [0.0, 0.0, 1.0]]
    );
}

#[test]
fn selected_pcurve_variants_are_use_scoped() {
    let source_id = PcurveId("step:data:pcurve#shared".into());
    let source_geometry = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let selected_geometry = PcurveGeometry::Line {
        origin: Point2::new(4.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.pcurves.push(Pcurve {
        id: source_id.clone(),
        geometry: source_geometry.clone(),
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: None,
        fit_tolerance: None,
    });
    let selected = SelectedPcurve {
        id: source_id.clone(),
        geometry: selected_geometry.clone(),
        parameter_range: Some([0.0, 1.0]),
    };

    let (first, first_variant) =
        materialize_pcurve_variant(&ir, &selected, &CoedgeId("step:data:coedge#first".into()));
    let (second, second_variant) =
        materialize_pcurve_variant(&ir, &selected, &CoedgeId("step:data:coedge#second".into()));

    assert_ne!(first, source_id);
    assert_ne!(second, source_id);
    assert_ne!(first, second);
    assert_eq!(ir.model.pcurves[0].geometry, source_geometry);
    assert_eq!(
        first_variant.expect("first pcurve variant").geometry,
        selected_geometry
    );
    assert_eq!(
        second_variant.expect("second pcurve variant").geometry,
        selected_geometry
    );
}

#[test]
fn shared_step_pcurve_variants_are_use_scoped() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc04_shared_pcurve.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode shared pcurve witness");

    let source = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#33")
        .expect("shared source pcurve");
    assert!(matches!(
        &source.geometry,
        PcurveGeometry::Trimmed {
            parameter_range,
            same_sense: true,
            ..
        } if *parameter_range == [0.0, 1.0]
    ));

    let variant = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| {
            pcurve
                .id
                .as_str()
                .contains("pcurve#33-use-step-data-coedge-51-face-56")
        })
        .expect("use-scoped pcurve variant");
    let PcurveGeometry::Transformed { transform, .. } = &variant.geometry else {
        panic!("expected transformed pcurve variant");
    };
    assert_eq!(
        transform.rows,
        [[-1.0, 0.0, 5.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]]
    );

    let first_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#42")
        .and_then(|coedge| coedge.pcurves.first())
        .expect("first shared pcurve use");
    let second_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#45")
        .and_then(|coedge| coedge.pcurves.first())
        .expect("second shared pcurve use");
    assert_eq!(first_use.pcurve.as_str(), "step:data:pcurve#33");
    assert_eq!(
        second_use.pcurve.as_str(),
        "step:data:pcurve#33-use-step-data-coedge-51-face-56"
    );
    assert_ne!(first_use.pcurve, second_use.pcurve);

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn periodic_surface_line_seeds_cover_both_parameter_axes() {
    let surface_id = SurfaceId("step:data:surface#periodic-seeds".into());
    let surface_geometry = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 1.0,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });
    let index = ModelIndex::new(&ir);

    for direction in [Point2::new(1.0, 0.0), Point2::new(0.0, 1.0)] {
        let geometry = PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction,
        };
        let seeds = pcurve_selection_seeds(&index, &surface_id, &geometry, &surface_geometry);
        assert!(seeds
            .iter()
            .any(|seed| { (*seed - std::f64::consts::PI).abs() <= 1.0e-12 }));
    }
}

#[test]
fn bounded_nurbs_pcurve_seeds_cover_each_knot_span() {
    let surface_id = SurfaceId("step:data:surface#nurbs-seed-spans".into());
    let surface_geometry = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });
    let geometry = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 0.001, 0.999, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 1.0),
            Point2::new(1.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };

    let seeds = pcurve_selection_seeds(
        &ModelIndex::new(&ir),
        &surface_id,
        &geometry,
        &surface_geometry,
    );
    for expected in [0.001, 0.999, 0.0005, 0.5, 0.9995] {
        assert!(
            seeds.contains(&expected),
            "missing NURBS knot-span seed {expected}: {seeds:?}"
        );
    }
}

#[test]
fn shared_surface_carrier_is_staged_once() {
    let surface = Surface {
        id: SurfaceId("step:data:surface#shared".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let body_id = BodyId("step:data:body#shared-surface".into());
    let region_id = RegionId("step:data:region#shared-surface".into());
    let built = super::super::staged_topology(
        HashSet::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![surface.clone(), surface],
        Vec::new(),
        Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: Vec::new(),
        },
        Body {
            id: body_id,
            kind: BodyKind::Sheet,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    )
    .expect("duplicate references to one source surface must stage");

    assert_eq!(built.draft.model().surfaces.len(), 1);
}
