// SPDX-License-Identifier: Apache-2.0

use super::validate_native;
use crate::drawing::MetadataReference;
use crate::native::MetaSectionRecord;
use crate::records::MetaSectionNumber;
use cadmpeg_core::decode::View;
use cadmpeg_ir::native::NativeNamespace;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn reference(ordinal: u32, tag: u8) -> Result<MetadataReference, cadmpeg_core::CodecError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&7_u16.to_le_bytes());
    payload.push(tag);
    payload.extend_from_slice(if tag == 3 { &[0; 8] } else { &[0; 16] });
    let entry = crate::records::references::Entry::read(&mut View::over_retained(&payload))?;
    Ok(MetadataReference {
        id: format!("inventor:drawing:metadata-reference#generated-{ordinal}"),
        segment_token: cadmpeg_ir::identity_key!("generated"),
        ordinal,
        payload: payload.into(),
        entry,
    })
}

fn section(records: &[MetadataReference]) -> Result<MetaSectionRecord, std::num::TryFromIntError> {
    let bytes = records
        .iter()
        .flat_map(|record| record.payload.iter().copied())
        .collect::<Vec<_>>();
    Ok(MetaSectionRecord {
        id: "inventor:rse:meta-section#generated-9".into(),
        token: "generated".into(),
        number: MetaSectionNumber::Nine,
        discriminator: u32::try_from(records.len())?,
        payload_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
        payload_sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
    })
}

fn reference_findings(
    records: &[MetadataReference],
    section: &MetaSectionRecord,
) -> Result<usize, cadmpeg_core::CodecError> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut namespace = NativeNamespace::default();
    namespace.set_arena(&ctx, "drawing_metadata_references", records)?;
    let mut findings = Vec::new();
    validate_native(
        &ctx,
        &namespace,
        &[],
        &[],
        &[],
        std::slice::from_ref(section),
        &mut findings,
    )?;
    Ok(findings.len())
}

#[test]
fn every_metadata_reference_form_validates_against_its_section_bytes() -> TestResult {
    for tag in [1, 2, 3] {
        let records = [reference(0, tag)?];
        assert!(records[0].valid()?);
        assert_eq!(reference_findings(&records, &section(&records)?)?, 0);
        let source = section(&records)?;
        let mut changed = records.clone();
        changed[0].entry.key = 8;
        assert!(reference_findings(&changed, &source)? > 0);
        changed = records.clone();
        changed[0].payload[0] = 8;
        changed[0].entry =
            crate::records::references::Entry::read(&mut View::over_retained(&changed[0].payload))?;
        assert!(changed[0].valid()?);
        assert!(reference_findings(&changed, &source)? > 0);
    }
    Ok(())
}

#[test]
fn metadata_reference_ordinals_counts_and_lengths_must_close() -> TestResult {
    let records = [reference(0, 1)?, reference(1, 3)?];
    let source = section(&records)?;
    assert_eq!(
        reference_findings(&[records[1].clone(), records[0].clone()], &source)?,
        0
    );
    let mut changed = records.clone();
    changed[1].ordinal = 0;
    changed[1].id = changed[0].id.clone();
    assert!(reference_findings(&changed, &source)? > 0);
    assert!(reference_findings(&records[..1], &source)? > 0);
    let mut wrong = source.clone();
    wrong.payload_len += 1;
    assert!(reference_findings(&records, &wrong)? > 0);
    Ok(())
}

#[test]
fn drawing_family_validation_requires_one_registry_identity_and_exact_type() {
    use super::family_for_segment;
    use crate::drawing::Family;
    use crate::native::SegmentRegistryRecord;
    let registry = SegmentRegistryRecord {
        id: "inventor:rse:registry#0".into(),
        ordinal: 0,
        display_name: "Generated".into(),
        segment_id: "11".repeat(16),
        revision_id: "22".repeat(16),
        type_name: Family::Sheet.type_name().into(),
        object_count: 1,
        node_count: 0,
    };
    let segment_id = registry.segment_id.clone();
    assert_eq!(
        family_for_segment(
            &segment_id,
            Family::Sheet.type_name(),
            std::slice::from_ref(&registry)
        ),
        Some(Family::Sheet)
    );
    assert!(family_for_segment(
        &segment_id,
        Family::Browser.type_name(),
        std::slice::from_ref(&registry)
    )
    .is_none());
    assert!(family_for_segment(&segment_id, Family::Sheet.type_name(), &[]).is_none());
    assert!(family_for_segment(
        &segment_id,
        Family::Sheet.type_name(),
        &[registry.clone(), registry.clone()]
    )
    .is_none());
    let mut invalid = registry;
    invalid.type_name = "DlSheetDc".into();
    assert!(family_for_segment(&segment_id, Family::Sheet.type_name(), &[invalid]).is_none());
}

#[test]
fn drawing_refusal_records_require_unique_identity_scope_and_detail() -> TestResult {
    use crate::native::StructuralIssueRecord;
    let ctx = cadmpeg_test_support::service_decode_context();
    let valid = StructuralIssueRecord {
        id: "inventor:drawing:record-issue#generated-0".into(),
        scope: "generated-0".into(),
        detail: "unsupported prefix".into(),
    };
    let mut inputs = vec![vec![valid.clone()]];
    for field in ["id", "scope", "detail"] {
        let mut invalid = valid.clone();
        match field {
            "id" => invalid.id.push('x'),
            "scope" => invalid.scope.clear(),
            "detail" => invalid.detail.clear(),
            _ => unreachable!(),
        }
        inputs.push(vec![invalid]);
    }
    inputs.push(vec![valid.clone(), valid]);
    for (index, issues) in inputs.into_iter().enumerate() {
        let mut namespace = NativeNamespace::default();
        namespace.set_arena(&ctx, "drawing_record_issues", &issues)?;
        let mut findings = Vec::new();
        validate_native(&ctx, &namespace, &[], &[], &[], &[], &mut findings)?;
        assert_eq!(findings.is_empty(), index == 0);
    }
    Ok(())
}
