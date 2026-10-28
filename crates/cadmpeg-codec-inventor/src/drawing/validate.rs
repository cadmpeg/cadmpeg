// SPDX-License-Identifier: Apache-2.0
//! Drawing native identity, payload, and metadata-reference checks.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;

use super::{
    references::{record_links_are_valid, Tables},
    MetadataReference, Record,
};
use crate::native::{
    MetaSectionRecord, RseRecordRecord, SegmentMetaRecord, SegmentRegistryRecord,
    StructuralIssueRecord,
};
use crate::records::MetaSectionNumber;

pub(crate) fn validate_native(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    frames: &[RseRecordRecord],
    metadata: &[SegmentMetaRecord],
    registry: &[SegmentRegistryRecord],
    sections: &[MetaSectionRecord],
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    if !namespace
        .arenas()
        .keys()
        .any(|name| name.starts_with("drawing_"))
    {
        return Ok(());
    }
    let records: Vec<Record> = match namespace.arena_as_for_decode(ctx, "drawing_records") {
        Ok(records) => records,
        Err(error) => {
            let detail = crate::issue_detail(error.into())?;
            findings.push(finding(
                format!("Inventor drawing native records are invalid: {detail}"),
                None,
            ));
            return Ok(());
        }
    };
    let tables: Vec<Tables> = match namespace.arena_as_for_decode(ctx, "drawing_reference_tables") {
        Ok(tables) => tables,
        Err(error) => {
            let detail = crate::issue_detail(error.into())?;
            findings.push(finding(
                format!("Inventor drawing reference tables are invalid: {detail}"),
                None,
            ));
            return Ok(());
        }
    };
    let mut table_tokens = HashSet::new();
    for table in &tables {
        let valid = table.valid(ctx)?
            && table_tokens.insert(table.segment_token.as_str())
            && metadata
                .iter()
                .filter(|meta| {
                    meta.token == table.segment_token.as_str()
                        && meta.segment_id == table.segment_id.to_string()
                        && family_for_segment(&meta.segment_id, &meta.kind, registry).is_some()
                })
                .count()
                == 1
            && [
                MetaSectionNumber::Seven,
                MetaSectionNumber::Eight,
                MetaSectionNumber::Ten,
            ]
            .into_iter()
            .enumerate()
            .all(|(index, number)| {
                let matching = sections
                    .iter()
                    .filter(|section| {
                        section.token == table.segment_token.as_str() && section.number == number
                    })
                    .collect::<Vec<_>>();
                matching.len() == 1
                    && matching[0].discriminator == table.counts[index]
                    && matching[0].payload_len
                        == cadmpeg_core::decode::u64_from_index(table.payloads[index].len())
                    && matching[0].payload_sha256
                        == cadmpeg_ir::hash::sha256_hex(&table.payloads[index])
            });
        if !valid {
            findings.push(finding(
                "Inventor drawing reference tables disagree with their metadata sections".into(),
                Some(table.id.clone()),
            ));
        }
    }
    let mut identities = HashSet::new();
    let mut keys = records
        .iter()
        .map(|record| (record.segment_token.as_str(), record.ordinal))
        .collect::<HashSet<_>>();
    for record in &records {
        for reference in record.local_references() {
            if let Some(ordinal) = reference.record_ordinal() {
                keys.insert((record.segment_token.as_str(), ordinal));
            }
        }
    }
    let source = cadmpeg_core::decode::unique_index(
        frames
            .iter()
            .filter(|frame| keys.contains(&(frame.token.as_str(), frame.ordinal)))
            .map(|frame| ((frame.token.as_str(), frame.ordinal), frame)),
    );
    for record in &records {
        let valid = record.valid(ctx)?
            && record.operands_are_valid(|ordinal| {
                source
                    .get(&(record.segment_token.as_str(), ordinal))
                    .and_then(|frame| *frame)
                    .map(|frame| frame.type_id.as_str())
            })
            && record_links_are_valid(record, &tables, &records)
            && identities.insert((&record.segment_token, record.ordinal))
            && metadata
                .iter()
                .filter(|meta| {
                    meta.token == record.segment_token.as_str()
                        && family_for_segment(&meta.segment_id, &meta.kind, registry)
                            == Some(record.family)
                })
                .count()
                == 1
            && source
                .get(&(record.segment_token.as_str(), record.ordinal))
                .and_then(|frame| *frame)
                .is_some_and(|frame| {
                    frame.type_id == record.type_id.as_str()
                        && frame.matches_payload(&record.payload)
                });
        if !valid {
            findings.push(finding(
                "Inventor drawing record disagrees with its identity, fields, or RSe payload"
                    .into(),
                Some(record.id.clone()),
            ));
        }
    }
    let references: Vec<MetadataReference> =
        match namespace.arena_as_for_decode(ctx, "drawing_metadata_references") {
            Ok(records) => records,
            Err(error) => {
                let detail = crate::issue_detail(error.into())?;
                findings.push(finding(
                    format!("Inventor drawing metadata references are invalid: {detail}"),
                    None,
                ));
                return Ok(());
            }
        };
    let mut by_token = BTreeMap::<&str, Vec<&MetadataReference>>::new();
    for record in &references {
        let valid = record.valid()?
            && record.id
                == format!(
                    "inventor:drawing:metadata-reference#{}-{}",
                    record.segment_token, record.ordinal
                );
        if !valid {
            findings.push(finding(
                "Inventor drawing metadata reference identity or payload is invalid".into(),
                Some(record.id.clone()),
            ));
        }
        by_token
            .entry(record.segment_token.as_str())
            .or_default()
            .push(record);
    }
    for (token, mut entries) in by_token {
        entries.sort_by_key(|record| record.ordinal);
        let matches = sections
            .iter()
            .filter(|section| section.token == token && section.number == MetaSectionNumber::Nine)
            .collect::<Vec<_>>();
        let count_matches = matches.len() == 1
            && u32::try_from(entries.len()).is_ok_and(|count| count == matches[0].discriminator)
            && entries
                .iter()
                .enumerate()
                .all(|(ordinal, record)| usize::try_from(record.ordinal) == Ok(ordinal));
        if !count_matches {
            findings.push(finding(
                "Inventor drawing metadata references disagree with their section count".into(),
                None,
            ));
            continue;
        }
        let mut len = 0_usize;
        for record in &entries {
            len = len.checked_add(record.payload.len()).ok_or_else(|| {
                CodecError::malformed("drawing metadata reference payload length overflow")
            })?;
        }
        if cadmpeg_core::decode::u64_from_index(len) != matches[0].payload_len {
            findings.push(finding(
                "Inventor drawing metadata references disagree with their section length".into(),
                None,
            ));
            continue;
        }
        let mut payload =
            ctx.alloc_filled(len, 0_u8, "validate drawing metadata reference payload")?;
        let mut target = payload.as_mut_slice();
        for record in entries {
            let (head, rest) = target.split_at_mut(record.payload.len());
            head.copy_from_slice(&record.payload);
            target = rest;
        }
        if cadmpeg_ir::hash::sha256_hex(&payload) != matches[0].payload_sha256 {
            findings.push(finding(
                "Inventor drawing metadata references disagree with their section payload".into(),
                None,
            ));
        }
    }
    let issues: Vec<StructuralIssueRecord> =
        match namespace.arena_as_for_decode(ctx, "drawing_record_issues") {
            Ok(records) => records,
            Err(error) => {
                let detail = crate::issue_detail(error.into())?;
                findings.push(finding(
                    format!("Inventor drawing refusal records are invalid: {detail}"),
                    None,
                ));
                return Ok(());
            }
        };
    let mut issue_ids = HashSet::new();
    for issue in issues {
        if issue.scope.is_empty()
            || issue.detail.is_empty()
            || issue.id != format!("inventor:drawing:record-issue#{}", issue.scope)
            || !issue_ids.insert(issue.id.clone())
        {
            findings.push(finding(
                "Inventor drawing refusal identity or detail is invalid".into(),
                Some(issue.id),
            ));
        }
    }
    Ok(())
}

fn family_for_segment(
    segment_id: &str,
    kind: &str,
    registry: &[SegmentRegistryRecord],
) -> Option<super::Family> {
    let mut matching = registry
        .iter()
        .filter(|entry| entry.segment_id == segment_id);
    let entry = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    let family = super::Family::from_type_name(&entry.type_name)?;
    (kind == family.type_name()).then_some(family)
}

fn finding(message: String, entity: Option<String>) -> Finding {
    Finding {
        related: Vec::new(),
        check: Check::NativeLinks,
        severity: Severity::Error,
        message,
        entity,
    }
}

#[cfg(test)]
mod tests;
