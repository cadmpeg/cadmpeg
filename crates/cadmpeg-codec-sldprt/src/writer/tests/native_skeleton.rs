// SPDX-License-Identifier: Apache-2.0
//! The native skeleton of a generated part: its identifiers beside the
//! history summary, and the input that keeps it off.

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

/// Writes `ir` as a generated part: no retained source.
fn write(ir: &cadmpeg_ir::CadIr) -> (Vec<u8>, Vec<LossNote>) {
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap();
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    (bytes, losses)
}

fn stream_names(bytes: &[u8]) -> Vec<String> {
    crate::test_support::container::scan(bytes)
        .sections()
        .filter_map(|section| section.name().map(str::to_owned))
        .collect()
}

/// A part with five flex features, decoded and then stripped of its
/// `SolidWorks` records, as a document from another format holds them.
fn five_flex_features() -> cadmpeg_ir::CadIr {
    let mut source = sldprt_with_body(&triangle_body());
    let records = (1..=5).fold(String::new(), |mut records, id| {
        write!(
            records,
            r#"<Flex Name="Bend{id}" Type="Flex" id="{}" Mode="Bending" Axis="0,1,0"><Dimension Name="Angle">30deg</Dimension></Flex>"#,
            40 + id
        )
        .unwrap();
        records
    });
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!("<Keywords>{records}</Keywords>").as_bytes(),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    ir.native.0.remove("sldprt");
    ir.model.configurations.clear();
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .retain(|key, _| !std::borrow::Borrow::<str>::borrow(key).starts_with("sldprt_"));
    }
    for feature in &mut ir.model.features {
        feature.native_ref = None;
    }
    ir
}

#[test]
fn skeleton_identifiers_bind_no_summary_feature_to_a_default_tree_node() {
    let ir = five_flex_features();
    assert_eq!(ir.model.features.len(), 5);
    let (bytes, losses) = write(&ir);
    assert!(stream_names(&bytes).contains(&"Contents/Config-0".to_owned()));
    assert!(losses
        .iter()
        .all(|loss| loss.code != SldprtLossCode::WriterNativeSkeletonOmitted.kind()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let features = &decoded.ir().model.features;
    let named = |name: &str| {
        features
            .iter()
            .filter(|feature| feature.name.as_deref() == Some(name))
            .collect::<Vec<_>>()
    };
    // The summary numbers its features 1 to 5. The default tree's
    // identifiers follow them, so each decodes as its own flex feature.
    for index in 1..=5 {
        let bends = named(&format!("Bend{index}"));
        assert_eq!(bends.len(), 1, "Bend{index}");
        assert!(
            matches!(
                bends[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Flex { .. })
            ),
            "Bend{index}"
        );
    }
}

#[test]
fn a_solidworks_feature_history_keeps_the_skeleton_off() {
    let mut ir = crate::test_support::ir::source_less_cube();
    let native = crate::native::SldprtNative {
        feature_histories: vec![crate::records::FeatureHistory {
            id: "sldprt:file:feature-history#0".into(),
            part_name: None,
            properties: std::collections::BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: Vec::new(),
        }],
        feature_input_lanes: Vec::new(),
        pmi_dimensions: Vec::new(),
    };
    native
        .store(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            ir.native.namespace_mut("sldprt"),
        )
        .unwrap();
    let (bytes, losses) = write(&ir);
    let omitted = losses
        .iter()
        .find(|loss| loss.code == SldprtLossCode::WriterNativeSkeletonOmitted.kind())
        .unwrap();
    assert!(
        omitted
            .message
            .starts_with("the native skeleton is not written: the document's feature history"),
        "{}",
        omitted.message
    );
    let names = stream_names(&bytes);
    assert!(!names.contains(&"Contents/Config-0".to_owned()));
    assert!(!names.contains(&"swXmlContents/KeyWords".to_owned()));
}
