// SPDX-License-Identifier: Apache-2.0
//! Sheets whose faces are whole bounded surfaces, written as geometric sets.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// A sheet whose one face is the whole unit-square patch, with no loops.
fn whole_patch_sheet() -> CadIr {
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Face, FaceLoops, Region, Sense, Shell};
    let id = |kind: &str| format!("test:model:{kind}#sheet");
    let mut ir = CadIr::empty();
    ir.geometry.surfaces.push(Surface {
        id: SurfaceId::mint(id("surface")).unwrap(),
        geometry: SurfaceGeometry::Solved(crate::test_support::curve_bounded::unit_square_patch()),
        source_object: None,
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: FaceId::mint(id("face")).unwrap(),
        shell: ShellId::mint(id("shell")).unwrap(),
        surface: SurfaceId::mint(id("surface")).unwrap(),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.shells.push(Shell::new(
        ShellId::mint(id("shell")).unwrap(),
        RegionId::mint(id("region")).unwrap(),
    ));
    ir.geometry.regions.push(Region {
        id: RegionId::mint(id("region")).unwrap(),
        body: BodyId::mint(id("body")).unwrap(),
        shells: vec![ShellId::mint(id("shell")).unwrap()],
        native_ref: None,
    });
    ir.geometry.bodies.push(Body {
        id: BodyId::mint(id("body")).unwrap(),
        kind: BodyKind::Sheet,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.finalize();
    ir
}

/// The writer states a face that is a whole bounded surface as a member of
/// a geometric set, which decodes as the same one-face sheet.
#[test]
fn a_sheet_of_whole_bounded_surfaces_round_trips_as_a_geometric_set() {
    let mut output = Vec::new();
    let report = write_step(
        &whole_patch_sheet(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Error));
    let text = String::from_utf8(output).unwrap();
    assert_eq!(
        text.matches("= GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION(")
            .count(),
        1
    );
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 1);
    assert_eq!(text.matches("= ADVANCED_FACE(").count(), 0);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    let geometry = &decoded.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1);
    assert_eq!(
        geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    let [face] = geometry.faces.as_slice() else {
        panic!("one face: {:#?}", geometry.faces);
    };
    assert!(face.loops.is_empty());
}

/// The `#n = TYPE(...)` records of a written file, by instance number:
/// simple records only, which is every record the walk below follows.
fn records(text: &str) -> std::collections::HashMap<u64, (&str, &str)> {
    text.lines()
        .filter_map(|line| {
            let (id, rest) = line.strip_prefix('#')?.split_once(" = ")?;
            let (name, arguments) = rest.split_once('(')?;
            Some((id.parse().ok()?, (name, arguments)))
        })
        .collect()
}

/// The instance references in a record's arguments, in order.
fn references(arguments: &str) -> Vec<u64> {
    arguments
        .split('#')
        .skip(1)
        .filter_map(|tail| {
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

/// The `GEOMETRIC_SET`s a consumer reaches from `representation` by
/// following only its items and, from a `MAPPED_ITEM`, its map's mapped
/// representation: no `SHAPE_REPRESENTATION_RELATIONSHIP`, in either
/// direction.
fn directed_sets(
    records: &std::collections::HashMap<u64, (&str, &str)>,
    representation: u64,
) -> Vec<u64> {
    let mut found = Vec::new();
    let mut pending = vec![representation];
    while let Some(representation) = pending.pop() {
        let (_, arguments) = records[&representation];
        // The items are the representation's one list argument; the context
        // reference follows it.
        let items = arguments
            .split_once('(')
            .and_then(|(_, tail)| tail.split_once(')'))
            .map(|(items, _)| references(items))
            .unwrap_or_default();
        for item in items {
            match records[&item] {
                ("GEOMETRIC_SET", _) => found.push(item),
                ("MAPPED_ITEM", arguments) => {
                    let map = references(arguments)[0];
                    let (name, map_arguments) = records[&map];
                    assert_eq!(name, "REPRESENTATION_MAP");
                    pending.push(references(map_arguments)[1]);
                }
                _ => {}
            }
        }
    }
    found
}

fn placed_patch_sheet() -> CadIr {
    let mut ir = whole_patch_sheet();
    ir.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
        ])
        .unwrap(),
    );
    ir
}

/// A placed sheet's body-local representation would hold nothing but its
/// set, and `representation.items` is SET [1:?]: the map names the
/// bounded-surface representation itself, and a reader that follows the
/// product's representation, its mapped item and the map's representation,
/// and no relationship, reaches the set. The decoded body keeps its
/// placement, and so does a second write of it.
#[test]
fn a_placed_sheet_maps_its_bounded_surface_representation() {
    let ir = placed_patch_sheet();
    let mut text = String::new();
    let mut decoded_ir = ir.clone();
    for pass in 0..2 {
        let mut output = Vec::new();
        write_step(
            &decoded_ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .unwrap();
        text = String::from_utf8(output).unwrap();
        let records = records(&text);
        for (id, (name, arguments)) in &records {
            assert!(
                !(name.ends_with("REPRESENTATION") && arguments.contains(",(),")),
                "pass {pass}: #{id} {name} has no items:\n{text}"
            );
        }
        let (_, definition) = records
            .values()
            .find(|(name, _)| *name == "SHAPE_DEFINITION_REPRESENTATION")
            .unwrap();
        let product_representation = references(definition)[1];
        assert_eq!(
            directed_sets(&records, product_representation).len(),
            1,
            "pass {pass}:\n{text}"
        );
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(text.clone()), &DecodeOptions::default())
            .unwrap();
        assert!(
            !decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("GEOMETRICALLY_BOUNDED")),
            "pass {pass}: {:#?}",
            decoded.report().losses
        );
        let geometry = &decoded.ir().geometry;
        let [body] = geometry.bodies.as_slice() else {
            panic!("pass {pass}: one body: {:#?}", geometry.bodies);
        };
        assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Sheet);
        assert_eq!(body.transform, ir.geometry.bodies[0].transform);
        let [face] = geometry.faces.as_slice() else {
            panic!("pass {pass}: one face: {:#?}", geometry.faces);
        };
        assert!(face.loops.is_empty());
        decoded_ir = decoded.ir().clone();
    }
    assert_eq!(text.matches("= MAPPED_ITEM(").count(), 1);
}

/// A geometric set of two curve-bounded surfaces on one plane: a ring of
/// outer radius 5 with a hole of radius 2, and a disc of radius 1.
fn ring_and_disc_set() -> String {
    let mut source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .unwrap()
    .replace(
        "#10=(BOUNDARY_CURVE() ",
        "#10=(BOUNDARY_CURVE() OUTER_BOUNDARY_CURVE() ",
    )
    .replace(
        "#11=CURVE_BOUNDED_SURFACE('disc',#8,(#10),.F.);",
        "#11=CURVE_BOUNDED_SURFACE('ring',#8,(#10,#16),.F.);",
    )
    .replace(
        "#12=GEOMETRIC_SET('',(#11));",
        "#12=GEOMETRIC_SET('',(#11,#20));",
    );
    let end = source.rfind("ENDSEC;").unwrap();
    source.insert_str(
        end,
        "#14=CIRCLE('',#6,2.);\n\
         #15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#14);\n\
         #16=BOUNDARY_CURVE('hole',(#15),.F.);\n\
         #17=CIRCLE('',#6,1.);\n\
         #18=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#17);\n\
         #19=OUTER_BOUNDARY_CURVE('',(#18),.F.);\n\
         #20=CURVE_BOUNDED_SURFACE('disc',#8,(#19),.F.);\n",
    );
    source
}

/// The loop counts of the faces of a one-body sheet, ascending.
fn sheet_loop_counts(ir: &CadIr) -> Vec<usize> {
    let geometry = &ir.geometry;
    assert_eq!(geometry.bodies.len(), 1, "{:#?}", geometry.bodies);
    assert_eq!(
        geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    let mut counts: Vec<usize> = geometry.faces.iter().map(|face| face.loops.len()).collect();
    counts.sort_unstable();
    counts
}

/// The writer states the curve-bounded faces of a sheet as the members of
/// one geometric set, each bounded by its boundary curves, so the sheet
/// decodes as one body with the same faces and loops.
#[test]
fn a_sheet_of_curve_bounded_surfaces_round_trips_as_one_geometric_set() {
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(ring_and_disc_set()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(sheet_loop_counts(source.ir()), [1, 2]);
    let mut output = Vec::new();
    write_step(
        source.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 1, "{text}");
    assert_eq!(text.matches("= CURVE_BOUNDED_SURFACE(").count(), 2);
    assert_eq!(text.matches("= OUTER_BOUNDARY_CURVE(").count(), 2);
    assert_eq!(text.matches("= BOUNDARY_CURVE(").count(), 1);
    assert_eq!(text.matches("= SHELL_BASED_SURFACE_MODEL(").count(), 0);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    assert_eq!(sheet_loop_counts(decoded.ir()), [1, 2]);
}

/// A surface profile tolerance on `target`.
fn profile_on(target: cadmpeg_ir::pmi::PmiTarget) -> cadmpeg_ir::pmi::PmiAnnotation {
    use cadmpeg_ir::design::options::NativeOptions;
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{
        GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiMagnitude, PmiQuantity, PmiValue,
    };
    PmiAnnotation {
        id: PmiId::mint("test:surface-set:pmi#profile").unwrap(),
        name: Some("profile".into()),
        visible: None,
        targets: vec![target.into()],
        definition: PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::SurfaceProfile,
            magnitude: PmiMagnitude::new(PmiValue::new(0.5, PmiQuantity::Length).unwrap()).unwrap(),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers: Vec::new(),
            displacement: None,
            maximum_upper: None,
            zones: Vec::new(),
        },
        native_ref: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    }
}

/// A PMI annotation on a curve-bounded face, on an edge of its loops, or
/// on a vertex of those edges needs that face, edge, or vertex as a written
/// item. The sheet is then written as a shell of faces, and the annotation
/// names its target through a `GEOMETRIC_ITEM_SPECIFIC_USAGE`.
#[test]
fn a_pmi_target_on_a_curve_bounded_face_keeps_the_sheet_a_shell() {
    use cadmpeg_ir::pmi::PmiTarget;
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(ring_and_disc_set()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ring = source
        .ir()
        .geometry
        .faces
        .iter()
        .find(|face| face.loops.len() == 2)
        .unwrap();
    let edge = source
        .ir()
        .geometry
        .loops
        .iter()
        .find(|face_loop| ring.loops.contains(&face_loop.id))
        .and_then(|face_loop| face_loop.coedges().first())
        .and_then(|coedge| {
            source
                .ir()
                .geometry
                .coedges
                .iter()
                .find(|candidate| &candidate.id == coedge)
        })
        .map(|coedge| coedge.edge.clone())
        .unwrap();
    let mut targets = vec![
        PmiTarget::Face {
            face: ring.id.clone(),
        },
        PmiTarget::Edge { edge: edge.clone() },
    ];
    if let Some(vertex) = source
        .ir()
        .geometry
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.vertices().next())
    {
        targets.push(PmiTarget::Vertex {
            vertex: vertex.clone(),
        });
    }
    for target in targets {
        let mut ir = source.ir().clone();
        ir.model.pmi = vec![profile_on(target.clone())];
        let mut output = Vec::new();
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{target:?}");
        assert_eq!(
            text.matches("= SHELL_BASED_SURFACE_MODEL(").count(),
            1,
            "{target:?}"
        );
        assert!(
            text.contains("= GEOMETRIC_ITEM_SPECIFIC_USAGE("),
            "{target:?}\n{text}"
        );
    }
}

/// A PMI annotation on the point of a vertex of a curve-bounded face's
/// loop needs that vertex written, since its point is not a free point.
/// The sheet is then written as a shell of faces, and the annotation names
/// the point through a `GEOMETRIC_ITEM_SPECIFIC_USAGE`.
#[test]
fn a_pmi_target_on_a_vertex_point_of_a_curve_bounded_face_keeps_the_sheet_a_shell() {
    use cadmpeg_ir::pmi::PmiTarget;
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_geometric_set.p21"
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let write = |ir: &CadIr| {
        let mut output = Vec::new();
        let report = write_step(
            ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        (String::from_utf8(output).unwrap(), report)
    };
    let (text, _) = write(source.ir());
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 1, "{text}");
    let geometry = &source.ir().geometry;
    let [edge] = geometry.edges.as_slice() else {
        panic!("one edge: {:#?}", geometry.edges);
    };
    let vertex = edge.vertices().next().unwrap();
    let point = geometry
        .vertices
        .iter()
        .find(|candidate| &candidate.id == vertex)
        .map(|vertex| vertex.point.clone())
        .unwrap();
    let mut ir = source.ir().clone();
    ir.model.pmi = vec![profile_on(PmiTarget::Point { point })];
    let (text, report) = write(&ir);
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= SHELL_BASED_SURFACE_MODEL(").count(), 1);
    assert!(text.contains("= GEOMETRIC_ITEM_SPECIFIC_USAGE("), "{text}");
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::StepLossCode::PmiAnnotationNotWritten.kind()),
        "{:#?}",
        report.losses
    );
}

/// A set member writes neither a face name nor a coedge pcurve, so a named
/// curve-bounded face, or one with a pcurve on a coedge, keeps the sheet a
/// shell of faces, and the name reads back.
#[test]
fn a_named_or_pcurve_bearing_curve_bounded_face_keeps_the_sheet_a_shell() {
    use cadmpeg_ir::geometry::pcurve::{
        Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs, PcurveNurbsPoles,
    };
    use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
    use cadmpeg_ir::ids::PcurveId;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::topology::PcurveUse;
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_geometric_set.p21"
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let write = |ir: &CadIr| {
        let mut output = Vec::new();
        write_step(
            ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .unwrap();
        String::from_utf8(output).unwrap()
    };
    let text = write(source.ir());
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 1, "{text}");

    let mut named = source.ir().clone();
    let [face] = named.geometry.faces.as_mut_slice() else {
        panic!("one face: {:#?}", named.geometry.faces);
    };
    face.name = Some("named".into());
    let text = write(&named);
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    let [face] = decoded.ir().geometry.faces.as_slice() else {
        panic!("one face: {:#?}", decoded.ir().geometry.faces);
    };
    assert_eq!(face.name.as_deref(), Some("named"));

    let mut with_pcurve = source.ir().clone();
    let pcurve = PcurveId::mint("test:surface-set:pcurve#line").unwrap();
    with_pcurve.geometry.pcurves.push(Pcurve {
        id: pcurve.clone(),
        geometry: PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                PcurveNurbsPoles::Polynomial {
                    points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                },
                false,
            )
            .unwrap(),
        },
        metadata: PcurveMetadata::default(),
        native_ref: None,
    });
    let [coedge] = with_pcurve.geometry.coedges.as_mut_slice() else {
        panic!("one coedge: {:#?}", with_pcurve.geometry.coedges);
    };
    coedge.pcurves.push(PcurveUse {
        pcurve,
        isoparametric: None,
        parameter_range: Some(DirectedParameterRange::new([0.0, 1.0]).unwrap()),
    });
    let text = write(&with_pcurve);
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
}

/// The disc of `ap242_geometric_set.p21`, its loop split into two
/// half-circle edges on the one boundary circle.
fn disc_split_into_two_edges() -> CadIr {
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{EdgeCarrier, EdgeEnds, Point, Vertex};
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_geometric_set.p21"
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = source.ir().clone();
    let geometry = &mut ir.geometry;
    let [face_loop] = geometry.loops.as_mut_slice() else {
        panic!("one loop: {:#?}", geometry.loops);
    };
    let [coedge] = face_loop.coedges() else {
        panic!("one coedge: {face_loop:#?}");
    };
    let coedge = geometry
        .coedges
        .iter()
        .find(|candidate| &candidate.id == coedge)
        .unwrap()
        .clone();
    let edge_index = geometry
        .edges
        .iter()
        .position(|edge| edge.id == coedge.edge)
        .unwrap();
    let edge = geometry.edges[edge_index].clone();
    let circle = edge.curve().unwrap().clone();
    let start = edge.start().unwrap().clone();
    let point = PointId::mint("test:surface-set:point#half").unwrap();
    let middle = VertexId::mint("test:surface-set:vertex#half").unwrap();
    geometry.points.push(Point::new(
        point.clone(),
        FinitePoint3::new(Point3::new(-5.0, 0.0, 0.0)).unwrap(),
        None,
    ));
    geometry.vertices.push(Vertex {
        id: middle.clone(),
        point,
        tolerance: None,
        native_ref: None,
    });
    let half = std::f64::consts::PI;
    geometry.edges[edge_index].carrier =
        EdgeCarrier::new(Some(circle.clone()), Some([0.0, half])).unwrap();
    geometry.edges[edge_index].ends = Some(EdgeEnds::new(start.clone(), middle.clone()));
    let second_edge = EdgeId::mint("test:surface-set:edge#half").unwrap();
    geometry.edges.push(cadmpeg_ir::topology::Edge {
        id: second_edge.clone(),
        carrier: EdgeCarrier::new(Some(circle), Some([half, 2.0 * half])).unwrap(),
        ends: Some(EdgeEnds::new(middle, start)),
        ..edge
    });
    let second_coedge = CoedgeId::mint("test:surface-set:coedge#half").unwrap();
    geometry.coedges.push(cadmpeg_ir::topology::Coedge {
        id: second_coedge.clone(),
        edge: second_edge,
        radial_next: second_coedge.clone(),
        pcurves: Vec::new(),
        ..coedge.clone()
    });
    let [face_loop] = geometry.loops.as_mut_slice() else {
        unreachable!();
    };
    face_loop
        .replace_ring(vec![coedge.id, second_coedge], Vec::new())
        .unwrap();
    ir.finalize();
    ir
}

/// A curve-bounded face whose loop splits a boundary segment into two
/// edges has no edge per segment: the writer keeps the face a shell
/// member, so both edges survive a round trip.
#[test]
fn a_loop_that_splits_a_boundary_segment_keeps_the_sheet_a_shell() {
    let ir = disc_split_into_two_edges();
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().geometry.edges.len(), 2);
    assert_eq!(decoded.ir().geometry.vertices.len(), 2);
}

/// A ring of outer radius 5 with a hole of radius 2, and a disc of radius
/// 2 that fills the hole, in one shell: the disc's loop and the ring's
/// hole use one edge on the shared boundary circle.
fn ring_and_disc_sharing_an_edge() -> CadIr {
    let source = ring_and_disc_set().replace(
        "#19=OUTER_BOUNDARY_CURVE('',(#18),.F.);",
        "#19=OUTER_BOUNDARY_CURVE('',(#15),.F.);",
    );
    let source = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = source.ir().clone();
    let geometry = &mut ir.geometry;
    let ring = geometry
        .faces
        .iter()
        .find(|face| face.loops.len() == 2)
        .unwrap()
        .clone();
    let disc_index = geometry
        .faces
        .iter()
        .position(|face| face.loops.len() == 1)
        .unwrap();
    let disc = geometry.faces[disc_index].clone();
    let loop_coedges = |geometry: &cadmpeg_ir::document::Topology,
                        face: &cadmpeg_ir::topology::Face| {
        geometry
            .loops
            .iter()
            .filter(|face_loop| face.loops.contains(&face_loop.id))
            .flat_map(|face_loop| face_loop.coedges().to_vec())
            .collect::<Vec<_>>()
    };
    let coedge = |geometry: &cadmpeg_ir::document::Topology, id: &cadmpeg_ir::ids::CoedgeId| {
        geometry
            .coedges
            .iter()
            .find(|candidate| &candidate.id == id)
            .unwrap()
            .clone()
    };
    let edge = |geometry: &cadmpeg_ir::document::Topology, id: &cadmpeg_ir::ids::EdgeId| {
        geometry
            .edges
            .iter()
            .find(|candidate| &candidate.id == id)
            .unwrap()
            .clone()
    };
    let disc_coedges = loop_coedges(geometry, &disc);
    let [disc_coedge] = disc_coedges.as_slice() else {
        panic!("one disc coedge: {disc_coedges:#?}");
    };
    let disc_coedge = coedge(geometry, disc_coedge);
    let disc_edge = edge(geometry, &disc_coedge.edge);
    let hole_coedge = loop_coedges(geometry, &ring)
        .iter()
        .map(|id| coedge(geometry, id))
        .find(|candidate| edge(geometry, &candidate.edge).curve() == disc_edge.curve())
        .expect("the ring's hole coedge on the disc's circle");
    for candidate in &mut geometry.coedges {
        if candidate.id == disc_coedge.id {
            candidate.edge = hole_coedge.edge.clone();
            candidate.radial_next = hole_coedge.id.clone();
        } else if candidate.id == hole_coedge.id {
            candidate.radial_next = disc_coedge.id.clone();
        }
    }
    geometry
        .edges
        .retain(|candidate| candidate.id != disc_edge.id);
    let used: std::collections::BTreeSet<_> = geometry
        .edges
        .iter()
        .flat_map(|candidate| candidate.vertices().cloned().collect::<Vec<_>>())
        .collect();
    geometry.vertices.retain(|vertex| used.contains(&vertex.id));
    geometry.faces[disc_index].shell = ring.shell.clone();
    geometry.shells.retain(|shell| shell.id != disc.shell);
    for region in &mut geometry.regions {
        region.shells.retain(|shell| shell != &disc.shell);
    }
    ir.finalize();
    ir
}

/// Two curve-bounded faces that share an edge: set members state no
/// edge, so the writer keeps the sheet a shell, and the shared edge and
/// the one shell survive a round trip.
#[test]
fn curve_bounded_faces_that_share_an_edge_keep_the_sheet_a_shell() {
    let ir = ring_and_disc_sharing_an_edge();
    assert_eq!(sheet_loop_counts(&ir), [1, 2]);
    assert_eq!(ir.geometry.shells.len(), 1);
    assert_eq!(ir.geometry.edges.len(), 2);
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    let geometry = &decoded.ir().geometry;
    assert_eq!(sheet_loop_counts(decoded.ir()), [1, 2]);
    assert_eq!(geometry.shells.len(), 1, "{:#?}", geometry.shells);
    assert_eq!(geometry.edges.len(), 2, "{:#?}", geometry.edges);
    let shared = geometry
        .edges
        .iter()
        .filter(|edge| {
            geometry
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
                .count()
                == 2
        })
        .count();
    assert_eq!(shared, 1, "{:#?}", geometry.coedges);
}

/// The implicit-outer sheet of `support`, with its four hole lines as the
/// segments of one composite boundary and no coedge pcurves, so its face
/// pairs its one loop with that boundary.
fn implicit_outer_composite_sheet(support: cadmpeg_ir::geometry::SolvedSurfaceGeometry) -> CadIr {
    use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, Curve,
        CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::topology::Sense;
    let mut ir = crate::test_support::curve_bounded::implicit_outer_sheet(support, Sense::Forward);
    let composite = CurveId::mint("test:surface-set:curve#hole").unwrap();
    let [procedural] = ir.geometry.procedural_surfaces.as_mut_slice() else {
        panic!("one procedural surface");
    };
    let segments = procedural.edit_definition(|definition| {
        let ProceduralSurfaceDefinition::CurveBounded {
            boundaries,
            boundary_pcurves,
            ..
        } = definition
        else {
            panic!("a curve-bounded surface: {definition:#?}");
        };
        boundary_pcurves.clear();
        std::mem::replace(boundaries, vec![composite.clone()])
    });
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: composite,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: CompositeCurveSegments::try_from(
                segments
                    .into_iter()
                    .map(|curve| CompositeCurveSegment {
                        curve,
                        same_sense: true,
                        transition: CompositeCurveTransition::Continuous,
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
            self_intersect: None,
        }),
        source_object: None,
        native_ref: None,
    });
    for coedge in &mut ir.geometry.coedges {
        coedge.pcurves.clear();
    }
    ir.geometry.pcurves.clear();
    ir
}

/// A set member states its implicit outer boundary as the support's
/// natural boundary, which the writer states for a face only through the
/// face's own bounds. A face whose outer boundary is implicit keeps the
/// sheet a shell of faces: on an unbounded plane the writer omits the face
/// with a loss, and on a NURBS patch it writes the parameter rectangle as
/// the face's outer bound.
#[test]
fn an_implicit_outer_curve_bounded_face_keeps_the_sheet_a_shell() {
    use crate::loss::StepLossCode;
    use crate::test_support::curve_bounded::{unbounded_plane, unit_square_patch};
    let write = |ir: &CadIr| {
        let mut output = Vec::new();
        let report = write_step(
            ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .unwrap();
        (String::from_utf8(output).unwrap(), report.losses)
    };

    let (text, losses) = write(&implicit_outer_composite_sheet(unbounded_plane()));
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= ADVANCED_FACE(").count(), 0, "{text}");
    assert!(
        losses.iter().any(|loss| {
            loss.code == StepLossCode::FaceImplicitOuterBoundUnwritable.kind()
                && loss.severity == cadmpeg_ir::report::Severity::Error
                && loss.message.contains("the support is unbounded")
        }),
        "{losses:#?}"
    );

    let (text, losses) = write(&implicit_outer_composite_sheet(unit_square_patch()));
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
    assert_eq!(text.matches("= ADVANCED_FACE(").count(), 1, "{text}");
    assert_eq!(text.matches("= FACE_OUTER_BOUND(").count(), 1, "{text}");
    assert_eq!(text.matches("= FACE_BOUND(").count(), 1, "{text}");
    assert!(
        !losses
            .iter()
            .any(|loss| loss.code == StepLossCode::FaceImplicitOuterBoundUnwritable.kind()),
        "{losses:#?}"
    );
}

/// A ring of outer radius 5 with a hole of radius 2, and a disc of radius
/// 1 centred at `(1, 0, 0)` inside the hole, in one shell: the disc
/// touches the hole at `(2, 0, 0)`, where both boundary circles start, and
/// the two loops use one vertex there and share no edge or curve.
fn ring_and_disc_sharing_a_vertex() -> CadIr {
    let source = ring_and_disc_set().replace(
        "#17=CIRCLE('',#6,1.);",
        "#17=CIRCLE('',#22,1.);\n\
             #21=CARTESIAN_POINT('',(1.,0.,0.));\n\
             #22=AXIS2_PLACEMENT_3D('',#21,#4,#5);",
    );
    let source = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = source.ir().clone();
    let geometry = &mut ir.geometry;
    let ring = geometry
        .faces
        .iter()
        .find(|face| face.loops.len() == 2)
        .unwrap()
        .clone();
    let disc_index = geometry
        .faces
        .iter()
        .position(|face| face.loops.len() == 1)
        .unwrap();
    let disc = geometry.faces[disc_index].clone();
    let face_edges = |geometry: &cadmpeg_ir::document::Topology,
                      face: &cadmpeg_ir::topology::Face| {
        geometry
            .loops
            .iter()
            .filter(|face_loop| face.loops.contains(&face_loop.id))
            .flat_map(|face_loop| face_loop.coedges().to_vec())
            .filter_map(|id| geometry.coedges.iter().find(|coedge| coedge.id == id))
            .map(|coedge| coedge.edge.clone())
            .collect::<Vec<_>>()
    };
    let at_touch = |geometry: &cadmpeg_ir::document::Topology,
                    vertex: &cadmpeg_ir::ids::VertexId| {
        let vertex = geometry.vertices.iter().find(|v| &v.id == vertex).unwrap();
        let point = geometry
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap();
        point
            .position()
            .get()
            .distance(cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0))
            < 1e-9
    };
    let vertex_of = |geometry: &cadmpeg_ir::document::Topology,
                     edges: &[cadmpeg_ir::ids::EdgeId]| {
        edges
            .iter()
            .filter_map(|id| geometry.edges.iter().find(|edge| &edge.id == id))
            .filter_map(|edge| edge.start().cloned())
            .find(|vertex| at_touch(geometry, vertex))
            .expect("a boundary vertex at (2, 0, 0)")
    };
    let hole_vertex = vertex_of(geometry, &face_edges(geometry, &ring));
    let disc_edges = face_edges(geometry, &disc);
    let disc_vertex = vertex_of(geometry, &disc_edges);
    assert_ne!(hole_vertex, disc_vertex);
    for edge in &mut geometry.edges {
        if disc_edges.contains(&edge.id) {
            edge.ends = Some(cadmpeg_ir::topology::EdgeEnds::new(
                hole_vertex.clone(),
                hole_vertex.clone(),
            ));
        }
    }
    geometry.vertices.retain(|vertex| vertex.id != disc_vertex);
    geometry.faces[disc_index].shell = ring.shell.clone();
    geometry.shells.retain(|shell| shell.id != disc.shell);
    for region in &mut geometry.regions {
        region.shells.retain(|shell| shell != &disc.shell);
    }
    ir.finalize();
    ir
}

/// Two curve-bounded faces that meet at one vertex and share no edge: set
/// members state no vertex, so the writer keeps the sheet a shell, and the
/// shared vertex survives a round trip.
#[test]
fn curve_bounded_faces_that_share_a_vertex_keep_the_sheet_a_shell() {
    let ir = ring_and_disc_sharing_a_vertex();
    assert_eq!(sheet_loop_counts(&ir), [1, 2]);
    assert_eq!(ir.geometry.shells.len(), 1);
    assert_eq!(ir.geometry.edges.len(), 3);
    assert_eq!(ir.geometry.vertices.len(), 2);
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    let geometry = &decoded.ir().geometry;
    assert_eq!(sheet_loop_counts(decoded.ir()), [1, 2]);
    assert_eq!(geometry.shells.len(), 1, "{:#?}", geometry.shells);
    assert_eq!(geometry.edges.len(), 3, "{:#?}", geometry.edges);
    assert_eq!(geometry.vertices.len(), 2, "{:#?}", geometry.vertices);
    let shared = geometry
        .vertices
        .iter()
        .filter(|vertex| {
            geometry
                .edges
                .iter()
                .filter(|edge| edge.start() == Some(&vertex.id))
                .count()
                == 2
        })
        .count();
    assert_eq!(shared, 1, "{:#?}", geometry.edges);
}

/// A ring of outer radius 5 whose hole, of radius 2 centred at `(3, 0, 0)`,
/// touches the outer boundary at `(5, 0, 0)`, where both boundary circles
/// start: the two loops of the one face use one vertex there and share no
/// edge or curve.
fn ring_whose_loops_share_a_vertex() -> CadIr {
    let source = ring_and_disc_set()
        .replace(
            "#12=GEOMETRIC_SET('',(#11,#20));",
            "#12=GEOMETRIC_SET('',(#11));",
        )
        .replace("#17=CIRCLE('',#6,1.);\n", "")
        .replace("#18=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#17);\n", "")
        .replace("#19=OUTER_BOUNDARY_CURVE('',(#18),.F.);\n", "")
        .replace("#20=CURVE_BOUNDED_SURFACE('disc',#8,(#19),.F.);\n", "")
        .replace(
            "#14=CIRCLE('',#6,2.);",
            "#14=CIRCLE('',#22,2.);\n\
             #21=CARTESIAN_POINT('',(3.,0.,0.));\n\
             #22=AXIS2_PLACEMENT_3D('',#21,#4,#5);",
        );
    let source = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = source.ir().clone();
    let geometry = &mut ir.geometry;
    assert_eq!(geometry.faces.len(), 1);
    let loop_edges = |geometry: &cadmpeg_ir::document::Topology,
                      loop_id: &cadmpeg_ir::ids::LoopId| {
        geometry
            .loops
            .iter()
            .filter(|face_loop| &face_loop.id == loop_id)
            .flat_map(|face_loop| face_loop.coedges().to_vec())
            .filter_map(|id| geometry.coedges.iter().find(|coedge| coedge.id == id))
            .map(|coedge| coedge.edge.clone())
            .collect::<Vec<_>>()
    };
    let start_of = |geometry: &cadmpeg_ir::document::Topology,
                    edges: &[cadmpeg_ir::ids::EdgeId]| {
        let [edge] = edges else {
            panic!("one edge per loop: {edges:?}");
        };
        geometry
            .edges
            .iter()
            .find(|candidate| &candidate.id == edge)
            .and_then(|edge| edge.start().cloned())
            .unwrap()
    };
    let loops: Vec<_> = geometry.faces[0].loops.iter().cloned().collect();
    let first_edges = loop_edges(geometry, &loops[0]);
    let second_edges = loop_edges(geometry, &loops[1]);
    let kept = start_of(geometry, &first_edges);
    let dropped = start_of(geometry, &second_edges);
    assert_ne!(kept, dropped);
    for edge in &mut geometry.edges {
        if second_edges.contains(&edge.id) {
            edge.ends = Some(cadmpeg_ir::topology::EdgeEnds::new(
                kept.clone(),
                kept.clone(),
            ));
        }
    }
    geometry.vertices.retain(|vertex| vertex.id != dropped);
    ir.finalize();
    ir
}

/// The two loops of one curve-bounded face that meet at one vertex: the
/// reader states a vertex per boundary of a set member, so the writer keeps
/// the sheet a shell, and the shared vertex survives a round trip.
#[test]
fn a_curve_bounded_face_whose_loops_share_a_vertex_keeps_the_sheet_a_shell() {
    let ir = ring_whose_loops_share_a_vertex();
    assert_eq!(sheet_loop_counts(&ir), [2]);
    assert_eq!(ir.geometry.edges.len(), 2);
    assert_eq!(ir.geometry.vertices.len(), 1);
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    let geometry = &decoded.ir().geometry;
    assert_eq!(sheet_loop_counts(decoded.ir()), [2]);
    assert_eq!(geometry.edges.len(), 2, "{:#?}", geometry.edges);
    assert_eq!(geometry.vertices.len(), 1, "{:#?}", geometry.vertices);
}

/// The ring of [`ring_and_disc_set`] alone, with `edit` applied to its
/// source, decoded.
fn ring_alone(edit: impl FnOnce(String) -> String) -> CadIr {
    let source = ring_and_disc_set()
        .replace(
            "#12=GEOMETRIC_SET('',(#11,#20));",
            "#12=GEOMETRIC_SET('',(#11));",
        )
        .replace("#17=CIRCLE('',#6,1.);\n", "")
        .replace("#18=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#17);\n", "")
        .replace("#19=OUTER_BOUNDARY_CURVE('',(#18),.F.);\n", "")
        .replace("#20=CURVE_BOUNDED_SURFACE('disc',#8,(#19),.F.);\n", "");
    StepCodec::default()
        .decode(&mut Cursor::new(edit(source)), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

/// A ring whose two loops, each bounded by a segment on the radius-5
/// circle, use one edge on it: the hole loop's coedge is moved onto the
/// outer loop's edge. The edge has no vertices, so no shared vertex keeps
/// the face out of a geometric set.
fn ring_whose_loops_share_an_edge() -> CadIr {
    let mut ir = ring_alone(|source| {
        source.replace(
            "#15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#14);",
            "#15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#7);",
        )
    });
    let geometry = &mut ir.geometry;
    assert_eq!(geometry.faces.len(), 1);
    assert_eq!(geometry.edges.len(), 2);
    let loops: Vec<_> = geometry.faces[0].loops.iter().cloned().collect();
    let coedge_of = |geometry: &cadmpeg_ir::document::Topology,
                     loop_id: &cadmpeg_ir::ids::LoopId| {
        let face_loop = geometry
            .loops
            .iter()
            .find(|face_loop| &face_loop.id == loop_id)
            .unwrap();
        let [coedge] = face_loop.coedges() else {
            panic!("one coedge per loop");
        };
        coedge.clone()
    };
    let kept_coedge = coedge_of(geometry, &loops[0]);
    let moved_coedge = coedge_of(geometry, &loops[1]);
    let kept_edge = geometry
        .coedges
        .iter()
        .find(|coedge| coedge.id == kept_coedge)
        .unwrap()
        .edge
        .clone();
    let coedge = geometry
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == moved_coedge)
        .unwrap();
    let dropped_edge = std::mem::replace(&mut coedge.edge, kept_edge.clone());
    geometry.edges.retain(|edge| edge.id != dropped_edge);
    let edge = geometry
        .edges
        .iter_mut()
        .find(|edge| edge.id == kept_edge)
        .unwrap();
    edge.ends = None;
    geometry.vertices.clear();
    ir.finalize();
    ir
}

/// Two loops of one curve-bounded face that use one edge: the reader
/// states an edge per boundary segment of a set member, so the writer does
/// not write the face as one. The edge has no vertices, so the shell path
/// omits it and reports the loss.
#[test]
fn a_curve_bounded_face_whose_loops_share_an_edge_is_not_a_set_member() {
    let ir = ring_whose_loops_share_an_edge();
    assert_eq!(sheet_loop_counts(&ir), [2]);
    assert_eq!(ir.geometry.edges.len(), 1);
    assert!(ir.geometry.vertices.is_empty());
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(
        text.matches("= CURVE_BOUNDED_SURFACE(").count(),
        0,
        "{text}"
    );
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::StepLossCode::EdgeNoVertex.kind()),
        "{:#?}",
        report.losses
    );
}

/// A disc whose one boundary runs the radius-5 circle and then the radius-2
/// circle centred at `(3, 0, 0)`, both from `(5, 0, 0)`: the loop passes
/// through that point twice, and its two vertices there are merged into
/// one, so the loop is pinched.
fn pinched_disc() -> CadIr {
    let mut ir = ring_alone(|source| {
        source
            .replace("COMPOSITE_CURVE((#9),.F.)", "COMPOSITE_CURVE((#9,#15),.F.)")
            .replace(
                "#11=CURVE_BOUNDED_SURFACE('ring',#8,(#10,#16),.F.);",
                "#11=CURVE_BOUNDED_SURFACE('ring',#8,(#10),.F.);",
            )
            .replace("#16=BOUNDARY_CURVE('hole',(#15),.F.);\n", "")
            .replace(
                "#14=CIRCLE('',#6,2.);",
                "#14=CIRCLE('',#22,2.);\n\
                 #21=CARTESIAN_POINT('',(3.,0.,0.));\n\
                 #22=AXIS2_PLACEMENT_3D('',#21,#4,#5);",
            )
    });
    let geometry = &mut ir.geometry;
    assert_eq!(geometry.faces.len(), 1);
    assert_eq!(geometry.edges.len(), 2);
    assert_eq!(geometry.vertices.len(), 2);
    let kept = geometry.vertices[0].id.clone();
    let dropped = geometry.vertices[1].id.clone();
    for edge in &mut geometry.edges {
        let ends = edge.ends.as_mut().unwrap();
        for vertex in [&mut ends.start, &mut ends.end] {
            if *vertex == dropped {
                *vertex = kept.clone();
            }
        }
    }
    geometry.vertices.retain(|vertex| vertex.id != dropped);
    ir.finalize();
    ir
}

/// A curve-bounded face whose loop passes through one vertex twice: the
/// reader states a vertex per boundary segment of a set member, so the
/// writer keeps the sheet a shell, and the one vertex survives a round
/// trip.
#[test]
fn a_curve_bounded_face_with_a_pinched_loop_keeps_the_sheet_a_shell() {
    let ir = pinched_disc();
    assert_eq!(sheet_loop_counts(&ir), [1]);
    assert_eq!(ir.geometry.edges.len(), 2);
    assert_eq!(ir.geometry.vertices.len(), 1);
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(text.matches("= OPEN_SHELL(").count(), 1, "{text}");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .unwrap();
    let geometry = &decoded.ir().geometry;
    assert_eq!(sheet_loop_counts(decoded.ir()), [1]);
    assert_eq!(geometry.edges.len(), 2, "{:#?}", geometry.edges);
    assert_eq!(geometry.vertices.len(), 1, "{:#?}", geometry.vertices);
}

/// A disc whose one boundary runs the radius-5 circle twice, and whose loop
/// uses one edge, with no vertices, for both segments.
fn disc_with_a_seam_edge() -> CadIr {
    let mut ir = ring_alone(|source| {
        source
            .replace("COMPOSITE_CURVE((#9),.F.)", "COMPOSITE_CURVE((#9,#9),.F.)")
            .replace(
                "#11=CURVE_BOUNDED_SURFACE('ring',#8,(#10,#16),.F.);",
                "#11=CURVE_BOUNDED_SURFACE('ring',#8,(#10),.F.);",
            )
            .replace("#14=CIRCLE('',#6,2.);\n", "")
            .replace("#15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#14);\n", "")
            .replace("#16=BOUNDARY_CURVE('hole',(#15),.F.);\n", "")
    });
    let geometry = &mut ir.geometry;
    assert_eq!(geometry.faces.len(), 1);
    assert_eq!(geometry.edges.len(), 2);
    let kept = geometry.edges[0].id.clone();
    let dropped = geometry.edges[1].id.clone();
    for coedge in &mut geometry.coedges {
        if coedge.edge == dropped {
            coedge.edge = kept.clone();
        }
    }
    geometry.edges.retain(|edge| edge.id != dropped);
    geometry.edges[0].ends = None;
    geometry.vertices.clear();
    ir.finalize();
    ir
}

/// A curve-bounded face whose loop uses one edge twice: the reader states
/// an edge per boundary segment of a set member, so the writer does not
/// write the face as one. The edge has no vertices, so the shell path omits
/// it and reports the loss.
#[test]
fn a_curve_bounded_face_whose_loop_uses_an_edge_twice_is_not_a_set_member() {
    let ir = disc_with_a_seam_edge();
    assert_eq!(sheet_loop_counts(&ir), [1]);
    assert_eq!(ir.geometry.edges.len(), 1);
    assert_eq!(ir.geometry.coedges.len(), 2);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.matches("= GEOMETRIC_SET(").count(), 0, "{text}");
    assert_eq!(
        text.matches("= CURVE_BOUNDED_SURFACE(").count(),
        0,
        "{text}"
    );
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::StepLossCode::EdgeNoVertex.kind()),
        "{:#?}",
        report.losses
    );
}
