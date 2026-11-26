use cadmpeg_ir::geometry::nurbs::NurbsCurve;
// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_ir::geometry::blends::{
    RollingBallJetDerivative, RollingBallJetSite, RollingBallJetStation, RollingBallJetStations,
};
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::math::Vector3;

fn cylinder_section() -> ProceduralSurfaceDefinition {
    let derivative = |z| RollingBallJetDerivative {
        first_limit: Vector3::new(0.0, 0.0, z),
        second_limit: Vector3::new(0.0, 0.0, z),
        center: Vector3::new(0.0, 0.0, z),
        angle: 0.0,
    };
    let stations = [0.0, 1.0].map(|z| RollingBallJetStation {
        knot: z,
        multiplicity: 6,
        site: RollingBallJetSite {
            first_limit: Point3::new(2.0, 0.0, z),
            second_limit: Point3::new(0.0, 2.0, z),
            center: Point3::new(0.0, 0.0, z),
            angle: std::f64::consts::FRAC_PI_2,
            first_derivative: derivative(1.0),
            second_derivative: derivative(0.0),
        },
    });
    ProceduralSurfaceDefinition::RollingBallJet(
        RollingBallJetStations::try_new(5, stations.to_vec()).expect("quarter-cylinder jet"),
    )
}

fn sheet(definition: &ProceduralSurfaceDefinition, v_min: f64) -> DecodedFace<'_> {
    let corners = [[0.2, v_min], [0.8, v_min], [0.8, 0.5], [0.2, 0.5]];
    let trims = (0..4)
        .map(|i| {
            let j = (i + 1) % 4;
            Trim {
                edge: 10 + i,
                source: 10 + i,
                vertices: [i, j],
                pcurve: line_pcurve([0.0, 1.0], [corners[i], corners[j]]).expect("trim"),
                range: [0.0, 1.0],
                mapped_range: None,
                affine_map: false,
                spline_range: None,
                stations: [Some(0.0), Some(1.0)],
                primary_stations: [None, None],
                sphere_circle: None,
                alternate_line: None,
                symmetric_line: false,
                inferred_accuracy: None,
            }
        })
        .collect();
    DecodedFace {
        face_pos: 20,
        member: Member {
            body: (0, 1),
            forward: true,
        },
        carrier: Carrier::RollingBall(definition, 1.0),
        outer: Ring {
            trims,
            bounds: None,
        },
        inner: vec![],
    }
}

fn transfer(patch: Patch) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    use crate::families::FamilyEntityAdmission;
    use cadmpeg_ir::annotations::AnnotationBuilder;
    let mut ir = CadIr::empty();
    let mut notes = LaneRefusals::new();
    crate::test_support::with_service_context(|ctx| {
        super::super::super::transfer::append(
            &mut ir,
            &mut AnnotationBuilder::new(),
            vec![patch],
            &mut notes,
            &mut FamilyEntityAdmission::new(ctx),
            &super::super::super::roster::BodyRoots::new(),
        )
    })
    .expect("transfer recovered patch");
    ir.finalize();
    cadmpeg_ir::examples::add_document_root(&mut ir);
    let validation = cadmpeg_ir::validate_neutral(&ir, vec![]).expect("validate cylinder patch");
    assert!(validation.is_ok(), "{:?}", validation.findings);
    (ir, notes.take_notes())
}

#[test]
fn rolling_extension_recovers_and_reports_a_bounded_cylindrical_strip() {
    let definition = cylinder_section();
    let face = sheet(&definition, -0.0015);
    let patch = face
        .recover_rolling_boundary(&CadIr::empty(), None)
        .expect("bounded extension");
    let expected_gap = 4.0 * (0.0015 * std::f64::consts::FRAC_PI_2 / 2.0).sin();
    assert!((patch.recovery.rolling_extension_mm - expected_gap).abs() < 1.0e-9);
    let (ir, notes) = transfer(patch);
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(ir.geometry.edges.len(), 4);
    let note = notes
        .iter()
        .find(|note| note.code.local_code() == "geometry.rolling-sheet-extended")
        .expect("rolling extension loss");
    assert!(note.message.contains("1 emitted sheet faces"));
    assert!(note.message.contains("0.004712388 mm"));
    assert!(notes
        .iter()
        .any(|note| note.code.local_code() == "geometry.sheet-surface-fitted"));
}

#[test]
fn rolling_extension_rejects_physical_and_transverse_overruns() {
    let definition = cylinder_section();
    for v_min in [-0.002, -0.02] {
        assert!(sheet(&definition, v_min)
            .recover_rolling_boundary(&CadIr::empty(), None)
            .is_none());
    }
}

#[test]
fn rolling_recovery_retains_the_active_spline_pcurve_interval() {
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    let definition = cylinder_section();
    let mut face = sheet(&definition, -0.0015);
    for trim in &mut face.outer.trims {
        let a = trim.uv(0.0).expect("start");
        let b = trim.uv(1.0).expect("end");
        let at = |t: f64| {
            Point2::new(
                (b[0] - a[0]).mul_add(t, a[0]),
                (b[1] - a[1]).mul_add(t, a[1]),
            )
        };
        trim.pcurve = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![-1.0, -1.0, 2.0, 2.0],
                vec![at(-1.0), at(2.0)],
                None,
                false,
            )
            .expect("extended spline basis"),
        };
    }
    let patch = face
        .recover_rolling_boundary(&CadIr::empty(), None)
        .expect("spline boundary extension");
    assert!(patch
        .sides
        .iter()
        .all(|side| matches!(side.pcurve, PcurveGeometry::Trimmed(_))));
    let (ir, _) = transfer(patch);
    assert_eq!(ir.geometry.faces.len(), 1);
}

#[test]
fn spline_trims_share_the_lifted_edges_clamped_chart() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;

    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1000.0, 0.0)],
                vec![
                    Point3::new(1000.0, 0.0, 0.0),
                    Point3::new(1000.0, 1000.0, 0.0),
                ],
            ],
            None,
        ),
        false,
    )
    .expect("planar tensor");
    let definition = cylinder_section();
    for (axis, native_bound) in [(0, None), (1, None), (1, Some(0.01))] {
        let mut face = sheet(&definition, -5.0e-6);
        face.carrier = Carrier::CachedTensor(&surface);
        for trim in &mut face.outer.trims {
            trim.mapped_range = Some([0.0, 1.0]);
            trim.inferred_accuracy = native_bound.map(|bound_mm| InferredEdgeAccuracy {
                source: 1,
                bound_mm,
            });
            let points = [0.0, 1.0].map(|t| {
                let mut uv = trim.uv(t).expect("trim endpoint");
                if trim.edge != 10 {
                    uv[1] = uv[1].max(0.0);
                }
                if axis == 0 {
                    uv.swap(0, 1);
                }
                Point2::new(uv[0], uv[1])
            });
            trim.pcurve = PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    points.to_vec(),
                    None,
                    false,
                )
                .expect("spline trim"),
            };
        }
        let original_pcurve = face.outer.trims[0].pcurve.clone();
        let patch = fit_ring(
            &CadIr::empty(),
            &face.carrier,
            face.face_pos,
            face.member,
            face.outer.trims,
            &RingFit {
                vertices: None,
                shared_surface: None,
                transverse_recovery: TransverseRecovery::None,
                bounds: None,
            },
            false,
        )
        .expect("clamped spline ring");
        if native_bound.is_some() {
            assert_eq!(
                patch
                    .sides
                    .iter()
                    .find(|side| side.edge == 10)
                    .expect("bottom edge")
                    .pcurve,
                original_pcurve
            );
        }
        for side in &patch.sides {
            for step in 0..=32 {
                let t = f64::from(step) / 32.0;
                let uv = eval::decode::pcurve_uv(EvaluationAdmission::Standard, &side.pcurve, t)
                    .expect("pcurve point")
                    .get();
                if native_bound.is_none() {
                    assert!(uv.u >= 0.0 && uv.u <= 1.0 && uv.v >= 0.0 && uv.v <= 1.0);
                }
                let point = eval::decode::surface_point_solved(
                    EvaluationAdmission::Standard,
                    &patch.surface,
                    uv.u,
                    uv.v,
                )
                .expect("lifted point")
                .get();
                assert!(
                    point.distance(curve_point(&side.curve, t).expect("edge point"))
                        <= GAP_MM + native_bound.unwrap_or(0.0)
                );
            }
        }
        transfer(patch);
    }
}

#[test]
fn spherical_joint_preserves_the_native_station_in_model_space() {
    let definition = cylinder_section();
    let face = sheet(&definition, 0.0);
    let trim = &face.outer.trims[0];
    let world = |[u, v]: [f64; 2]| Some(Point3::new(u.cos() * v.cos(), u.sin() * v.cos(), v.sin()));
    assert!(joint_matches_station(trim, 0, 0.0, world));
    assert!(joint_matches_station(trim, 0, 0.001, world));
    assert!(!joint_matches_station(trim, 0, 0.004, world));
}

#[test]
fn offset_construction_recovers_trims_outside_the_cache_window() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};

    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .expect("planar construction surface");
    let definition = cylinder_section();
    let ir = CadIr::empty();
    for distance in [-0.25, 0.25] {
        let mut face = sheet(&definition, 0.2);
        face.carrier = Carrier::OffsetTensor(&surface, [[0.0, 1.0], [0.0, 0.1]], distance);
        assert!(face
            .outer
            .patch(&ir, &face.carrier, face.face_pos, face.member, None, None)
            .is_none());
        let patch = face
            .patch(&ir, None)
            .expect("offset construction covers the trim");
        assert_eq!(patch.sides.len(), 4);
        let corners = [[0.2, 0.2], [0.8, 0.2], [0.8, 0.5], [0.2, 0.5]];
        for side in patch.sides {
            for (vertex, point) in side.vertices {
                let [u, v] = corners[vertex];
                assert!(point.distance(Point3::new(u, v, distance)) <= GAP_MM);
            }
        }
        let face = DecodedFace {
            outer: Ring {
                bounds: Some(RingBounds {
                    chart: [[0.2, 0.8], [0.2, 0.5]],
                    model: [[0.2, 0.8], [0.2, 0.5], [distance + 1.0, distance + 2.0]],
                }),
                ..face.outer
            },
            ..face
        };
        assert!(face.patch(&ir, None).is_none());
    }
}

#[test]
fn native_bounded_joins_preserve_a_short_connector_between_collinear_sides() {
    let starts = [
        [0.0, 0.0],
        [1.0, -0.0001],
        [1.002, 0.0],
        [2.0, 0.0],
        [2.0, 1.0],
        [0.0, 1.0],
    ];
    let ends = [
        [1.0, 0.0],
        [1.002, 0.0],
        [2.0, 0.0],
        [2.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ];
    let trims: Vec<_> = (0..6)
        .map(|i| Trim {
            edge: i,
            source: i,
            vertices: [i, (i + 1) % 6],
            pcurve: line_pcurve([0.0, 1.0], [starts[i], ends[i]]).expect("bounded trim"),
            range: [0.0, 1.0],
            mapped_range: Some([0.0, 1.0]),
            affine_map: false,
            spline_range: None,
            stations: [None, None],
            primary_stations: [None, None],
            sphere_circle: None,
            alternate_line: None,
            symmetric_line: false,
            inferred_accuracy: None,
        })
        .collect();
    let world = |uv: [f64; 2]| Some(Point3::new(uv[0], uv[1], 0.0));
    assert!(joint_recovery::world_join([&trims[0], &trims[1]], [None; 2], world).is_none());
    let entry = joint_recovery::bounded_world_join([&trims[0], &trims[1]], [None; 2], world)
        .expect("bounded connector entry");
    let exit = joint_recovery::bounded_world_join([&trims[1], &trims[2]], [None; 2], world)
        .expect("bounded connector exit");
    let points = [entry[1], exit[0]]
        .map(|t| world(trims[1].uv(t).expect("connector point")).expect("world point"));
    assert!(points[0].distance(points[1]) > 0.0019);
    for (point, expected) in points.into_iter().zip([starts[1], ends[1]]) {
        assert!(point.distance(Point3::new(expected[0], expected[1], 0.0)) <= GAP_MM);
    }
}

#[test]
fn tensor_stations_join_within_model_space_coincidence() {
    let make_trim = |edge, start, end| Trim {
        edge,
        source: edge,
        vertices: [edge, edge + 1],
        pcurve: line_pcurve([0.0, 1.0], [start, end]).expect("line trim"),
        range: [0.0, 1.0],
        mapped_range: Some([0.0, 1.0]),
        affine_map: false,
        spline_range: None,
        stations: [Some(0.0), Some(1.0)],
        primary_stations: [None, None],
        sphere_circle: None,
        alternate_line: None,
        symmetric_line: false,
        inferred_accuracy: None,
    };
    let a = make_trim(1, [0.0, 0.0], [1.0, 0.0]);
    let b = make_trim(2, [1.0, 0.0008], [1.0, 1.0]);
    let world = |uv: [f64; 2]| Some(Point3::new(uv[0], uv[1], 0.0));
    assert_eq!(
        joint_recovery::tensor_station_join([&a, &b], [Some(1.0), Some(0.0)], world),
        Some([1.0, 0.0])
    );
    let stretched = |uv: [f64; 2]| Some(Point3::new(uv[0], uv[1] * 2.0, 0.0));
    assert!(
        joint_recovery::tensor_station_join([&a, &b], [Some(1.0), Some(0.0)], stretched).is_none()
    );
}

#[test]
fn reversed_parameterizations_retain_one_physical_patch_boundary() {
    let definition = cylinder_section();
    let first = sheet(&definition, 0.2)
        .patch(&CadIr::empty(), None)
        .expect("first patch");
    let mut second = sheet(&definition, 0.2)
        .patch(&CadIr::empty(), None)
        .expect("second patch");
    for side in &mut second.sides {
        side.curve.reverse_parameterization();
        side.vertices.swap(0, 1);
        side.forward = !side.forward;
    }
    assert!(same_patch_boundary(&first, &second));
    second.sides[0].vertices[0].1.z += 2.0 * GAP_MM;
    assert!(!same_patch_boundary(&first, &second));
}

#[test]
fn patch_boundary_comparison_checks_curve_interiors() {
    let definition = cylinder_section();
    let first = sheet(&definition, 0.2)
        .patch(&CadIr::empty(), None)
        .expect("first patch");
    let mut second = sheet(&definition, 0.2)
        .patch(&CadIr::empty(), None)
        .expect("second patch");
    let side = &mut second.sides[0];
    let [(_, a), (_, b)] = side.vertices;
    side.curve = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            a,
            Point3::new(
                a.x.midpoint(b.x),
                a.y.midpoint(b.y),
                a.z.midpoint(b.z) + 0.01,
            ),
            b,
        ],
        None,
        false,
    )
    .expect("different curve with the same endpoints");
    assert!(!same_patch_boundary(&first, &second));
}

#[test]
fn world_join_keeps_distinct_spatial_solutions_ambiguous() {
    let definition = cylinder_section();
    let mut face = sheet(&definition, 0.2);
    face.outer.trims[1].pcurve = face.outer.trims[0].pcurve.clone();
    assert!(joint_recovery::bounded_world_join(
        [&face.outer.trims[0], &face.outer.trims[1]],
        [None, None],
        |uv| Some(Point3::new(uv[0], uv[1], 0.0)),
    )
    .is_none());
}

#[test]
fn cropped_rolling_results_keep_the_native_transverse_parameterization() {
    use crate::test_support::test_a5a8::a5_freeform_curve_stream;
    use crate::wire::records::consolidated_records;
    use cadmpeg_ir::features::geometry::FinitePoint3;

    let bytes = a5_freeform_curve_stream();
    let records = consolidated_records(&bytes);
    let mut jet = a5_owned_rolling_curves(&bytes, &records)
        .next()
        .expect("native rolling chart");
    jet.sites[1].site = jet.sites[0].site.clone();
    let upper = |p: FinitePoint3| {
        FinitePoint3::new(Point3::new(p.get().x, p.get().y, 1.0)).expect("upper rail")
    };
    let site = &mut jet.sites[1].site;
    site.limit1 = upper(site.limit1);
    site.limit2 = upper(site.limit2);
    site.center = upper(site.center);
    for bounds in [[0.0, 2.0], [0.0, 1.5], [0.5, 1.5], [1.0, 1.5]] {
        let carrier = Carrier::rolling_ball_at_radius(
            jet.clone(),
            1.0,
            jet.chart_v_end,
            Some([[0.0, 1.0], bounds]),
        )
        .expect("quarter-cylinder");
        let point = carrier
            .evaluate(carrier.point([0.5, 1.0]))
            .expect("native midpoint");
        let radial = std::f64::consts::FRAC_1_SQRT_2;
        assert!(point.distance(Point3::new(radial, radial, 0.5)) < 1.0e-10);
    }
}

#[test]
fn repeated_implicit_line_direction_preserves_native_endpoint_stations() {
    let definition = cylinder_section();
    let mut face = sheet(&definition, 0.2);
    let corners = [[0.2, 0.2], [0.8, 0.2], [0.8, 0.203], [0.8, 0.5], [0.2, 0.5]];
    let mut trims = Vec::new();
    for i in 0..corners.len() {
        let j = (i + 1) % corners.len();
        let mut trim = face.outer.trims[0].clone();
        trim.edge = 10 + i;
        trim.source = 10 + i;
        trim.vertices = [i, j];
        trim.pcurve = line_pcurve([0.0, 1.0], [corners[i], corners[j]]).expect("ring side");
        if i == 1 || i == 2 {
            trim.source = 11;
            trim.symmetric_line = true;
            trim.pcurve = line_pcurve([0.0, 1.0], [corners[1], corners[3]]).expect("implicit line");
            trim.alternate_line = Some(line_geometry([corners[3], corners[1]]).expect("reverse"));
            trim.stations = if i == 1 {
                [Some(1.0), Some(0.99)]
            } else {
                [Some(0.99), Some(0.0)]
            };
            trim.mapped_range = Some(if i == 1 { [0.99, 1.0] } else { [0.0, 0.99] });
        }
        trims.push(trim);
    }
    face.outer.trims = trims;
    let patch = face
        .patch(&CadIr::empty(), None)
        .expect("station-consistent ring");
    for side in patch.sides {
        for (vertex, point) in side.vertices {
            let expected = face
                .carrier
                .evaluate(corners[vertex])
                .expect("native corner");
            assert!(point.distance(expected) <= GAP_MM);
        }
    }
}

#[test]
fn repeated_line_occurrences_share_one_direction() {
    let definition = cylinder_section();
    let mut face = sheet(&definition, 0.2);
    let corners = [
        [0.2, 0.1],
        [0.8, 0.1],
        [0.8, 0.3],
        [0.5, 0.3],
        [0.5, 0.5],
        [0.8, 0.5],
        [0.8, 0.7],
        [0.2, 0.7],
    ];
    let mut trims = Vec::new();
    for i in 0..corners.len() {
        let j = (i + 1) % corners.len();
        let mut trim = face.outer.trims[0].clone();
        trim.edge = 10 + i;
        trim.source = 10 + i;
        trim.vertices = [i, j];
        trim.stations = [None; 2];
        trim.pcurve = line_pcurve([0.0, 1.0], [corners[i], corners[j]]).expect("ring side");
        if i == 1 || i == 5 {
            trim.source = 11;
            trim.pcurve =
                line_pcurve([0.0, 1.0], [[0.8, -0.4], [0.8, 0.4]]).expect("implicit line");
            trim.alternate_line =
                Some(line_geometry([[0.8, 0.8], [0.8, 0.0]]).expect("alternate direction"));
        }
        trims.push(trim);
    }
    face.outer.trims = trims;
    let independent = fit_with_independent_lines(
        &CadIr::empty(),
        &face.carrier,
        face.face_pos,
        face.member,
        &face.outer.trims,
        &RingFit {
            vertices: None,
            shared_surface: None,
            transverse_recovery: TransverseRecovery::None,
            bounds: None,
        },
        false,
    )
    .expect("equivalent parameterizations preserve one boundary");
    let patch = face
        .patch(&CadIr::empty(), None)
        .expect("coherent repeated line");
    assert!(same_patch_boundary(&independent, &patch));
    for side in patch.sides {
        for (vertex, point) in side.vertices {
            let expected = face
                .carrier
                .evaluate(corners[vertex])
                .expect("native corner");
            assert!(point.distance(expected) <= GAP_MM);
        }
    }
}

#[test]
fn mapped_edge_intervals_locate_a_collinear_ring_vertex() {
    for explicit_joint in [false, true] {
        let mut bytes = vec![0x84, 0x00];
        for value in [
            0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 5.0, -1.0, 3.0,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        let carrier = Carrier::Plane(SpatialPlane::read(&bytes).expect("plane chart"));
        let corners = [[0.0, 0.0], [2.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0]];
        let definition = cylinder_section();
        let template = sheet(&definition, 0.1).outer.trims.remove(0);
        let mut trims = Vec::new();
        for i in 0..corners.len() {
            let j = (i + 1) % corners.len();
            let mut trim = template.clone();
            trim.edge = i;
            trim.source = i;
            trim.vertices = [i, j];
            trim.pcurve =
                PcurveGeometry::Line(line_geometry([corners[i], corners[j]]).expect("edge chart"));
            trim.stations = [Some(0.0), Some(1.0)];
            if i < 2 {
                let endpoints = if i == 0 {
                    [[0.0, 0.0], [3.0, 0.0]]
                } else {
                    [[1.0, 0.0], [4.0, 0.0]]
                };
                trim.pcurve = PcurveGeometry::Line(line_geometry(endpoints).expect("basis line"));
                trim.mapped_range = Some(if i == 0 {
                    [0.0, 2.0 / 3.0]
                } else {
                    [1.0 / 3.0, 1.0]
                });
                trim.stations = if i == 0 {
                    [Some(0.0), explicit_joint.then_some(1.0 / 3.0)]
                } else {
                    [Some(0.0), Some(1.0)]
                };
            }
            trims.push(trim);
        }
        let patch = fit(
            &CadIr::empty(),
            &carrier,
            20,
            Member {
                body: (0, 1),
                forward: true,
            },
            &trims,
            &RingFit {
                vertices: None,
                shared_surface: None,
                transverse_recovery: TransverseRecovery::None,
                bounds: None,
            },
        )
        .expect("mapped collinear boundary");
        for side in patch.sides {
            for (vertex, point) in side.vertices {
                let [mut x, y] = corners[vertex];
                if explicit_joint && vertex == 1 {
                    x = 1.0;
                }
                assert!(point.distance(Point3::new(x, y, 0.0)) <= GAP_MM);
            }
        }
    }
}

#[test]
fn bounded_plane_chart_reports_omitted_bounds_on_emission_and_shell_edge_recovery() {
    let definition = cylinder_section();
    let mut face = sheet(&definition, 0.1);
    let mut bytes = vec![0xd4, 0xa0];
    for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, -1.0, -4.0, 4.0, -6.0, 6.0] {
        bytes.extend(value.to_le_bytes());
    }
    face.carrier = Carrier::Plane(
        super::super::super::sheet_carrier::SpatialPlane::read(&bytes).expect("bounded plane"),
    );
    let patch = face.patch(&CadIr::empty(), None).expect("planar ring");
    assert_eq!(
        patch.recovery.untransferred_plane_bounds,
        Some([[-4.0, 4.0], [-6.0, 6.0]])
    );
    let edges = patch.sides.iter().map(|side| (side.edge, side)).collect();
    let reused = face
        .recover_from_shell_edges(&edges)
        .expect("established planar edges");
    assert_eq!(
        reused.recovery.untransferred_plane_bounds,
        patch.recovery.untransferred_plane_bounds
    );
    for candidate in [patch, reused] {
        let (ir, notes) = transfer(candidate);
        assert_eq!(ir.geometry.faces.len(), 1);
        let notes = notes
            .iter()
            .filter(|note| note.code.local_code() == "geometry.plane-bounds-not-transferred")
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].message.contains("[[-4.0, 4.0], [-6.0, 6.0]]"));
    }
}

#[test]
fn unrestricted_plane_emission_has_no_omitted_bounds_note() {
    let definition = cylinder_section();
    let mut face = sheet(&definition, 0.1);
    let mut bytes = vec![0x08, 0xe8, 0x1d];
    for value in [0.0_f64, 0.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    face.carrier = Carrier::Plane(
        super::super::super::sheet_carrier::SpatialPlane::read(&bytes).expect("unrestricted plane"),
    );
    let patch = face.patch(&CadIr::empty(), None).expect("planar ring");
    assert!(patch.recovery.untransferred_plane_bounds.is_none());
    let (ir, notes) = transfer(patch);
    assert_eq!(ir.geometry.faces.len(), 1);
    assert!(notes
        .iter()
        .all(|note| note.code.local_code() != "geometry.plane-bounds-not-transferred"));
}
