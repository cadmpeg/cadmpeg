// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn operation_source_properties_require_unique_owned_structures() {
    let record = crate::native::features::operation_record::FeatureOperationRecord {
        id: "record".into(),
        operation_label: "operation".into(),
        ordinal: 3,
        sha256: crate::native::hex::Sha256Hex::digest(b"record-hash"),
        payload_sha256: crate::native::hex::Sha256Hex::digest(b"payload-hash"),
        stable_identity: None,
        span: crate::native::features::operation_record::OperationRecordSpan::new(100, 110, 10)
            .unwrap(),
    };
    let common = crate::native::features::FeatureOperationCommonFrame {
        id: "common".into(),
        operation_record: record.id.clone(),
        ordinal: 0,
        frame: crate::om::common_frame::CommonFrame::<u64, Option<String>>::new(
            crate::om::common_frame::CommonFramePrefix::from_wire(
                [0, 351, 171],
                &[vec![0], vec![0x81, 0x5f], vec![0x80, 0xab]],
                [1, 3, 2],
            )
            .unwrap(),
            [1, 2, 1, 1, 1, 0, 0, 0],
            crate::om::common_frame::CommonFrameSuffix::from_wire(41, &[0x29], Some(65), &[0x41])
                .unwrap()
                .with_target(None)
                .unwrap(),
            101,
        )
        .unwrap(),
    };
    let frame = crate::native::features::FeatureOperationTerminalFrame {
        id: "frame".into(),
        operation_record: record.id.clone(),
        immediate_common_frame: Some(common.id.clone()),
        frame: crate::om::common_frame::TerminalFrame::<u64, Option<String>>::new(
            common.frame.suffix().clone(),
            117,
        )
        .unwrap(),
    };
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            std::slice::from_ref(&common),
            std::slice::from_ref(&frame),
        ),
        BTreeMap::from([
            ("operation_common_frame.0".into(), "common".into()),
            ("operation_record".into(), "record".into()),
            ("operation_terminal_frame".into(), "frame".into()),
        ])
    );
    assert!(super::operation_source_properties("missing", &[], &[], &[]).is_empty());
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            &[],
            &[],
        ),
        BTreeMap::from([("operation_record".into(), "record".into())])
    );
    let mut noncontiguous_common = common.clone();
    noncontiguous_common.ordinal = 1;
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            std::slice::from_ref(&noncontiguous_common),
            std::slice::from_ref(&frame),
        ),
        BTreeMap::from([
            ("operation_record".into(), "record".into()),
            ("operation_terminal_frame".into(), "frame".into()),
        ])
    );
    assert!(super::operation_source_properties(
        &record.operation_label,
        &[record.clone(), record.clone()],
        std::slice::from_ref(&common),
        std::slice::from_ref(&frame),
    )
    .is_empty());
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            &[],
            &[frame.clone(), frame],
        ),
        BTreeMap::from([("operation_record".into(), "record".into())])
    );
}
