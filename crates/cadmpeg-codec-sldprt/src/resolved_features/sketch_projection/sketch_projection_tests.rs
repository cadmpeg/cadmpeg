//! Tests for the `sketch_projection` module.

use super::super::curves::{resolve_connected_marker_arcs, resolve_slot_marker_arcs};
use super::super::LEGACY_EXTENDED_SKETCH_MARKER;
use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};

#[test]
fn indexed_arc_uses_its_consecutive_middle_point_as_center() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let point = |id: &str, offset: u64, position| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
        .with_native_ref(Some(format!("native:{offset}")))
    };
    let mut entities = vec![
        point("synthetic:test:id#start", 100, Point2::new(1.0, 0.0)),
        point("synthetic:test:id#center", 200, Point2::new(0.0, 0.0)),
        point("synthetic:test:id#end", 300, Point2::new(0.0, 1.0)),
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#arc").unwrap(),
            sketch,
            SketchGeometry::try_from(SketchGeometryDefinition::Native {
                native_kind: cadmpeg_core::text::NonBlankString::new("sldprt:marker-geometry:2")
                    .expect("nonempty source identity"),
            })
            .unwrap(),
        )
        .with_native_ref(Some("native:400".into()))
        .with_endpoint_refs(vec!["native:100".into(), "native:300".into()]),
    ];

    resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert_eq!(
        entities[3].geometry,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            end_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap()
    );
}

fn slot_cycle_fixture() -> (
    Vec<u8>,
    [SketchInputEntity; 11],
    Vec<cadmpeg_ir::sketches::SketchEntity>,
) {
    let slot_offset = 500;
    let mut payload = vec![0; slot_offset + 140];
    let declaration = b"\xff\xff\x01\x00\x08\x00sgSlot_c\0\0\0\0\x01\0\0\0";
    payload[slot_offset - declaration.len()..slot_offset].copy_from_slice(declaration);
    payload[slot_offset..slot_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[slot_offset + 5..slot_offset + 13].fill(0xff);
    payload[slot_offset + 13..slot_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[slot_offset + 23..slot_offset + 29]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[slot_offset + 31..slot_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[slot_offset + 48..slot_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    for (index, (tag, id)) in [
        (0x8156_u16, 0_u16),
        (0x814c, 3),
        (0x8156, 1),
        (0x8156, 2),
        (0x8294, 0),
        (0x8294, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let start = slot_offset + 64 + index * 12;
        payload[start..start + 2].copy_from_slice(&tag.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }

    let input = |id: &str, offset, kind, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let inputs = [
        input("center-left", 100, SketchInputKind::Point, Some([0.0, 0.0])),
        input(
            "center-right",
            110,
            SketchInputKind::Point,
            Some([2.0, 0.0]),
        ),
        input("left-top", 120, SketchInputKind::Point, Some([0.0, 1.0])),
        input("right-top", 130, SketchInputKind::Point, Some([2.0, 1.0])),
        input(
            "left-bottom",
            140,
            SketchInputKind::Point,
            Some([0.0, -1.0]),
        ),
        input(
            "right-bottom",
            150,
            SketchInputKind::Point,
            Some([2.0, -1.0]),
        ),
        input("top", 200, SketchInputKind::LineOrCircle, None),
        input("bottom", 210, SketchInputKind::LineOrCircle, None),
        input("right", 220, SketchInputKind::Arc, None),
        input("left", 230, SketchInputKind::Arc, None),
        input(
            "slot",
            cadmpeg_core::decode::u64_from_index(slot_offset),
            SketchInputKind::Point,
            None,
        ),
    ];
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let point = |id: &str, position| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#model:{id}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
        .with_native_ref(Some(id.into()))
    };
    let curve = |id: &str, geometry, endpoint_refs: &[&str]| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#model:{id}")).unwrap(),
            sketch.clone(),
            geometry,
        )
        .with_native_ref(Some(id.into()))
        .with_endpoint_refs(endpoint_refs.iter().map(|id| (*id).into()).collect())
    };
    let entities = vec![
        point("center-left", Point2::new(0.0, 0.0)),
        point("center-right", Point2::new(2.0, 0.0)),
        point("left-top", Point2::new(0.0, 1.0)),
        point("right-top", Point2::new(2.0, 1.0)),
        point("left-bottom", Point2::new(0.0, -1.0)),
        point("right-bottom", Point2::new(2.0, -1.0)),
        curve(
            "top",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(2.0, 1.0),
            })
            .unwrap(),
            &["left-top", "right-top"],
        ),
        curve(
            "bottom",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, -1.0),
                end: Point2::new(2.0, -1.0),
            })
            .unwrap(),
            &["left-bottom", "right-bottom"],
        ),
        curve(
            "right",
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(2.0, 0.0),
                radius: Length::new(1.0).unwrap(),
                start_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
                end_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            })
            .unwrap(),
            &["right-top", "right-bottom"],
        ),
        curve(
            "left",
            SketchGeometry::try_from(SketchGeometryDefinition::Native {
                native_kind: cadmpeg_core::text::NonBlankString::new("sldprt:marker-geometry:2")
                    .expect("nonempty source identity"),
            })
            .unwrap(),
            &[],
        ),
    ];

    (payload, inputs, entities)
}

#[test]
fn slot_cycle_supplies_the_missing_cap_endpoints_and_center() {
    let (payload, inputs, mut entities) = slot_cycle_fixture();
    let markers = inputs.iter().collect::<Vec<_>>();

    resolve_slot_marker_arcs(&payload, &markers, &mut entities, 1.0e-9);

    assert_eq!(
        entities[9].endpoint_refs,
        ["left-top".to_string(), "left-bottom".to_string()]
    );
    assert!(matches!(*entities[9].geometry.definition(),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            ..
        } if center == Point2::new(0.0, 0.0) && radius.get() == 1.0
    ));
}

/// A planar face whose loops are `order` of a line loop (0) and a closed
/// NURBS loop (1) whose poles leave the finite range when projected onto
/// the face's diagonal sketch axes. The B-rep holds only the topology of
/// the face's loops.
fn face_with_an_unprojectable_loop(order: &[usize]) -> crate::brep::graph::Brep {
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point,
        Sense, Vertex,
    };
    let id = |kind: &str, n: u32| format!("test:profile:{kind}#{n}");
    let surface = SurfaceId::mint(id("surface", 1)).unwrap();
    let face = FaceId::mint(id("face", 1)).unwrap();
    let loops = [1, 2].map(|n| LoopId::mint(id("loop", n)).unwrap());
    let coedges = [1, 2].map(|n| CoedgeId::mint(id("coedge", n)).unwrap());
    let edges = [1, 2].map(|n| EdgeId::mint(id("edge", n)).unwrap());
    let curves = [1, 2].map(|n| CurveId::mint(id("curve", n)).unwrap());
    let vertices = [1, 2, 3].map(|n| VertexId::mint(id("vertex", n)).unwrap());
    let points = [1, 2, 3].map(|n| PointId::mint(id("point", n)).unwrap());
    let far = Point3::new(1.5e308, 1.5e308, 0.0);
    let positions = [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0), far];
    let diagonal = Vector3::new(
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
    );
    let brep = crate::brep::graph::Brep {
        surfaces: vec![Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    diagonal,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        }],
        curves: vec![
            Curve {
                parameter_range: None,
                id: curves[0].clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    LineCurve::try_new(positions[0], Vector3::new(1.0, 0.0, 0.0)).unwrap(),
                )),
                source_object: None,
                native_ref: None,
            },
            Curve {
                parameter_range: None,
                id: curves[1].clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                    NurbsCurve::from_lanes(
                        1,
                        vec![0.0, 0.0, 0.5, 1.0, 1.0],
                        vec![far, Point3::new(1.6e308, 1.5e308, 0.0), far],
                        None,
                        false,
                    )
                    .unwrap(),
                )),
                source_object: None,
                native_ref: None,
            },
        ],
        faces: vec![Face {
            id: face.clone(),
            shell: ShellId::mint(id("shell", 1)).unwrap(),
            surface,
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(
                order.iter().map(|index| loops[*index].clone()).collect(),
            ),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        }],
        loops: (0..2)
            .map(|index| Loop {
                id: loops[index].clone(),
                face: face.clone(),
                boundary: LoopBoundary::Ring(
                    LoopRing::new(vec![coedges[index].clone()], Vec::new()).unwrap(),
                ),
                native_ref: None,
            })
            .collect(),
        coedges: (0..2)
            .map(|index| Coedge {
                id: coedges[index].clone(),
                owner_loop: loops[index].clone(),
                edge: edges[index].clone(),
                radial_next: coedges[index].clone(),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
                native_ref: None,
            })
            .collect(),
        edges: vec![
            Edge {
                id: edges[0].clone(),
                carrier: EdgeCarrier::unbounded(Some(curves[0].clone())),
                ends: Some(EdgeEnds::new(vertices[0].clone(), vertices[1].clone())),
                tolerance: None,
                native_ref: None,
            },
            Edge {
                id: edges[1].clone(),
                carrier: EdgeCarrier::unbounded(Some(curves[1].clone())),
                ends: Some(EdgeEnds::new(vertices[2].clone(), vertices[2].clone())),
                tolerance: None,
                native_ref: None,
            },
        ],
        vertices: (0..3)
            .map(|index| Vertex {
                id: vertices[index].clone(),
                point: points[index].clone(),
                tolerance: None,
                native_ref: None,
            })
            .collect(),
        points: (0..3)
            .map(|index| {
                Point::new(
                    points[index].clone(),
                    FinitePoint3::new(positions[index]).unwrap(),
                    None,
                )
            })
            .collect(),
        ..Default::default()
    };
    let kept_vertices = order
        .iter()
        .flat_map(|index| if *index == 0 { vec![0, 1] } else { vec![2] })
        .map(|index| vertices[index].clone())
        .collect::<Vec<_>>();
    let kept_points = kept_vertices
        .iter()
        .map(|vertex| PointId::mint(vertex.as_str().replace("vertex", "point")).unwrap())
        .collect::<Vec<_>>();
    crate::brep::graph::Brep {
        curves: brep
            .curves
            .into_iter()
            .filter(|curve| order.iter().any(|index| curves[*index] == curve.id))
            .collect(),
        loops: brep
            .loops
            .into_iter()
            .filter(|loop_| order.iter().any(|index| loops[*index] == loop_.id))
            .collect(),
        coedges: brep
            .coedges
            .into_iter()
            .filter(|coedge| order.iter().any(|index| coedges[*index] == coedge.id))
            .collect(),
        edges: brep
            .edges
            .into_iter()
            .filter(|edge| order.iter().any(|index| edges[*index] == edge.id))
            .collect(),
        vertices: brep
            .vertices
            .into_iter()
            .filter(|vertex| kept_vertices.contains(&vertex.id))
            .collect(),
        points: brep
            .points
            .into_iter()
            .filter(|point| kept_points.contains(&point.id))
            .collect(),
        ..brep
    }
}

/// The sketches the face of `brep` projects to, and the losses.
fn project(
    brep: &crate::brep::graph::Brep,
) -> (
    super::ProjectedSketches,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let stream = cadmpeg_ir::stream_name!("Contents/Config-0-ResolvedFeatures");
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut losses = Vec::new();
    let mut projected = super::ProjectedSketches::default();
    super::project_brep(
        &ctx,
        brep,
        &super::BrepSketchSource {
            block_offset: 0,
            stream_ordinal: 0,
            stream_offset: 0,
            source_stream: &stream,
            sketch_name: "profile",
            configuration: None,
            native_ref: "sldprt:feature-input:resolved-features#0",
        },
        &mut annotations,
        &mut losses,
        &mut projected,
    )
    .expect("one unprojectable edge does not fail the projection");
    (projected, losses)
}

/// `given_up` is the `record.unreadable` of the edge the fixture refuses.
fn assert_refused_edge_given_up(given_up: &cadmpeg_ir::report::loss::LossNote) {
    assert_eq!(
        given_up.code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(given_up
        .message
        .split_whitespace()
        .any(|token| token == "test:profile:edge#2"));
}

#[test]
fn a_refused_loop_before_a_projected_loop_leaves_no_profile_in_its_place() {
    let (projected, losses) = project(&face_with_an_unprojectable_loop(&[1, 0]));
    let [sketch] = projected.sketches.as_slice() else {
        panic!("the face states one sketch");
    };
    let [entity] = projected.entities.as_slice() else {
        panic!("the line edge states the one entity");
    };
    assert_eq!(entity.native_ref.as_deref(), Some("0:test:profile:edge#1"));
    assert!(sketch.profiles.is_empty());
    let [loss, given_up] = losses.as_slice() else {
        panic!("one loss names the refused edge, which is given up");
    };
    assert_refused_edge_given_up(given_up);
    assert!(loss.message.contains("test:profile:edge#2"));
    assert!(loss.message.contains(&format!(
        "sketch {} states no entity for the edge and no profile for loop \
         test:profile:loop#2 or a later loop of face test:profile:face#1",
        sketch.id
    )));
}

#[test]
fn a_face_whose_only_loop_is_refused_states_no_sketch_and_the_loss_names_none() {
    let (projected, losses) = project(&face_with_an_unprojectable_loop(&[1]));
    assert!(projected.sketches.is_empty());
    assert!(projected.entities.is_empty());
    let [loss, given_up] = losses.as_slice() else {
        panic!("one loss names the refused edge, which is given up");
    };
    assert_refused_edge_given_up(given_up);
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::SketchProfileRejected.kind()
    );
    assert!(loss.message.contains("test:profile:edge#2"));
    assert!(loss
        .message
        .ends_with("; face test:profile:face#1 of the feature-input stream states no sketch"));
    assert!(!loss.message.contains("sldprt:model:sketch#"));
}

#[test]
fn an_unprojectable_edge_costs_its_loop_profile_and_keeps_the_sketch() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let brep = face_with_an_unprojectable_loop(&[0, 1]);
    let stream = cadmpeg_ir::stream_name!("Contents/Config-0-ResolvedFeatures");
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut losses = Vec::new();
    let mut projected = super::ProjectedSketches::default();
    super::project_brep(
        &ctx,
        &brep,
        &super::BrepSketchSource {
            block_offset: 0,
            stream_ordinal: 0,
            stream_offset: 0,
            source_stream: &stream,
            sketch_name: "profile",
            configuration: None,
            native_ref: "sldprt:feature-input:resolved-features#0",
        },
        &mut annotations,
        &mut losses,
        &mut projected,
    )
    .expect("one unprojectable edge does not fail the projection");
    let [sketch] = projected.sketches.as_slice() else {
        panic!("the face states one sketch");
    };
    let [entity] = projected.entities.as_slice() else {
        panic!("the line edge states the one entity");
    };
    assert_eq!(entity.native_ref.as_deref(), Some("0:test:profile:edge#1"));
    assert_eq!(sketch.profiles.len(), 1);
    assert_eq!(sketch.profiles[0].len(), 1);
    assert_eq!(sketch.profiles[0][0].entity, *entity.id());
    let [loss, given_up] = losses.as_slice() else {
        panic!("one loss names the refused edge, which is given up");
    };
    assert_refused_edge_given_up(given_up);
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::SketchProfileRejected.kind()
    );
    assert!(loss.message.contains("test:profile:edge#2"));
    assert!(loss.message.contains("test:profile:loop#2"));
    assert!(loss.message.contains(sketch.id.as_str()));
}
