// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn replace_face_writer_keeps_stored_tangent_chain_settings_it_cannot_encode() {
    for tangent_chain in [None, Some(false), Some(true)] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><ReplaceFace Name="Patch" Type="ReplaceFace" id="21" Faces="face:4,face:5" ReplacementFaces="face:8"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                    &source,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let mut decoded = EditableDecodeResult::from(decoded);
        decoded.ir_mut().model.features[0]
            .evaluation
            .edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) =
                    definition
                else {
                    panic!("typed face replacement");
                };
                *operands = operands.clone().with_tangent_chain(tangent_chain);
            });
        let (unwritten, _) =
            crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());
        assert_eq!(
            unwritten.is_empty(),
            tangent_chain.is_none(),
            "{unwritten:?}"
        );
    }
}
