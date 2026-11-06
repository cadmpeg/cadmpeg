// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::design::geometry::AtomicSketchRegion;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalCylinder, AsmHistoricalEdge, AsmHistoricalEdgeSample,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology,
};

use super::{changed_planes, face_cap, outlines_agree, Sampled};

const TOLERANCE: f64 = 1e-6;
/// The cap face every test topology draws.
const CAP: i64 = 100;

fn xy_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:id#cap-sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

fn slot(index: usize) -> i64 {
    i64::try_from(index).unwrap()
}

/// A topology whose cap face has one loop of `edges`, each `(start, end)`
/// vertex pair stored in that direction, at height 2 above the sketch.
fn loop_topology(corners: &[(f64, f64)], edges: &[(usize, usize)]) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: CAP,
            member_refs: vec![200],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 200,
            member_refs: (0..edges.len()).map(|k| 300 + slot(k)).collect(),
        }],
        ..AsmHistoricalTopology::default()
    };
    for (k, (u, v)) in corners.iter().enumerate() {
        let k = slot(k);
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: 500 + k,
            carrier: 600 + k,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: 600 + k,
            position: Point3::new(*u, *v, 2.0),
        });
    }
    for (k, (start, end)) in edges.iter().enumerate() {
        let k = slot(k);
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge: 300 + k,
            owner_loop: 200,
            edge: 400 + k,
            next: 0,
            previous: 0,
            radial_next: 0,
        });
        topology.edge_vertices.push(AsmHistoricalEdge {
            edge: 400 + k,
            start_vertex: 500 + slot(*start),
            end_vertex: 500 + slot(*end),
        });
    }
    topology
}

/// Gives every edge a straight carrier through its stored ends.
fn straight_carriers(topology: &mut AsmHistoricalTopology) {
    for row in topology.edge_vertices.clone() {
        let at = |vertex: i64| {
            let point = topology
                .vertex_points
                .iter()
                .find(|binding| binding.entity == vertex)
                .unwrap()
                .carrier;
            topology
                .point_positions
                .iter()
                .find(|position| position.point == point)
                .unwrap()
                .position
        };
        let (start, end) = (at(row.start_vertex), at(row.end_vertex));
        topology
            .edge_curves
            .push(AsmHistoricalOptionalCarrierBinding {
                entity: row.edge,
                carrier: Some(row.edge + 1000),
            });
        topology.curve_axes.push(AsmHistoricalCurveAxis {
            curve: row.edge + 1000,
            origin: start,
            direction: end.vector_from(start),
        });
    }
}

fn unit_square_corners() -> Vec<(f64, f64)> {
    vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
}

#[test]
fn a_loop_whose_first_edge_is_stored_backwards_keeps_every_corner() {
    // The first edge is stored from corner 1 to corner 0; the loop runs
    // 0 -> 1 -> 2 -> 3, so it must be drawn from 0 to 1 before the rest.
    let mut topology = loop_topology(&unit_square_corners(), &[(1, 0), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    let cap = face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).unwrap();
    assert_eq!(cap.loops[0].len(), 4);
    for corner in unit_square_corners() {
        assert!(
            cap.loops[0]
                .iter()
                .any(|point| (point.u - corner.0).hypot(point.v - corner.1) < 1e-12),
            "{corner:?} missing from {:?}",
            cap.loops[0]
        );
    }
    assert!((cap.area - 1.0).abs() < 1e-12);
}

#[test]
fn a_loop_that_does_not_close_proves_no_cap() {
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 1)]);
    straight_carriers(&mut topology);
    assert!(face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).is_none());
}

#[test]
fn an_edge_without_a_proven_carrier_proves_no_cap() {
    // An edge with no curve axis may be a spline through the same points.
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    topology.curve_axes.remove(2);
    assert!(face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).is_none());

    // A curve axis across the chord is a circle's or an ellipse's.
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    topology.curve_axes[2].direction = Vector3::new(0.0, 0.0, 1.0);
    assert!(face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).is_none());
}

#[test]
fn a_closed_edge_is_a_circle_only_on_a_cylinder_about_the_cap_normal() {
    // One closed edge on a circle of radius 2 about (1, 1).
    let circle_topology = |axis: Vector3| {
        let mut topology = loop_topology(&[(3.0, 1.0)], &[(0, 0)]);
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: 101,
            member_refs: vec![201],
        });
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge: 350,
            owner_loop: 201,
            edge: 400,
            next: 0,
            previous: 0,
            radial_next: 0,
        });
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: 101,
            carrier: 700,
        });
        topology.surface_cylinders.push(AsmHistoricalCylinder {
            surface: 700,
            origin: Point3::new(1.0, 1.0, -5.0),
            axis,
            radius: 2.0,
        });
        topology.edge_samples.push(AsmHistoricalEdgeSample {
            edge: 400,
            interior: Point3::new(-1.0, 1.0, 2.0),
            tangent: None,
        });
        topology
    };
    let cap = face_cap(
        &xy_sketch(),
        &circle_topology(Vector3::new(0.0, 0.0, 1.0)),
        CAP,
        TOLERANCE,
    )
    .unwrap();
    let circle = std::f64::consts::PI * 4.0;
    assert!((cap.area - circle).abs() < 1e-2 * circle);
    assert!(cap.boundary_distance(Point2::new(1.0, 3.0)) < 1e-12);
    assert!(face_cap(
        &xy_sketch(),
        &circle_topology(Vector3::new(1.0, 0.0, 0.0)),
        CAP,
        TOLERANCE,
    )
    .is_none());
}

/// One region of area 1 sampled at the unit square's centre.
fn sampled(region: &AtomicSketchRegion) -> Vec<Sampled<'_>> {
    vec![Sampled {
        region,
        point: Point2::new(0.5, 0.5),
        area: 1.0,
        nested: Vec::new(),
    }]
}

#[test]
fn regions_of_the_caps_area_must_also_share_its_outline() {
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    let cap = face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).unwrap();
    let square = |du: f64| AtomicSketchRegion {
        boundary: Vec::new(),
        polyline: unit_square_corners()
            .iter()
            .map(|(u, v)| Point2::new(u + du, *v))
            .collect(),
        encloses_island: false,
    };
    let same = square(0.0);
    assert!(outlines_agree(&cap, &sampled(&same), &[0], TOLERANCE));
    // Equal area and a shared interior point, but another outline.
    let shifted = square(0.4);
    assert!(!outlines_agree(&cap, &sampled(&shifted), &[0], TOLERANCE));
}

#[test]
fn a_cut_plane_compares_the_faces_it_deleted_with_the_faces_it_inserted() {
    // The cut deletes a 2 x 1 face and inserts a 1 x 1 face on the same
    // plane, so it removed the right half.
    let mut before = loop_topology(
        &[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)],
        &[(0, 1), (1, 2), (2, 3), (3, 0)],
    );
    straight_carriers(&mut before);
    let mut after = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut after);
    for topology in [&mut before, &mut after] {
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: CAP,
            carrier: 800,
        });
        topology
            .surface_planes
            .push(crate::history_records::AsmHistoricalPlane {
                surface: 800,
                origin: Point3::new(0.0, 0.0, 2.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            });
    }
    let faces = crate::history_records::AsmHistoricalEntityDelta {
        inserted: vec![CAP],
        deleted: vec![CAP],
        updated: Vec::new(),
    };
    let planes = changed_planes(&xy_sketch(), (&before, &after), &faces, TOLERANCE).unwrap();
    let [plane] = planes.as_slice() else {
        panic!("one changed plane");
    };
    assert!((plane.offset - 2.0).abs() < 1e-12);
    let held = |point: Point2| plane.before.iter().any(|cap| cap.contains(point));
    let holds = |point: Point2| plane.after.iter().any(|cap| cap.contains(point));
    assert!(held(Point2::new(1.5, 0.5)) && !holds(Point2::new(1.5, 0.5)));
    assert!(held(Point2::new(0.5, 0.5)) && holds(Point2::new(0.5, 0.5)));

    // A changed face on the plane without a proven cap leaves the plane out.
    after.curve_axes.remove(1);
    assert!(
        changed_planes(&xy_sketch(), (&before, &after), &faces, TOLERANCE)
            .unwrap()
            .is_empty()
    );
}

/// A planar face parallel to the XY sketch: its slot, its height, and its
/// loops as corner rings.
type PlaneFace = (i64, f64, Vec<Vec<(f64, f64)>>);

/// Faces whose edges all have straight carriers.
fn plane_faces(faces: &[PlaneFace]) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology::default();
    let mut next = 10_000;
    let mut mint = || {
        next += 1;
        next
    };
    for (face, height, loops) in faces {
        let surface = mint();
        topology.faces.push(*face);
        topology.surfaces.push(surface);
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: *face,
            carrier: surface,
        });
        topology
            .surface_planes
            .push(crate::history_records::AsmHistoricalPlane {
                surface,
                origin: Point3::new(0.0, 0.0, *height),
                normal: Vector3::new(0.0, 0.0, 1.0),
            });
        let mut face_loops = Vec::new();
        for ring in loops {
            let loop_ = mint();
            face_loops.push(loop_);
            topology.loops.push(loop_);
            let vertices: Vec<i64> = ring
                .iter()
                .map(|(u, v)| {
                    let (vertex, point) = (mint(), mint());
                    topology.vertices.push(vertex);
                    topology.points.push(point);
                    topology.vertex_points.push(AsmHistoricalCarrierBinding {
                        entity: vertex,
                        carrier: point,
                    });
                    topology.point_positions.push(AsmHistoricalPoint {
                        point,
                        position: Point3::new(*u, *v, *height),
                    });
                    vertex
                })
                .collect();
            let mut coedges = Vec::new();
            for k in 0..ring.len() {
                let (edge, coedge, curve) = (mint(), mint(), mint());
                let (start, end) = (ring[k], ring[(k + 1) % ring.len()]);
                topology.edges.push(edge);
                topology.coedges.push(coedge);
                topology.curves.push(curve);
                coedges.push(coedge);
                topology.coedge_topology.push(AsmHistoricalCoedge {
                    coedge,
                    owner_loop: loop_,
                    edge,
                    next: 0,
                    previous: 0,
                    radial_next: 0,
                });
                topology.edge_vertices.push(AsmHistoricalEdge {
                    edge,
                    start_vertex: vertices[k],
                    end_vertex: vertices[(k + 1) % ring.len()],
                });
                topology
                    .edge_curves
                    .push(AsmHistoricalOptionalCarrierBinding {
                        entity: edge,
                        carrier: Some(curve),
                    });
                topology.curve_axes.push(AsmHistoricalCurveAxis {
                    curve,
                    origin: Point3::new(start.0, start.1, *height),
                    direction: Vector3::new(end.0 - start.0, end.1 - start.1, 0.0),
                });
            }
            topology.loop_coedges.push(AsmHistoricalRelation {
                owner_ref: loop_,
                member_refs: coedges,
            });
        }
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: *face,
            member_refs: face_loops,
        });
    }
    topology
}

/// A history whose state 2 follows state 1.
fn cut_history(
    before: AsmHistoricalTopology,
    after: AsmHistoricalTopology,
) -> crate::history_records::AsmHistory {
    let state = |state_id: i64, topology| {
        (
            crate::history_records::AsmDeltaState {
                id: format!("synthetic:test:state#{state_id}"),
                parent: "synthetic:test:history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology),
            Vec::new(),
        )
    };
    crate::test_support::history::history(
        "synthetic:test:history".into(),
        0,
        None,
        false,
        vec![state(2, after), state(1, before)],
    )
}

/// A 20 x 20 window centred on the origin, braced by two bars along its
/// diagonals, and the corners of its four openings.
struct BracedWindow {
    sketch: Sketch,
    entities: Vec<cadmpeg_ir::sketches::SketchEntity>,
    openings: [Vec<(f64, f64)>; 4],
}

fn braced_window() -> BracedWindow {
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
    };
    let mut sketch = xy_sketch();
    let lines = [
        ("bottom", (-10.0, -10.0), (10.0, -10.0)),
        ("right", (10.0, -10.0), (10.0, 10.0)),
        ("top", (10.0, 10.0), (-10.0, 10.0)),
        ("left", (-10.0, 10.0), (-10.0, -10.0)),
        ("rising-upper", (-10.0, -8.0), (8.0, 10.0)),
        ("rising-lower", (-8.0, -10.0), (10.0, 8.0)),
        ("falling-upper", (-8.0, 10.0), (10.0, -8.0)),
        ("falling-lower", (-10.0, 8.0), (8.0, -10.0)),
    ];
    let entities: Vec<SketchEntity> = lines
        .iter()
        .map(|(name, start, end)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                })
                .unwrap(),
            )
        })
        .collect();
    // One stored loop that crosses itself and names every curve.
    let stored = entities
        .iter()
        .map(|entity| SketchEntityUse {
            entity: entity.id().clone(),
            reversed: false,
        })
        .collect::<Vec<_>>();
    sketch.profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(vec![stored]).unwrap();
    BracedWindow {
        sketch,
        entities,
        openings: [
            vec![(-8.0, -10.0), (8.0, -10.0), (0.0, -2.0)],
            vec![(10.0, -8.0), (10.0, 8.0), (2.0, 0.0)],
            vec![(8.0, 10.0), (-8.0, 10.0), (0.0, 2.0)],
            vec![(-10.0, 8.0), (-10.0, -8.0), (-2.0, 0.0)],
        ],
    }
}

/// The regions selected when the `cut` openings of `window` are cut through
/// a 40 x 40 plate of thickness 5 whose top face is the sketch plane.
fn cut_selection(
    window: &BracedWindow,
    cut: &[usize],
    work: &crate::design::geometry::ProfileGeometryWork,
) -> Option<super::ResolvedProfileSelection> {
    let plate = vec![(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)];
    let mut holed = vec![plate.clone()];
    holed.extend(cut.iter().map(|index| window.openings[*index].clone()));
    let before = plane_faces(&[(1, 0.0, vec![plate.clone()]), (2, -5.0, vec![plate])]);
    let after = plane_faces(&[(3, 0.0, holed.clone()), (4, -5.0, holed)]);
    let history = cut_history(before, after);
    let ctx = cadmpeg_test_support::service_decode_context();
    let resolution = super::super::ScopedExtrudeProfileResolution {
        entities: &window.entities,
        spatial_entities: &[],
        histories: std::slice::from_ref(&history),
        linear_tolerance: TOLERANCE,
        angular_tolerance: 1e-9,
        profile_work: work,
        ctx: &ctx,
    };
    super::removed_profile_selection(
        &window.sketch,
        resolution,
        2,
        1,
        Some(&super::ResolvedProfileSelection::Loops(vec![0])),
    )
    .unwrap()
}

#[test]
fn a_cut_through_an_x_braced_window_selects_its_four_openings() {
    use crate::loss::F3dLossCode;
    let window = braced_window();
    let work = crate::design::geometry::ProfileGeometryWork::new();
    let Some(super::ResolvedProfileSelection::Regions(regions)) =
        cut_selection(&window, &[0, 1, 2, 3], &work)
    else {
        panic!("sketch regions");
    };
    assert_eq!(regions.len(), 4);
    let mut sides = Vec::new();
    for region in &regions {
        let cadmpeg_ir::features::SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        // Each opening is bounded by one window side and one line of each bar.
        let names: Vec<String> = outer_boundary
            .iter()
            .map(|use_| {
                let id = use_.entity.to_string();
                id.rsplit('#').next().unwrap().to_owned()
            })
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
        let count = |prefix: &str| names.iter().filter(|name| name.starts_with(prefix)).count();
        assert_eq!((count("rising"), count("falling")), (1, 1), "{names:?}");
        sides.extend(
            names
                .into_iter()
                .filter(|name| ["bottom", "right", "top", "left"].contains(&name.as_str())),
        );
    }
    sides.sort();
    assert_eq!(sides, ["bottom", "left", "right", "top"]);
    work.finish_selection();
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureProfileCutRegionsAssumed.kind()
    );
    assert!(loss.message.starts_with("1 extrude profile selection(s)"));
}

#[test]
fn a_cut_that_leaves_stored_profile_curves_unused_selects_no_regions() {
    let window = braced_window();
    let work = crate::design::geometry::ProfileGeometryWork::new();
    // The bottom and top openings alone leave the left and right sides
    // bounding no cut region.
    assert!(cut_selection(&window, &[0, 2], &work).is_none());
    work.finish_selection();
    assert!(work.losses().is_empty());
}
