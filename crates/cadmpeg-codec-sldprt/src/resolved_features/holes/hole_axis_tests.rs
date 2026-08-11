//! Tests for the `holes` module.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::features::{
    Angle, FeatureDefinition, FeatureId, HoleBottom, HoleKind, HolePlacement, Length, Termination,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SpatialSketch,
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
};
use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Sense, Vertex};

use super::super::compact_reference_planes::CompactReferencePlaneIndex;
use super::super::curves::{SketchPlaneFrame, SketchPlaneUAxisSource};
use super::*;
use crate::records::{
    FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputGeneratedSurfaceIdentity,
    FeatureInputLane, FeatureInputName, FeatureInputRelationFamily, FeatureInputScalar,
    FeatureInputScalarRole, SketchInputEntity, SketchInputKind, SketchRelationKind,
};

fn profile_reference_plane_payload(with_component_frame: bool) -> Vec<u8> {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 11]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(19u32.to_le_bytes());
    payload.extend([0, 0, 3, 0]);
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 4]);
    if with_component_frame {
        let mut component = [0u8; 138];
        component[..4].copy_from_slice(&2u32.to_le_bytes());
        component[14] = 1;
        for (index, value) in [
            0.0f64, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            let offset = 15 + index * 8;
            component[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        component[122..126].copy_from_slice(&4u32.to_le_bytes());
        component[126..130].copy_from_slice(&[0xff; 4]);
        payload.extend(component);
    }
    payload
}

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

fn model_hole() -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: FeatureId("hole".into()),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            position: None,
            direction: None,
            placements: Vec::new(),
            kind: HoleKind::Simple,
            exit_kind: None,
            diameter: Some(Length(4.0)),
            extent: None,
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
        native_ref: Some("native-hole".into()),
    }
}

fn native_history() -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::records::Feature {
            id: "native-hole".into(),
            parent: "history".into(),
            xml_tag: "HoleWizard".into(),
            tree_parent: None,
            source_id: Some("7".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "Hole".into(),
            kind: "HoleWizard".into(),
            input_class: Some("moHoleWzd_c".into()),
            suppressed: false,
            parameters: BTreeMap::default(),
            dimension_properties: BTreeMap::default(),
            properties: BTreeMap::default(),
            text: None,
            content: Vec::new(),
        }],
    }
}

fn lane() -> FeatureInputLane {
    let identity = |ordinal| FeatureInputGeneratedSurfaceIdentity {
        id: format!("identity-{ordinal}"),
        parent: "lane".into(),
        ordinal,
        offset: u64::from(ordinal),
        type_prefix: [0xc3, 0x80, 0xc5, 0],
        feature_source_id: 7,
        local_identity: 2,
        components: Vec::new(),
    };
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: vec![identity(0), identity(1)],
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn lane_with_position_reference(position_source: u32) -> FeatureInputLane {
    let mut lane = lane();
    lane.native_payload.resize(200, 0);
    lane.names.push(FeatureInputName {
        id: "hole-name".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        value: "Hole".into(),
        object_id: Some(7),
    });
    let trailer = 6 + "Hole".encode_utf16().count() * 2;
    lane.native_payload[trailer..trailer + 8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    lane.native_payload[trailer + 8..trailer + 12].copy_from_slice(&7u32.to_le_bytes());
    lane.native_payload[trailer + 48..trailer + 50].copy_from_slice(&[0, 0xc0]);
    lane.native_payload[trailer + 50..trailer + 54].copy_from_slice(&position_source.to_le_bytes());
    lane
}

fn cylinder(id: usize, x: f64) -> Surface {
    Surface {
        id: SurfaceId(format!("surface-{id}")),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(x, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    }
}

#[test]
fn cylindrical_support_point_defines_its_radial_axis() {
    let surface = Surface {
        id: SurfaceId("support".into()),
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
            id: FaceId("bore".into()),
            shell: ShellId("shell".into()),
            surface: surfaces[0].id.clone(),
            sense: Sense::Reversed,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        },
        Face {
            id: FaceId("boss".into()),
            shell: ShellId("shell".into()),
            surface: surfaces[1].id.clone(),
            sense: Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        },
        Face {
            id: FaceId("coaxial-bore-segment".into()),
            shell: ShellId("shell".into()),
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
            id: FaceId(format!("face-{index}")),
            shell: ShellId("shell".into()),
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
    assert_eq!(placements.len(), 2);

    placements.clear();
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
    assert!(placements.is_empty());
}

#[test]
fn identical_hole_siblings_partition_unclaimed_bore_axes() {
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
            id: FaceId(format!("face-{index}")),
            shell: ShellId("shell".into()),
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
    let mut placed = model_hole();
    placed.id = FeatureId("placed".into());
    let FeatureDefinition::Hole {
        placements, kind, ..
    } = &mut placed.definition
    else {
        unreachable!();
    };
    *kind = HoleKind::Counterbore {
        diameter: Length(6.0),
        depth: Length(1.0),
    };
    placements.push(HolePlacement::Axis {
        origin: Point3::new(-5.0, 0.0, 100.0),
        axis: Vector3::new(0.0, 0.0, -1.0),
    });
    let mut unplaced = model_hole();
    unplaced.id = FeatureId("unplaced".into());
    let FeatureDefinition::Hole { kind, .. } = &mut unplaced.definition else {
        unreachable!();
    };
    *kind = HoleKind::Counterbore {
        diameter: Length(6.0),
        depth: Length(1.0),
    };

    let mut features = [placed.clone(), unplaced.clone()];
    project_partitioned_hole_axes(&mut features, &topology);
    let FeatureDefinition::Hole { placements, .. } = &features[1].definition else {
        unreachable!();
    };
    assert_eq!(
        placements,
        &[
            HolePlacement::Axis {
                origin: Point3::new(5.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            HolePlacement::Axis {
                origin: Point3::new(20.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
        ]
    );

    let mut ambiguous = [placed.clone(), unplaced.clone(), unplaced.clone()];
    ambiguous[2].id = FeatureId("also-unplaced".into());
    project_partitioned_hole_axes(&mut ambiguous, &topology);
    let FeatureDefinition::Hole { placements, .. } = &ambiguous[1].definition else {
        unreachable!();
    };
    assert!(placements.is_empty());

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
    project_partitioned_hole_axes(&mut unmatched_signature, &unmatched_topology);
    let FeatureDefinition::Hole { placements, .. } = &unmatched_signature[1].definition else {
        unreachable!();
    };
    assert!(placements.is_empty());

    let FeatureDefinition::Hole { placements, .. } = &mut placed.definition else {
        unreachable!();
    };
    placements[0] = HolePlacement::Axis {
        origin: Point3::new(-50.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
    };
    let mut incomplete_topology = [placed, unplaced];
    project_partitioned_hole_axes(&mut incomplete_topology, &topology);
    let FeatureDefinition::Hole { placements, .. } = &incomplete_topology[1].definition else {
        unreachable!();
    };
    assert!(placements.is_empty());
}

#[test]
fn topological_hole_projection_uses_a_reversed_bore_span() {
    let surface = Surface {
        id: SurfaceId("surface".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    };
    let face = Face {
        id: FaceId("face".into()),
        shell: ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: vec![LoopId("loop".into())],
        name: None,
        color: None,
        tolerance: None,
    };
    let loop_ = Loop {
        id: LoopId("loop".into()),
        face: face.id.clone(),
        boundary_role: LoopBoundaryRole::Outer,
        coedges: vec![CoedgeId("coedge".into())],
        vertex_uses: Vec::new(),
    };
    let coedge = Coedge {
        id: CoedgeId("coedge".into()),
        owner_loop: loop_.id.clone(),
        edge: EdgeId("edge".into()),
        next: CoedgeId("coedge".into()),
        previous: CoedgeId("coedge".into()),
        radial_next: CoedgeId("coedge".into()),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
        use_curve_parameter_range: None,
    };
    let edge = Edge {
        id: EdgeId("edge".into()),
        curve: None,
        start: VertexId("start".into()),
        end: VertexId("end".into()),
        param_range: None,
        tolerance: None,
    };
    let vertices = [
        Vertex {
            id: VertexId("start".into()),
            point: PointId("start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("end".into()),
            point: PointId("end-point".into()),
            tolerance: None,
        },
    ];
    let points = [
        Point {
            id: PointId("start-point".into()),
            position: Point3::new(2.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("end-point".into()),
            position: Point3::new(2.0, 0.0, -10.0),
            source_object: None,
        },
    ];

    let mut bore_face = face;
    bore_face.sense = Sense::Reversed;
    let mut hole = model_hole();
    let FeatureDefinition::Hole {
        placements,
        diameter,
        ..
    } = &mut hole.definition
    else {
        unreachable!();
    };
    placements.push(HolePlacement::Axis {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
    });
    *diameter = None;
    project_topological_hole_constructions(
        std::slice::from_mut(&mut hole),
        &HoleTopology {
            surfaces: &[surface],
            faces: &[bore_face],
            loops: &[loop_],
            coedges: &[coedge],
            edges: &[edge],
            vertices: &vertices,
            points: &points,
        },
    );
    let FeatureDefinition::Hole {
        diameter, extent, ..
    } = hole.definition
    else {
        unreachable!();
    };
    assert_eq!(diameter, Some(Length(4.0)));
    assert_eq!(
        extent,
        Some(Termination::Blind {
            length: Length(10.0)
        })
    );
}

fn profile_line(sketch: &SketchId, ordinal: usize, start: Point2, end: Point2) -> SketchEntity {
    SketchEntity {
        id: SketchEntityId(format!("profile-line-{ordinal}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    }
}

#[test]
fn axial_profile_resolves_counterbore_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "118°".into()),
        ("b".into(), "5.7".into()),
        ("c".into(), "<MOD-DIAM>10".into()),
        ("d".into(), "15".into()),
        ("e".into(), "<MOD-DIAM>5.5".into()),
    ]
    .into_iter()
    .collect();
    profile.content = ["a", "b", "c", "d", "e"]
        .into_iter()
        .map(|name| crate::records::FeatureContent::Dimension(name.into()))
        .collect();
    profile.parameters.insert("display".into(), "101.6".into());
    let sketch = SketchId("profile".into());
    let drill_length = 2.75 / (118_f64.to_radians() / 2.0).tan();
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 5.0), Point2::new(-5.7, 5.0)),
        profile_line(&sketch, 1, Point2::new(-5.7, 5.0), Point2::new(-5.7, 2.75)),
        profile_line(
            &sketch,
            2,
            Point2::new(-5.7, 2.75),
            Point2::new(-15.0, 2.75),
        ),
        profile_line(
            &sketch,
            3,
            Point2::new(-15.0, 2.75),
            Point2::new(-15.0 - drill_length, 0.0),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact profile");
    assert_eq!(construction.diameter, Length(5.5));
    assert_eq!(
        construction.extent,
        Termination::Blind {
            length: Length(15.0)
        }
    );
    assert!(matches!(
        construction.kind,
        HoleKind::CounterboreDrilled {
            diameter: Length(10.0),
            depth: Length(5.7),
            drill_point_angle: Angle(angle),
        } if (angle - 118_f64.to_radians()).abs() < 1.0e-12
    ));
    assert_eq!(construction.bottom, None);

    let mut translated_entities = entities.clone();
    for entity in &mut translated_entities {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            unreachable!();
        };
        start.u += 42.0;
        start.v -= 17.0;
        end.u += 42.0;
        end.v -= 17.0;
    }
    let translated = profiled_hole_construction(&profile, &sketch, &translated_entities)
        .expect("translated exact profile");
    assert_eq!(translated.diameter, construction.diameter);
    assert_eq!(translated.extent, construction.extent);
    assert_eq!(translated.kind, construction.kind);
    assert_eq!(translated.bottom, construction.bottom);
    assert_eq!(translated.taper_angle, construction.taper_angle);

    let mut independently_translated_entities = entities.clone();
    for (ordinal, entity) in independently_translated_entities.iter_mut().enumerate() {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            unreachable!();
        };
        let offset = (ordinal + 1) as f64 * 100.0;
        start.u += offset;
        start.v -= offset;
        end.u += offset;
        end.v -= offset;
    }
    assert!(
        profiled_hole_construction(&profile, &sketch, &independently_translated_entities).is_none()
    );

    profile.parameters.insert("a".into(), "180°".into());
    let construction =
        profiled_hole_construction(&profile, &sketch, &entities[..3]).expect("flat-bottom profile");
    assert_eq!(
        construction.extent,
        Termination::Blind {
            length: Length(15.0)
        }
    );
    assert_eq!(
        construction.kind,
        HoleKind::Counterbore {
            diameter: Length(10.0),
            depth: Length(5.7),
        }
    );
    assert_eq!(construction.bottom, Some(HoleBottom::Flat));
}

#[test]
fn axial_profile_resolves_counterdrill_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "<MOD-DIAM>2.9".into()),
        ("b".into(), "15".into()),
        ("c".into(), "<MOD-DIAM>5.5".into()),
        ("d".into(), "2.9".into()),
        ("e".into(), "<MOD-DIAM>5.55".into()),
        ("f".into(), "90°".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId("profile".into());
    let profile_point = |ordinal: usize, position| SketchEntity {
        id: SketchEntityId(format!("profile-point-{ordinal}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let entities = [
        profile_point(0, Point2::new(0.0, 2.775)),
        profile_point(1, Point2::new(-0.025, 2.75)),
        profile_line(
            &sketch,
            2,
            Point2::new(-0.025, 2.75),
            Point2::new(-2.9, 2.75),
        ),
        profile_point(3, Point2::new(-2.9, 2.75)),
        profile_point(4, Point2::new(-2.9, 1.45)),
        profile_line(
            &sketch,
            5,
            Point2::new(-2.9, 1.45),
            Point2::new(-15.0, 1.45),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact profile");
    assert_eq!(construction.diameter, Length(2.9));
    assert_eq!(construction.extent, Termination::ThroughAll);
    assert_eq!(
        construction.kind,
        HoleKind::Counterdrill {
            diameter: Length(5.5),
            entry_diameter: Some(Length(5.55)),
            depth: Length(2.9),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        }
    );

    let mut translated = entities.clone();
    for entity in &mut translated {
        match &mut entity.geometry {
            SketchGeometry::Point { position } => {
                position.u -= 11.0;
                position.v += 7.0;
            }
            SketchGeometry::Line { start, end } => {
                start.u -= 11.0;
                start.v += 7.0;
                end.u -= 11.0;
                end.v += 7.0;
            }
            _ => unreachable!(),
        }
    }
    assert_eq!(
        profiled_hole_construction(&profile, &sketch, &translated)
            .expect("translated exact profile")
            .kind,
        construction.kind
    );

    assert!(profiled_hole_construction(&profile, &sketch, &entities[..5]).is_none());
}

#[test]
fn single_diameter_axial_profile_resolves_flat_and_drilled_holes() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("diameter".into(), "<MOD-DIAM>14.5".into()),
        ("depth".into(), "15".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId("profile".into());

    let flat = profiled_hole_construction(&profile, &sketch, &[]).expect("exact flat profile");
    assert_eq!(flat.diameter, Length(14.5));
    assert_eq!(
        flat.extent,
        Termination::Blind {
            length: Length(15.0)
        }
    );
    assert_eq!(flat.kind, HoleKind::Simple);
    assert_eq!(flat.bottom, Some(HoleBottom::Flat));
    assert_eq!(flat.taper_angle, None);
    assert!(profiled_hole_construction_with_evidence(
        &profile,
        &sketch,
        &[],
        ProfileEvidence::AxialTopology,
    )
    .is_none());
    let radius = 14.5 / 2.0;
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 0.0), Point2::new(0.0, radius)),
        profile_line(
            &sketch,
            1,
            Point2::new(0.0, radius),
            Point2::new(-15.0, radius),
        ),
        profile_line(
            &sketch,
            2,
            Point2::new(-15.0, radius),
            Point2::new(-15.0, 0.0),
        ),
        profile_line(&sketch, 3, Point2::new(-15.0, 0.0), Point2::new(0.0, 0.0)),
    ];
    let topology_proven = profiled_hole_construction_with_evidence(
        &profile,
        &sketch,
        &entities,
        ProfileEvidence::AxialTopology,
    )
    .expect("axial rectangle");
    assert_eq!(topology_proven.diameter, flat.diameter);
    assert_eq!(topology_proven.extent, flat.extent);

    profile.parameters.insert("point".into(), "118°".into());
    let drilled =
        profiled_hole_construction(&profile, &sketch, &[]).expect("exact drilled profile");
    assert!(matches!(
        drilled.kind,
        HoleKind::SimpleDrilled {
            drill_point_angle: Angle(angle),
        } if (angle - 118_f64.to_radians()).abs() < 1.0e-12
    ));
    assert_eq!(
        drilled.bottom,
        Some(HoleBottom::Angled {
            included_angle: Angle(118_f64.to_radians()),
            depth_to_tip: false,
        })
    );
}

#[test]
fn closed_tapered_axial_profile_resolves_conical_hole() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("entry".into(), "<MOD-DIAM>12.2".into()),
        ("terminal".into(), "<MOD-DIAM>13.66623".into()),
        ("depth".into(), "42".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId("profile".into());
    let entry_radius = 6.1;
    let terminal_radius = 6.833_115;
    let terminal_geometry_radius = 6.833_112_73;
    let entities = [
        profile_line(
            &sketch,
            0,
            Point2::new(0.0, 0.0),
            Point2::new(0.0, entry_radius),
        ),
        profile_line(
            &sketch,
            1,
            Point2::new(0.0, entry_radius),
            Point2::new(-42.0, terminal_geometry_radius),
        ),
        profile_line(
            &sketch,
            2,
            Point2::new(-42.0, terminal_geometry_radius),
            Point2::new(-42.0, 0.0),
        ),
        profile_line(&sketch, 3, Point2::new(-42.0, 0.0), Point2::new(0.0, 0.0)),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact taper");
    assert_eq!(construction.diameter, Length(12.2));
    assert_eq!(
        construction.extent,
        Termination::Blind {
            length: Length(42.0)
        }
    );
    assert_eq!(construction.kind, HoleKind::Simple);
    assert_eq!(construction.bottom, Some(HoleBottom::Flat));
    let Angle(included_angle) = construction.taper_angle.expect("included taper angle");
    assert!(
        (included_angle - 2.0 * ((terminal_radius - entry_radius) / 42.0_f64).atan()).abs()
            < 1.0e-12
    );
}

#[test]
fn tapered_profile_reconstructs_missing_edges_from_endpoint_points() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("entry".into(), "<MOD-DIAM>12.2".into()),
        ("terminal".into(), "<MOD-DIAM>13.66623".into()),
        ("depth".into(), "42".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId("profile".into());
    let point = |ordinal: usize, position| SketchEntity {
        id: SketchEntityId(format!("profile-point-{ordinal}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let entities = [
        point(0, Point2::new(0.0, 0.0)),
        point(1, Point2::new(-42.0, 0.0)),
        point(2, Point2::new(-42.0, 6.833_112_73)),
        point(3, Point2::new(0.0, 6.1)),
        profile_line(&sketch, 0, Point2::new(0.0, 0.0), Point2::new(-42.0, 0.0)),
        profile_line(
            &sketch,
            1,
            Point2::new(-42.0, 0.0),
            Point2::new(-42.0, 6.833_112_73),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("endpoint proof");
    assert_eq!(construction.diameter, Length(12.2));
    assert_eq!(
        construction.extent,
        Termination::Blind {
            length: Length(42.0)
        }
    );
    assert_eq!(construction.kind, HoleKind::Simple);
    assert_eq!(construction.bottom, Some(HoleBottom::Flat));
    let Angle(taper_angle) = construction.taper_angle.expect("taper angle");
    assert!((taper_angle - 2.0 * ((6.833_115 - 6.1) / 42.0_f64).atan()).abs() < 1.0e-12);
}

#[test]
fn axial_profile_resolves_countersink_and_drill_point_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "120°".into()),
        ("b".into(), "5".into()),
        ("c".into(), "<MOD-DIAM>4.134".into()),
        ("d".into(), "<MOD-DIAM>5".into()),
        ("e".into(), "90°".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId("profile".into());
    let point = |ordinal: usize, position| SketchEntity {
        id: SketchEntityId(format!("profile-point-{ordinal}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let entities = [
        point(0, Point2::new(0.0, 2.5)),
        point(1, Point2::new(-0.433, 2.067)),
        profile_line(
            &sketch,
            2,
            Point2::new(-0.433, 2.067),
            Point2::new(-5.0, 2.067),
        ),
        profile_line(
            &sketch,
            3,
            Point2::new(-5.0, 2.067),
            Point2::new(-6.193_383_012, 0.0),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact profile");
    assert_eq!(construction.diameter, Length(4.134));
    assert_eq!(
        construction.extent,
        Termination::Blind {
            length: Length(5.0)
        }
    );
    assert!(matches!(
        construction.kind,
        HoleKind::Countersink {
            diameter: Length(5.0),
            angle: Angle(angle),
        } if (angle - 90_f64.to_radians()).abs() < 1.0e-12
    ));
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: Angle(120_f64.to_radians()),
            depth_to_tip: false,
        })
    );

    let mut translated_entities = entities.clone();
    for entity in &mut translated_entities {
        match &mut entity.geometry {
            SketchGeometry::Point { position } => {
                position.u += 21.0;
                position.v -= 33.0;
            }
            SketchGeometry::Line { start, end } => {
                start.u += 21.0;
                start.v -= 33.0;
                end.u += 21.0;
                end.v -= 33.0;
            }
            _ => unreachable!(),
        }
    }
    let translated = profiled_hole_construction(&profile, &sketch, &translated_entities)
        .expect("translated exact profile");
    assert_eq!(translated.diameter, construction.diameter);
    assert_eq!(translated.extent, construction.extent);
    assert_eq!(translated.kind, construction.kind);
    assert_eq!(translated.bottom, construction.bottom);

    let insufficient = [
        point(0, Point2::new(0.0, 2.5)),
        point(1, Point2::new(-0.433, 2.067)),
        point(2, Point2::new(-5.0, 2.067)),
        profile_line(
            &sketch,
            3,
            Point2::new(-5.0, 2.067),
            Point2::new(-6.193_383_012, 0.0),
        ),
    ];
    assert!(profiled_hole_construction(&profile, &sketch, &insufficient).is_none());
}

#[test]
fn axial_profile_resolves_open_countersink_with_optional_terminal_overrun() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "6".into()),
        ("b".into(), "<MOD-DIAM>6.4".into()),
        ("c".into(), "<MOD-DIAM>13.2".into()),
        ("d".into(), "90°".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId("profile".into());
    let entities = |terminal, mirror_wall: bool| {
        let wall_radius = if mirror_wall { -3.2 } else { 3.2 };
        [
            profile_line(&sketch, 0, Point2::new(0.0, 6.6), Point2::new(-3.4, 3.2)),
            profile_line(
                &sketch,
                1,
                Point2::new(-3.4, wall_radius),
                Point2::new(terminal, wall_radius),
            ),
        ]
    };

    for (terminal, mirror_wall) in [(-6.0, false), (-6.000_05, false), (-6.001, true)] {
        let exact_entities = entities(terminal, mirror_wall);
        let construction =
            profiled_hole_construction(&profile, &sketch, &exact_entities).expect("exact profile");
        assert_eq!(construction.diameter, Length(6.4));
        assert_eq!(construction.extent, Termination::ThroughAll);
        assert_eq!(
            construction.kind,
            HoleKind::Countersink {
                diameter: Length(13.2),
                angle: Angle(std::f64::consts::FRAC_PI_2),
            }
        );
        assert_eq!(construction.bottom, None);

        let mut translated_entities = exact_entities;
        for entity in &mut translated_entities {
            let SketchGeometry::Line { start, end } = &mut entity.geometry else {
                unreachable!();
            };
            start.u += 20.0;
            start.v += 30.0;
            end.u += 20.0;
            end.v += 30.0;
        }
        assert_eq!(
            profiled_hole_construction(&profile, &sketch, &translated_entities)
                .expect("translated exact profile")
                .kind,
            construction.kind
        );
    }
    assert!(profiled_hole_construction(&profile, &sketch, &entities(-6.002, true)).is_none());

    let mut independently_translated = entities(-6.0, false);
    for (index, entity) in independently_translated.iter_mut().enumerate() {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            unreachable!();
        };
        let offset = (index + 1) as f64 * 20.0;
        start.u += offset;
        start.v += offset;
        end.u += offset;
        end.v += offset;
    }
    assert!(profiled_hole_construction(&profile, &sketch, &independently_translated).is_none());
}

#[test]
fn incomplete_axial_profile_does_not_assign_dimension_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "8.6".into()),
        ("b".into(), "<MOD-DIAM>15".into()),
        ("c".into(), "23".into()),
        ("d".into(), "<MOD-DIAM>9".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId("profile".into());
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 7.5), Point2::new(-8.6, 7.5)),
        profile_line(&sketch, 1, Point2::new(-8.6, 4.5), Point2::new(-23.0, 4.5)),
    ];

    assert!(profiled_hole_construction(&profile, &sketch, &entities).is_none());
}

#[test]
fn unique_axial_profile_resolves_the_unique_incomplete_hole() {
    let mut history = native_history();
    history.features[0]
        .properties
        .insert("DissectableChildren".into(), "6,9".into());
    let mut profile = history.features[0].clone();
    profile.id = "native-profile".into();
    profile.source_id = Some("9".into());
    profile.ordinal = 1;
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile.parameters = [
        ("a".into(), "8.6".into()),
        ("b".into(), "<MOD-DIAM>15".into()),
        ("c".into(), "23".into()),
        ("d".into(), "<MOD-DIAM>9".into()),
    ]
    .into_iter()
    .collect();
    history.features.push(profile);
    let mut position = history.features[0].clone();
    position.id = "native-position".into();
    position.source_id = Some("6".into());
    position.ordinal = 2;
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    position.parameters = [("D1".into(), "50".into()), ("D2".into(), "35".into())]
        .into_iter()
        .collect();
    history.features.push(position);

    let sketch = SketchId("profile".into());
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 7.5), Point2::new(-8.6, 7.5)),
        profile_line(&sketch, 1, Point2::new(-8.6, 7.5), Point2::new(-8.6, 4.5)),
        profile_line(&sketch, 2, Point2::new(-8.6, 4.5), Point2::new(-23.0, 4.5)),
    ];
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("profile-feature".into()),
        ordinal: 1,
        name: Some("Profile".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch),
        },
        native_ref: Some("native-profile".into()),
    };
    let position_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-feature".into()),
        ordinal: 2,
        name: Some("Position".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(SketchId("position".into())),
        },
        native_ref: Some("native-position".into()),
    };
    let mut features = vec![model_hole(), sketch_feature, position_feature];
    let lane = lane_with_position_reference(6);
    let model_sketches = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.clone()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let histories = [history.clone()];
    assert_eq!(
        direct_hole_position_feature(
            &histories[0].features[0],
            &histories,
            &model_sketches,
            &entities,
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );

    let mut single_child_history = history.clone();
    single_child_history.features[0]
        .properties
        .insert("DissectableChildren".into(), "9".into());
    single_child_history.features[1].ordinal = 2;
    single_child_history.features[2].ordinal = 1;
    assert_eq!(
        direct_hole_position_feature(
            &single_child_history.features[0],
            std::slice::from_ref(&single_child_history),
            &model_sketches,
            &entities,
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );
    single_child_history.features[0]
        .properties
        .remove("DissectableChildren");
    assert_eq!(
        direct_hole_position_feature(
            &single_child_history.features[0],
            std::slice::from_ref(&single_child_history),
            &model_sketches,
            &entities,
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );

    project_profiled_hole_constructions(&mut features, &entities, &[history], &[lane]);

    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Hole {
            diameter: Some(Length(9.0)),
            extent: Some(Termination::ThroughAll),
            kind: HoleKind::Counterbore {
                diameter: Length(15.0),
                depth: Length(8.6),
            },
            ..
        }
    ));
}

#[test]
fn ordered_profile_fallback_excludes_claimed_profiles() {
    let mut history = native_history();
    let mut second_hole = history.features[0].clone();
    second_hole.id = "second-hole".into();
    second_hole.source_id = Some("8".into());
    second_hole.ordinal = 1;
    let mut claimed_hole = history.features[0].clone();
    claimed_hole.id = "claimed-hole".into();
    claimed_hole.source_id = Some("11".into());
    claimed_hole.ordinal = 2;
    claimed_hole
        .properties
        .insert("DissectableChildren".into(), "9".into());
    let profile = |id: &str, source: &str, ordinal, diameter: &str, depth: &str| {
        let mut profile = history.features[0].clone();
        profile.id = id.into();
        profile.source_id = Some(source.into());
        profile.ordinal = ordinal;
        profile.xml_tag = "Sketch".into();
        profile.kind = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        profile.parameters = [
            ("diameter".into(), format!("<MOD-DIAM>{diameter}")),
            ("depth".into(), depth.into()),
        ]
        .into();
        profile
    };
    let claimed_profile = profile("claimed-profile", "9", 3, "15", "23");
    let first_profile = profile("first-profile", "10", 4, "4.2", "6.8");
    let second_profile = profile("second-profile", "12", 5, "6", "14");
    history.features.extend([
        second_hole,
        claimed_hole,
        claimed_profile,
        first_profile,
        second_profile,
    ]);

    let model_sketch = |id: &str, sketch: &str, ordinal| cadmpeg_ir::features::Feature {
        id: FeatureId(format!("{id}-feature")),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(SketchId(sketch.into())),
        },
        native_ref: Some(id.into()),
    };
    let mut second_model_hole = model_hole();
    second_model_hole.id = FeatureId("second-model-hole".into());
    second_model_hole.ordinal = 1;
    second_model_hole.native_ref = Some("second-hole".into());
    let mut features = vec![
        model_hole(),
        second_model_hole,
        model_sketch("claimed-profile", "claimed-sketch", 1),
        model_sketch("first-profile", "first-sketch", 2),
        model_sketch("second-profile", "second-sketch", 3),
    ];
    let axial_rectangle = |sketch: &str, radius: f64, depth: f64, first_ordinal| {
        let sketch = SketchId(sketch.into());
        [
            profile_line(
                &sketch,
                first_ordinal,
                Point2::new(0.0, 0.0),
                Point2::new(0.0, radius),
            ),
            profile_line(
                &sketch,
                first_ordinal + 1,
                Point2::new(0.0, radius),
                Point2::new(-depth, radius),
            ),
            profile_line(
                &sketch,
                first_ordinal + 2,
                Point2::new(-depth, radius),
                Point2::new(-depth, 0.0),
            ),
            profile_line(
                &sketch,
                first_ordinal + 3,
                Point2::new(-depth, 0.0),
                Point2::new(0.0, 0.0),
            ),
        ]
    };
    let entities = [
        axial_rectangle("first-sketch", 2.1, 6.8, 0),
        axial_rectangle("second-sketch", 3.0, 14.0, 4),
    ]
    .concat();

    project_profiled_hole_constructions(&mut features, &entities, &[history], &[]);

    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Hole {
            diameter: Some(Length(4.2)),
            extent: Some(Termination::Blind {
                length: Length(6.8)
            }),
            ..
        }
    ));
    assert!(matches!(
        features[1].definition,
        FeatureDefinition::Hole {
            diameter: Some(Length(6.0)),
            extent: Some(Termination::Blind {
                length: Length(14.0)
            }),
            ..
        }
    ));
}

#[test]
fn compact_position_graph_selects_the_unique_bore_loci() {
    use FeatureInputRelationFamily::{
        PointPointDistance, PointPointHorizontalDistance, PointPointVerticalDistance,
    };

    let loci = [
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 16.0),
        Point2::new(0.0, 41.0),
    ];
    let relations = [
        (PointPointDistance, 0, 2, 25.0),
        (PointPointVerticalDistance, 0, 5, 0.0),
        (PointPointHorizontalDistance, 0, 5, 16.0),
    ];
    let placement_loci = [1, 2].into_iter().collect();
    assert_eq!(
        compact_position_loci(&loci, &placement_loci, &relations),
        Some(vec![1, 2])
    );

    let ambiguous = [loci[0], loci[1], loci[2], Point2::new(0.0, -9.0)];
    let ambiguous_placements = [1, 2, 3].into_iter().collect();
    assert_eq!(
        compact_position_loci(&ambiguous, &ambiguous_placements, &relations),
        None
    );
}

#[test]
fn object_indexed_line_handles_select_a_congruent_bore_pattern() {
    let mut lane = lane();
    lane.sketch_entities = [(1, [0.013, 0.007]), (2, [-0.009, 0.007])]
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (object_index, coordinates_m))| SketchInputEntity {
                id: format!("marker-{ordinal}"),
                parent: "lane".into(),
                feature_ref: Some("position".into()),
                ordinal: ordinal as u32,
                offset: ordinal as u64,
                object_index: Some(object_index),
                local_id: None,
                kind: SketchInputKind::LineOrCircle,
                state_value: Some(1.0),
                coordinates_m: Some(coordinates_m),
                links: Vec::new(),
                link_selector: None,
            },
        )
        .collect();
    let surface = |id, x| Surface {
        id: SurfaceId(format!("surface-{id}")),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(x, 7.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.1,
        },
        source_object: None,
    };
    let mut surfaces = vec![surface(0, -9.0), surface(1, 13.0), surface(2, 100.0)];

    let placements = marker_pattern_bore_axes(&lane, "position", 2.1, &surfaces, None)
        .expect("required invariant");
    assert_eq!(placements.len(), 2);
    assert!(placements.iter().any(|placement| matches!(
        placement,
        cadmpeg_ir::features::HolePlacement::Axis { origin, .. }
            if origin.x == -9.0 && origin.y == 7.0 && origin.z == 10.0
    )));
    assert!(placements.iter().any(|placement| matches!(
        placement,
        cadmpeg_ir::features::HolePlacement::Axis { origin, .. }
            if origin.x == 13.0 && origin.y == 7.0 && origin.z == 10.0
    )));

    let opposite_side = |id, x| Surface {
        id: SurfaceId(format!("surface-{id}")),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(x, 30.0, 10.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.1,
        },
        source_object: None,
    };
    surfaces.extend([opposite_side(3, -9.0), opposite_side(4, 13.0)]);
    assert!(marker_pattern_bore_axes(&lane, "position", 2.1, &surfaces, None).is_none());
    assert_eq!(
        marker_pattern_bore_axes(
            &lane,
            "position",
            2.1,
            &surfaces,
            Some(Vector3::new(0.0, 0.0, 1.0)),
        )
        .expect("required invariant")
        .len(),
        2
    );

    let mut opposite = surface(5, -9.0);
    let SurfaceGeometry::Cylinder { axis, .. } = &mut opposite.geometry else {
        unreachable!();
    };
    *axis = Vector3::new(0.0, 0.0, -1.0);
    surfaces.push(opposite);
    assert_eq!(
        marker_pattern_bore_axes(
            &lane,
            "position",
            2.1,
            &surfaces,
            Some(Vector3::new(0.0, 0.0, 1.0)),
        )
        .expect("required invariant")
        .len(),
        2
    );
}

#[test]
fn paired_object_loci_select_a_congruent_bore_pattern() {
    let marker = |id: &str, ordinal, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("position".into()),
        ordinal,
        offset: u64::from(ordinal) * 10,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = lane();
    lane.sketch_entities = vec![
        marker(
            "first",
            0,
            Some(1),
            SketchInputKind::Arc,
            Some([0.013, 0.0]),
        ),
        marker(
            "first-origin",
            1,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "second",
            2,
            Some(2),
            SketchInputKind::Relation(SketchRelationKind::Horizontal),
            Some([-0.009, 0.0]),
        ),
        marker(
            "second-origin",
            3,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "auxiliary",
            4,
            Some(3),
            SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        marker(
            "paired-duplicate",
            5,
            Some(4),
            SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        marker(
            "paired-duplicate-origin",
            6,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
    ];

    let paired = paired_object_locus_markers(&lane, "position")
        .into_iter()
        .map(|marker| marker.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paired, ["first", "second", "paired-duplicate"]);

    let mut surfaces = vec![cylinder(0, -9.0), cylinder(1, 13.0), cylinder(2, 100.0)];
    let placements = marker_pattern_bore_axes(&lane, "position", 2.0, &surfaces, None)
        .expect("unique congruent pattern");
    assert_eq!(placements.len(), 2);

    let opposite = surfaces[..2]
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, mut surface)| {
            surface.id = SurfaceId(format!("opposite-{index}"));
            let SurfaceGeometry::Cylinder { origin, axis, .. } = &mut surface.geometry else {
                unreachable!();
            };
            origin.z = 20.0;
            *axis = Vector3::new(0.0, 0.0, -1.0);
            surface
        })
        .collect::<Vec<_>>();
    surfaces.extend(opposite);
    assert_eq!(
        marker_pattern_bore_axes(&lane, "position", 2.0, &surfaces, None)
            .expect("unoriented coincident axes")
            .len(),
        2
    );

    surfaces.push(Surface {
        id: SurfaceId("duplicate-locus-bore".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(1000.0, 1000.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    });
    assert_eq!(
        marker_pattern_bore_axes(&lane, "position", 2.0, &surfaces, None)
            .expect("complete paired roster takes precedence")
            .len(),
        3
    );
}

#[test]
fn hole_temporary_axis_decodes_depth_point_direction_layout() {
    let mut payload = vec![0; 500];
    let declaration = 40;
    payload[declaration..declaration + 4].copy_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    payload[declaration + 4..declaration + 6].copy_from_slice(&15u16.to_le_bytes());
    payload[declaration + 6..declaration + 21].copy_from_slice(b"moTempAxisRef_w");
    payload[declaration + 267..declaration + 275]
        .copy_from_slice(b"\xc7\xcf\xff\xff\xc7\xcf\xff\xff");
    payload[declaration + 279..declaration + 283].copy_from_slice(&4700u32.to_le_bytes());
    for (index, value) in [0.0075, -0.045, 0.028, -0.03, -1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = declaration + 299 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[declaration + 364..declaration + 368].copy_from_slice(&[0xff, 0xfe, 0xff, 0x00]);

    assert_eq!(
        hole_temporary_axis(&payload, 32, payload.len()),
        Some((
            Point3::new(-45.0, 28.0, -30.0),
            Vector3::new(-1.0, 0.0, 0.0),
        ))
    );
}

#[test]
fn embedded_position_sketch_name_resolves_its_typed_source() {
    let history = native_history();
    let mut lane = lane();
    lane.native_payload.resize(200, 0);
    lane.names.push(FeatureInputName {
        id: "hole-name".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        value: "Hole".into(),
        object_id: Some(7),
    });
    let hole_trailer = 6 + "Hole".encode_utf16().count() * 2;
    lane.native_payload[hole_trailer..hole_trailer + 8]
        .copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    lane.native_payload[hole_trailer + 8..hole_trailer + 12].copy_from_slice(&7u32.to_le_bytes());

    let child_offset = hole_trailer + 32;
    lane.names.push(FeatureInputName {
        id: "position-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: child_offset as u64,
        value: "Position".into(),
        object_id: Some(6),
    });
    let child_trailer = child_offset + 6 + "Position".encode_utf16().count() * 2;
    lane.native_payload[child_trailer..child_trailer + 8]
        .copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    lane.native_payload[child_trailer + 8..child_trailer + 12].copy_from_slice(&6u32.to_le_bytes());

    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        Some(6)
    );
    let mut classless_history = history.clone();
    classless_history.features[0].input_class = None;
    assert_eq!(
        hole_position_sketch_source(&classless_history.features[0], &lane),
        Some(6)
    );
    lane.native_payload[hole_trailer + 16..hole_trailer + 18].copy_from_slice(&[0, 0xc0]);
    lane.native_payload[hole_trailer + 18..hole_trailer + 22].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        None
    );
    lane.native_payload[hole_trailer + 16..hole_trailer + 28].fill(0);
    lane.native_payload[child_trailer + 8] = 5;
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        None
    );

    let mut legacy_history = history.clone();
    legacy_history.features[0].source_id = None;
    legacy_history.features.push(crate::records::Feature {
        id: "native-position".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    lane.native_payload[hole_trailer + 16..hole_trailer + 28].fill(0);
    lane.native_payload[hole_trailer + 16..hole_trailer + 28]
        .copy_from_slice(&[0, 0xc0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        hole_position_feature(
            &legacy_history.features[0],
            std::slice::from_ref(&legacy_history),
            &[lane],
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );
}

#[test]
fn typed_position_sketch_reference_lifts_authored_object_loci() {
    let hole = model_hole();
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-sketch".into()),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(SketchId("position-geometry".into())),
        },
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("6".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut lane = lane_with_position_reference(6);
    let trailer = 6 + "Hole".encode_utf16().count() * 2;
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        Some(6)
    );
    lane.native_payload[trailer + 58..trailer + 60].copy_from_slice(&[0xff, 0xfe]);
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        Some(6)
    );
    lane.sketch_entities.push(SketchInputEntity {
        id: "authored-point".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 0,
        offset: 80,
        object_index: Some(1),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.002, 0.003]),
        links: Vec::new(),
        link_selector: None,
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "origin-marker".into(),
        object_index: None,
        ordinal: 1,
        offset: 90,
        coordinates_m: Some([0.0, 0.0]),
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "point-identity".into(),
        object_index: Some(2),
        ordinal: 4,
        offset: 120,
        coordinates_m: None,
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "authored-arc-locus".into(),
        object_index: Some(2),
        ordinal: 2,
        offset: 100,
        kind: SketchInputKind::Arc,
        coordinates_m: Some([0.014, 0.025]),
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "arc-origin-marker".into(),
        object_index: None,
        ordinal: 3,
        offset: 110,
        kind: SketchInputKind::Point,
        coordinates_m: Some([0.0, 0.0]),
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.sort_by_key(|marker| marker.ordinal);
    let sketch = Sketch {
        id: SketchId("position-geometry".into()),
        name: Some("Position".into()),
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 20.0, 30.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let entities = [SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.id.clone(),
        construction: false,
        native_ref: Some("authored-point".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(2.0, 3.0),
        },
    }];
    let mut features = vec![hole, sketch_feature];
    let mut paired_lane = lane.clone();
    paired_lane.sketch_entities.truncate(4);
    paired_lane.sketch_entities[0].kind = SketchInputKind::Arc;
    paired_lane.sketch_entities[0].coordinates_m = Some([0.012, 0.023]);
    let mut alternate_configuration = lane.clone();
    alternate_configuration.id = "alternate-lane".into();
    alternate_configuration.configuration = Some("alternate".into());

    project_hole_position_sketches(
        &mut features,
        std::slice::from_ref(&sketch),
        &entities,
        std::slice::from_ref(&history),
        &[lane, alternate_configuration],
    );

    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        panic!("expected hole");
    };
    assert_eq!(placements.len(), 1);
    assert!(matches!(
        placements[0],
        cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3 {
                x: 12.0,
                y: 23.0,
                z: 30.0
            },
            axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
    assert_eq!(
        features[0].dependencies,
        [FeatureId("position-sketch".into())]
    );

    let mut paired_features = vec![model_hole(), features[1].clone()];
    project_hole_position_sketches(
        &mut paired_features,
        std::slice::from_ref(&sketch),
        &[],
        std::slice::from_ref(&history),
        std::slice::from_ref(&paired_lane),
    );
    let FeatureDefinition::Hole {
        placements: paired_placements,
        ..
    } = &paired_features[0].definition
    else {
        panic!("expected hole");
    };
    assert_eq!(paired_placements.len(), 2);
    assert!(matches!(
        paired_placements[0],
        cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3 {
                x: 12.0,
                y: 23.0,
                z: 30.0
            },
            axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
    assert!(matches!(
        paired_placements[1],
        cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3 {
                x: 14.0,
                y: 25.0,
                z: 30.0
            },
            axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));

    let mut incomplete_lane = paired_lane;
    incomplete_lane.sketch_entities.push(SketchInputEntity {
        id: "unpaired-object-locus".into(),
        object_index: Some(3),
        ordinal: 4,
        offset: 120,
        kind: SketchInputKind::Arc,
        coordinates_m: Some([0.016, 0.027]),
        ..incomplete_lane.sketch_entities[0].clone()
    });
    let mut incomplete_features = vec![model_hole(), features[1].clone()];
    project_hole_position_sketches(
        &mut incomplete_features,
        std::slice::from_ref(&sketch),
        &[],
        std::slice::from_ref(&history),
        std::slice::from_ref(&incomplete_lane),
    );
    let FeatureDefinition::Hole { placements, .. } = &incomplete_features[0].definition else {
        panic!("expected hole");
    };
    assert!(placements.is_empty());
}

#[test]
fn spatial_position_point_uses_unique_radius_matched_bore_axis() {
    let hole = model_hole();
    let sketch_id = SpatialSketchId("position-geometry".into());
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-sketch".into()),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("6".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "3DSketch".into(),
        input_class: Some("mo3DProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut lane = lane_with_position_reference(6);
    lane.sketch_entities.push(SketchInputEntity {
        id: "authored-point".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 0,
        offset: 80,
        object_index: Some(1),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "same-axis-endpoint".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 1,
        offset: 90,
        object_index: Some(2),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "construction-point".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 2,
        offset: 100,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    });
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: Some("Position".into()),
        configuration: None,
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let point = Point3::new(12.0, 23.0, 30.0);
    let entity = SpatialSketchEntity {
        id: SpatialSketchEntityId("point".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: Some("authored-point".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point { position: point },
    };
    let same_axis_endpoint = SpatialSketchEntity {
        id: SpatialSketchEntityId("same-axis-endpoint".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: Some("same-axis-endpoint".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(12.0, 23.0, 20.0),
        },
    };
    let construction_point = SpatialSketchEntity {
        id: SpatialSketchEntityId("construction-point".into()),
        sketch: sketch_id,
        construction: true,
        native_ref: Some("construction-point".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(100.0, 100.0, 100.0),
        },
    };
    let surface = Surface {
        id: SurfaceId("bore".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(12.0, 23.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    };
    let mut features = vec![hole, sketch_feature];

    project_spatial_hole_position_sketches(
        &mut features,
        &[sketch],
        &[entity, same_axis_endpoint, construction_point],
        &[surface],
        &[history],
        &[lane],
    );

    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        panic!("expected hole");
    };
    assert_eq!(
        placements,
        &[cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(12.0, 23.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        }]
    );
}

#[test]
fn source_intervals_supply_legacy_hole_profiles() {
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-profile-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("9".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Profile".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: [
            ("bore".into(), "<MOD-DIAM>4.2".into()),
            ("depth".into(), "6.8".into()),
            ("tip".into(), "118°".into()),
        ]
        .into(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            crate::records::FeatureContent::Dimension("bore".into()),
            crate::records::FeatureContent::Dimension("depth".into()),
            crate::records::FeatureContent::Dimension("tip".into()),
        ],
    });

    let mut lane = lane_with_position_reference(12);
    lane.names.push(FeatureInputName {
        id: "depth-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 120,
        value: "depth".into(),
        object_id: None,
    });
    lane.names.push(FeatureInputName {
        id: "profile-name".into(),
        parent: "lane".into(),
        ordinal: 2,
        offset: 100,
        value: "Profile".into(),
        object_id: Some(8),
    });
    lane.scalars.push(FeatureInputScalar {
        id: "depth-scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("native-profile-sketch".into()),
        ordinal: 0,
        offset: 150,
        object_id: 1,
        name: "depth-name".into(),
        value: 0.0068,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    });
    let mut histories = [history];
    enrich_history_parameters(&mut histories, [&lane], true);
    assert_eq!(histories[0].features[1].parameters["depth"], "6.8mm");
    enrich_history_hole_constructions(&mut histories, &[lane]);
    assert_eq!(
        histories[0].features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("9")
    );

    histories[0].features[0]
        .properties
        .remove("DissectableChildren");
    histories[0].features[1].ordinal = 5;
    let mut next_hole = histories[0].features[0].clone();
    next_hole.id = "next-hole".into();
    next_hole.source_id = Some("20".into());
    next_hole.ordinal = 1;
    histories[0].features.push(next_hole);
    enrich_history_hole_constructions(&mut histories, &[]);
    assert_eq!(
        histories[0].features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("9")
    );
}

#[test]
fn serialized_position_successor_owns_legacy_hole_profile() {
    let mut history = native_history();
    let mut position = history.features[0].clone();
    position.id = "native-position-sketch".into();
    position.source_id = Some("12".into());
    position.ordinal = 5;
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    position.parameters.clear();
    position.content.clear();
    history.features.push(position);
    let profile = crate::records::Feature {
        id: "native-profile-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("58".into()),
        parent_source_id: None,
        ordinal: 9,
        name: "Profile".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: [
            ("bore".into(), "<MOD-DIAM>9".into()),
            ("depth".into(), "30".into()),
        ]
        .into(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            crate::records::FeatureContent::Dimension("bore".into()),
            crate::records::FeatureContent::Dimension("depth".into()),
        ],
    };
    history.features.push(profile.clone());

    let mut lane = lane_with_position_reference(12);
    lane.native_payload.resize(300, 0);
    lane.names.extend([
        FeatureInputName {
            id: "position-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            value: "Position".into(),
            object_id: Some(12),
        },
        FeatureInputName {
            id: "profile-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 200,
            value: "Profile".into(),
            object_id: Some(58),
        },
    ]);

    enrich_history_hole_constructions(std::slice::from_mut(&mut history), &[lane.clone()]);
    assert_eq!(
        history.features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("58")
    );

    history.features[0].properties.remove("DissectableChildren");
    let mut alternate_profile = profile;
    alternate_profile.id = "alternate-profile-sketch".into();
    alternate_profile.source_id = Some("59".into());
    alternate_profile.ordinal = 10;
    history.features.push(alternate_profile);
    let mut alternate_lane = lane_with_position_reference(12);
    alternate_lane.native_payload.resize(300, 0);
    alternate_lane.names.extend([
        FeatureInputName {
            id: "alternate-position-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            value: "Position".into(),
            object_id: Some(12),
        },
        FeatureInputName {
            id: "alternate-profile-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 150,
            value: "Alternate profile".into(),
            object_id: Some(59),
        },
        FeatureInputName {
            id: "later-profile-name".into(),
            parent: "lane".into(),
            ordinal: 3,
            offset: 200,
            value: "Profile".into(),
            object_id: Some(58),
        },
    ]);
    enrich_history_hole_constructions(std::slice::from_mut(&mut history), &[lane, alternate_lane]);
    assert!(!history.features[0]
        .properties
        .contains_key("DissectableChildren"));
}

#[test]
fn ordered_legacy_sketch_children_identify_the_unique_hole_profile() {
    let mut history = native_history();
    let mut position = history.features[0].clone();
    position.id = "native-position-sketch".into();
    position.source_id = Some("8".into());
    position.ordinal = 1;
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    position.parameters.clear();
    position.content.clear();
    history.features.push(position);
    history.features.push(crate::records::Feature {
        id: "native-profile-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 2,
        name: "Profile".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: [
            ("bore".into(), "<MOD-DIAM>4.2".into()),
            ("depth".into(), "6.8".into()),
        ]
        .into(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            crate::records::FeatureContent::Dimension("bore".into()),
            crate::records::FeatureContent::Dimension("depth".into()),
        ],
    });

    enrich_history_hole_constructions(std::slice::from_mut(&mut history), &[]);

    assert_eq!(
        history.features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("native-profile-sketch")
    );
    assert_eq!(history.features[2].source_id, None);
}

#[test]
fn parameter_class_supplies_an_operandless_scalar_unit() {
    let mut history = native_history();
    let mut lane = lane_with_position_reference(6);
    lane.classes.push(FeatureInputClass {
        id: "angle-parameter-class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 100,
        name: "moAngleParameter_c".into(),
        role: FeatureInputClassRole::Parameter,
    });
    lane.names.push(FeatureInputName {
        id: "angle-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 120,
        value: "D1".into(),
        object_id: None,
    });
    lane.scalars.push(FeatureInputScalar {
        id: "angle-scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("native-hole".into()),
        ordinal: 0,
        offset: 150,
        object_id: 1,
        name: "angle-name".into(),
        value: std::f64::consts::TAU,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    });

    enrich_history_parameters(std::slice::from_mut(&mut history), [&lane], true);
    assert_eq!(
        history.features[0].parameters.get("D1").map(String::as_str),
        Some("6.283185307179586rad")
    );
}

#[test]
fn hole_axes_do_not_claim_unowned_same_radius_surfaces() {
    let history = native_history();
    let lane = lane();
    let mut features = vec![model_hole()];
    let surfaces = vec![cylinder(0, -5.0), cylinder(1, 5.0)];

    project_hole_axes(
        &mut features,
        &[],
        &HoleTopology {
            surfaces: &surfaces,
            faces: &[],
            loops: &[],
            coedges: &[],
            edges: &[],
            vertices: &[],
            points: &[],
        },
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );
    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        unreachable!();
    };
    assert!(placements.is_empty());
}
