//! Hole-axis topology and cylinder-span tests.

use super::{cylinder, lane, model_hole, native_history, profile_reference_plane_payload};
use std::collections::HashMap;

use cadmpeg_ir::features::{
    Angle, FeatureDefinition, FeatureId, HoleBottom, HoleKind, HolePlacement, Length,
    LinearTermination,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Sense, Vertex};

use super::super::super::compact_reference_planes::CompactReferencePlaneIndex;
use super::super::super::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
use super::super::*;

#[test]
fn midplane_sketch_uses_component_basis_and_never_arbitrary_datum_axis() {
    let plane_frame = SketchPlaneFrame::from_frame(
        (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        SketchPlaneUAxisSource::ConstructedMidPlane,
    );
    let frames = HashMap::from([(2, plane_frame)]);

    let with_component = profile_reference_plane_payload(true);
    let index = CompactReferencePlaneIndex::new(&with_component);
    assert_eq!(
        feature_input_sketch_frame(&with_component, &frames, &index, 0, 0, with_component.len(),),
        Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    let without_component = profile_reference_plane_payload(false);
    let index = CompactReferencePlaneIndex::new(&without_component);
    assert_eq!(
        feature_input_sketch_frame(
            &without_component,
            &frames,
            &index,
            0,
            0,
            without_component.len(),
        ),
        None
    );
}

#[test]
fn cylindrical_support_point_defines_its_radial_axis() {
    let surface = Surface {
        id: SurfaceId::mint("support").expect("identity grammar"),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 13.0,
        },
        source_object: None,
    };

    assert_eq!(
        cylindrical_support_normal(&surface, Point3::new(12.0, 5.0, 40.0)),
        Some(Vector3::new(12.0 / 13.0, 5.0 / 13.0, 0.0))
    );
    assert!(cylindrical_support_normal(&surface, Point3::new(12.0, 4.0, 40.0)).is_none());
}

#[test]
fn position_plane_owns_only_reversed_normal_cylinders() {
    let mut surfaces = [cylinder(0, -5.0), cylinder(1, 5.0), cylinder(2, -5.0)];
    let SurfaceGeometry::Cylinder { origin, .. } = &mut surfaces[2].geometry else {
        unreachable!();
    };
    origin.z = 20.0;
    let mut faces = [
        Face {
            id: FaceId::mint("bore").expect("identity grammar"),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: surfaces[0].id.clone(),
            sense: Sense::Reversed,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        },
        Face {
            id: FaceId::mint("boss").expect("identity grammar"),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: surfaces[1].id.clone(),
            sense: Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        },
        Face {
            id: FaceId::mint("coaxial-bore-segment").expect("identity grammar"),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: surfaces[2].id.clone(),
            sense: Sense::Reversed,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        },
    ];

    assert_eq!(
        plane_owned_bore_placements(
            Point3::new(0.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, 1.0),
            2.0,
            &HoleTopology {
                surfaces: &surfaces,
                faces: &faces,
                loops: &[],
                coedges: &[],
                edges: &[],
                vertices: &[],
                points: &[],
            },
        ),
        Some(vec![cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(-5.0, 0.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        }])
    );
    assert_eq!(
        bore_carrier_placements(
            2.0,
            &HoleTopology {
                surfaces: &surfaces,
                faces: &faces,
                loops: &[],
                coedges: &[],
                edges: &[],
                vertices: &[],
                points: &[],
            },
        ),
        Some(vec![cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(-5.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        }])
    );
    faces[1].sense = Sense::Reversed;
    assert_eq!(
        bore_carrier_placements(
            2.0,
            &HoleTopology {
                surfaces: &surfaces,
                faces: &faces,
                loops: &[],
                coedges: &[],
                edges: &[],
                vertices: &[],
                points: &[],
            },
        ),
        Some(vec![
            cadmpeg_ir::features::HolePlacement::Axis {
                origin: Point3::new(-5.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            cadmpeg_ir::features::HolePlacement::Axis {
                origin: Point3::new(5.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
        ])
    );
}

#[test]
fn generated_face_identities_resolve_primary_bore_axes() {
    let mut surfaces = [
        cylinder(0, -5.0),
        cylinder(1, 5.0),
        cylinder(2, 20.0),
        cylinder(3, 30.0),
    ];
    let SurfaceGeometry::Cylinder { radius, .. } = &mut surfaces[3].geometry else {
        unreachable!();
    };
    *radius = 3.0;
    let faces = surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| Face {
            id: FaceId::mint(format!("face-{index}")).expect("identity grammar"),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: surface.id.clone(),
            sense: Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        })
        .collect::<Vec<_>>();
    let identities = [
        (faces[0].id.0.clone(), 7, 2),
        (faces[1].id.0.clone(), 7, 2),
        (faces[2].id.0.clone(), 7, 3),
        (faces[3].id.0.clone(), 7, 2),
    ];
    let mut hole = model_hole();
    project_generated_hole_axes(
        std::slice::from_mut(&mut hole),
        &[native_history()],
        &[lane()],
        &identities,
        &faces,
        &surfaces,
    );
    let FeatureDefinition::Hole { placements, .. } = &mut hole.definition else {
        unreachable!();
    };
    assert_eq!(placements.as_deref().map(<[_]>::len), Some(2));

    *placements = None;
    let mut conflicting_lane = lane();
    for identity in &mut conflicting_lane.generated_surface_identities {
        identity.local_identity = 3;
    }
    project_generated_hole_axes(
        std::slice::from_mut(&mut hole),
        &[native_history()],
        &[lane(), conflicting_lane],
        &identities,
        &faces,
        &surfaces,
    );
    let FeatureDefinition::Hole { placements, .. } = &hole.definition else {
        unreachable!();
    };
    assert!(placements.is_none());
}

#[test]
fn counterbore_topology_assigns_unique_and_partitions_siblings() {
    let mut surfaces = [
        cylinder(0, -5.0),
        cylinder(1, 5.0),
        cylinder(2, 20.0),
        cylinder(3, -5.0),
        cylinder(4, 5.0),
        cylinder(5, 20.0),
    ];
    for surface in &mut surfaces[3..] {
        let SurfaceGeometry::Cylinder { radius, .. } = &mut surface.geometry else {
            unreachable!();
        };
        *radius = 3.0;
    }
    let faces = surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| Face {
            id: FaceId::mint(format!("face-{index}")).expect("identity grammar"),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: surface.id.clone(),
            sense: Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        })
        .collect::<Vec<_>>();
    let topology = HoleTopology {
        surfaces: &surfaces,
        faces: &faces,
        loops: &[],
        coedges: &[],
        edges: &[],
        vertices: &[],
        points: &[],
    };
    let mut placed = model_hole();
    placed.id = FeatureId("placed".into());
    let FeatureDefinition::Hole {
        placements,
        construction,
        ..
    } = &mut placed.definition
    else {
        unreachable!();
    };
    let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
        panic!("ordinary hole form");
    };
    *kind = HoleKind::Counterbore {
        diameter: Length(6.0),
        depth: Length(1.0),
    };
    placements
        .get_or_insert_default()
        .push(HolePlacement::Axis {
            origin: Point3::new(-5.0, 0.0, 100.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
        });
    let mut unplaced = model_hole();
    unplaced.id = FeatureId("unplaced".into());
    let FeatureDefinition::Hole { construction, .. } = &mut unplaced.definition else {
        unreachable!();
    };
    let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
        panic!("ordinary hole form");
    };
    *kind = HoleKind::Counterbore {
        diameter: Length(6.0),
        depth: Length(1.0),
    };

    let mut unique = [unplaced.clone()];
    project_hole_topology_axes(&mut unique, &topology);
    let FeatureDefinition::Hole { placements, .. } = &unique[0].definition else {
        unreachable!();
    };
    assert_eq!(placements.as_deref().map(<[_]>::len), Some(3));

    let mut features = [placed.clone(), unplaced.clone()];
    project_hole_topology_axes(&mut features, &topology);
    let FeatureDefinition::Hole { placements, .. } = &features[1].definition else {
        unreachable!();
    };
    assert_eq!(
        placements.as_deref(),
        Some(
            &[
                HolePlacement::Axis {
                    origin: Point3::new(5.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                HolePlacement::Axis {
                    origin: Point3::new(20.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
            ][..]
        )
    );

    let mut ambiguous = [placed.clone(), unplaced.clone(), unplaced.clone()];
    ambiguous[2].id = FeatureId("also-unplaced".into());
    project_hole_topology_axes(&mut ambiguous, &topology);
    let FeatureDefinition::Hole { placements, .. } = &ambiguous[1].definition else {
        unreachable!();
    };
    assert!(placements.is_none());

    let mut unmatched_surfaces = surfaces.clone();
    let SurfaceGeometry::Cylinder { radius, .. } = &mut unmatched_surfaces[5].geometry else {
        unreachable!();
    };
    *radius = 4.0;
    let unmatched_topology = HoleTopology {
        surfaces: &unmatched_surfaces,
        faces: &faces,
        loops: &[],
        coedges: &[],
        edges: &[],
        vertices: &[],
        points: &[],
    };
    let mut unmatched_signature = [placed.clone(), unplaced.clone()];
    project_hole_topology_axes(&mut unmatched_signature, &unmatched_topology);
    let FeatureDefinition::Hole { placements, .. } = &unmatched_signature[1].definition else {
        unreachable!();
    };
    assert!(placements.is_none());

    let FeatureDefinition::Hole { placements, .. } = &mut placed.definition else {
        unreachable!();
    };
    placements.as_mut().expect("seeded placement")[0] = HolePlacement::Axis {
        origin: Point3::new(-50.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
    };
    let mut incomplete_topology = [placed, unplaced];
    project_hole_topology_axes(&mut incomplete_topology, &topology);
    let FeatureDefinition::Hole { placements, .. } = &incomplete_topology[1].definition else {
        unreachable!();
    };
    assert!(placements.is_none());
}

#[test]
fn hole_topology_uses_exact_cylinder_spans() {
    let surface = Surface {
        id: SurfaceId::mint("surface").expect("identity grammar"),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    };
    let cone = Surface {
        id: SurfaceId::mint("cone").expect("identity grammar"),
        geometry: SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, -10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
            ratio: 1.0,
            half_angle: 1.0,
        },
        source_object: None,
    };
    let face = Face {
        id: FaceId::mint("face").expect("identity grammar"),
        shell: ShellId::mint("shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: vec![LoopId::mint("loop").expect("identity grammar")],
        name: None,
        color: None,
        tolerance: None,
    };
    let loop_ = Loop {
        id: LoopId::mint("loop").expect("identity grammar"),
        face: face.id.clone(),
        boundary_role: LoopBoundaryRole::Outer,
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: vec![CoedgeId::mint("coedge").expect("identity grammar")],
            vertex_uses: Vec::new(),
        },
    };
    let coedge = Coedge {
        id: CoedgeId::mint("coedge").expect("identity grammar"),
        owner_loop: loop_.id.clone(),
        edge: EdgeId::mint("edge").expect("identity grammar"),
        next: CoedgeId::mint("coedge").expect("identity grammar"),
        previous: CoedgeId::mint("coedge").expect("identity grammar"),
        radial_next: CoedgeId::mint("coedge").expect("identity grammar"),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    };
    let edge = Edge {
        id: EdgeId::mint("edge").expect("identity grammar"),
        curve: None,
        start: VertexId::mint("start").expect("identity grammar"),
        end: VertexId::mint("end").expect("identity grammar"),
        param_range: None,
        tolerance: None,
    };
    let vertices = [
        Vertex {
            id: VertexId::mint("start").expect("identity grammar"),
            point: PointId::mint("start-point").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("end").expect("identity grammar"),
            point: PointId::mint("end-point").expect("identity grammar"),
            tolerance: None,
        },
    ];
    let points = [
        Point {
            id: PointId::mint("start-point").expect("identity grammar"),
            position: Point3::new(2.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId::mint("end-point").expect("identity grammar"),
            position: Point3::new(2.0, 0.0, -10.0),
            source_object: None,
        },
    ];

    let mut bore_face = face;
    bore_face.sense = Sense::Reversed;
    let surfaces = [surface, cone];
    let faces = [bore_face];
    let loops = [loop_];
    let coedges = [coedge];
    let edges = [edge];
    let topology = HoleTopology {
        surfaces: &surfaces,
        faces: &faces,
        loops: &loops,
        coedges: &coedges,
        edges: &edges,
        vertices: &vertices,
        points: &points,
    };

    let mut unplaced = model_hole();
    let FeatureDefinition::Hole { extent, bottom, .. } = &mut unplaced.definition else {
        unreachable!();
    };
    *extent = Some(LinearTermination::Blind {
        length: Length(10.0),
    });
    *bottom = Some(HoleBottom::Flat);
    let mut exact = [unplaced.clone()];
    project_hole_topology_axes(&mut exact, &topology);
    let FeatureDefinition::Hole { placements, .. } = &exact[0].definition else {
        unreachable!();
    };
    assert_eq!(placements.as_deref().map(<[_]>::len), Some(1));

    let mut ambiguous = [unplaced.clone(), unplaced.clone()];
    ambiguous[1].id = FeatureId("second-hole".into());
    project_hole_topology_axes(&mut ambiguous, &topology);
    let FeatureDefinition::Hole { placements, .. } = &ambiguous[0].definition else {
        unreachable!();
    };
    assert!(placements.is_none());

    let FeatureDefinition::Hole { extent, .. } = &mut unplaced.definition else {
        unreachable!();
    };
    *extent = Some(LinearTermination::Blind {
        length: Length(9.0),
    });
    project_hole_topology_axes(std::slice::from_mut(&mut unplaced), &topology);
    let FeatureDefinition::Hole { placements, .. } = &unplaced.definition else {
        unreachable!();
    };
    assert!(placements.is_none());

    let mut drilled = model_hole();
    let FeatureDefinition::Hole {
        construction,
        extent,
        bottom,
        ..
    } = &mut drilled.definition
    else {
        unreachable!();
    };
    let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
        panic!("ordinary hole form");
    };
    *kind = HoleKind::SimpleDrilled {
        drill_point_angle: Angle(2.0),
    };
    *extent = Some(LinearTermination::Blind {
        length: Length(10.0),
    });
    *bottom = Some(HoleBottom::Angled {
        included_angle: Angle(2.0),
        depth_to_tip: false,
    });
    project_hole_topology_axes(std::slice::from_mut(&mut drilled), &topology);
    let FeatureDefinition::Hole { placements, .. } = &drilled.definition else {
        unreachable!();
    };
    assert_eq!(placements.as_deref().map(<[_]>::len), Some(1));

    let mut wrong_surfaces = surfaces.clone();
    let SurfaceGeometry::Cone { half_angle, .. } = &mut wrong_surfaces[1].geometry else {
        unreachable!();
    };
    *half_angle = 0.5;
    let wrong_topology = HoleTopology {
        surfaces: &wrong_surfaces,
        faces: &faces,
        loops: &loops,
        coedges: &coedges,
        edges: &edges,
        vertices: &vertices,
        points: &points,
    };
    let FeatureDefinition::Hole { placements, .. } = &mut drilled.definition else {
        unreachable!();
    };
    *placements = None;
    project_hole_topology_axes(std::slice::from_mut(&mut drilled), &wrong_topology);
    let FeatureDefinition::Hole { placements, .. } = &drilled.definition else {
        unreachable!();
    };
    assert!(placements.is_none());

    let mut hole = model_hole();
    let FeatureDefinition::Hole {
        placements,
        diameter,
        ..
    } = &mut hole.definition
    else {
        unreachable!();
    };
    placements
        .get_or_insert_default()
        .push(HolePlacement::Axis {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        });
    *diameter = None;
    project_topological_hole_constructions(std::slice::from_mut(&mut hole), &topology);
    let FeatureDefinition::Hole {
        diameter, extent, ..
    } = hole.definition
    else {
        unreachable!();
    };
    assert_eq!(diameter, Some(Length(4.0)));
    assert_eq!(
        extent,
        Some(LinearTermination::Blind {
            length: Length(10.0)
        })
    );
}

#[test]
fn seeded_hole_axes_partition_complete_topology_by_distinct_directions() {
    let placement = |x, y, axis| HolePlacement::Axis {
        origin: Point3::new(x, y, 0.0),
        axis,
    };
    let x_axis = Vector3::new(1.0, 0.0, 0.0);
    let y_axis = Vector3::new(0.0, 1.0, 0.0);
    let mut horizontal = model_hole();
    horizontal.id = FeatureId("horizontal".into());
    let FeatureDefinition::Hole {
        placements,
        construction,
        extent,
        bottom,
        ..
    } = &mut horizontal.definition
    else {
        unreachable!();
    };
    let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
        panic!("ordinary hole form");
    };
    *kind = HoleKind::SimpleDrilled {
        drill_point_angle: Angle(2.0),
    };
    *extent = Some(LinearTermination::Blind {
        length: Length(10.0),
    });
    *bottom = Some(HoleBottom::Angled {
        included_angle: Angle(2.0),
        depth_to_tip: false,
    });
    placements
        .get_or_insert_default()
        .push(placement(0.0, 30.0, x_axis));
    let mut vertical = horizontal.clone();
    vertical.id = FeatureId("vertical".into());
    let FeatureDefinition::Hole { placements, .. } = &mut vertical.definition else {
        unreachable!();
    };
    *placements = Some(vec![placement(-20.0, 0.0, y_axis)]);
    let candidates = vec![
        placement(0.0, -10.0, x_axis),
        placement(0.0, 30.0, x_axis),
        placement(0.0, 50.0, x_axis),
        placement(-20.0, 0.0, y_axis),
        placement(20.0, 0.0, y_axis),
    ];
    let mut features = [horizontal.clone(), vertical.clone()];

    partition_seeded_hole_axes(&mut features, &[0, 1], &candidates);

    let FeatureDefinition::Hole {
        placements: horizontal_placements,
        ..
    } = &features[0].definition
    else {
        unreachable!();
    };
    let FeatureDefinition::Hole {
        placements: vertical_placements,
        ..
    } = &features[1].definition
    else {
        unreachable!();
    };
    assert_eq!(horizontal_placements.as_deref().map(<[_]>::len), Some(3));
    assert_eq!(vertical_placements.as_deref().map(<[_]>::len), Some(2));

    let mut incomplete = [horizontal.clone(), vertical.clone()];
    let mut candidates_with_unowned_direction = candidates;
    candidates_with_unowned_direction.push(placement(0.0, 0.0, Vector3::new(0.0, 0.0, 1.0)));
    partition_seeded_hole_axes(&mut incomplete, &[0, 1], &candidates_with_unowned_direction);
    let FeatureDefinition::Hole { placements, .. } = &incomplete[0].definition else {
        unreachable!();
    };
    assert_eq!(placements.as_deref().map(<[_]>::len), Some(1));

    let FeatureDefinition::Hole { placements, .. } = &mut vertical.definition else {
        unreachable!();
    };
    *placements = Some(vec![placement(20.0, 0.0, x_axis)]);
    let mut ambiguous = [horizontal, vertical];
    partition_seeded_hole_axes(&mut ambiguous, &[0, 1], &candidates_with_unowned_direction);
    let FeatureDefinition::Hole { placements, .. } = &ambiguous[0].definition else {
        unreachable!();
    };
    assert_eq!(placements.as_deref().map(<[_]>::len), Some(1));
}

#[test]
fn seeded_drilled_bore_candidates_exclude_claimed_axes_and_unresolved_competitors() {
    let axes = [
        (Point3::new(0.0, -10.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (Point3::new(0.0, 30.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (Point3::new(-20.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        (Point3::new(70.0, 80.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
    ];
    let surfaces = axes
        .iter()
        .enumerate()
        .map(|(index, (origin, axis))| Surface {
            id: SurfaceId::mint(format!("seed-surface-{index}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Cylinder {
                origin: *origin,
                axis: *axis,
                ref_direction: if axis.z.abs() == 1.0 {
                    Vector3::new(1.0, 0.0, 0.0)
                } else {
                    Vector3::new(0.0, 0.0, 1.0)
                },
                radius: 2.0,
            },
            source_object: None,
        })
        .collect::<Vec<_>>();
    let faces = surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| Face {
            id: FaceId::mint(format!("seed-face-{index}")).expect("identity grammar"),
            shell: ShellId::mint("shell").expect("identity grammar"),
            surface: surface.id.clone(),
            sense: Sense::Reversed,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        })
        .collect::<Vec<_>>();
    let topology = HoleTopology {
        surfaces: &surfaces,
        faces: &faces,
        loops: &[],
        coedges: &[],
        edges: &[],
        vertices: &[],
        points: &[],
    };
    let placement = |origin, axis| HolePlacement::Axis { origin, axis };
    let mut horizontal = model_hole();
    horizontal.id = FeatureId("horizontal".into());
    let FeatureDefinition::Hole { placements, .. } = &mut horizontal.definition else {
        unreachable!();
    };
    placements
        .get_or_insert_default()
        .push(placement(axes[0].0, axes[0].1));
    let mut vertical = model_hole();
    vertical.id = FeatureId("vertical".into());
    let FeatureDefinition::Hole { placements, .. } = &mut vertical.definition else {
        unreachable!();
    };
    placements
        .get_or_insert_default()
        .push(placement(axes[2].0, axes[2].1));
    let mut other = model_hole();
    other.id = FeatureId("other".into());
    let FeatureDefinition::Hole { placements, .. } = &mut other.definition else {
        unreachable!();
    };
    placements
        .get_or_insert_default()
        .push(placement(axes[3].0, axes[3].1));
    let mut features = [horizontal, vertical, other];

    let candidates = seeded_drilled_bore_candidates(&features, &[0, 1], 4.0, &topology)
        .expect("complete competing ownership");

    assert_eq!(candidates.len(), 3);
    let claimed = hole_axis_key(&placement(axes[3].0, axes[3].1)).unwrap();
    assert!(candidates
        .iter()
        .all(|candidate| hole_axis_key(candidate) != Some(claimed)));

    let FeatureDefinition::Hole { placements, .. } = &mut features[2].definition else {
        unreachable!();
    };
    *placements = None;
    assert!(seeded_drilled_bore_candidates(&features, &[0, 1], 4.0, &topology).is_none());
}
