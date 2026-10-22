// SPDX-License-Identifier: Apache-2.0
//! Native-only detail survives source-less writes without overriding canonical controls.
#![allow(clippy::unwrap_used)]

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::{sldprt_native, update_sldprt_native};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::scalar::PositiveLength;
use std::io::Cursor;

#[test]
fn source_less_native_algorithm_edit_and_deletion_preserve_canonical_radius_and_edges() {
    for algorithm in [Some("FaceBlend"), None] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Fillet Name="Custom blend" Type="CustomFillet" id="7" Edges="edge:1" Algorithm="RollingBall"><Dimension Name="Radius">2mm</Dimension></Fillet></Keywords>"#));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
        let owner = decoded.ir().model.features[0].id.clone();
        decoded
            .ir_mut()
            .set_dimension(
                &owner,
                "/groups/0/radius/radius",
                PositiveLength::new(2.5).unwrap(),
            )
            .unwrap();
        update_sldprt_native(&mut decoded.ir_mut(), |native| {
            let record = &mut native.feature_histories[0].features[0];
            record.properties.remove("Algorithm");
            if let Some(algorithm) = algorithm {
                record.properties.insert(
                    cadmpeg_core::nonblank_literal!("Algorithm"),
                    algorithm.into(),
                );
            }
            record.properties.insert(
                cadmpeg_core::nonblank_literal!("Edges"),
                "edge:stale-copy".into(),
            );
            record
                .parameters
                .insert(cadmpeg_core::nonblank_literal!("Radius"), "99mm".into());
        });
        // This legacy projection copy is deliberately stale. The typed native
        // home owns Algorithm, including its explicit deletion, under W1/W2.
        assert_eq!(
            decoded.ir().model.features[0].source_properties["Algorithm"],
            "RollingBall"
        );
        let plan = SldprtCodec
            .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
            .unwrap();
        assert!(matches!(
            plan.report().write_path(),
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(written.ir());
        let record = &native.feature_histories[0].features[0];
        assert_eq!(record.kind, "CustomFillet");
        assert_eq!(
            record.properties.get("Algorithm").map(String::as_str),
            algorithm
        );
        assert_eq!(record.properties["Edges"], "edge:1");
        assert_eq!(record.parameters["Radius"], "2.5mm");
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
            written.ir().model.features[0].evaluation.definition()
        else {
            panic!("fillet must remain typed");
        };
        let cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant { radius } =
            &groups[0].radius
        else {
            panic!("constant radius");
        };
        assert_eq!(written.ir().dimension_value(radius).unwrap().get(), 2.5);
    }
}
