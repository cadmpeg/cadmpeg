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
fn source_less_t_spline_writes_symmetry_maps() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::subd::{
        SubdCage, SubdPlaneFrame, SubdRadialMapSelector as Selector, SubdRadialSymmetryMap,
        SubdSymmetry, SubdSymmetryKind,
    };
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_form(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.subds.push(decoded.ir().model.subds[0].clone());
    let plane = SubdPlaneFrame::new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    )
    .unwrap();
    let correspondence = SubdSymmetry::new(
        SubdSymmetryKind::Correspondence {},
        plane,
        vec![],
        vec![[0, 2]],
        vec![[0, 2]],
    )
    .unwrap();
    let maps = vec![
        SubdRadialSymmetryMap {
            selector: Selector::Ef,
            pairs: vec![[0, 1]],
        },
        SubdRadialSymmetryMap {
            selector: Selector::Er,
            pairs: vec![[1, 0]],
        },
        SubdRadialSymmetryMap {
            selector: Selector::Ff,
            pairs: vec![],
        },
        SubdRadialSymmetryMap {
            selector: Selector::Fr,
            pairs: vec![],
        },
        SubdRadialSymmetryMap {
            selector: Selector::Vf,
            pairs: vec![[0, 1]],
        },
        SubdRadialSymmetryMap {
            selector: Selector::Vr,
            pairs: vec![[1, 0]],
        },
    ];
    let radial = SubdSymmetry::new(
        SubdSymmetryKind::radial(std::num::NonZeroU32::new(4).unwrap(), 1.0, maps).unwrap(),
        plane,
        vec![],
        vec![[0, 1]],
        vec![[0, 1]],
    )
    .unwrap();
    let cage = &ir.model.subds[0].cage;
    ir.model.subds[0].cage = SubdCage::new(
        cage.vertices().to_vec(),
        cage.edges().to_vec(),
        cage.faces().to_vec(),
        vec![correspondence, radial],
    )
    .unwrap();
    let (written, _) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(written.ir().model.subds.len(), 1);
    assert_eq!(written.ir().model.subds[0].cage, ir.model.subds[0].cage);
    let cage = &ir.model.subds[0].cage;
    let unsupported = SubdSymmetry::new(
        SubdSymmetryKind::radial(
            std::num::NonZeroU32::new(4).unwrap(),
            1.0,
            vec![SubdRadialSymmetryMap {
                selector: Selector::Ef,
                pairs: vec![[0, 1]],
            }],
        )
        .unwrap(),
        cage.symmetries()[1].plane,
        vec![],
        vec![[0, 1]],
        vec![[0, 1]],
    )
    .unwrap();
    let retained_symmetry = cage.symmetries()[0].clone();
    let mut reduced = ir.clone();
    reduced.model.subds[0].cage = SubdCage::new(
        cage.vertices().to_vec(),
        cage.edges().to_vec(),
        cage.faces().to_vec(),
        vec![retained_symmetry.clone(), unsupported],
    )
    .unwrap();
    let (written, report) = super::source_less_analytic::synthesize_document(&reduced, None);
    assert_eq!(
        written.ir().model.subds[0].cage.symmetries(),
        &[retained_symmetry]
    );
    assert!(report.losses.iter().any(|loss| {
        loss.message.contains(reduced.model.subds[0].id.as_str())
            && loss.message.contains("symmetry 1")
    }));
    let cage = &ir.model.subds[0].cage;
    let plane = cage.symmetries()[1].plane;
    let radial = SubdSymmetry::new(
        SubdSymmetryKind::radial(std::num::NonZeroU32::new(4).unwrap(), 1.0, Vec::new()).unwrap(),
        plane,
        vec![],
        vec![[0, 1]],
        vec![[0, 1]],
    )
    .unwrap();
    ir.model.subds[0].cage = SubdCage::new(
        cage.vertices().to_vec(),
        cage.edges().to_vec(),
        cage.faces().to_vec(),
        vec![radial],
    )
    .unwrap();
    let (written, report) = super::source_less_analytic::synthesize_document(&ir, None);
    let symmetry = &written.ir().model.subds[0].cage.symmetries()[0];
    assert_eq!(symmetry.edge_pairs(), [[0, 1]]);
    assert_eq!(symmetry.vertex_pairs(), [[0, 1]]);
    let SubdSymmetryKind::Radial(radial) = symmetry.kind() else {
        panic!("radial symmetry")
    };
    let reverse = radial
        .radial_maps()
        .iter()
        .find(|map| map.selector == Selector::Er)
        .unwrap();
    assert_eq!(reverse.pairs, [[1, 0]]);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(ir.model.subds[0].id.as_str())));
}

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
        Some(SynthesisCause::PatchDeclined { .. })
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
