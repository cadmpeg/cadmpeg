// SPDX-License-Identifier: Apache-2.0

fn companion_findings(
    byte_offset: u64,
    timestamp_offset: u64,
    payload_offset: Option<u64>,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|decode| {
        use crate::records::decal::DesignRecordHeader;
        use crate::records::parameters::{
            DesignCompanionPayload, DesignParameterCompanion, DesignParameterOwner,
            DesignParameterOwnerWire,
        };
        let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:owner#6".into(),
            byte_offset: 0,
            frame_length: 99,
            class_tag: "268".to_owned().try_into().unwrap(),
            record_index: 6,
            scope_record_index: 4,
            local_ordinal: 0,
            evaluated_value: 1.0,
            evaluated_value_offset: 40,
            parameter_record_index: 7,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 8,
        })
        .unwrap();
        let mut companion = DesignParameterCompanion::unbound(
            "f3d:Design/BulkStream.dat:companion#8".into(),
            byte_offset,
            "258".to_owned().try_into().unwrap(),
            8,
            6,
            std::num::NonZeroU64::new(1).unwrap(),
            timestamp_offset,
        );
        if let Some(offset) = payload_offset {
            companion = companion.bound(DesignCompanionPayload::new(offset, 0, Vec::new()));
        }
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:header#8".into(),
            byte_offset,
            class_tag: "258".to_owned().try_into().unwrap(),
            record_index: 8,
        };
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_parameter_companions.push(companion);
        let mut ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        let stream = super::super::design_stream(native.design_parameter_companions[0].id());
        ctx.owners_by_index.insert((stream, 6), &owner);
        ctx.records_by_index.insert((stream, 8), &header);
        let mut findings = Vec::new();
        super::super::validate_parameter_companions(&ctx, &mut findings).unwrap();
        findings
    })
}

#[test]
fn companion_rejects_overflowed_timestamp_origin() {
    let findings = companion_findings(u64::MAX - 41, u64::MAX, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Fusion Design parameter companion has an invalid prefix or owner link"
    );
}

#[test]
fn companion_rejects_overflowed_payload_origin() {
    let findings = companion_findings(u64::MAX - 57, u64::MAX - 15, Some(u64::MAX));
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Fusion Design parameter companion has an invalid prefix or owner link"
    );
}

#[test]
fn companion_accepts_representable_prefix() {
    assert!(companion_findings(100, 142, None).is_empty());
    assert!(companion_findings(100, 142, Some(158)).is_empty());
}
