// SPDX-License-Identifier: Apache-2.0
//! High-level Inventor structural decode.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::{DecodeReport, LossKind, LossNote, Severity, TransferLedger};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::SourceFidelity;

use crate::container::InventorContainer;
use crate::native::{
    SegmentPairRecord, StorageBandRecord, UnpairedSegmentRecord, INVENTOR_NATIVE_VERSION,
};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let container = InventorContainer::open(ctx, root)?;
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
        .storage_bands
        .iter()
        .map(|(band, stream)| StorageBandRecord {
            id: format!("inventor:rse:database:v{}", band.value()),
            band: band.value(),
            database_directory_id: stream.directory_id(),
        })
        .collect::<Vec<_>>();
    let segment_pairs = container
        .rse
        .segments
        .iter()
        .map(|segment| SegmentPairRecord {
            id: format!("inventor:rse:segment:{}", segment.token.as_str()),
            token: segment.token.as_str().into(),
            metadata_directory_id: segment.metadata.directory_id(),
            bulk_directory_id: segment.bulk.directory_id(),
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
            .saturating_add(unpaired_segments.len()) as u64,
        "retain Inventor native structural records",
    )?;
    let namespace = ir.native.namespace_mut("inventor");
    namespace.version = INVENTOR_NATIVE_VERSION;
    namespace.set_arena("storage_bands", &storage_bands)?;
    namespace.set_arena("segment_pairs", &segment_pairs)?;
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
                ("rse_segment_pairs".into(), segment_pairs.len()),
            ]),
            losses,
            notes: Vec::new(),
            transfer_ledger: TransferLedger::default(),
        },
        SourceFidelity::default(),
    ))
}
