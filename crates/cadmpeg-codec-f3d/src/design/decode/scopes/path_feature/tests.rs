// SPDX-License-Identifier: Apache-2.0
use super::exact_path_feature_construction;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;

#[test]
fn compact_loft_prefix_reads_operation_at_offset_25_for_any_dynamic_class_tag() {
    for class_tag in ["301", "449"] {
        let mut bytes = Vec::new();
        let class_tag_bytes = class_tag
            .as_bytes()
            .try_into()
            .expect("three-byte class tag");
        indexed_header(&mut bytes, class_tag_bytes, 20);
        bytes.resize(128, 0);
        bytes[21..25].fill(1);
        bytes[25..29].copy_from_slice(&1u32.to_le_bytes());
        bytes[30..34].fill(0xff);

        let mut scope = DesignParameterScope::empty(
            "generated:loft#20",
            crate::records::feature::scope::DesignFeatureKind::Loft,
            20,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = 128;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let construction = exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[],
        )
        .expect("compact Loft operation");
        assert_eq!(
            construction,
            DesignPathFeatureConstruction::Loft(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation: DesignExtrudeOperation::Join,
                    operation_offset: Some(25),
                }
            )
        );

        bytes[24] = 0;
        assert_eq!(
            exact_path_feature_construction(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
                &[],
            ),
            None
        );
    }
}

#[test]
fn pipe_prefixes_keep_independent_hollow_and_opaque_fields() {
    use crate::records::feature::extrude::DesignExtrudeOperation;
    let controls: &[(&str, bool, DesignExtrudeOperation, usize)] = &[
        (
            "0300000033343011020000000000000000000000010000000001040000000101",
            true,
            DesignExtrudeOperation::NewBody,
            26,
        ),
        (
            "0300000032383607010000000000000000000000010000000001040000000101",
            true,
            DesignExtrudeOperation::NewBody,
            26,
        ),
        (
            "030000003338377a020000000000000000000000000004000000010101850200",
            false,
            DesignExtrudeOperation::NewBody,
            22,
        ),
        (
            "03000000333837bf020000000000000000000000000001000000010101d10200",
            false,
            DesignExtrudeOperation::Join,
            22,
        ),
    ];
    for (encoded, hollow, operation, operation_at) in controls {
        let raw: cadmpeg_ir::native::bytes::NativeBytes<Vec<u8>> =
            serde_json::from_value(serde_json::json!(encoded)).unwrap();
        let mut bytes = vec![0; 17];
        bytes.extend_from_slice(&raw);
        let actual = super::exact_pipe_prefix(&bytes, 17).unwrap();
        assert_eq!(actual.operation, *operation);
        assert_eq!(
            actual.operation_offset,
            u64::try_from(17 + operation_at).unwrap()
        );
        assert_eq!(actual.is_hollow, *hollow);
        assert_eq!(
            actual.is_hollow_offset,
            u64::try_from(16 + operation_at).unwrap()
        );
        assert_eq!(actual.opaque_post_operation, [1, 1]);
        assert_eq!(
            actual.opaque_post_operation_offset,
            u64::try_from(21 + operation_at).unwrap()
        );
        let mut altered = bytes.clone();
        altered[17 + operation_at + 4..17 + operation_at + 6].copy_from_slice(&[0, 255]);
        assert_eq!(
            super::exact_pipe_prefix(&altered, 17)
                .unwrap()
                .opaque_post_operation,
            [0, 255]
        );
        for (at, value) in [(17 + operation_at - 1, 2), (28, 1), (17 + operation_at, 0)] {
            let mut invalid = bytes.clone();
            invalid[at] = value;
            assert!(super::exact_pipe_prefix(&invalid, 17).is_none());
        }
        assert!(super::exact_pipe_prefix(&bytes[..17 + operation_at + 5], 17).is_none());
    }
    let mut unobserved = vec![0; 31];
    unobserved[25..29].copy_from_slice(&4u32.to_le_bytes());
    unobserved[29..31].copy_from_slice(&[1, 1]);
    assert!(super::exact_pipe_prefix(&unobserved, 0).is_none());
}
