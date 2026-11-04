// SPDX-License-Identifier: Apache-2.0
//! Sheet reductions preserve admitted faces without repairing malformed input.

use super::adjacent_quad_sheet;
use crate::writer::{brep_payload, model::WritableModel, reduce, WriteError};
use crate::{loss::RhinoLossCode, RhinoArchiveVersion};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::PositiveReal;
use std::cell::Cell;

const UNREPRESENTABLE_NATIVE_TOLERANCE: f64 = 1.0e-12;

fn admit(ir: &CadIr) -> Result<(), WriteError> {
    let arena = DecodeArena::new();
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let model = WritableModel::try_new(ir)?;
    brep_payload(&context, &model, RhinoArchiveVersion::V8)?;
    Ok(())
}

#[test]
fn sheet_reduction_omits_only_the_unsupported_face() {
    let mut ir = adjacent_quad_sheet();
    let body = ir.model.bodies[0].clone();
    let retained = ir.model.faces[0].id.clone();
    let omitted = ir.model.faces[1].id.clone();
    ir.model.faces[1].tolerance = PositiveReal::new(UNREPRESENTABLE_NATIVE_TOLERANCE);
    assert!(matches!(admit(&ir), Err(WriteError::Omit(_))));

    let (written, losses) = reduce::sheet(&ir, admit).expect("entity degradation");
    let written = written.expect("the writable face remains");
    assert_eq!(written.model.bodies, vec![body]);
    assert_eq!(written.model.faces.len(), 1);
    assert_eq!(written.model.faces[0].id, retained);
    assert_eq!(written.model.shells[0].faces(), &[retained]);
    assert_eq!(written.model.edges.len(), 4);
    assert_eq!(written.model.vertices.len(), 4);
    assert_eq!(written.model.points.len(), 4);
    assert_eq!(written.model.curves.len(), 4);
    assert_eq!(written.model.surfaces.len(), 1);
    assert!(written
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.radial_next == coedge.id));
    admit(&written).expect("reduced sheet really encodes");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, RhinoLossCode::WriterEntityOmitted.kind());
    assert_eq!(losses[0].severity, Severity::Error);
    assert!(losses[0].message.contains(omitted.as_str()));
}

#[test]
fn unsupported_first_face_does_not_discard_a_later_writable_face() {
    let mut ir = adjacent_quad_sheet();
    let retained = ir.model.faces[1].id.clone();
    ir.model.faces[0].tolerance = PositiveReal::new(UNREPRESENTABLE_NATIVE_TOLERANCE);
    let (written, losses) = reduce::sheet(&ir, admit).expect("reduce first face");
    let written = written.expect("later face remains");
    assert_eq!(written.model.faces[0].id, retained);
    assert_eq!(losses.len(), 1);
    admit(&written).expect("later face encodes");
}

#[test]
fn malformed_original_radial_cycle_is_not_repaired_by_omission() {
    let mut ir = adjacent_quad_sheet();
    ir.model.faces[1].tolerance = PositiveReal::new(UNREPRESENTABLE_NATIVE_TOLERANCE);
    let shared = ir
        .model
        .edges
        .iter()
        .find_map(|edge| {
            let uses = ir
                .model
                .coedges
                .iter()
                .enumerate()
                .filter(|(_, row)| row.edge == edge.id)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            (uses.len() == 2).then_some(uses)
        })
        .expect("shared edge");
    // Split a valid two-use ring into a premature self-cycle. Pruning the
    // unsupported neighbor would make that same graph appear valid.
    ir.model.coedges[shared[0]].radial_next = ir.model.coedges[shared[0]].id.clone();
    let calls = Cell::new(0);
    let error = reduce::sheet(&ir, |candidate| {
        calls.set(calls.get() + 1);
        admit(candidate)
    })
    .expect_err("malformed original graph");
    let WriteError::Fail(error) = error else {
        panic!("radial inconsistency must fail the write: {error:?}");
    };
    assert!(matches!(error, CodecError::Malformed(_)));
    assert!(error.to_string().contains("radial ring"));
    assert_eq!(
        calls.get(),
        0,
        "validate the original graph before any candidate"
    );
}

#[test]
fn dangling_original_radial_reference_is_a_precondition() {
    let mut ir = adjacent_quad_sheet();
    ir.model.coedges[0].radial_next = "cadir:model:coedge#missing".try_into().expect("identity");
    let error = reduce::sheet(&ir, |_| Ok(())).expect_err("dangling radial use");
    assert!(matches!(error, WriteError::Fail(CodecError::Malformed(_))));
}

#[test]
fn admission_preconditions_propagate_instead_of_becoming_entity_losses() {
    let ir = adjacent_quad_sheet();
    let error = reduce::sheet(&ir, |_| {
        Err(WriteError::Fail(CodecError::InvalidInput(
            "capacity sentinel".into(),
        )))
    })
    .expect_err("precondition");
    assert!(matches!(
        error,
        WriteError::Fail(CodecError::InvalidInput(message)) if message == "capacity sentinel"
    ));
}

#[test]
fn no_admitted_faces_produces_no_body_and_one_loss_per_face() {
    let ir = adjacent_quad_sheet();
    let (written, losses) =
        reduce::sheet(&ir, |_| Err(WriteError::Omit("unsupported carrier".into())))
            .expect("all faces degrade");
    assert!(written.is_none());
    assert_eq!(losses.len(), ir.model.faces.len());
    assert!(losses.iter().all(|loss| loss.severity == Severity::Error));
}

fn wrap_composite(
    ir: &mut CadIr,
    edge: usize,
    wrappers: usize,
) -> (cadmpeg_ir::ids::CurveId, cadmpeg_ir::ids::CurveId) {
    use cadmpeg_ir::geometry::{CompositeCurveSegment, CompositeCurveSegments};
    use cadmpeg_ir::geometry::{
        CompositeCurveTransition, Curve, CurveGeometry, SolvedCurveGeometry,
    };
    let leaf = ir.model.edges[edge]
        .curve()
        .expect("fixture edge carries a curve")
        .clone();
    ir.model.edges[edge].carrier = cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(leaf.clone()));
    let mut inner = leaf.clone();
    for depth in 0..wrappers {
        let id =
            cadmpeg_ir::ids::CurveId::mint(format!("cadir:model:curve#composite.{edge}.{depth}"))
                .expect("identity grammar");
        let segments = CompositeCurveSegments::try_from(vec![CompositeCurveSegment {
            curve: inner.clone(),
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }])
        .expect("one segment is a composite");
        ir.model.curves.push(Curve {
            native_ref: None,
            parameter_range: None,
            id: id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments,
                self_intersect: None,
            }),
            source_object: None,
        });
        inner = id;
    }
    ir.model.edges[edge].carrier =
        cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(inner.clone()));
    (inner, leaf)
}

#[test]
fn single_segment_composite_chain_resolves_to_its_line_leaf() {
    let mut ir = adjacent_quad_sheet();
    let (outer, leaf) = wrap_composite(&mut ir, 0, 2);
    let model = WritableModel::try_new(&ir).expect("composite edge admits");
    assert_eq!(model.reduced_carriers.len(), 1);
    assert_eq!(
        model.reduced_carriers[0].edge,
        ir.model.edges[0].id.as_str()
    );
    assert_eq!(model.reduced_carriers[0].leaf, Some(leaf.as_str()));
    assert!(model.reduced_carriers[0]
        .skipped
        .iter()
        .any(|id| *id == outer.as_str()));
    assert!(admit(&ir).is_ok());
}

#[test]
fn rigid_transformed_carrier_resolves_to_an_explicit_line() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    let mut ir = adjacent_quad_sheet();
    let target = ir.model.edges[1]
        .curve()
        .expect("fixture edge carries a curve")
        .clone();
    let basis = match ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == target)
        .expect("carrier curve")
        .geometry
        .solved()
    {
        Some(SolvedCurveGeometry::Line(line)) => *line,
        other => panic!("fixture carrier is a line, not {other:?}"),
    };
    let placed = cadmpeg_ir::geometry::PlacedCurve::try_new(
        Box::new(SolvedCurveGeometry::Line(basis)),
        cadmpeg_ir::transform::Transform::identity(),
    )
    .expect("identity placement is admitted");
    let wrapper =
        cadmpeg_ir::ids::CurveId::mint("cadir:model:curve#placed.1").expect("identity grammar");
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: wrapper.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Transformed(placed)),
        source_object: None,
    });
    ir.model.edges[1].carrier =
        cadmpeg_ir::topology::EdgeCarrier::from_finite_parts(Some(wrapper), None)
            .expect("carrier without range");
    let model = WritableModel::try_new(&ir).expect("transformed edge admits");
    assert!(model
        .reduced_carriers
        .iter()
        .any(|reduced| reduced.edge == ir.model.edges[1].id.as_str() && reduced.leaf.is_none()));
    assert!(admit(&ir).is_ok());
}

#[test]
fn carrier_resolution_loss_names_affected_coedges() {
    use super::super::{model::ReducedCarrier, reduction_loss};
    let loss = reduction_loss(&ReducedCarrier {
        edge: "cadir:model:edge#resolved",
        leaf: Some("cadir:model:curve#leaf"),
        skipped: vec!["cadir:model:curve#composite"],
        coedges: vec!["cadir:model:coedge#uses-leaf"],
    });
    assert!(
        loss.message.contains("cadir:model:coedge#uses-leaf"),
        "{loss:?}"
    );
    let loss = reduction_loss(&ReducedCarrier {
        edge: "cadir:model:edge#resolved",
        leaf: None,
        skipped: vec!["cadir:model:curve#placed"],
        coedges: Vec::new(),
    });
    assert!(
        !loss.message.contains("affecting coedge"),
        "{loss:?}"
    );
}
