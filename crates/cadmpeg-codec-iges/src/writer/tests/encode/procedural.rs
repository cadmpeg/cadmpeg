// SPDX-License-Identifier: Apache-2.0
//! Procedural carrier export coverage.

use super::*;

#[test]
fn encode_writes_a_ruled_surface_over_composite_rails_natively() {
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
    let ruled = decoded.ir().model.procedural_surfaces[0].id.as_str();
    assert!(
        !report.losses.iter().any(|loss| {
            loss.code == IgesLossCode::ProceduralReduced.kind() && loss.message.contains(ruled)
        }),
        "{:#?}",
        report.losses
    );

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.surfaces.len(), 1);
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces.as_slice(),
        [procedural] if matches!(
            procedural.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Ruled { .. }
        )
    ));
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
