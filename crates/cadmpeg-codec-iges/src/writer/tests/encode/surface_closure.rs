// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};

fn cylinder_fixture() -> Vec<u8> {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "analytic_cylinder_surface")
        .expect("cylinder fixture")
        .1
}

#[test]
fn analytic_cylinder_synthesis_preserves_held_records() {
    let options = DecodeOptions {
        depth: IgesCodec.request(),
        ..DecodeOptions::default()
    };
    let input = IgesCodec
        .decode(&mut Cursor::new(cylinder_fixture()), &options)
        .expect("cylinder decodes");
    let redecode = |written: &[u8]| -> Result<Redecoded, String> {
        let output = IgesCodec
            .decode(&mut Cursor::new(written), &options)
            .map_err(|error| error.to_string())?;
        Ok(Redecoded {
            ir: serde_json::to_value(output.ir()).map_err(|error| error.to_string())?,
            loss_codes: output
                .report()
                .losses
                .iter()
                .map(|loss| loss.code.to_string())
                .collect(),
            members: Vec::new(),
        })
    };
    let findings = without_fidelity(&input, &IgesCodec, &redecode);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn edited_cylinder_reference_direction_replaces_implicit_frame() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(cylinder_fixture()),
            &DecodeOptions::default(),
        )
        .expect("cylinder decodes");
    let mut ir = decoded.ir().clone();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            2.0,
        )
        .expect("edited frame is valid"),
    ));
    let plan = IgesCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("edited cylinder synthesizes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("edited cylinder writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("edited cylinder re-decodes");
    let Some(SolvedSurfaceGeometry::Cylinder(cylinder)) =
        output.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected cylinder");
    };
    assert_eq!(
        *cylinder.frame().reference().as_raw(),
        Vector3::new(0.0, 1.0, 0.0)
    );
}
