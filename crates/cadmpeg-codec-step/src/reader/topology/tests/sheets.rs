// SPDX-License-Identifier: Apache-2.0
//! STEP B-rep, shell, face, and wire tests.

#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
pub(crate) fn decode_and_write_singular_vertex_loops() {
    let bytes = include_bytes!("../../../../tests/fixtures/ap242_vertex_loop.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode vertex loops");
    assert_eq!(result.ir().geometry.loops.len(), 2);
    assert!(result
        .ir()
        .geometry
        .loops
        .iter()
        .all(|loop_| loop_.singular_vertex().is_some()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let mut encoded = Vec::new();
    write_step(
        result.ir(),
        &mut encoded,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write vertex loops");
    assert_eq!(
        String::from_utf8(encoded)
            .unwrap()
            .matches("VERTEX_LOOP")
            .count(),
        2
    );
}

#[test]
pub(crate) fn decode_builds_a_valid_connected_sheet_brep() {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let bytes = include_bytes!("../../../../tests/fixtures/ap214_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP214 sheet");

    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(result.ir().geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().geometry.regions.len(), 1);
    assert_eq!(result.ir().geometry.shells.len(), 1);
    assert_eq!(result.ir().geometry.faces.len(), 1);
    assert_eq!(result.ir().geometry.loops.len(), 1);
    assert_eq!(result.ir().geometry.coedges.len(), 3);
    assert_eq!(result.ir().geometry.edges.len(), 3);
    assert!(result
        .ir()
        .geometry
        .edges
        .iter()
        .all(|edge| { edge.param_range().is_some_and(|range| range[0] < range[1]) }));
    assert_eq!(result.ir().geometry.vertices.len(), 3);
    assert_eq!(result.ir().geometry.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .geometry
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(
        matches!(result.ir().geometry.pcurves[0].geometry, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve)
                if {
                    let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
                    *origin == cadmpeg_ir::math::Point2::new(0.0, 0.0)
                        && *direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
                })
    );
    assert!(result
        .ir()
        .geometry
        .coedges
        .iter()
        .all(|coedge| coedge.sense == Sense::Reversed));
    assert_eq!(result.ir().geometry.faces[0].sense, Sense::Reversed);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            &binding.target.entity,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        result.ir().geometry.faces[0].color,
        Some(cadmpeg_ir::topology::Color::new(0.9, 0.1, 0.1, 1.0).expect("valid color"))
    );
    assert_eq!(result.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        result.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(matches!(
        result.ir().model.presentation_layers[0]
            .items
            .iter()
            .map(|item| &item.entity)
            .collect::<Vec<_>>()
            .as_slice(),
        [cadmpeg_ir::PresentationItem::Face { .. }]
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write sheet pcurve");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("coedge pcurve(s) use unsupported")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written pcurve");
    assert_eq!(roundtrip.ir().geometry.pcurves.len(), 1);
    assert_eq!(roundtrip.ir().geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(roundtrip.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        roundtrip.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(roundtrip
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            &binding.target.entity,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        roundtrip
            .ir()
            .geometry
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
pub(crate) fn decode_builds_a_valid_ap203_sheet_brep() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../../../../tests/fixtures/ap203_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP203 sheet");

    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["schema"],
        "CONFIG_CONTROL_DESIGN"
    );
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(result.ir().geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().geometry.faces.len(), 1);
    assert_eq!(result.ir().geometry.edges.len(), 3);
    assert_eq!(result.ir().geometry.vertices.len(), 3);
    let composite = result
        .ir()
        .geometry
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#34")
        .expect("outer composite curve");
    assert!(matches!(
        &composite.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments,
            self_intersect: Some(false)
        }) if segments.len() == 1
            && segments[0].curve.as_str() == "step:data:curve#36"
            && segments[0].same_sense
            && segments[0].transition
                == cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradient
    ));
    assert!(result
        .ir()
        .geometry
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition(),
            cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                implicit_outer: false,
                ..
            } if support.as_str() == "step:data:surface#28"
                && boundaries.as_slice() == [cadmpeg_ir::ids::CurveId::mint("step:data:curve#34").expect("identity grammar")]
        )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut encoded = Vec::new();
    write_step(
        result.ir(),
        &mut encoded,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write composite curve graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode written composite curve graph");
    assert!(roundtrip.ir().geometry.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    )));
}

#[test]
fn decode_builds_a_face_based_surface_model() {
    use cadmpeg_ir::topology::BodyKind;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=FACE_BASED_SURFACE_MODEL('',(#30));",
            )
            .replace(
                "#30=OPEN_SHELL('',(#29));",
                "#30=CONNECTED_FACE_SET('',(#29));",
            );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode face-based surface model");

    assert_eq!(
        result.ir().geometry.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().geometry.faces.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("does not resolve to a complete")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn a_face_based_surface_model_accepts_an_open_shell_face_set() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=FACE_BASED_SURFACE_MODEL('',(#30));",
            );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode a face-based surface model over an open shell");

    assert_eq!(
        result.ir().geometry.faces.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_faceted_brep_polygon_loops() {
    use cadmpeg_ir::topology::BodyKind;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#25=EDGE_LOOP('',(#22,#23,#24));",
                "#25=POLY_LOOP('',(#3,#4,#5,#3));",
            )
            .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=FACETED_BREP('',#30);",
            );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode faceted brep");

    assert_eq!(
        result.ir().geometry.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().geometry.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().geometry.faces.len(), 1);
    assert_eq!(result.ir().geometry.coedges.len(), 3);
    let model = &result.ir().geometry;
    for edge in &model.edges {
        let curve = edge
            .curve()
            .expect("a poly loop segment has a line carrier");
        let curve = model
            .curves
            .iter()
            .find(|candidate| &candidate.id == curve)
            .expect("the carrier is in the model");
        assert!(matches!(
            curve.geometry.solved(),
            Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Line(_))
        ));
    }
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn sheet_root_salvages_independent_shells() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33,#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=ORIENTED_OPEN_SHELL('',*,#99,.T.);\n#99=UNSUPPORTED_SHELL('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with one invalid shell");

    assert_eq!(
        decoded.ir().geometry.bodies.len(),
        1,
        "{:#?}",
        decoded.report().losses
    );
    // The shell is given up alone, with the carrier that did not resolve.
    let shells = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.message.contains("does not resolve: shell carrier #34"))
        .collect::<Vec<_>>();
    let [shell] = shells.as_slice() else {
        panic!("one give-up names the shell: {shells:#?}");
    };
    assert_eq!(
        shell.code,
        cadmpeg_ir::unknown::part::record_undecoded_kind()
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_sheet_from_a_geometric_surface_set() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode geometric surface set");

    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(result.ir().geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().geometry.faces.len(), 1);
    assert!(matches!(
        result.ir().geometry.faces[0].loops,
        cadmpeg_ir::topology::FaceLoops::Classified { .. }
    ));
    assert_eq!(result.ir().geometry.coedges.len(), 1);
    assert_eq!(
        result.ir().geometry.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, std::f64::consts::TAU])
    );
    assert_eq!(
        result.ir().geometry.faces[0].surface.as_str(),
        "step:data:surface#11"
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn curve_bounded_surface_boundaries_take_the_winding_of_their_role() {
    use cadmpeg_ir::topology::Sense;

    let mut source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#9=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#7);",
        "#9=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.F.,#7);",
    )
    .replace(
        "#10=(BOUNDARY_CURVE() ",
        "#10=(BOUNDARY_CURVE() OUTER_BOUNDARY_CURVE() ",
    )
    .replace(
        "#11=CURVE_BOUNDED_SURFACE('disc',#8,(#10),.F.);",
        "#11=CURVE_BOUNDED_SURFACE('ring',#8,(#10,#16),.F.);",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#14=CIRCLE('',#6,2.);\n#15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#14);\n#16=BOUNDARY_CURVE('hole',(#15),.F.);\n",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode a ring");
    let model = &result.ir().geometry;
    let face = &model.faces[0];
    let cadmpeg_ir::topology::FaceLoops::Classified { outer, .. } = &face.loops else {
        panic!("the outer boundary curve is the outer loop");
    };
    assert_eq!(face.loops.stated().count(), 2);
    for coedge in &model.coedges {
        let expected = if &coedge.owner_loop == outer {
            Sense::Forward
        } else {
            Sense::Reversed
        };
        assert_eq!(coedge.sense, expected, "{}", coedge.id);
    }
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

/// The disc fixture as a ring: the radius 5 boundary `#10` and the radius 2
/// boundary `#16`, listed hole first, on `support`, with no outer boundary
/// curve and no implicit outer boundary.
fn unflagged_ring(support: &str, extra: &str) -> cadmpeg_ir::codec::DecodeResult {
    let mut source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#11=CURVE_BOUNDED_SURFACE('disc',#8,(#10),.F.);",
        &format!("#11=CURVE_BOUNDED_SURFACE('ring',{support},(#16,#10),.F.);"),
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        &format!(
            "#14=CIRCLE('',#6,2.);\n#15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#14);\n#16=BOUNDARY_CURVE('hole',(#15),.F.);\n{extra}"
        ),
    );
    StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode a ring")
}

#[test]
fn a_curve_bounded_surface_whose_boundary_does_not_close_is_a_partial_face() {
    // The ring's hole `#16` is an open arc, so the face's stored boundary
    // does not resolve. The support is a B-spline surface, whose own domain
    // is finite: the face is partial still, since the source bounds it.
    let mut source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#11=CURVE_BOUNDED_SURFACE('disc',#8,(#10),.F.);",
        "#11=CURVE_BOUNDED_SURFACE('ring',#21,(#10,#16),.F.);",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#14=CIRCLE('',#6,2.);
#22=TRIMMED_CURVE('',#14,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);
         #15=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#22);
#16=BOUNDARY_CURVE('open',(#15),.F.);
         #17=CARTESIAN_POINT('',(-10.,-10.,0.));
#18=CARTESIAN_POINT('',(10.,-10.,0.));
         #19=CARTESIAN_POINT('',(-10.,10.,0.));
#20=CARTESIAN_POINT('',(10.,10.,0.));
         #21=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#17,#19),(#18,#20)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);
",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode a ring with an open boundary");
    let faces = &result.ir().geometry.faces;
    assert_eq!(faces.len(), 1);
    assert_eq!(
        faces[0].loops,
        cadmpeg_ir::topology::FaceLoops::PartialUnspecified { loops: Vec::new() }
    );
    assert!(result.ir().geometry.loops.is_empty());
    let face_id = faces[0].id.as_str();
    let unresolved = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::StepLossCode::TopologyFaceBoundaryUnstated.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        unresolved,
        [format!(
            "CURVE_BOUNDED_SURFACE #11 boundary #16 does not resolve to a closed loop: face {face_id} is stated partial"
        )]
    );
}

#[test]
fn an_unflagged_boundary_enclosing_the_largest_area_is_the_outer_loop() {
    let result = unflagged_ring("#8", "");
    let model = &result.ir().geometry;
    let cadmpeg_ir::topology::FaceLoops::Classified { outer, .. } = &model.faces[0].loops else {
        panic!("the largest boundary is the outer loop");
    };
    let outer_edge = model
        .coedges
        .iter()
        .find(|coedge| &coedge.owner_loop == outer)
        .map(|coedge| &coedge.edge)
        .expect("outer coedge");
    let curve = model
        .edges
        .iter()
        .find(|edge| &edge.id == outer_edge)
        .and_then(|edge| edge.curve())
        .expect("outer edge curve");
    assert_eq!(curve.as_str(), "step:data:curve#7");
}

#[test]
fn unmeasured_unflagged_boundaries_stay_unclassified() {
    let result = unflagged_ring(
        "#21",
        "#17=CARTESIAN_POINT('',(-10.,-10.,0.));\n#18=CARTESIAN_POINT('',(10.,-10.,0.));\n\
         #19=CARTESIAN_POINT('',(-10.,10.,0.));\n#20=CARTESIAN_POINT('',(10.,10.,0.));\n\
         #21=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#17,#19),(#18,#20)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);\n",
    );
    let face = &result.ir().geometry.faces[0];
    assert!(matches!(
        face.loops,
        cadmpeg_ir::topology::FaceLoops::Unspecified { .. }
    ));
    assert_eq!(face.loops.stated().count(), 2);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "decode.warning"
            && loss.message.contains("left its 2 boundaries unclassified")
    }));
}

#[test]
fn a_sole_unflagged_boundary_is_the_outer_boundary() {
    assert_eq!(
        crate::reader::topology::largest_boundary([None].into_iter()),
        Some(0)
    );
    assert_eq!(
        crate::reader::topology::largest_boundary([Some(1.0), Some(-4.0)].into_iter()),
        Some(1)
    );
    assert_eq!(
        crate::reader::topology::largest_boundary([Some(4.0), None].into_iter()),
        None
    );
}

#[test]
fn complex_geometric_set_representation_uses_its_named_items() {
    use cadmpeg_ir::topology::BodyKind;

    let mut source = String::from_utf8(include_bytes!(
        "../../../../tests/fixtures/ap242_geometric_set.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#12=GEOMETRIC_SET('',(#11));",
        "#12=GEOMETRIC_SET('',(#11,#14,#15));",
    )
    .replace(
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);",
        "#13=(GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION() REPRESENTATION('',(#12),#2) SHAPE_REPRESENTATION());",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#14=(CIRCLE('',#6,5.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('free circle'));\n#15=UNSUPPORTED_ITEM('');\n",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometric surface set");

    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(result.ir().geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().geometry.faces.len(), 1);
    let free_circle = result
        .ir()
        .geometry
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("free complex representation member");
    assert_eq!(
        crate::test_support::carrier_name(result.ir(), free_circle.id.as_str()),
        Some("free circle")
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #13 omitted unsupported or unresolved member(s): #15",
        )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn reader_recovers_a_valid_solid_from_writer_output() {
    use cadmpeg_ir::topology::BodyKind;

    let source = unit_cube().expect("unit cube fixture is admitted");
    let mut bytes = Vec::new();
    write_step(
        &source,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode generated cube STEP");

    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(result.ir().geometry.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().geometry.faces.len(), 6);
    assert_eq!(result.ir().geometry.edges.len(), 12);
    assert_eq!(result.ir().geometry.vertices.len(), 8);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
