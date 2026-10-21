// SPDX-License-Identifier: Apache-2.0
//! Sheet reductions preserve admitted faces without repairing malformed input.

use super::adjacent_quad_sheet;
use crate::writer::{brep_payload, model::WritableModel, reduce};
use crate::{loss::RhinoLossCode, RhinoArchiveVersion, RhinoCodec};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::PositiveReal;
use std::cell::Cell;
use std::io::Cursor;

const EPS_FINE_STATED_TOLERANCE: f64 = 1.0e-12;

fn admit(ir: &CadIr) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let model = WritableModel::try_new(ir)?;
    brep_payload(&context, &model, RhinoArchiveVersion::V8)?;
    Ok(())
}

/// Use the same planar unit quad as an exact bilinear patch. CADIR permits
/// its existing C3 boundary without explicit C2 pcurves; this Rhino writer
/// requires explicit pcurves for NURBS faces and cannot author that trim form.
fn use_equivalent_nurbs_face_without_pcurves(ir: &mut CadIr, face: usize) {
    let x = match face {
        0 => 0.0,
        1 => 1.0,
        _ => panic!("the adjacent sheet has two unit quads"),
    };
    let surface = ir.model.faces[face].surface.clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|candidate| candidate.id == surface)
        .expect("face carrier");
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)],
                    vec![
                        Point3::new(x + 1.0, 0.0, 0.0),
                        Point3::new(x + 1.0, 1.0, 0.0),
                    ],
                ],
                None,
            ),
            false,
        )
        .expect("degree-one patch is the exact same planar unit quad"),
    ));
    let checked = cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate source sheet");
    assert!(checked.is_ok(), "valid unsupported trim form: {checked:#?}");
}

/// The old writer floor rejected these exact planar inputs at 1e-12. With
/// that floor removed under #210, their stated tolerance remains supported;
/// neither face position may turn into a reduction trigger.
#[test]
fn fine_face_tolerance_preserves_both_adjacent_faces() {
    for face in 0..2 {
        let mut ir = adjacent_quad_sheet();
        ir.model.faces[face].tolerance = PositiveReal::new(EPS_FINE_STATED_TOLERANCE);
        admit(&ir).expect("exact planar faces meet their actual stated tolerance");
        let (written, losses) = reduce::sheet(&ir, admit).expect("supported sheet");
        assert_eq!(written.expect("both faces remain").model, ir.model);
        assert!(losses.is_empty());
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            let report = RhinoCodec
                .plan(
                    EncodeInput::new(&ir, None),
                    TargetRequest::Explicit(version.descriptor().id.as_str()),
                )
                .and_then(|plan| plan.write_to(&mut bytes))
                .expect("fine-tolerance adjacent sheet writes");
            assert!(
                report
                    .losses
                    .iter()
                    .all(|loss| loss.code != RhinoLossCode::WriterEntityOmitted.kind()),
                "{version:?}: {:?}",
                report.losses
            );
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("fine-tolerance adjacent sheet decodes");
            let model = &decoded.ir().model;
            assert_eq!(model.bodies.len(), 1);
            assert_eq!(model.faces.len(), 2);
            assert_eq!(model.loops.len(), 2);
            assert_eq!(model.coedges.len(), 8);
            assert_eq!(model.edges.len(), 7);
            assert_eq!(model.vertices.len(), 6);
            assert_eq!(model.points.len(), 6);
            for point in &ir.model.points {
                let expected = point.position().get();
                assert!(model.points.iter().any(|point| {
                    let actual = point.position().get();
                    (actual.x - expected.x).abs() <= EPS_FINE_STATED_TOLERANCE
                        && (actual.y - expected.y).abs() <= EPS_FINE_STATED_TOLERANCE
                        && (actual.z - expected.z).abs() <= EPS_FINE_STATED_TOLERANCE
                }), "face {face}, {version:?}: exact source point must survive within its stated tolerance");
            }
            let checked = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
                .expect("validate written sheet");
            assert!(checked.is_ok(), "{version:?}: {checked:#?}");
        }
    }
}

#[test]
fn sheet_reduction_omits_only_the_unsupported_face() {
    let mut ir = adjacent_quad_sheet();
    let body = ir.model.bodies[0].clone();
    let retained = ir.model.faces[0].id.clone();
    let omitted = ir.model.faces[1].id.clone();
    ir.model.faces[1].tolerance = PositiveReal::new(EPS_FINE_STATED_TOLERANCE);
    use_equivalent_nurbs_face_without_pcurves(&mut ir, 1);
    assert!(
        matches!(admit(&ir), Err(CodecError::UnsupportedContent(reason))
        if reason.contains("has no explicit pcurve"))
    );

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
    ir.model.faces[0].tolerance = PositiveReal::new(EPS_FINE_STATED_TOLERANCE);
    use_equivalent_nurbs_face_without_pcurves(&mut ir, 0);
    assert!(
        matches!(admit(&ir), Err(CodecError::UnsupportedContent(reason))
        if reason.contains("has no explicit pcurve"))
    );
    let (written, losses) = reduce::sheet(&ir, admit).expect("reduce first face");
    let written = written.expect("later face remains");
    assert_eq!(written.model.faces[0].id, retained);
    assert_eq!(losses.len(), 1);
    admit(&written).expect("later face encodes");
}

#[test]
fn malformed_original_radial_cycle_is_not_repaired_by_omission() {
    let mut ir = adjacent_quad_sheet();
    ir.model.faces[1].tolerance = PositiveReal::new(EPS_FINE_STATED_TOLERANCE);
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
    // neighboring face would make that same graph appear valid.
    ir.model.coedges[shared[0]].radial_next = ir.model.coedges[shared[0]].id.clone();
    let calls = Cell::new(0);
    let error = reduce::sheet(&ir, |candidate| {
        calls.set(calls.get() + 1);
        admit(candidate)
    })
    .expect_err("malformed original graph");
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
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn admission_preconditions_propagate_instead_of_becoming_entity_losses() {
    let ir = adjacent_quad_sheet();
    let error = reduce::sheet(&ir, |_| {
        Err(CodecError::InvalidInput("capacity sentinel".into()))
    })
    .expect_err("precondition");
    assert!(matches!(error, CodecError::InvalidInput(message) if message == "capacity sentinel"));
}

#[test]
fn no_admitted_faces_produces_no_body_and_one_loss_per_face() {
    let ir = adjacent_quad_sheet();
    let (written, losses) = reduce::sheet(&ir, |_| {
        Err(CodecError::UnsupportedContent("unsupported carrier".into()))
    })
    .expect("all faces degrade");
    assert!(written.is_none());
    assert_eq!(losses.len(), ir.model.faces.len());
    assert!(losses.iter().all(|loss| loss.severity == Severity::Error));
}
