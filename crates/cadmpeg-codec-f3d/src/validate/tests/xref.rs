// SPDX-License-Identifier: Apache-2.0
use crate::test_support::{cross_document_reference, local_reference, lp_ascii, lp_utf16};

#[test]
fn placement_occurrence_key_must_match_the_complete_source_frame() {
    const KEY: &str = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    const OTHER: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    const ROLE: &str = "cccccccc-dddd-eeee-ffff-000000000000";
    const MESSAGE: &str = "Fusion XREF placement source does not match its repeated-target frame";
    for matrix in [false, true] {
        let mut frame = Vec::new();
        lp_ascii(&mut frame, "380");
        frame.extend_from_slice(&10_u64.to_le_bytes());
        lp_ascii(&mut frame, "");
        frame.push(1);
        frame.extend_from_slice(&1_u32.to_le_bytes());
        frame.extend(cross_document_reference(100, ROLE));
        frame.extend_from_slice(&1_u32.to_le_bytes());
        frame.push(0);
        frame.extend_from_slice(&27_u32.to_le_bytes());
        lp_utf16(&mut frame, OTHER);
        lp_utf16(&mut frame, KEY);
        frame.push(0);
        frame.extend(cross_document_reference(100, ROLE));
        frame.push(u8::from(!matrix));
        if matrix {
            for value in cadmpeg_ir::transform::Transform::identity()
                .rows()
                .into_iter()
                .flatten()
            {
                frame.extend_from_slice(&value.to_le_bytes());
            }
        }
        frame.extend_from_slice(&0_u32.to_le_bytes());
        lp_utf16(&mut frame, ROLE);
        frame.push(0);
        frame.extend(local_reference(3));

        let reference = serde_json::from_value(serde_json::json!({
            "id":"f3d:xref:reference#0", "ordinal":0,
            "from":"root.f3d", "relative_path":"part.f3d",
            "neutron_role":ROLE, "neutron_data":"",
            "placement_source": {
                "stream":"f3d:Design/BulkStream.dat", "carrier_record_index":10,
                "occurrence_guid":KEY, "frame_bytes":cadmpeg_ir::native::bytes::NativeBytes::from(frame), "owner":{"target":3}
            }
        })).unwrap();
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        crate::test_support::native_test::f3d_native_mut(&mut ir)
            .xref_references
            .push(reference);
        let rejected = |ir: &cadmpeg_ir::CadIr| {
            crate::test_support::with_decode_context(|ctx| {
                super::super::validate_native(ctx, ir)
                    .unwrap()
                    .iter()
                    .any(|finding| {
                        finding.message == MESSAGE
                            && finding.entity.as_deref() == Some("f3d:xref:reference#0")
                    })
            })
        };
        assert!(!rejected(&ir));
        crate::test_support::native_test::f3d_native_mut(&mut ir).xref_references[0]
            .placement_source
            .as_mut()
            .unwrap()
            .carrier_record_index = 11;
        assert!(rejected(&ir));
        {
            let mut native = crate::test_support::native_test::f3d_native_mut(&mut ir);
            let source = native.xref_references[0].placement_source.as_mut().unwrap();
            source.carrier_record_index = 10;
            source.owner.as_mut().unwrap().target = 4;
        }
        assert!(rejected(&ir));
        crate::test_support::native_test::f3d_native_mut(&mut ir).xref_references[0]
            .placement_source
            .as_mut()
            .unwrap()
            .owner
            .as_mut()
            .unwrap()
            .target = 3;
        for key in [Some(OTHER), None] {
            crate::test_support::native_test::f3d_native_mut(&mut ir).xref_references[0]
                .placement_source
                .as_mut()
                .unwrap()
                .occurrence_guid = key.map(|value| value.to_owned().try_into().unwrap());
            assert!(rejected(&ir));
        }
        {
            let mut native = crate::test_support::native_test::f3d_native_mut(&mut ir);
            let source = native.xref_references[0].placement_source.as_mut().unwrap();
            source.occurrence_guid = Some(KEY.to_owned().try_into().unwrap());
            source.frame_bytes.pop();
        }
        assert!(rejected(&ir));
    }
}
