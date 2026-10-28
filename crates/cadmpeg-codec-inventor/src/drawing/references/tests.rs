// SPDX-License-Identifier: Apache-2.0

use super::{parse, Tables};
use cadmpeg_core::decode::View;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn payloads() -> [Vec<u8>; 3] {
    let pairs = vec![0x11; 32];
    let mut revisions = vec![0x22; 16];
    revisions.extend_from_slice(&0_u16.to_le_bytes());
    revisions.extend_from_slice(&1_u16.to_le_bytes());
    let mut external = Vec::new();
    external.extend_from_slice(&0_u32.to_le_bytes());
    external.extend_from_slice(&0_u16.to_le_bytes());
    external.extend_from_slice(&17_u16.to_le_bytes());
    [pairs, revisions, external]
}

fn table() -> Result<Tables, cadmpeg_core::CodecError> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let bytes = payloads();
    let (pairs, revisions, external) = parse(
        &ctx,
        [1; 3],
        bytes.each_ref().map(|bytes| View::over_retained(bytes)),
    )?;
    Ok(Tables {
        id: "inventor:drawing:reference-tables#generated".into(),
        segment_token: cadmpeg_ir::identity_key!("generated"),
        segment_id: [0xaa; 16].into(),
        counts: [1; 3],
        payloads: bytes.map(Into::into),
        pairs,
        revisions,
        external,
    })
}

#[test]
fn fixed_reference_forms_share_decode_and_native_validation() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    let table = table()?;
    assert!(table.valid(&ctx)?);
    assert_eq!(table.pairs.len(), 1);
    assert_eq!(table.revisions.len(), 1);
    assert_eq!(table.external.len(), 1);
    let restored: Tables = serde_json::from_value(serde_json::to_value(&table)?)?;
    assert_eq!(restored, table);
    assert!(restored.valid(&ctx)?);
    let target = table
        .target(crate::pmdc::PmDcReference::from_packed(1))
        .ok_or("external target")?;
    assert_eq!(target.0.segment_id.as_ref(), &[0x11; 16]);
    assert_eq!(target.1.header_id, 17);
    for reference in [0, 2, 0x8000_0001] {
        assert!(table
            .target(crate::pmdc::PmDcReference::from_packed(reference))
            .is_none());
    }
    Ok(())
}

#[test]
fn every_reference_width_rejects_short_long_and_wrong_count_forms() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    for index in 0..3 {
        let valid = table()?;
        for extra in [false, true] {
            let mut invalid = valid.clone();
            if extra {
                invalid.payloads[index].push(0);
            } else {
                invalid.payloads[index].pop();
            }
            assert!(parse(
                &ctx,
                invalid.counts,
                invalid
                    .payloads
                    .each_ref()
                    .map(|bytes| View::over_retained(bytes))
            )
            .is_err());
            assert!(!invalid.valid(&ctx)?);
        }
        for count in [0, 2, u32::MAX] {
            let mut invalid = valid.clone();
            invalid.counts[index] = count;
            assert!(parse(
                &ctx,
                invalid.counts,
                invalid
                    .payloads
                    .each_ref()
                    .map(|bytes| View::over_retained(bytes))
            )
            .is_err());
            assert!(!invalid.valid(&ctx)?);
        }
    }
    Ok(())
}

#[test]
fn unresolved_reference_words_and_counts_remain_exact_native_fields() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    for word in [0x0001_0000_u32, 0x0002_0000, u32::MAX] {
        let mut bytes = payloads();
        bytes[2][..4].copy_from_slice(&word.to_le_bytes());
        let (pairs, revisions, external) = parse(
            &ctx,
            [1; 3],
            bytes.each_ref().map(|bytes| View::over_retained(bytes)),
        )?;
        let mut table = table()?;
        table.payloads = bytes.map(Into::into);
        table.pairs = pairs;
        table.revisions = revisions;
        table.external = external;
        assert_eq!(table.external[0].revision_word, word);
        assert!(table.valid(&ctx)?);
        assert!(table
            .target(crate::pmdc::PmDcReference::from_packed(1))
            .is_none());
    }
    let mut invalid = table()?;
    invalid.external[0].key_extension = 1;
    assert!(!invalid.valid(&ctx)?);
    invalid = table()?;
    invalid.revisions[0].reference_count = 2;
    assert!(!invalid.valid(&ctx)?);
    invalid = table()?;
    invalid.pairs[0].segment_id = [0x33; 16].into();
    assert!(!invalid.valid(&ctx)?);
    Ok(())
}

fn cache_record() -> Result<crate::drawing::Record, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "id": "inventor:drawing:record#generated-0", "segment_token": "generated", "record_ordinal": 0,
        "segment_version_major": 19, "type_id": "81136d8ad111566b600038bd861c3cb0", "family": "DlSheetSmSegmentType", "payload": "",
        "fields": {"form": "cached_view", "prefix": "00".repeat(28), "view": crate::pmdc::PmDcReference::from_packed(1), "tail": {"offset": 32, "len": 0, "sha256": cadmpeg_ir::hash::sha256_hex(&[])}}
    }))
}

fn view_record() -> Result<crate::drawing::Record, serde_json::Error> {
    let mut prefix = [0_u8; 30];
    prefix[4..6].copy_from_slice(&17_u16.to_le_bytes());
    serde_json::from_value(serde_json::json!({
        "id": "inventor:drawing:record#target-0", "segment_token": "target", "record_ordinal": 0,
        "segment_version_major": 19, "type_id": "5942cb22d111c557600037bd861c3cb0", "family": "DlSheetDcSegmentType", "payload": "",
        "fields": {"form": "view", "prefix": cadmpeg_ir::native::bytes::NativeBytes::new(prefix), "operands": {"marker": 2, "metadata": null, "references": []}, "word": 0, "values": [0, 0], "extension": null, "text": "Generated", "tail": {"offset": 0, "len": 0, "sha256": cadmpeg_ir::hash::sha256_hex(&[])}}
    }))
}

#[test]
fn cached_view_join_requires_one_zero_scope_zero_extension_view_target() -> TestResult {
    use super::record_links_are_valid;
    let source = table()?;
    let mut target = source.clone();
    target.id = "inventor:drawing:reference-tables#target".into();
    target.segment_token = cadmpeg_ir::identity_key!("target");
    target.segment_id = [0x11; 16].into();
    let cache = cache_record()?;
    let view = view_record()?;
    let tables = [source, target];
    assert!(record_links_are_valid(
        &cache,
        &tables,
        std::slice::from_ref(&view)
    ));
    assert!(!record_links_are_valid(
        &cache,
        &[],
        std::slice::from_ref(&view)
    ));
    assert!(!record_links_are_valid(&cache, &tables, &[]));
    assert!(!record_links_are_valid(
        &cache,
        &tables,
        &[view.clone(), view.clone()]
    ));
    assert!(!record_links_are_valid(
        &cache,
        &[tables[0].clone(), tables[0].clone(), tables[1].clone()],
        std::slice::from_ref(&view)
    ));
    assert!(!record_links_are_valid(
        &cache,
        &[tables[0].clone(), tables[1].clone(), tables[1].clone()],
        std::slice::from_ref(&view)
    ));
    for scope in [1_u32, 2] {
        let mut invalid = tables.clone();
        invalid[0].external[0].revision_word = scope << 16;
        assert!(!record_links_are_valid(
            &cache,
            &invalid,
            std::slice::from_ref(&view)
        ));
    }
    let mut invalid = tables.clone();
    invalid[0].external[0].key_extension = 1;
    assert!(!record_links_are_valid(
        &cache,
        &invalid,
        std::slice::from_ref(&view)
    ));
    invalid = tables.clone();
    invalid[0].revisions[0].segment_pair_index = 1;
    assert!(!record_links_are_valid(
        &cache,
        &invalid,
        std::slice::from_ref(&view)
    ));
    for offset in [0, 4] {
        let mut wrong_header = view.clone();
        let crate::drawing::records::Fields::View { prefix, .. } = &mut wrong_header.fields else {
            return Err("view fields".into());
        };
        prefix[offset] = prefix[offset].wrapping_add(1);
        assert!(!record_links_are_valid(&cache, &tables, &[wrong_header]));
    }
    let mut wrong_type = view.clone();
    wrong_type.type_id = crate::record_identity::RecordTypeId::from_bytes([0x11; 16]);
    assert!(!record_links_are_valid(&cache, &tables, &[wrong_type]));
    Ok(())
}

#[test]
fn sheet_view_lists_require_unique_local_targets_of_the_exact_class() -> TestResult {
    use super::record_links_are_valid;
    use crate::drawing::records::Fields;
    use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};
    let list = || {
        PmDcReferenceList::new(
            2,
            Some(PmDcListMetadata::U32([0; 2])),
            vec![PmDcReference::from_packed(0x8000_0001)],
        )
        .ok_or("reference list")
    };
    let empty = || PmDcReferenceList::new(2, None, Vec::new()).ok_or("empty list");
    let mut target = view_record()?;
    let mut sheet = target.clone();
    sheet.id = "inventor:drawing:record#target-1".into();
    sheet.ordinal = 1;
    sheet.type_id = crate::record_identity::RecordTypeId::from_bytes([
        0xcd, 0x90, 0x7c, 0xd3, 0xd0, 0x11, 0x16, 0xfa, 0x60, 0x00, 0x0d, 0xbd, 0x86, 0x1c, 0x3c,
        0xb0,
    ]);
    sheet.fields = Fields::Sheet {
        header: [0; 14].into(),
        identifier: [0; 16].into(),
        label: "Generated".into(),
        properties: empty()?,
        controls: [0; 24].into(),
        views: list()?,
        tail_prefix: [0; 14].into(),
        auxiliary_lists: Box::new([empty()?, empty()?, empty()?]),
        terminal: [0; 8].into(),
        tail: serde_json::from_value(serde_json::json!({
            "offset": 0, "len": 0, "sha256": cadmpeg_ir::hash::sha256_hex(&[])
        }))?,
    };
    assert!(record_links_are_valid(
        &sheet,
        &[],
        std::slice::from_ref(&target)
    ));
    assert!(!record_links_are_valid(&sheet, &[], &[]));
    assert!(!record_links_are_valid(
        &sheet,
        &[],
        &[target.clone(), target.clone()]
    ));
    target.segment_token = cadmpeg_ir::identity_key!("other");
    assert!(!record_links_are_valid(
        &sheet,
        &[],
        std::slice::from_ref(&target)
    ));
    target = view_record()?;
    target.type_id = crate::record_identity::RecordTypeId::from_bytes([0x11; 16]);
    assert!(!record_links_are_valid(
        &sheet,
        &[],
        std::slice::from_ref(&target)
    ));
    if let Fields::Sheet {
        views,
        auxiliary_lists,
        ..
    } = &mut sheet.fields
    {
        *views = empty()?;
        auxiliary_lists[2] = list()?;
    }
    target = view_record()?;
    assert!(!record_links_are_valid(
        &sheet,
        &[],
        std::slice::from_ref(&target)
    ));
    target.type_id = crate::record_identity::RecordTypeId::from_bytes([
        0xc1, 0x96, 0x59, 0x62, 0xd4, 0x11, 0x4e, 0x8e, 0x60, 0x00, 0x95, 0x87, 0x3f, 0xa6, 0xfb,
        0xb0,
    ]);
    assert!(record_links_are_valid(
        &sheet,
        &[],
        std::slice::from_ref(&target)
    ));
    if let Fields::Sheet {
        auxiliary_lists, ..
    } = &mut sheet.fields
    {
        auxiliary_lists[2] = PmDcReferenceList::new(
            2,
            Some(PmDcListMetadata::U32([0; 2])),
            vec![PmDcReference::from_packed(1)],
        )
        .ok_or("unqualified list")?;
    }
    assert!(!record_links_are_valid(&sheet, &[], &[target]));
    Ok(())
}
