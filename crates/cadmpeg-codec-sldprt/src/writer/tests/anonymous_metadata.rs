// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn anonymous_metadata_replays_but_only_a_real_named_owner_can_be_edited() {
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
        // The member has no typed stream owner, so the source keeps an
        // unrepresented class and replay and patch are ineligible (#209 §1.4).
        assert!(!decoded.source_fidelity().unrepresented().is_empty());
        let mut replay = Vec::new();
        let path = crate::test_support::plan_inherited_write(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut replay,
        )
        .unwrap();
        assert!(matches!(
            path,
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        {
            let mut ir = decoded.ir_mut();
            let bbox = ir
                .model
                .attributes
                .iter_mut()
                .find(|attribute| attribute.name == "bounding_envelope")
                .unwrap();
            let AttributeValue::Vector(values) = &mut bbox.values[0] else {
                panic!("bbox")
            };
            values[3] = cadmpeg_ir::scalar::FiniteReal::new(41.0).unwrap();
        }
        let mut written = Vec::new();
        let path = crate::test_support::plan_inherited_write(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut written,
        )
        .unwrap();
        assert!(matches!(
            path,
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        let readback = SldprtCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap();
        let bbox = readback
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "bounding_envelope")
            .unwrap();
        let AttributeValue::Vector(values) = &bbox.values[0] else {
            panic!("bbox")
        };
        assert_eq!(values[3].get(), 41.0);
    }
}
