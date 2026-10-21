// SPDX-License-Identifier: Apache-2.0
//! B-rep agreement tests for edited construction history.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::container::make_block;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::native::update_sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn extrusion_depth_edit_without_brep_edit_writes_canonical_design_with_retained_layout() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let feature = decoded.ir().model.features[0].id.clone();
    decoded
        .ir_mut()
        .set_dimension(
            &feature,
            "/extent/side/termination/length",
            cadmpeg_ir::scalar::NonZeroLength::new(50.0).unwrap(),
        )
        .expect("edit canonical extrusion depth");
    assert_written_design(&decoded, ExpectedPath::Patched);
}

#[test]
fn feature_output_scope_edit_writes_canonical_design_with_retained_layout() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let body = decoded.ir().model.bodies[0].id.clone();
    let outputs = decoded.ir().model.features[0].evaluation.outputs().clone();
    let replacement = if outputs.is_empty() {
        vec![body]
    } else {
        Vec::new()
    };
    decoded.ir_mut().model.features[0]
        .evaluation
        .set_outputs(cadmpeg_ir::features::DistinctMembers::try_from(replacement).unwrap());
    assert_written_design(&decoded, ExpectedPath::Patched);
}

#[test]
fn feature_output_edit_with_nongeometric_history_edit_writes_canonical_design_with_retained_layout()
{
    let mut source = sldprt_with_body_and_history(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords-Equations",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="99"><Dimension Name="Width">4mm</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let modeling = decoded
        .ir()
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    let nongeometric = decoded
        .ir()
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("Equations"))
        .unwrap();
    let body = decoded.ir().model.bodies[0].id.clone();
    let replacement = if decoded.ir().model.features[modeling]
        .evaluation
        .outputs()
        .is_empty()
    {
        vec![body]
    } else {
        Vec::new()
    };
    let mut edit = decoded.ir_mut();
    edit.model.features[modeling]
        .evaluation
        .set_outputs(cadmpeg_ir::features::DistinctMembers::try_from(replacement).unwrap());
    edit.model.features[nongeometric].source_text = Some("changed".into());
    drop(edit);
    assert_written_design(&decoded, ExpectedPath::Patched);
}

#[test]
fn extrusion_parameter_value_edit_without_brep_edit_writes_canonical_design_with_retained_layout() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let mut edit = decoded.ir_mut();
    let parameter = edit
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    parameter.expression = "50mm".into();
    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
    ));
    drop(edit);
    assert_written_design(&decoded, ExpectedPath::Patched);
}

#[test]
fn removing_referenced_extrusion_parameter_is_malformed() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let parameter_id = decoded.ir().model.parameters[0].id.clone();
    decoded.ir_mut().model.parameters.clear();
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(_)),
        "{error:?}"
    );
    assert!(error.to_string().contains(parameter_id.as_str()), "{error}");
}

#[test]
fn native_extrusion_depth_edit_writes_canonical_design_with_retained_layout() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), "50mm".into());
    });
    assert_written_design(&decoded, ExpectedPath::Patched);
}

#[test]
fn native_extrusion_edit_without_source_image_is_synthesized() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), "50mm".into());
    });
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    assert_written_design(&decoded, ExpectedPath::Synthesized);
}

#[test]
fn parameter_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let source_partition =
        container::select_active_parasolid_site(&crate::test_support::container::scan(&source))
            .unwrap()
            .section
            .payload()
            .to_vec();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.parameters[0].name = "RenamedDepth".into();
    let mut output = Vec::new();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    let report = plan.report().clone();
    plan.write_to(&mut output).unwrap();
    let output_scan = crate::test_support::container::scan(&output);
    let output_partition = container::select_active_parasolid_site(&output_scan)
        .unwrap()
        .section
        .payload();
    assert_eq!(
        output_partition,
        source_partition,
        "unchanged B-rep path: {report:?}; source classes: {:?}",
        decoded.source_fidelity().unrepresented()
    );
}

#[test]
fn feature_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let source_partition =
        container::select_active_parasolid_site(&crate::test_support::container::scan(&source))
            .unwrap()
            .section
            .payload()
            .to_vec();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.features[0].name = Some("RenamedBoss".into());
    let mut output = Vec::new();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    let report = plan.report().clone();
    plan.write_to(&mut output).unwrap();
    let output_scan = crate::test_support::container::scan(&output);
    let output_partition = container::select_active_parasolid_site(&output_scan)
        .unwrap()
        .section
        .payload();
    assert_eq!(
        output_partition,
        source_partition,
        "unchanged B-rep path: {report:?}; source classes: {:?}",
        decoded.source_fidelity().unrepresented()
    );
}

#[test]
fn native_feature_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let source_partition =
        container::select_active_parasolid_site(&crate::test_support::container::scan(&source))
            .unwrap()
            .section
            .payload()
            .to_vec();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0].name = "RenamedBoss".into();
    });
    let mut output = Vec::new();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    let report = plan.report().clone();
    plan.write_to(&mut output).unwrap();
    let output_scan = crate::test_support::container::scan(&output);
    let output_partition = container::select_active_parasolid_site(&output_scan)
        .unwrap()
        .section
        .payload();
    assert_eq!(
        output_partition,
        source_partition,
        "unchanged B-rep path: {report:?}; source classes: {:?}",
        decoded.source_fidelity().unrepresented()
    );
}

enum ExpectedPath {
    Patched,
    Synthesized,
}

fn assert_written_design(
    decoded: &cadmpeg_test_support::EditableDecodeResult,
    expected_path: ExpectedPath,
) {
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    let report = plan.report().clone();
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = SldprtCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    // E1: retained layout cannot add or substitute document content.
    let synthesized = SldprtCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let synthesized_report = synthesized.report().clone();
    let mut synthesized_bytes = Vec::new();
    synthesized.write_to(&mut synthesized_bytes).unwrap();
    let source_free = SldprtCodec
        .decode(
            &mut Cursor::new(synthesized_bytes),
            &DecodeOptions::default(),
        )
        .unwrap();
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &synthesized_report,
        written.ir(),
        source_free.ir(),
    );
    let expected_stale = decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.needs_recompute);
    assert!(
        written
            .ir()
            .model
            .features
            .iter()
            .all(|feature| feature.needs_recompute == expected_stale),
        "the stored swConfigurationNeedsUpdate flag states the input evaluation state"
    );
    assert_eq!(
        written.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
    let mut expected = decoded.ir().model.parameters.iter().collect::<Vec<_>>();
    let mut actual = written.ir().model.parameters.iter().collect::<Vec<_>>();
    expected.sort_by_key(|parameter| &parameter.name);
    actual.sort_by_key(|parameter| &parameter.name);
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_eq!(actual.name, expected.name);
        assert_eq!(
            actual.value, expected.value,
            "the full typed cache survives"
        );
        let setter_literal = expected.value.as_ref().filter(|value| {
            expected.expression == value.literal_expression()
                && expected.expression.literal_value_as(value).as_ref() == Some(*value)
        });
        if let Some(value) = setter_literal {
            // D5: API-generated single literals equal their typed value. The
            // native grammar may render 50 mm as 50mm; quantities stay exact.
            assert_eq!(
                actual.expression.literal_value_as(value).as_ref(),
                Some(value)
            );
        } else {
            assert_eq!(
                actual.expression, expected.expression,
                "authored expression tokens decide the written history"
            );
        }
    }
    // E1 permits retained layout after the independent content check above.
    // Without its source image the same canonical design is synthesized.
    assert!(
        match expected_path {
            ExpectedPath::Patched => matches!(
                report.write_path(),
                cadmpeg_ir::report::export::WritePath::Patched { .. }
            ),
            ExpectedPath::Synthesized => matches!(
                report.write_path(),
                cadmpeg_ir::report::export::WritePath::Synthesized { .. }
            ),
        },
        "retained-path report after exact content checks: {report:?}"
    );
}
