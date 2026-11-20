// SPDX-License-Identifier: Apache-2.0
//! The linear uncertainty of each representation context reaches the faces,
//! edges, and vertices of the bodies its representations hold.

use std::fmt::Write as _;

use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::Tolerances;
use cadmpeg_ir::CadIr;

use crate::loss::StepLossCode;
use crate::test_support::box_brep::{box_shell, BoxFaceUse};
use crate::test_support::exchange::decode_inline;

const EPS_TOLERANCE_MM: f64 = 1.0e-12;
const MM_PER_INCH: f64 = 25.4;

/// The length unit of a context.
#[derive(Clone, Copy)]
enum Length {
    Millimetre,
    Inch,
}

/// The unit, uncertainty, and context records of one model context,
/// numbered from `first`, with `uncertainty` in `length` units. Gives the
/// records and the context instance number.
fn context(first: u64, length: Length, uncertainty: f64) -> (String, u64) {
    let millimetre = first;
    let inch_factor = first + 1;
    let unit = match length {
        Length::Millimetre => millimetre,
        Length::Inch => first + 2,
    };
    let angle = first + 3;
    let measure = first + 4;
    let context = first + 5;
    let records = format!(
        "#{millimetre}=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #{inch_factor}=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE({MM_PER_INCH:?}),#{millimetre});\n\
         #{inch}=(CONVERSION_BASED_UNIT('inch',#{inch_factor}) LENGTH_UNIT() NAMED_UNIT(*));\n\
         #{angle}=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #{measure}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({uncertainty:?}),#{unit},'distance_accuracy_value','');\n\
         #{context}=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{measure})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unit},#{angle})) REPRESENTATION_CONTEXT('model','3D'));\n",
        inch = first + 2,
    );
    (records, context)
}

/// A solid box `min..max` in its own context, numbered from `first`. Gives
/// the records and the first free instance number.
fn box_part(
    first: u64,
    min: [f64; 3],
    max: [f64; 3],
    length: Length,
    uncertainty: f64,
) -> (String, u64) {
    let shell = box_shell(first, min, max, BoxFaceUse::Outward);
    let solid = shell.next;
    let (context_records, context) = context(solid + 1, length, uncertainty);
    let representation = context + 1;
    let records = format!(
        "{}#{solid}=MANIFOLD_SOLID_BREP('',#{});\n{context_records}\
         #{representation}=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{solid}),#{context});\n",
        shell.records, shell.shell,
    );
    (records, representation + 1)
}

/// A planar disc of radius 5 mm at height `z`, bounded by one closed circle
/// edge whose vertex lies `offset` mm radially outside the circle, in a
/// millimetre context of `uncertainty`. Numbered from `first`; gives the
/// records and the first free instance number.
fn off_carrier_disc(first: u64, z: f64, offset: f64, uncertainty: f64) -> (String, u64) {
    let n = |index: u64| first + index;
    let vertex_x = 5.0 + offset;
    let mut records = format!(
        "#{}=CARTESIAN_POINT('',(0.,0.,{z:?}));\n\
         #{}=DIRECTION('',(0.,0.,1.));\n\
         #{}=DIRECTION('',(1.,0.,0.));\n\
         #{}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n\
         #{}=CIRCLE('',#{},5.);\n\
         #{}=CARTESIAN_POINT('',({vertex_x:?},0.,{z:?}));\n\
         #{}=VERTEX_POINT('',#{});\n\
         #{}=EDGE_CURVE('',#{},#{},#{},.T.);\n\
         #{}=ORIENTED_EDGE('',*,*,#{},.T.);\n\
         #{}=EDGE_LOOP('',(#{}));\n\
         #{}=FACE_OUTER_BOUND('',#{},.T.);\n\
         #{}=PLANE('',#{});\n\
         #{}=ADVANCED_FACE('',(#{}),#{},.T.);\n\
         #{}=OPEN_SHELL('',(#{}));\n\
         #{}=SHELL_BASED_SURFACE_MODEL('',(#{}));\n",
        n(0),
        n(1),
        n(2),
        n(3),
        n(0),
        n(1),
        n(2),
        n(4),
        n(3),
        n(5),
        n(6),
        n(5),
        n(7),
        n(6),
        n(6),
        n(4),
        n(8),
        n(7),
        n(9),
        n(8),
        n(10),
        n(9),
        n(11),
        n(3),
        n(12),
        n(10),
        n(11),
        n(13),
        n(12),
        n(14),
        n(13),
    );
    let (context_records, context) = context(n(15), Length::Millimetre, uncertainty);
    let representation = context + 1;
    records.push_str(&context_records);
    writeln!(
        records,
        "#{representation}=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#{}),#{context});",
        n(14)
    )
    .expect("write to a String");
    (records, representation + 1)
}

fn assert_tolerance(name: &str, tolerance: Option<f64>, expected: f64) {
    let tolerance = tolerance.unwrap_or_else(|| panic!("{name} carries a tolerance"));
    assert!(
        (tolerance - expected).abs() < EPS_TOLERANCE_MM,
        "{name}: {tolerance:e} against {expected:e}"
    );
}

fn assert_valid(ir: &CadIr, losses: Vec<cadmpeg_ir::report::loss::LossNote>) {
    let validation = cadmpeg_ir::validate_neutral(ir, losses).expect("validate the neutral model");
    assert!(validation.is_ok(), "{validation:#?}");
}

fn assert_no_ambiguity(losses: &[cadmpeg_ir::report::loss::LossNote]) {
    assert!(
        losses
            .iter()
            .all(|loss| loss.code != StepLossCode::UncertaintyLengthAmbiguous.kind()),
        "{losses:#?}"
    );
}

#[test]
fn each_part_carries_its_own_context_uncertainty() {
    let metric_uncertainty = 1.0e-3;
    let inch_uncertainty = 5.0e-3;
    let (metric, next) = box_part(
        100,
        [0.0; 3],
        [10.0; 3],
        Length::Millimetre,
        metric_uncertainty,
    );
    let (inch, _) = box_part(
        next,
        [20.0, 0.0, 0.0],
        [21.0, 1.0, 1.0],
        Length::Inch,
        inch_uncertainty,
    );
    let result = decode_inline(&format!("{metric}{inch}"));
    let ir = result.ir();

    assert_eq!(
        ir.tolerances.linear,
        Tolerances::default().linear,
        "the disagreeing contexts keep the document default"
    );
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert_eq!(ir.geometry.faces.len(), 12);
    // The inch box lies beyond x = 20 in = 508 mm; the metric box below 10 mm.
    let expected = |x: f64| {
        if x > 100.0 {
            inch_uncertainty * MM_PER_INCH
        } else {
            metric_uncertainty
        }
    };
    let position = |vertex: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir
            .geometry
            .vertices
            .iter()
            .find(|candidate| &candidate.id == vertex)
            .expect("edge end vertex");
        ir.geometry
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .expect("vertex point")
            .position()
            .get()
    };
    for vertex in &ir.geometry.vertices {
        assert_tolerance(
            vertex.id.as_str(),
            vertex.tolerance.map(PositiveReal::get),
            expected(position(&vertex.id).x),
        );
    }
    for edge in &ir.geometry.edges {
        let start = &edge.ends.as_ref().expect("bounded edge").start;
        assert_tolerance(
            edge.id.as_str(),
            edge.tolerance.map(PositiveReal::get),
            expected(position(start).x),
        );
    }
    for face in &ir.geometry.faces {
        let edge = ir
            .geometry
            .loops
            .iter()
            .filter(|loop_| loop_.face == face.id)
            .flat_map(cadmpeg_ir::topology::Loop::coedges)
            .find_map(|coedge| {
                ir.geometry
                    .coedges
                    .iter()
                    .find(|candidate| &candidate.id == coedge)
            })
            .expect("face coedge")
            .edge
            .clone();
        let start = ir
            .geometry
            .edges
            .iter()
            .find(|candidate| candidate.id == edge)
            .and_then(|edge| edge.ends.as_ref())
            .expect("bounded edge")
            .start
            .clone();
        assert_tolerance(
            face.id.as_str(),
            face.tolerance.map(PositiveReal::get),
            expected(position(&start).x),
        );
    }
    assert_no_ambiguity(&result.report().losses);
    assert_valid(ir, result.report().losses.clone());
}

#[test]
fn a_single_context_sets_the_document_tolerance_only() {
    let uncertainty = 1.0e-3;
    let (records, _) = box_part(100, [0.0; 3], [10.0; 3], Length::Millimetre, uncertainty);
    let result = decode_inline(&records);
    let ir = result.ir();

    assert!((ir.tolerances.linear.get() - uncertainty).abs() < EPS_TOLERANCE_MM);
    assert!(ir
        .geometry
        .faces
        .iter()
        .all(|face| face.tolerance.is_none()));
    assert!(ir
        .geometry
        .edges
        .iter()
        .all(|edge| edge.tolerance.is_none()));
    assert!(ir
        .geometry
        .vertices
        .iter()
        .all(|vertex| vertex.tolerance.is_none()));
    assert_no_ambiguity(&result.report().losses);
    assert_valid(ir, result.report().losses.clone());
}

#[test]
fn an_off_carrier_edge_keeps_the_larger_of_its_bound_and_its_context_uncertainty() {
    let offset = 1.0e-2;
    let fine = 1.0e-3;
    let coarse = 5.0e-2;
    let (fine_disc, next) = off_carrier_disc(100, 0.0, offset, fine);
    let (coarse_disc, _) = off_carrier_disc(next, 20.0, offset, coarse);
    let result = decode_inline(&format!("{fine_disc}{coarse_disc}"));
    let ir = result.ir();

    let document = Tolerances::default().linear.get();
    assert!((ir.tolerances.linear.get() - document).abs() < EPS_TOLERANCE_MM);
    assert_eq!(ir.geometry.edges.len(), 2);
    let height = |edge: &cadmpeg_ir::topology::Edge| {
        let start = &edge.ends.as_ref().expect("bounded edge").start;
        let vertex = ir
            .geometry
            .vertices
            .iter()
            .find(|candidate| &candidate.id == start)
            .expect("edge vertex");
        ir.geometry
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .expect("vertex point")
            .position()
            .get()
            .z
    };
    for edge in &ir.geometry.edges {
        // The off-carrier admission bound is the document tolerance plus the
        // radial offset; it exceeds the fine context and stays below the
        // coarse one.
        let expected = if height(edge) > 10.0 {
            coarse
        } else {
            document + offset
        };
        assert_tolerance(
            edge.id.as_str(),
            edge.tolerance.map(PositiveReal::get),
            expected,
        );
    }
    assert_no_ambiguity(&result.report().losses);
}

#[test]
fn a_context_of_two_uncertainties_reports_the_value_its_body_does_not_carry() {
    let fine = 0.1;
    let coarse = 0.2;
    let shell = box_shell(100, [0.0; 3], [10.0; 3], BoxFaceUse::Outward);
    let solid = shell.next;
    let n = |index: u64| solid + index;
    let records = format!(
        "{}#{solid}=MANIFOLD_SOLID_BREP('',#{});\n\
         #{}=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({fine:?}),#{},'manufacturing_accuracy','distance_accuracy_value');\n\
         #{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({coarse:?}),#{},'other_accuracy','');\n\
         #{}=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #{}=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{},#{})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{})) REPRESENTATION_CONTEXT('model','3D'));\n\
         #{}=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{solid}),#{});\n",
        shell.records,
        shell.shell,
        n(1),
        n(2),
        n(1),
        n(3),
        n(1),
        n(4),
        n(5),
        n(2),
        n(3),
        n(1),
        n(4),
        n(6),
        n(5),
    );
    let result = decode_inline(&records);
    let ir = result.ir();

    let document = Tolerances::default().linear.get();
    assert!((ir.tolerances.linear.get() - document).abs() < EPS_TOLERANCE_MM);
    assert_eq!(ir.geometry.faces.len(), 6);
    for face in &ir.geometry.faces {
        assert_tolerance(
            face.id.as_str(),
            face.tolerance.map(PositiveReal::get),
            coarse,
        );
    }
    for edge in &ir.geometry.edges {
        assert_tolerance(
            edge.id.as_str(),
            edge.tolerance.map(PositiveReal::get),
            coarse,
        );
    }
    for vertex in &ir.geometry.vertices {
        assert_tolerance(
            vertex.id.as_str(),
            vertex.tolerance.map(PositiveReal::get),
            coarse,
        );
    }
    let losses = &result.report().losses;
    let ambiguous = losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::UncertaintyLengthAmbiguous.kind())
        .collect::<Vec<_>>();
    assert_eq!(ambiguous.len(), 1, "{losses:#?}");
    let message = &ambiguous[0].message;
    assert!(
        message.contains(&format!("values in millimetres ({fine:?}, {coarse:?})")),
        "{message}"
    );
    assert!(
        message.contains(&format!("default {document:?}")),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "1 value(s) ({fine:?}) reach no decoded face, edge, or vertex"
        )),
        "{message}"
    );
    assert_valid(ir, losses.clone());
}

/// One solid box listed by one `ADVANCED_BREP_SHAPE_REPRESENTATION` per
/// entry of `contexts`, each in a millimetre context whose uncertainty
/// measures are that entry's values. None of the measures is named
/// `distance_accuracy_value`, so each contributes.
fn box_in_contexts(contexts: &[&[f64]]) -> String {
    let shell = box_shell(100, [0.0; 3], [10.0; 3], BoxFaceUse::Outward);
    let solid = shell.next;
    let mut records = format!(
        "{}#{solid}=MANIFOLD_SOLID_BREP('',#{});\n",
        shell.records, shell.shell
    );
    let mut next = solid + 1;
    for values in contexts {
        let unit = next;
        let angle = next + 1;
        next += 2;
        writeln!(
            records,
            "#{unit}=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
             #{angle}=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));"
        )
        .expect("write to a String");
        let mut measures = Vec::new();
        for value in *values {
            writeln!(
                records,
                "#{next}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({value:?}),#{unit},'accuracy','');"
            )
            .expect("write to a String");
            measures.push(format!("#{next}"));
            next += 1;
        }
        let context = next;
        let representation = next + 1;
        next += 2;
        writeln!(
            records,
            "#{context}=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unit},#{angle})) REPRESENTATION_CONTEXT('model','3D'));\n\
             #{representation}=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{solid}),#{context});",
            measures.join(",")
        )
        .expect("write to a String");
    }
    records
}

/// Decode `records`, check every face, edge, and vertex carries `kept`, and
/// give the message of the single ambiguity note.
fn kept_and_ambiguity(records: &str, kept: f64) -> String {
    let result = decode_inline(records);
    let ir = result.ir();
    let document = Tolerances::default().linear.get();
    assert!((ir.tolerances.linear.get() - document).abs() < EPS_TOLERANCE_MM);
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 6);
    for face in &ir.geometry.faces {
        assert_tolerance(
            face.id.as_str(),
            face.tolerance.map(PositiveReal::get),
            kept,
        );
    }
    for edge in &ir.geometry.edges {
        assert_tolerance(
            edge.id.as_str(),
            edge.tolerance.map(PositiveReal::get),
            kept,
        );
    }
    for vertex in &ir.geometry.vertices {
        assert_tolerance(
            vertex.id.as_str(),
            vertex.tolerance.map(PositiveReal::get),
            kept,
        );
    }
    let losses = &result.report().losses;
    let ambiguous = losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::UncertaintyLengthAmbiguous.kind())
        .collect::<Vec<_>>();
    assert_eq!(ambiguous.len(), 1, "{losses:#?}");
    assert_valid(ir, losses.clone());
    ambiguous[0].message.clone()
}

#[test]
fn a_body_in_two_contexts_reports_the_smaller_value_it_gives_up() {
    let (fine, coarse) = (0.2, 0.3);
    let message = kept_and_ambiguity(&box_in_contexts(&[&[fine], &[coarse]]), coarse);
    assert!(
        message.contains(&format!("values in millimetres ({fine:?}, {coarse:?})")),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "1 value(s) ({fine:?}) reach no decoded face, edge, or vertex"
        )),
        "{message}"
    );
}

#[test]
fn a_body_in_two_contexts_reports_each_value_below_the_one_it_keeps() {
    let (finest, fine, coarse) = (0.1, 0.2, 0.3);
    let message = kept_and_ambiguity(&box_in_contexts(&[&[finest, fine], &[coarse]]), coarse);
    assert!(
        message.contains(&format!(
            "values in millimetres ({finest:?}, {fine:?}, {coarse:?})"
        )),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "2 value(s) ({finest:?}, {fine:?}) reach no decoded face, edge, or vertex"
        )),
        "{message}"
    );
}

/// A 2 x 3 x 4 mm `BLOCK` at `(x, 0, 0)` listed by a
/// `CSG_SHAPE_REPRESENTATION` in the context `context`, numbered from
/// `first`. With `wrapped`, the representation lists a `CSG_SOLID` of the
/// block rather than the block itself. Gives the records and the first free
/// instance number.
fn csg_block(first: u64, x: f64, context: u64, wrapped: bool) -> (String, u64) {
    let n = |index: u64| first + index;
    let item = if wrapped { n(5) } else { n(4) };
    let records = format!(
        "#{}=CARTESIAN_POINT('',({x:?},0.,0.));\n\
         #{}=DIRECTION('',(0.,0.,1.));\n\
         #{}=DIRECTION('',(1.,0.,0.));\n\
         #{}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n\
         #{}=BLOCK('block',#{},2.,3.,4.);\n\
         #{}=CSG_SOLID('',#{});\n\
         #{}=CSG_SHAPE_REPRESENTATION('',(#{item}),#{context});\n",
        n(0),
        n(1),
        n(2),
        n(3),
        n(0),
        n(1),
        n(2),
        n(4),
        n(3),
        n(5),
        n(4),
        n(6),
    );
    (records, n(7))
}

/// The x coordinate of the start vertex of `edge`.
fn edge_x(ir: &CadIr, edge: &cadmpeg_ir::ids::EdgeId) -> f64 {
    let start = &ir
        .geometry
        .edges
        .iter()
        .find(|candidate| &candidate.id == edge)
        .and_then(|edge| edge.ends.as_ref())
        .expect("bounded edge")
        .start;
    vertex_x(ir, start)
}

/// The x coordinate of `vertex`.
fn vertex_x(ir: &CadIr, vertex: &cadmpeg_ir::ids::VertexId) -> f64 {
    let vertex = ir
        .geometry
        .vertices
        .iter()
        .find(|candidate| &candidate.id == vertex)
        .expect("vertex");
    ir.geometry
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .expect("vertex point")
        .position()
        .get()
        .x
}

/// The x coordinate of a vertex on the boundary of `face`.
fn face_x(ir: &CadIr, face: &cadmpeg_ir::topology::Face) -> f64 {
    let edge = ir
        .geometry
        .loops
        .iter()
        .filter(|loop_| loop_.face == face.id)
        .flat_map(cadmpeg_ir::topology::Loop::coedges)
        .find_map(|coedge| {
            ir.geometry
                .coedges
                .iter()
                .find(|candidate| &candidate.id == coedge)
        })
        .expect("face coedge")
        .edge
        .clone();
    edge_x(ir, &edge)
}

/// Check every face, edge, and vertex of `ir` carries `expected` of the x
/// coordinate of one of its vertices.
fn assert_tolerances_by_x(ir: &CadIr, expected: impl Fn(f64) -> f64) {
    for face in &ir.geometry.faces {
        assert_tolerance(
            face.id.as_str(),
            face.tolerance.map(PositiveReal::get),
            expected(face_x(ir, face)),
        );
    }
    for edge in &ir.geometry.edges {
        assert_tolerance(
            edge.id.as_str(),
            edge.tolerance.map(PositiveReal::get),
            expected(edge_x(ir, &edge.id)),
        );
    }
    for vertex in &ir.geometry.vertices {
        assert_tolerance(
            vertex.id.as_str(),
            vertex.tolerance.map(PositiveReal::get),
            expected(vertex_x(ir, &vertex.id)),
        );
    }
}

#[test]
fn a_csg_body_carries_its_context_uncertainty_beside_a_brep_body() {
    let brep_uncertainty = 5.0e-2;
    let csg_uncertainty = 1.0e-3;
    let (brep, next) = box_part(
        100,
        [0.0; 3],
        [10.0; 3],
        Length::Millimetre,
        brep_uncertainty,
    );
    let (context_records, context) = context(next, Length::Millimetre, csg_uncertainty);
    let (block, _) = csg_block(context + 1, 20.0, context, false);
    let result = decode_inline(&format!("{brep}{context_records}{block}"));
    let ir = result.ir();

    assert_eq!(
        ir.tolerances.linear,
        Tolerances::default().linear,
        "the disagreeing contexts keep the document default"
    );
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert_eq!(ir.geometry.faces.len(), 12);
    // The block spans x = 20..22 mm; the box x = 0..10 mm.
    assert_tolerances_by_x(ir, |x| {
        if x > 15.0 {
            csg_uncertainty
        } else {
            brep_uncertainty
        }
    });
    assert_no_ambiguity(&result.report().losses);
}

#[test]
fn a_csg_body_in_a_context_of_two_uncertainties_keeps_the_larger() {
    let fine = 0.1;
    let coarse = 0.2;
    let records = format!(
        "#10=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #11=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({fine:?}),#10,'manufacturing_accuracy','');\n\
         #12=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({coarse:?}),#10,'other_accuracy','');\n\
         #13=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #14=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#11,#12)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#10,#13)) REPRESENTATION_CONTEXT('model','3D'));\n\
         {}",
        csg_block(100, 0.0, 14, true).0,
    );
    let message = kept_and_ambiguity(&records, coarse);
    assert!(
        message.contains(&format!("values in millimetres ({fine:?}, {coarse:?})")),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "1 value(s) ({fine:?}) reach no decoded face, edge, or vertex"
        )),
        "{message}"
    );
}
