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
    crate::writer::patch::write_semantic(
        &edited,
        &source,
        cadmpeg_core::decode::ResourceLimits::desktop(),
        &mut encoded,
        &mut Vec::new(),
    )
    .expect("patch user knots");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(&encoded), &DecodeOptions::default())
        .expect("read user knots");
    assert_eq!(round_trip.ir().model.subds, edited.model.subds);
    let (mut removed, _, _) = round_trip.into_parts();
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
    crate::writer::patch::write_semantic(
        &removed,
        &encoded,
        cadmpeg_core::decode::ResourceLimits::desktop(),
        &mut removed_bytes,
        &mut Vec::new(),
    )
    .expect("patch extension removal");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(removed_bytes), &DecodeOptions::default())
        .expect("read extension removal");
    assert_eq!(round_trip.ir().model.subds, removed.model.subds);
}
