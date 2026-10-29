// SPDX-License-Identifier: Apache-2.0
//! Exact revision fields, unused payload bytes, and source rollback.

use crate::container::InventorContainer;
use crate::native::{DatabaseRecord, RevisionPayloadForm, RevisionRecord};
use crate::rse::ParsedState;
use crate::test_support::given_up_records;
use crate::test_support::test_fixtures::primary_envelope_fixture_with_revisions;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::CodecFormat;

fn plain() -> Vec<u8> {
    let mut record = vec![0x61; 16];
    record.extend(0x0102_0304_u32.to_le_bytes());
    record.extend(42_u16.to_le_bytes());
    record
}

fn revisions(records: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend(u32::try_from(records.len()).unwrap().to_le_bytes());
    for record in records {
        bytes.extend(record);
    }
    bytes
}

fn short() -> Vec<u8> {
    let mut record = vec![0x62; 16];
    record.extend(0x1122_3344_u32.to_le_bytes());
    record.extend(u16::MAX.to_le_bytes());
    record.push(7);
    record.extend([0x77; 8]);
    record
}

#[test]
fn actual_revision_records_keep_exact_native_fields_and_source_read_ranges() {
    let bytes = primary_envelope_fixture_with_revisions(&revisions(&[plain(), short()]));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let source = container.rse.revision_source.unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert_eq!(super::stage(&ctx, &mut output, source).unwrap(), 2);
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(given_up_records(&fidelity).is_empty());
    let records = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<RevisionRecord>("revisions")
        .unwrap();
    assert_eq!(
        records,
        [
            RevisionRecord {
                id: "inventor:rse:revision#0".into(),
                ordinal: 0,
                revision_id: "61616161616161616161616161616161".into(),
                flags: 0x0102_0304,
                kind: 42,
                payload_form: RevisionPayloadForm::None,
            },
            RevisionRecord {
                id: "inventor:rse:revision#1".into(),
                ordinal: 1,
                revision_id: "62626262626262626262626262626262".into(),
                flags: 0x1122_3344,
                kind: u16::MAX,
                payload_form: RevisionPayloadForm::Short,
            }
        ]
    );
    assert!(losses.is_empty());
}

fn decoded_revisions(bytes: &[u8]) -> Vec<RevisionRecord> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    super::stage(&ctx, &mut output, container.rse.revision_source.unwrap()).unwrap();
    let (_, native, _, losses) = output.into_parts();
    assert!(losses.is_empty());
    native
        .namespace("inventor")
        .unwrap()
        .arena_as::<RevisionRecord>("revisions")
        .unwrap()
}

#[test]
fn discarded_revision_payload_bytes_do_not_claim_content_but_flags_and_selector_do() {
    let original = revisions(&[short()]);
    let baseline = decoded_revisions(&primary_envelope_fixture_with_revisions(&original));
    let mut changed = original.clone();
    changed[31..39].fill(0xab);
    assert_eq!(
        decoded_revisions(&primary_envelope_fixture_with_revisions(&changed)),
        baseline
    );
    changed[24] ^= 1;
    let flags = decoded_revisions(&primary_envelope_fixture_with_revisions(&changed));
    assert_eq!(flags[0].flags, 0x1122_3345);
    assert_eq!(flags[0].payload_form, RevisionPayloadForm::Short);
    let mut long = original;
    long[30] = 0;
    long.extend([0x88; 8]);
    let long = decoded_revisions(&primary_envelope_fixture_with_revisions(&long));
    assert_eq!(long[0].payload_form, RevisionPayloadForm::Long);
    assert_eq!(long[0].revision_id, baseline[0].revision_id);
    assert_eq!(long[0].flags, baseline[0].flags);
}

#[test]
fn late_revision_stream_failure_discards_formed_records_and_keeps_exact_database_peer() {
    let mut table = revisions(&[plain(), short()]);
    table.push(0xff);
    let bytes = primary_envelope_fixture_with_revisions(&table);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    assert!(matches!(
        container.rse.revisions,
        ParsedState::Unavailable(_)
    ));
    let source = container.rse.revision_source.unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(
        super::super::database_parts::stage(&ctx, &mut output, &container.rse.databases[0])
            .unwrap()
    );
    assert_eq!(super::stage(&ctx, &mut output, source).unwrap(), 0);
    let (_, native, fidelity, losses) = output.into_parts();
    let ns = native.namespace("inventor").unwrap();
    assert!(ns
        .arena_as::<RevisionRecord>("revisions")
        .unwrap()
        .is_empty());
    let peer = ns.arena_as::<DatabaseRecord>("databases").unwrap();
    assert_eq!(peer.len(), 1);
    assert_eq!(peer[0].id, "inventor:rse:database#v1");
    assert_eq!(peer[0].schema, 31);
    assert_eq!(peer[0].note, "synthetic primary document");
    assert!(!ns.arenas().contains_key("unknowns"));
    let given_up = given_up_records(&fidelity);
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].data(), Some(table.as_slice()));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(!losses[0].message.contains("inventor:rse:database#v1"));
}

#[test]
fn actual_revision_native_entity_refusal_stays_a_fatal_resource_limit() {
    let bytes = primary_envelope_fixture_with_revisions(&revisions(&[plain()]));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The primary fixture has one metadata stream with one type descriptor;
    // that descriptor forms one `meta_types` record, so
    // the open admits exactly one entity and the revision stage admits none.
    policy.limits.max_entities = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let result = super::stage(&ctx, &mut output, container.rse.revision_source.unwrap());
    let Err(CodecError::ResourceLimit(expected)) = result else {
        panic!("native insertion must refuse actual entity budget")
    };
    assert_eq!(expected.dimension, ResourceDimension::Entities);
    assert!(output.native().namespace("inventor").is_none());
}
