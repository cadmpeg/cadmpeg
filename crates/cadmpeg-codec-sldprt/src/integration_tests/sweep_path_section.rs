// SPDX-License-Identifier: Apache-2.0
//! A sweep whose path child is a declared `moCompReferenceCurve_c` keeps its circular section.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::profiles::PathRef;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

fn class(bytes: &mut Vec<u8>, name: &str) {
    bytes.extend_from_slice(&[0xff, 0xff, 1, 0]);
    bytes.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(name.as_bytes());
}

/// The wire contact, the path wrapper with its declared component, and the options.
fn path_operand(bytes: &mut Vec<u8>, diameter_m: f64) {
    class(bytes, "moWireContact_c");
    for value in [0.0f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0u32, 1, u32::MAX] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&2u16.to_le_bytes());
    for value in [0.0f64, 0.0, -1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    class(bytes, "moGeneralCurveRef_w");
    class(bytes, "moCompReferenceCurve_c");
    let prefix = bytes.len();
    bytes.extend_from_slice(&[0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes.resize(prefix + 81 + 52 + 33, 0);
    bytes[prefix + 45..prefix + 61].fill(0xff);
    let source = prefix + 81;
    bytes[source..source + 4].copy_from_slice(&11u32.to_le_bytes());
    bytes[source + 4..source + 8].copy_from_slice(&7u32.to_le_bytes());
    for at in [32, 36, 40] {
        bytes[source + at..source + at + 4].copy_from_slice(&(-12345i32).to_le_bytes());
    }
    bytes[source + 48..source + 52].copy_from_slice(&16000u32.to_le_bytes());
    let tail = source + 52;
    bytes[tail + 12..tail + 16].fill(0xff);
    bytes[tail + 24..tail + 32].copy_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&[0; 6]);
    let options = bytes.len();
    bytes.resize(options + 105, 0);
    bytes[options + 86..options + 90].copy_from_slice(&[1, 0, 0, 0]);
    bytes[options + 90..options + 98].copy_from_slice(&diameter_m.to_le_bytes());
}

#[test]
fn declared_reference_curve_path_keeps_the_circular_section() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
        <Feature Name="Curve1" Type="ProfileFeature" id="11"/>
        <Feature Name="Sweep1" Type="Sweep" id="31"/>
    </Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[
        ("moProfileFeature_c", "Curve1", 11),
        ("moSweep_c", "Sweep1", 31),
    ]);
    path_operand(&mut resolved, 0.008);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let sweep = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sweep1"))
        .expect("sweep feature");
    let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) =
        sweep.evaluation.definition()
    else {
        panic!("sweep definition")
    };
    let Some(cadmpeg_ir::features::sweeps::GeneratedSweepSection::CircularRegion { region }) =
        shape.generated_section()
    else {
        panic!("circular section")
    };
    assert_eq!(region.outer_radius().get(), 4.0);
    assert!(
        matches!(path, Some(PathRef::Native(id)) if id.starts_with("sldprt:feature-input:component-reference-curve:"))
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SldprtLossCode::FeatureSweepMergeAssumed.kind()));
}
