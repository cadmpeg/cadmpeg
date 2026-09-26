// SPDX-License-Identifier: Apache-2.0
//! B-rep agreement tests for edited construction history.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::container::make_block;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::native::{sldprt_native, update_sldprt_native};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn extrusion_depth_edit_without_brep_edit_is_refused() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let feature_id = decoded.ir().model.features[0].id.clone();
    decoded.ir_mut().model.features[0]
        .evaluation
        .edit(|definition, _| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Extrude { extent, .. },
            ) = definition
            else {
                panic!("typed extrusion feature");
            };
            *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(50.0).unwrap(),
                    },
                    draft: None,
                },
            };
        });
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(_)),
        "{error:?}"
    );
    assert!(error.to_string().contains(feature_id.as_str()), "{error}");
}

#[test]
fn feature_output_scope_edit_is_refused() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let feature_id = decoded.ir().model.features[0].id.clone();
    let body = decoded.ir().model.bodies[0].id.clone();
    let outputs = decoded.ir().model.features[0].evaluation.outputs().clone();
    let replacement = if outputs.is_empty() {
        vec![body]
    } else {
        Vec::new()
    };
    decoded.ir_mut().model.features[0]
        .evaluation
        .set_outputs((replacement).try_into().unwrap());
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains(feature_id.as_str()), "{error}");
}

#[test]
fn feature_output_edit_with_nongeometric_history_edit_is_refused() {
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
    let feature_id = decoded.ir().model.features[modeling].id.clone();
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
        .set_outputs((replacement).try_into().unwrap());
    edit.model.features[nongeometric].source_text = Some("changed".into());
    drop(edit);
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains(feature_id.as_str()), "{error}");
}

#[test]
fn extrusion_parameter_value_edit_without_brep_edit_is_refused() {
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
    let parameter_id = parameter.id.clone();
    parameter.expression = "50mm".into();
    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
    ));
    drop(edit);
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains(parameter_id.as_str()), "{error}");
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
fn native_extrusion_depth_edit_is_refused() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native_id = sldprt_native(decoded.ir()).feature_histories[0].features[0]
        .id
        .clone();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), "50mm".into());
    });
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains(&native_id), "{error}");
}

#[test]
fn native_extrusion_edit_without_source_image_is_refused() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native_id = sldprt_native(decoded.ir()).feature_histories[0].features[0]
        .id
        .clone();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), "50mm".into());
    });
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains(&native_id), "{error}");
}

#[test]
fn parameter_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let source_partition = container::select_active_parasolid_site(&container::scan_bytes(&source))
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
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .unwrap();
    let output_scan = container::scan_bytes(&output);
    let output_partition = container::select_active_parasolid_site(&output_scan)
        .unwrap()
        .section
        .payload();
    assert_eq!(output_partition, source_partition);
}

#[test]
fn feature_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let source_partition = container::select_active_parasolid_site(&container::scan_bytes(&source))
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
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .unwrap();
    let output_scan = container::scan_bytes(&output);
    let output_partition = container::select_active_parasolid_site(&output_scan)
        .unwrap()
        .section
        .payload();
    assert_eq!(output_partition, source_partition);
}

#[test]
fn native_feature_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let source_partition = container::select_active_parasolid_site(&container::scan_bytes(&source))
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
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .unwrap();
    let output_scan = container::scan_bytes(&output);
    let output_partition = container::select_active_parasolid_site(&output_scan)
        .unwrap()
        .section
        .payload();
    assert_eq!(output_partition, source_partition);
}
