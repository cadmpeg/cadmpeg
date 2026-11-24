use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn semantic_writer_keeps_stored_partial_or_controlled_axes_and_names_the_edit() {
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::options::NativeOption;
    use cadmpeg_ir::features::{datums::DatumAxisDraft, FeatureDefinition, FeatureOperation};
    for case in 0..5 {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(0x42,"Contents/Keywords",br#"<Keywords><ReferenceAxis Name="Axis" Type="ReferenceAxis" id="26" Origin="1mm,2mm,3mm" Direction="0,0,1"/></Keywords>"#));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut decoded = EditableDecodeResult::from(decoded);
        {
            let mut ir = decoded.ir_mut();
            let feature = &mut ir.model.features[0];
            let FeatureOperation::DatumAxis { axis } = feature.evaluation.definition().operation()
            else {
                panic!("datum axis")
            };
            let mut draft = DatumAxisDraft::from(axis.clone());
            match case {
                0 => draft.direction = None,
                1 => draft.origin = None,
                2 => {
                    draft.native_options.insert(
                        cadmpeg_ir::codec_name!("sldprt:unknown"),
                        NativeOption::Bool(Dimension::Literal(false)),
                    );
                }
                3 => {
                    draft.source_settings.insert(
                        cadmpeg_ir::codec_name!("sldprt:setting"),
                        NativeOption::Bool(Dimension::Literal(false)),
                    );
                }
                _ => draft
                    .inputs
                    .push(cadmpeg_ir::design::construction::ConstructionInput {
                        role: None,
                        reference: None,
                        value: None,
                        reversed: None,
                    }),
            }
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(
                    draft.into_operation().unwrap(),
                ));
        }
        let unwritten =
            crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());
        assert!(
            unwritten.iter().any(|message| message
                .contains("unsupported datum-axis construction or partial geometry")),
            "{unwritten:?}"
        );
    }
}
