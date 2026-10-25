// SPDX-License-Identifier: Apache-2.0
//! Document-driven synthesis and retained-source admission.

use super::*;
use crate::test_support::plan_at;
use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use cadmpeg_ir::codec::write::{ArenaCoverage, ArenaDisposition};
use cadmpeg_ir::report::export::WritePath;

#[test]
fn unsupported_arena_is_dropped_while_neutral_geometry_is_written() {
    let decoded = IgesCodec.decode(&mut Cursor::new(point_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;"
    )), &DecodeOptions::default()).expect("a point fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.model.assets.push(cadmpeg_ir::assets::Asset {
        id: "test:model:asset#external"
            .try_into()
            .expect("valid authored test identity"),
        name: None,
        media_type: None,
        content: cadmpeg_ir::assets::AssetContent::External {
            uri: cadmpeg_core::text::NonBlankString::new("resource.png")
                .expect("external asset URI is nonblank"),
        },
        native_ref: None,
    });
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("unsupported assets are omitted");
    let ArenaCoverage::Declared(coverage) = plan.report().coverage else {
        panic!("synthesis declares arena coverage");
    };
    assert_eq!(coverage.assets, ArenaDisposition::Dropped);
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::WriterArenaOmitted.kind()));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("held geometry writes while unsupported assets are reported");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written geometry decodes");
    assert_eq!(output.ir().model.points.len(), ir.model.points.len());
}

#[test]
fn unrepresented_source_never_reaches_replay() {
    let bytes = explicit_tetrahedron_solid_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("explicit tetrahedron fixture decodes");
    let mut fidelity = decoded.source_fidelity().clone();
    let image = fidelity
        .retained_records()
        .iter()
        .find(|(_, record)| record.data() == Some(bytes.as_slice()))
        .map(|(id, _)| id.to_string())
        .expect("retained source image");
    fidelity
        .mark_untyped(&image, [cadmpeg_ir::ArenaClass::Design])
        .expect("source record exists");
    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("an encodable target falls back to synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::SourceUnrepresented { classes }) if classes.contains(&cadmpeg_ir::ArenaClass::Design)
    ));
}

#[test]
fn container_source_bytes_cannot_supply_undeclared_geometry() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .expect("container-level tetrahedron fixture decodes");
    assert_eq!(decoded.ir().scope, cadmpeg_ir::ContentScope::container());
    assert!(decoded
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::ArenaClass::Model));
    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("a container can synthesize its held content");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("held container content writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written container content decodes");
    assert!(output.ir().model.bodies.is_empty());
    assert!(output.ir().model.points.is_empty());
    assert!(output.ir().model.faces.is_empty());
}

#[test]
fn unencodable_free_surface_keeps_an_independent_solid() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("independent solid fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface::new(
        "test:model:surface#free"
            .try_into()
            .expect("valid authored test identity"),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::new(10.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("authored sphere has a valid frame and radius"),
        )),
        None,
    ));
    let plan = plan_at(IgesVersion::V5_3, &ir, None)
        .expect("independent solid receives a valid synthesis plan");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::WriterEntityOmitted.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("supported solid writes while free surface is reported");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written independent solid decodes");
    assert_eq!(
        output.ir().model.faces.len(),
        decoded.ir().model.faces.len()
    );
    assert_eq!(
        output.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
}

#[test]
fn unsupported_uv_images_preserve_the_model_space_boundary() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::explicit_multi_pcurve_loop_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("multi-pcurve loop fixture decodes");
    let mut ir = decoded.ir().clone();
    assert!(!ir.model.pcurves.is_empty());
    for coedge in &mut ir.model.coedges {
        for pcurve in &mut coedge.pcurves {
            pcurve.isoparametric = None;
        }
    }
    let plan = plan_at(IgesVersion::V5_3, &ir, None)
        .expect("model-space boundary receives a valid synthesis plan");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::WriterPcurvesOmitted.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("supported model-space boundary writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written model-space boundary decodes");
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert_eq!(output.ir().model.edges.len(), ir.model.edges.len());
    assert!(output.ir().model.pcurves.is_empty());
}
