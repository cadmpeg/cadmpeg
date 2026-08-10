// SPDX-License-Identifier: Apache-2.0
//! High-level Inventor structural decode.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::{DecodeReport, LossKind, LossNote, Severity, TransferLedger};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::SourceFidelity;

use crate::container::{ContainerPurpose, InventorContainer};
use crate::database::{RevisionPayload, VersionTuple};
use crate::native::{
    DatabaseIssueRecord, DatabaseRecord, RevisionRecord, SegmentBulkIssueRecord, SegmentBulkRecord,
    SegmentMetaIssueRecord, SegmentMetaRecord, SegmentPairRecord, SegmentRegistryRecord,
    StorageBandRecord, StructuralIssueRecord, UnpairedSegmentRecord, VersionTupleRecord,
    INVENTOR_NATIVE_VERSION,
};
use crate::rse::{ParsedState, SegmentBulkState, SegmentMetaState};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let container = InventorContainer::open(ctx, root, ContainerPurpose::Decode)?;
    let mut ir = CadIr::empty(Units::default());
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "cfb_major_version".into(),
        container.snapshot.major_version().to_string(),
    );
    attributes.insert(
        "cfb_sector_size".into(),
        container.snapshot.sector_size().to_string(),
    );
    attributes.insert(
        "rse_segment_pairs".into(),
        container.rse.segments.len().to_string(),
    );
    ir.source = Some(SourceMeta {
        format: "inventor".into(),
        attributes,
    });
    let storage_bands = container
        .rse
        .databases
        .iter()
        .map(|database| StorageBandRecord {
            id: format!("inventor:rse:database:v{}", database.band.value()),
            band: database.band.value(),
            database_directory_id: database.stream.directory_id(),
        })
        .collect::<Vec<_>>();
    let databases = container
        .rse
        .databases
        .iter()
        .filter_map(|descriptor| {
            let ParsedState::Parsed(database) = &descriptor.state else {
                return None;
            };
            Some(DatabaseRecord {
                id: format!("inventor:rse:database-record:v{}", descriptor.band.value()),
                band: descriptor.band.value(),
                database_id: hex(&database.id),
                schema: database.schema.value(),
                created_by: version_record(database.created_by),
                created_filetime: database.created_filetime,
                saved_by: version_record(database.saved_by),
                saved_filetime: database.saved_filetime,
                note: database.note.clone(),
            })
        })
        .collect::<Vec<_>>();
    let database_issues = container
        .rse
        .databases
        .iter()
        .filter_map(|descriptor| {
            let ParsedState::Unavailable(detail) = &descriptor.state else {
                return None;
            };
            Some(DatabaseIssueRecord {
                id: format!("inventor:rse:database-issue:v{}", descriptor.band.value()),
                band: descriptor.band.value(),
                detail: detail.clone(),
            })
        })
        .collect::<Vec<_>>();
    let segment_registry = match &container.rse.registry {
        ParsedState::Parsed(registry) => registry
            .entries
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| SegmentRegistryRecord {
                id: format!("inventor:rse:registry:{ordinal}"),
                ordinal: ordinal as u32,
                display_name: entry.display_name.clone(),
                segment_id: hex(&entry.segment_id),
                revision_id: hex(&entry.revision_id),
                type_name: entry.type_name.clone(),
                object_count: entry.objects.len() as u64,
                node_count: entry.nodes.len() as u64,
            })
            .collect(),
        ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
    };
    let revisions = match &container.rse.revisions {
        ParsedState::Parsed(table) => table
            .entries
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| RevisionRecord {
                id: format!("inventor:rse:revision:{ordinal}"),
                ordinal: ordinal as u32,
                revision_id: hex(&entry.id),
                flags: entry.flags,
                kind: entry.kind,
                payload_form: match entry.payload {
                    RevisionPayload::None => "none",
                    RevisionPayload::Short { .. } => "short",
                    RevisionPayload::Long { .. } => "long",
                }
                .into(),
            })
            .collect(),
        ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
    };
    let mut structural_issues = Vec::new();
    if let ParsedState::Unavailable(detail) = &container.rse.registry {
        structural_issues.push(structural_issue("segment_registry", detail));
    }
    if let ParsedState::Unavailable(detail) = &container.rse.revisions {
        structural_issues.push(structural_issue("revision_table", detail));
    }
    structural_issues.extend(container.rse.segments.iter().flat_map(|segment| {
        segment
            .identity_issues
            .iter()
            .enumerate()
            .map(move |(ordinal, detail)| StructuralIssueRecord {
                id: format!(
                    "inventor:rse:structural-issue:segment:{}:{ordinal}",
                    segment.pair.token.as_str()
                ),
                scope: format!("segment:{}", segment.pair.token.as_str()),
                detail: detail.clone(),
            })
    }));
    let segment_pairs = container
        .rse
        .segments
        .iter()
        .map(|segment| SegmentPairRecord {
            id: format!("inventor:rse:segment:{}", segment.pair.token.as_str()),
            token: segment.pair.token.as_str().into(),
            metadata_directory_id: segment.pair.metadata.directory_id(),
            bulk_directory_id: segment.pair.bulk.directory_id(),
        })
        .collect::<Vec<_>>();
    let segment_meta = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let SegmentMetaState::Parsed(meta) = &segment.meta else {
                return None;
            };
            Some(SegmentMetaRecord {
                id: format!("inventor:rse:segment-meta:{}", segment.pair.token.as_str()),
                token: segment.pair.token.as_str().into(),
                version: meta.version.value(),
                kind: segment.kind.label().into(),
                display_name: meta.display_name.clone(),
                segment_id: hex(&meta.segment_id),
                header_words: meta.header_words,
                state_words: meta.state_words,
                created: meta.created.clone(),
                modified: meta.modified.clone(),
                body_form: meta.body_form,
                expanded_body_len: meta.body.window().len() as u64,
                expanded_body_sha256: sha256_hex(meta.body.window()),
            })
        })
        .collect::<Vec<_>>();
    let segment_meta_issues = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let (status, detail) = match &segment.meta {
                SegmentMetaState::Parsed(_) => return None,
                SegmentMetaState::Unsupported { marker, version } => (
                    "unsupported",
                    format!("marker {marker:?}, version {version}"),
                ),
                SegmentMetaState::Malformed(detail) => ("malformed", detail.clone()),
            };
            Some(SegmentMetaIssueRecord {
                id: format!(
                    "inventor:rse:segment-meta-issue:{}",
                    segment.pair.token.as_str()
                ),
                token: segment.pair.token.as_str().into(),
                status: status.into(),
                detail,
            })
        })
        .collect::<Vec<_>>();
    let segment_bulk = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let SegmentBulkState::Framed(bulk) = &segment.bulk else {
                return None;
            };
            let expanded = bulk
                .expanded
                .expect("decode-purpose container expands every framed bulk stream");
            Some(SegmentBulkRecord {
                id: format!("inventor:rse:segment-bulk:{}", segment.pair.token.as_str()),
                token: segment.pair.token.as_str().into(),
                prefix: hex(&bulk.prefix),
                form: bulk.form.value(),
                compressed_len: bulk.compressed.window().len() as u64,
                compressed_sha256: sha256_hex(bulk.compressed.window()),
                expanded_len: expanded.window().len() as u64,
                expanded_sha256: sha256_hex(expanded.window()),
            })
        })
        .collect::<Vec<_>>();
    let segment_bulk_issues = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let SegmentBulkState::Malformed(detail) = &segment.bulk else {
                return None;
            };
            Some(SegmentBulkIssueRecord {
                id: format!(
                    "inventor:rse:segment-bulk-issue:{}",
                    segment.pair.token.as_str()
                ),
                token: segment.pair.token.as_str().into(),
                detail: detail.clone(),
            })
        })
        .collect::<Vec<_>>();
    let unpaired_segments = container
        .rse
        .unpaired_metadata
        .iter()
        .map(|token| UnpairedSegmentRecord {
            id: format!("inventor:rse:unpaired-metadata:{}", token.as_str()),
            token: token.as_str().into(),
            missing_member: "bulk".into(),
        })
        .chain(
            container
                .rse
                .unpaired_bulk
                .iter()
                .map(|token| UnpairedSegmentRecord {
                    id: format!("inventor:rse:unpaired-bulk:{}", token.as_str()),
                    token: token.as_str().into(),
                    missing_member: "metadata".into(),
                }),
        )
        .collect::<Vec<_>>();
    ctx.charge_collection_items(
        storage_bands
            .len()
            .saturating_add(segment_pairs.len())
            .saturating_add(databases.len())
            .saturating_add(database_issues.len())
            .saturating_add(segment_registry.len())
            .saturating_add(revisions.len())
            .saturating_add(structural_issues.len())
            .saturating_add(segment_meta.len())
            .saturating_add(segment_meta_issues.len())
            .saturating_add(segment_bulk.len())
            .saturating_add(segment_bulk_issues.len())
            .saturating_add(unpaired_segments.len()) as u64,
        "retain Inventor native structural records",
    )?;
    let namespace = ir.native.namespace_mut("inventor");
    namespace.version = INVENTOR_NATIVE_VERSION;
    namespace.set_arena("storage_bands", &storage_bands)?;
    namespace.set_arena("databases", &databases)?;
    namespace.set_arena("database_issues", &database_issues)?;
    namespace.set_arena("segment_registry", &segment_registry)?;
    namespace.set_arena("revisions", &revisions)?;
    namespace.set_arena("structural_issues", &structural_issues)?;
    namespace.set_arena("segment_pairs", &segment_pairs)?;
    namespace.set_arena("segment_meta", &segment_meta)?;
    namespace.set_arena("segment_meta_issues", &segment_meta_issues)?;
    namespace.set_arena("segment_bulk", &segment_bulk)?;
    namespace.set_arena("segment_bulk_issues", &segment_bulk_issues)?;
    namespace.set_arena("unpaired_segments", &unpaired_segments)?;
    let mut losses = Vec::new();
    if ctx.container_only() {
        losses.push(LossNote::new(
            LossKind::ContainerOnly,
            "Container-only decode was requested.",
        ));
    } else {
        losses.push(
            LossNote::new(
                LossKind::GeometryNotTransferred,
                "Inventor RSe geometry records have not been transferred.",
            )
            .with_severity(Severity::Blocking),
        );
        if !segment_pairs.is_empty() {
            losses.push(LossNote::new(
                LossKind::RecordNotTyped,
                format!(
                    "Retained {} structurally paired RSe segment(s) without record semantics.",
                    segment_pairs.len()
                ),
            ));
        }
    }
    if !segment_meta_issues.is_empty() {
        losses.push(LossNote::new(
            LossKind::DecodeDiagnostic,
            format!(
                "{} RSe metadata stream(s) are malformed or outside the implemented envelope.",
                segment_meta_issues.len()
            ),
        ));
    }
    if !segment_bulk_issues.is_empty() {
        losses.push(LossNote::new(
            LossKind::DecodeDiagnostic,
            format!(
                "{} RSe bulk stream(s) have invalid envelope or zlib framing.",
                segment_bulk_issues.len()
            ),
        ));
    }
    if !container.rse.unpaired_metadata.is_empty() || !container.rse.unpaired_bulk.is_empty() {
        losses.push(LossNote::new(
            LossKind::DecodeDiagnostic,
            format!(
                "RSe contains {} unpaired metadata stream(s) and {} unpaired bulk stream(s).",
                container.rse.unpaired_metadata.len(),
                container.rse.unpaired_bulk.len()
            ),
        ));
    }
    ctx.charge_entities(ir.model.entity_count() as u64, "admit Inventor entities")?;
    Ok(DecodeResult::new(
        ir,
        DecodeReport {
            format: "inventor".into(),
            container_only: ctx.container_only(),
            geometry_transferred: false,
            coverage: BTreeMap::from([
                ("rse_storage_bands".into(), storage_bands.len()),
                ("rse_databases".into(), databases.len()),
                ("rse_registry_entries".into(), segment_registry.len()),
                ("rse_revisions".into(), revisions.len()),
                ("rse_segment_pairs".into(), segment_pairs.len()),
                ("rse_segment_meta".into(), segment_meta.len()),
                ("rse_segment_meta_issues".into(), segment_meta_issues.len()),
                ("rse_segment_bulk".into(), segment_bulk.len()),
                ("rse_segment_bulk_issues".into(), segment_bulk_issues.len()),
            ]),
            losses,
            notes: Vec::new(),
            transfer_ledger: TransferLedger::default(),
        },
        SourceFidelity::default(),
    ))
}

fn version_record(version: VersionTuple) -> VersionTupleRecord {
    VersionTupleRecord {
        revision: version.revision,
        minor: version.minor,
        major: version.major,
        state: hex(&version.state),
    }
}

fn structural_issue(scope: &str, detail: &str) -> StructuralIssueRecord {
    StructuralIssueRecord {
        id: format!("inventor:rse:structural-issue:{scope}"),
        scope: scope.into(),
        detail: detail.into(),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}
