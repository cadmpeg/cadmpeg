// SPDX-License-Identifier: Apache-2.0
//! Procedural carrier export coverage.

use super::*;

#[test]
fn encode_reduces_exact_procedural_carriers_to_solved_geometry() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(composite_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(decoded.ir().model.procedural_curves.len(), 2);

    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    let model = &decoded.ir().model;
    let surfaces = model
        .procedural_surfaces
        .iter()
        .map(|record| record.id.as_str());
    let curves = model
        .procedural_curves
        .iter()
        .map(|record| record.id.as_str());
    for id in surfaces.chain(curves) {
        assert!(
            report.losses.iter().any(|loss| {
                loss.code == IgesLossCode::ProceduralReduced.kind() && loss.message.contains(id)
            }),
            "missing loss for {id}"
        );
    }
    assert!(
        report.losses.iter().all(
            |loss| accepts_procedural_reduction_loss(loss.code.taxonomy())
                || loss.code == IgesLossCode::WriterSupportEdgeNotRepresented.kind()
                || loss.code == IgesLossCode::WriterSupportGeometryNotRepresented.kind()
        ),
        "{:#?}",
        report.losses
    );
    // The Type 128 output has no rails, so the two composite rails and their
    // four constituents have no written parent.
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::WriterSupportGeometryNotRepresented.kind())
            .count(),
        6
    );

    let support_edge_losses = report
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::WriterSupportEdgeNotRepresented.kind())
        .collect::<Vec<_>>();
    assert_eq!(support_edge_losses.len(), 2);
    assert!(support_edge_losses
        .iter()
        .all(|loss| loss.message.starts_with("1 standalone edge record")));

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.surfaces.len(), 1);
    // The solved carrier writes with its domain-rectangle boundary: four
    // isocurve model curves bound the Type 143 face.
    assert_eq!(round_trip.ir().model.curves.len(), 4);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
