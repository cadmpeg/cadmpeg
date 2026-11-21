// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn anonymous_metadata_replays_and_only_a_real_named_owner_is_patched() {
    for name in ["", "block@8"] {
        let mut payload = b"moBBoxCenterData_c".to_vec();
        payload.extend(1u32.to_le_bytes());
        for value in [0.01f64, 0.02, -0.03, 0.04] {
            payload.extend(value.to_le_bytes());
        }
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(0x43, name, &payload));
        let source = if name.is_empty() {
            source
        } else {
            native_envelope_fixture(&source)
        };
        let scan = crate::test_support::container::scan(&source);
        let owner = scan
            .blocks
            .iter()
            .find(|block| block.payload == payload)
            .unwrap();
        assert_eq!(owner.section.name(), (!name.is_empty()).then_some(name));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
            .unwrap();
        let mut decoded = EditableDecodeResult::from(decoded);
        let mut replay = Vec::new();
        let path = crate::test_support::plan_inherited_write(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut replay,
        )
        .unwrap();
        assert!(matches!(
            path,
            cadmpeg_ir::report::export::WritePath::VerbatimReplay { .. }
        ));
        assert_eq!(replay, source);
        {
            let mut ir = decoded.ir_mut();
            let bbox = ir
                .model
                .attributes
                .iter_mut()
                .find(|attribute| attribute.name == "bounding_envelope")
                .unwrap();
            let Some(AttributeValue::Vector(values)) = &mut bbox.values[0] else {
                panic!("bbox")
            };
            values[3] = cadmpeg_ir::scalar::FiniteReal::new(41.0).unwrap();
        }
        let (written, losses) = crate::test_support::write_with_unwritten_attributes(
            decoded.ir(),
            decoded.source_fidelity(),
        )
        .unwrap();
        if name.is_empty() {
            let scan = crate::test_support::container::scan(&written);
            assert!(scan.blocks.iter().any(|block| block.payload == payload));
            let bbox = decoded
                .ir()
                .model
                .attributes
                .iter()
                .find(|attribute| attribute.name == "bounding_envelope")
                .unwrap()
                .id
                .as_str()
                .to_owned();
            losses.assert_charged(
                crate::writer::Held::Source,
                "does not patch: one of several, unnamed",
                &bbox,
            );
        } else {
            let scan = crate::test_support::container::scan(&written);
            let owned: Vec<_> = scan
                .blocks
                .iter()
                .filter(|block| block.section.name() == Some(name))
                .collect();
            assert_eq!(owned.len(), 1);
            let mut expected = payload;
            let radius = expected.len() - 8;
            expected[radius..].copy_from_slice(&0.041f64.to_le_bytes());
            assert_eq!(owned[0].payload, expected);
        }
    }
}
