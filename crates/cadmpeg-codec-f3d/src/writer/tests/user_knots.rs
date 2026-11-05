// SPDX-License-Identifier: Apache-2.0
use crate::F3dCodec;
use cadmpeg_ir::{
    codec::write::{target::TargetRequest, EncodeInput, Encoder, SynthesisCause},
    codec::{Codec, DecodeOptions},
    scalar::NonNegativeReal,
    subd::SubdUserKnot,
};
use std::io::Cursor;

#[test]
fn source_less_t_spline_writes_user_knot_state() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_form(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut neutral = cadmpeg_ir::CadIr::empty();
    let mut cage = decoded.ir().model.subds[0].clone();
    cage.native_ref = None;
    cage.source_object = None;
    neutral.model.subds.push(cage);
    let mut limited = EncodeInput::new(&neutral, None);
    limited.limits.max_entities = 1;
    assert!(matches!(
        F3dCodec.plan(limited, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(_))
    ));
    let (written, report) = super::source_less_analytic::synthesize_document(&neutral, None);
    assert_eq!(written.ir().model.subds.len(), 1);
    assert_eq!(
        written.ir().model.subds[0].cage,
        neutral.model.subds[0].cage
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(neutral.model.subds[0].id.as_str())));
    let mut ir = decoded.ir().clone();
    ir.model.subds[0]
        .cage
        .edit_edges(|edges| {
            edges[0].user_knot = Some(SubdUserKnot::Override(NonNegativeReal::new(0.0).unwrap()));
            Ok(())
        })
        .unwrap();
    let (written, _) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(written.ir().model.subds, ir.model.subds);
    let mut removed = written.ir().clone();
    removed.model.subds[0]
        .cage
        .edit_edges(|edges| {
            for edge in edges {
                edge.user_knot = None;
            }
            Ok(())
        })
        .unwrap();
    let (written, _) = super::source_less_analytic::synthesize_document(&removed, None);
    assert_eq!(written.ir().model.subds, removed.model.subds);
}

#[test]
fn retained_f3d_round_trips_user_knot_edits_and_extension_removal() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_form(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("source decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let mut unsupported = edited.clone();
    unsupported.model.subds[0]
        .cage
        .edit_vertices(|vertices| {
            vertices[0].set_point(
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    17.0, 0.0, 0.0,
                ))
                .expect("finite control point"),
            );
            Ok(())
        })
        .expect("admitted cage vertex edit");
    let error = crate::writer::patch::write_semantic(
        &unsupported,
        &source,
        cadmpeg_core::decode::ResourceLimits::desktop(),
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .expect_err("a cage edit declines a retained-source patch");
    assert!(error
        .to_string()
        .contains("unchanged cage geometry and topology"));
    let plan = F3dCodec
        .plan(
            EncodeInput::new(&unsupported, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(SynthesisCause::SourceUnrepresented { .. })
    ));

    edited.model.subds[0]
        .cage
        .edit_edges(|edges| {
            edges[0].user_knot = Some(SubdUserKnot::Override(
                NonNegativeReal::new(0.0).expect("zero"),
            ));
            Ok(())
        })
        .expect("user-knot edit");
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut encoded)
        .expect("write user knots");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(&encoded), &DecodeOptions::default())
        .expect("read user knots");
    assert_eq!(round_trip.ir().model.subds, edited.model.subds);
    let (mut removed, _, fidelity) = round_trip.into_parts();
    removed.model.subds[0]
        .cage
        .edit_edges(|edges| {
            for edge in edges {
                edge.user_knot = None;
            }
            Ok(())
        })
        .expect("remove extension");
    let mut removed_bytes = Vec::new();
    crate::test_support::plan_inherited_write(&removed, &fidelity, &mut removed_bytes)
        .expect("write extension removal");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(removed_bytes), &DecodeOptions::default())
        .expect("read extension removal");
    assert_eq!(round_trip.ir().model.subds, removed.model.subds);
}
