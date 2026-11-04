// SPDX-License-Identifier: Apache-2.0
//! STEP writer recovery binding: wire and point fallback items reach the
//! product-less shape representation, and bodies with no writable topology
//! keep a loss naming them.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{BodyId, CurveId, EdgeId, PointId, RegionId, ShellId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Edge, Point, Region, Shell, Vertex};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;

use crate::loss::StepLossCode;
use crate::StepCodec;

#[test]
fn missing_inner_loop_keeps_the_other_faces() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let outer = ir.model.faces[0]
        .loops
        .iter()
        .next()
        .expect("outer loop")
        .clone();
    let missing = cadmpeg_ir::ids::LoopId::mint("test:model:loop#missing-inner").expect("identity");
    ir.model.faces[0].loops =
        cadmpeg_ir::topology::FaceLoops::classified(outer, vec![missing.clone()]);
    let mut bytes = Vec::new();
    let report = crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .expect("write surviving boundaries");
    assert!(report.losses.iter().any(|loss| loss.code
        == StepLossCode::FaceOmittedInnerLoop.kind()
        && loss.message.contains(missing.as_str())));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode surviving faces");
    assert_eq!(decoded.ir().model.faces.len(), 6);
}

#[test]
fn unwritable_inner_loop_names_every_unwritten_coedge() {
    use cadmpeg_ir::ids::{CoedgeId, LoopId};
    use cadmpeg_ir::topology::{EdgeCarrier, FaceLoops, Loop, LoopBoundary, LoopRing};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let face = ir.model.faces[0].id.clone();
    let outer = ir.model.faces[0]
        .loops
        .iter()
        .next()
        .expect("outer loop")
        .clone();
    let source = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == outer)
        .expect("loop")
        .clone();
    let inner = LoopId::mint("test:model:loop#inner").expect("identity");
    let mut ids = Vec::new();
    for (index, id) in source.coedges().iter().enumerate() {
        let mut coedge = ir
            .model
            .coedges
            .iter()
            .find(|coedge| &coedge.id == id)
            .expect("coedge")
            .clone();
        let new_id = CoedgeId::mint(format!("test:model:coedge#inner-{index}")).expect("identity");
        coedge.id = new_id.clone();
        coedge.owner_loop = inner.clone();
        coedge.radial_next = new_id.clone();
        if index + 1 == source.coedges().len() {
            let mut edge = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .expect("edge")
                .clone();
            edge.id = EdgeId::mint("test:model:edge#inner-missing-curve").expect("identity");
            edge.carrier = EdgeCarrier::unbounded(None);
            coedge.edge = edge.id.clone();
            ir.model.edges.push(edge);
        }
        ids.push(new_id);
        ir.model.coedges.push(coedge);
    }
    ir.model.loops.push(Loop {
        native_ref: None,
        id: inner.clone(),
        face,
        boundary: LoopBoundary::Ring(LoopRing::new(ids.clone(), Vec::new()).expect("ring")),
    });
    ir.model.faces[0].loops = FaceLoops::classified(outer, vec![inner]);
    let mut bytes = Vec::new();
    let report = crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .expect("write surviving faces");
    for id in ids {
        assert!(
            report.losses.iter().any(|loss| loss.code
                == StepLossCode::CoedgeLoopMemberUnwritten.kind()
                && loss.message.contains(id.as_str())),
            "coedge {id}"
        );
    }
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode surviving faces");
    assert_eq!(decoded.ir().model.faces.len(), 6);
}

#[test]
fn placed_curve_and_surface_keep_their_replica_transforms() {
    let template = cadmpeg_ir::examples::unit_cube().expect("cube");
    let mut ir = CadIr::empty();
    let transform = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 15.0],
        [0.0, 1.0, 0.0, 10.0],
        [0.0, 0.0, 1.0, 2.0],
    ])
    .expect("translation");
    let mut curve = template.model.curves[0].clone();
    let curve_basis = curve.geometry.solved().expect("curve basis").clone();
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(Box::new(curve_basis), transform)
            .expect("placed curve"),
    ));
    let mut surface = template.model.surfaces[0].clone();
    let surface_basis = surface.geometry.solved().expect("surface basis").clone();
    surface.geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedSurface::try_new(Box::new(surface_basis), transform)
                .expect("placed surface"),
        ),
    );
    ir.model.curves.push(curve);
    ir.model.surfaces.push(surface);
    let mut bytes = Vec::new();
    crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .expect("write replicas");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode replicas");
    assert!(decoded.ir().model.curves.iter().any(|curve| matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Transformed(placed)) if *placed.transform() == transform)));
    assert!(decoded.ir().model.surfaces.iter().any(|surface| matches!(surface.geometry.solved(), Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Transformed(placed)) if *placed.transform() == transform)));
}

#[test]
fn distinct_coincident_points_remain_distinct() {
    let mut ir = CadIr::empty();
    for name in ["first", "second"] {
        ir.model.points.push(Point::new(
            PointId::mint(format!("test:model:point#{name}")).expect("identity"),
            FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("point"),
            None,
        ));
    }
    let mut bytes = Vec::new();
    crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .expect("write points");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode points");
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.position().get() == Point3::new(1.0, 2.0, 3.0)));
}

#[test]
fn foreign_unknown_stubs_are_named_individually() {
    let mut ir = CadIr::empty();
    let unknowns = [
        "foreign:native:record#first",
        "foreign:native:record#second",
    ]
    .into_iter()
    .map(|id| {
        cadmpeg_ir::native::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id).expect("identity"),
            Default::default(),
        )
        .expect("native record")
    })
    .collect();
    ir.native
        .namespace_mut("foreign")
        .arenas_mut()
        .insert("unknowns".to_owned(), unknowns);
    let mut bytes = Vec::new();
    let report = crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .expect("write");
    for id in [
        "foreign:native:record#first",
        "foreign:native:record#second",
    ] {
        assert!(report.losses.iter().any(|loss| {
            loss.code.local_code() == "native.records-omitted" && loss.message.contains(id)
        }));
    }
}

#[test]
fn stranded_pcurve_and_its_surface_round_trip() {
    let source = include_bytes!("../../../../tests/fixtures/damaged/ap214_sheet_bad_real.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode damaged sheet");
    let mut document = decoded.ir().clone();
    let original = document.model.pcurves[0].geometry.clone();
    let mut second = document.model.pcurves[0].clone();
    second.id =
        cadmpeg_ir::ids::PcurveId::mint("test:model:pcurve#second").expect("identity grammar");
    document.model.pcurves[0].id =
        cadmpeg_ir::ids::PcurveId::mint("test:model:pcurve#first").expect("identity grammar");
    document.model.pcurves.push(second);
    let mut bytes = Vec::new();
    crate::export::write_step(
        &document,
        &mut bytes,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .expect("write pcurve");
    let written = String::from_utf8_lossy(&bytes).into_owned();
    let reread = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written pcurve");
    assert_eq!(
        reread.ir().model.pcurves.len(),
        2,
        "{written}\n{:?}",
        reread.report().losses
    );
    assert!(reread.ir().model.pcurves.iter().all(|pcurve| {
        pcurve.geometry == original
            && pcurve.support_surface().is_some_and(|support| {
                reread
                    .ir()
                    .model
                    .surfaces
                    .iter()
                    .any(|surface| &surface.id == support)
            })
    }));
    assert_eq!(
        reread.ir().model.pcurves[0].support_surface(),
        reread.ir().model.pcurves[1].support_surface()
    );
}

/// A wire body holding one free vertex and no edges, as a point-only
/// source decodes one.
fn point_only_doc() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.points.push(Point::new(
        PointId::mint("test:model:point#recovery0").expect("identity grammar"),
        FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("a finite position is a point"),
        None,
    ));
    ir.model.vertices.push(Vertex {
        native_ref: None,
        id: VertexId::mint("test:model:vertex#recovery0").expect("identity grammar"),
        point: PointId::mint("test:model:point#recovery0").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.shells.push(Shell::with_free_vertex(
        ShellId::mint("test:model:shell#recovery").expect("identity grammar"),
        RegionId::mint("test:model:region#recovery").expect("identity grammar"),
        VertexId::mint("test:model:vertex#recovery0").expect("identity grammar"),
    ));
    ir.model.regions.push(Region {
        native_ref: None,
        id: RegionId::mint("test:model:region#recovery").expect("identity grammar"),
        body: BodyId::mint("test:model:body#recovery").expect("identity grammar"),
        shells: vec![ShellId::mint("test:model:shell#recovery").expect("identity grammar")],
    });
    ir.model.bodies.push(Body {
        native_ref: None,
        id: BodyId::mint("test:model:body#recovery").expect("identity grammar"),
        kind: BodyKind::Wire,
        regions: vec![RegionId::mint("test:model:region#recovery").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir
}

#[test]
fn orphan_edges_without_carriers_each_have_a_loss() {
    let mut ir = point_only_doc();
    for name in ["first", "second"] {
        ir.model.edges.push(Edge {
            native_ref: None,
            id: EdgeId::mint(format!("test:model:edge#{name}")).expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
            start: ir.model.vertices[0].id.clone(),
            end: ir.model.vertices[0].id.clone(),
            tolerance: None,
        });
    }
    let mut bytes = Vec::new();
    let report = crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap214,
        &crate::StepWriteOptions::default(),
    )
    .expect("write document");
    let losses: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::EdgeNo3dCurve.kind())
        .collect();
    assert_eq!(losses.len(), 2);
    for name in ["first", "second"] {
        assert!(losses
            .iter()
            .any(|loss| loss.message.contains(&format!("test:model:edge#{name}"))));
    }
}

#[test]
fn point_only_wire_body_names_its_drop_and_binds_its_point() {
    let mut bytes = Vec::new();
    let report = crate::export::write_step(
        &point_only_doc(),
        &mut bytes,
        crate::StepSchema::Ap214,
        &crate::StepWriteOptions::default(),
    )
    .expect("write point recovery");
    let text = String::from_utf8(bytes.clone()).expect("STEP is UTF-8");
    assert!(
        text.contains("GEOMETRIC_CURVE_SET("),
        "the retained point reaches the file"
    );
    assert!(
        text.contains("SHAPE_REPRESENTATION("),
        "recovery items bind in the product-less representation"
    );
    assert!(
        report.losses.iter().any(|loss| {
            loss.code == StepLossCode::BodyNoWritableTopology.kind()
                && loss.message.contains("test:model:body#recovery")
        }),
        "the dropped body keeps a loss: {:?}",
        report.losses
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode point recovery");
    assert_eq!(decoded.ir().model.points.len(), 1);
}

/// A wire body with one edge and no other topology keeps no
/// `BodyNoWritableTopology` loss: its bound wireframe decodes to a body.
#[test]
fn wire_body_with_an_edge_forms_no_body_drop() {
    use cadmpeg_ir::geometry::analytic::LineCurve;

    let mut ir = point_only_doc();
    ir.model.points.push(Point::new(
        PointId::mint("test:model:point#recovery1").expect("identity grammar"),
        FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).expect("a finite position is a point"),
        None,
    ));
    ir.model.vertices.push(Vertex {
        native_ref: None,
        id: VertexId::mint("test:model:vertex#recovery1").expect("identity grammar"),
        point: PointId::mint("test:model:point#recovery1").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: CurveId::mint("test:model:curve#recovery").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
            FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite origin"),
            UnitVector3::normalized(Vector3::new(1.0, 1.0, 1.0)).expect("unit direction"),
        ))),
        source_object: None,
    });
    ir.model.edges.push(Edge {
        native_ref: None,
        id: EdgeId::mint("test:model:edge#recovery").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(
            CurveId::mint("test:model:curve#recovery").expect("identity grammar"),
        )),
        start: VertexId::mint("test:model:vertex#recovery0").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#recovery1").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.shells[0] = Shell::with_wire_edge(
        ShellId::mint("test:model:shell#recovery").expect("identity grammar"),
        RegionId::mint("test:model:region#recovery").expect("identity grammar"),
        EdgeId::mint("test:model:edge#recovery").expect("identity grammar"),
    );

    let mut bytes = Vec::new();
    let report = crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap214,
        &crate::StepWriteOptions::default(),
    )
    .expect("write wire recovery");
    let text = String::from_utf8(bytes.clone()).expect("STEP is UTF-8");
    assert!(text.contains("EDGE_BASED_WIREFRAME_MODEL("));
    assert!(
        !report.losses.iter().any(|loss| {
            loss.code == StepLossCode::BodyNoWritableTopology.kind()
                && loss.message.contains("test:model:body#recovery")
        }),
        "a bound wireframe represents the body: {:?}",
        report.losses
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode wire recovery");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 1);
}

/// A sheet face that loses its carrier-less edge keeps no
/// `BodyNoWritableTopology` loss when its surviving edge is retained as
/// wire: the bound wireframe decodes to a wire body.
#[test]
fn fallback_wire_recovery_forms_no_body_drop() {
    use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CoedgeId, FaceId, LoopId, SurfaceId};
    use cadmpeg_ir::topology::{Coedge, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Sense};

    let mut ir = point_only_doc();
    ir.model.bodies[0].kind = BodyKind::Sheet;
    ir.model.points.push(Point::new(
        PointId::mint("test:model:point#recovery1").expect("identity grammar"),
        FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("a finite position is a point"),
        None,
    ));
    ir.model.vertices.push(Vertex {
        native_ref: None,
        id: VertexId::mint("test:model:vertex#recovery1").expect("identity grammar"),
        point: PointId::mint("test:model:point#recovery1").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: CurveId::mint("test:model:curve#recovery").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
            FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite origin"),
            UnitVector3::normalized(Vector3::new(-1.0, 0.0, 0.0)).expect("unit direction"),
        ))),
        source_object: None,
    });
    ir.model.edges.push(Edge {
        native_ref: None,
        id: EdgeId::mint("test:model:edge#bare").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
        start: VertexId::mint("test:model:vertex#recovery0").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#recovery1").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.edges.push(Edge {
        native_ref: None,
        id: EdgeId::mint("test:model:edge#recovery").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(
            CurveId::mint("test:model:curve#recovery").expect("identity grammar"),
        )),
        start: VertexId::mint("test:model:vertex#recovery1").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#recovery0").expect("identity grammar"),
        tolerance: None,
    });
    let coedge = |id: &str, edge: &str| Coedge {
        native_ref: None,
        id: CoedgeId::mint(id).expect("identity grammar"),
        owner_loop: LoopId::mint("test:model:loop#recovery").expect("identity grammar"),
        edge: EdgeId::mint(edge).expect("identity grammar"),
        radial_next: CoedgeId::mint(id).expect("identity grammar"),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    };
    ir.model
        .coedges
        .push(coedge("test:model:coedge#bare", "test:model:edge#bare"));
    ir.model.coedges.push(coedge(
        "test:model:coedge#recovery",
        "test:model:edge#recovery",
    ));
    ir.model.loops.push(Loop {
        native_ref: None,
        id: LoopId::mint("test:model:loop#recovery").expect("identity grammar"),
        face: FaceId::mint("test:model:face#recovery").expect("identity grammar"),
        boundary: LoopBoundary::Ring(
            LoopRing::new(
                vec![
                    CoedgeId::mint("test:model:coedge#bare").expect("identity grammar"),
                    CoedgeId::mint("test:model:coedge#recovery").expect("identity grammar"),
                ],
                Vec::new(),
            )
            .expect("valid loop ring"),
        ),
    });
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: SurfaceId::mint("test:model:surface#recovery").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.faces.push(Face {
        native_ref: None,
        id: FaceId::mint("test:model:face#recovery").expect("identity grammar"),
        shell: ShellId::mint("test:model:shell#recovery").expect("identity grammar"),
        surface: SurfaceId::mint("test:model:surface#recovery").expect("identity grammar"),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(vec![
            LoopId::mint("test:model:loop#recovery").expect("identity grammar")
        ]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells[0] = Shell::with_face(
        ShellId::mint("test:model:shell#recovery").expect("identity grammar"),
        RegionId::mint("test:model:region#recovery").expect("identity grammar"),
        FaceId::mint("test:model:face#recovery").expect("identity grammar"),
    );

    let mut bytes = Vec::new();
    let report = crate::export::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap214,
        &crate::StepWriteOptions::default(),
    )
    .expect("write fallback wire recovery");
    let text = String::from_utf8(bytes.clone()).expect("STEP is UTF-8");
    assert!(text.contains("EDGE_BASED_WIREFRAME_MODEL("));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::RegionEdgesRecovered.kind()
            && loss.message.contains("test:model:edge#recovery")
            && loss.message.contains("test:model:curve#recovery")
    }));
    assert!(
        !report.losses.iter().any(|loss| {
            loss.code == StepLossCode::BodyNoWritableTopology.kind()
                && loss.message.contains("test:model:body#recovery")
        }),
        "a bound recovery wireframe represents the body: {:?}",
        report.losses
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode fallback wire recovery");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 1);
}
