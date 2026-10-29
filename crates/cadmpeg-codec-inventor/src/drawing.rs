// SPDX-License-Identifier: Apache-2.0
//! Native drawing segment records.

mod records;
mod references;
mod token_wire;
mod validate;

use cadmpeg_core::decode::{DecodeContext, DecodeDepth};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::report::loss::LossNote;
use serde::{Deserialize, Serialize};

use crate::container::InventorContainer;
use crate::loss::InventorLossCode;
use crate::native::StructuralIssueRecord;
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind, SegmentMetaState};

pub(crate) use records::Record;
pub(crate) use validate::validate_native;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Family {
    #[serde(rename = "DlDocDcSegmentType")]
    Document,
    #[serde(rename = "DlBRxSegmentType")]
    Browser,
    #[serde(rename = "DlDirectorySegmentType")]
    Directory,
    #[serde(rename = "AppSegmentType")]
    Application,
    #[serde(rename = "DlSheetDcSegmentType")]
    Sheet,
    #[serde(rename = "DlSheetSmSegmentType")]
    SheetCache,
    #[serde(rename = "DlSheetDlSegmentType")]
    SheetDisplay,
}

impl Family {
    pub(crate) fn from_type_name(name: &str) -> Option<Self> {
        match name {
            "DlDocDcSegmentType" => Some(Self::Document),
            "DlBRxSegmentType" => Some(Self::Browser),
            "DlDirectorySegmentType" => Some(Self::Directory),
            "AppSegmentType" => Some(Self::Application),
            "DlSheetDcSegmentType" => Some(Self::Sheet),
            "DlSheetSmSegmentType" => Some(Self::SheetCache),
            "DlSheetDlSegmentType" => Some(Self::SheetDisplay),
            _ => None,
        }
    }

    pub(crate) const fn type_name(self) -> &'static str {
        match self {
            Self::Document => "DlDocDcSegmentType",
            Self::Browser => "DlBRxSegmentType",
            Self::Directory => "DlDirectorySegmentType",
            Self::Application => "AppSegmentType",
            Self::Sheet => "DlSheetDcSegmentType",
            Self::SheetCache => "DlSheetSmSegmentType",
            Self::SheetDisplay => "DlSheetDlSegmentType",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MetadataReference {
    pub(crate) id: String,
    #[serde(with = "token_wire")]
    pub(crate) segment_token: cadmpeg_ir::ids::IdentityKey,
    pub(crate) ordinal: u32,
    pub(crate) payload: cadmpeg_ir::native::bytes::NativeBytes,
    #[serde(flatten)]
    pub(crate) entry: crate::records::references::Entry,
}

impl MetadataReference {
    pub(crate) fn valid(&self) -> Result<bool, CodecError> {
        let mut view = cadmpeg_core::decode::View::over_retained(&self.payload);
        match crate::records::references::Entry::read(&mut view) {
            Ok(entry) => Ok(entry == self.entry && view.is_empty()),
            Err(error @ CodecError::ResourceLimit(_)) => Err(error),
            Err(_) => Ok(false),
        }
    }
}

#[derive(Default)]
pub(crate) struct Inventory {
    pub(crate) records: Vec<Record>,
    pub(crate) references: Vec<MetadataReference>,
    pub(crate) reference_tables: Vec<references::Tables>,
    pub(crate) issues: Vec<StructuralIssueRecord>,
    segment_count: usize,
    record_count: usize,
}

impl Inventory {
    pub(crate) fn write(
        &self,
        ctx: &DecodeContext<'_>,
        namespace: &mut NativeNamespace,
    ) -> Result<(), CodecError> {
        if !self.records.is_empty() {
            namespace.set_arena(ctx, "drawing_records", &self.records)?;
        }
        if !self.references.is_empty() {
            namespace.set_arena(ctx, "drawing_metadata_references", &self.references)?;
        }
        if !self.reference_tables.is_empty() {
            namespace.set_arena(ctx, "drawing_reference_tables", &self.reference_tables)?;
        }
        if !self.issues.is_empty() {
            namespace.set_arena(ctx, "drawing_record_issues", &self.issues)?;
        }
        Ok(())
    }

    fn refuse_references(
        &mut self,
        ctx: &DecodeContext<'_>,
        record: &Record,
    ) -> Result<(), CodecError> {
        ctx.charge_entities(1, "retain Inventor drawing reference refusal")?;
        let scope = format!("{}-{}", record.segment_token, record.ordinal);
        self.issues.push(StructuralIssueRecord {
            id: format!("inventor:drawing:record-issue#{scope}"),
            scope,
            detail: "drawing record does not select the required unique reference targets".into(),
        });
        Ok(())
    }

    pub(crate) fn losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        if self.segment_count != 0 {
            losses.push(InventorLossCode::DrawingNativeOnly.note(format!(
                "{} drawing segments and {} records remain native; drawing semantics are unresolved",
                self.segment_count, self.record_count
            )));
        }
        if !self.issues.is_empty() {
            losses.push(InventorLossCode::DrawingRecordMalformed.note(format!(
                "{} drawing records retain untyped payloads after a typed-form refusal",
                self.issues.len()
            )));
        }
        losses
    }
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    container: &InventorContainer<'_>,
    frames: &[crate::native::RseRecordRecord],
) -> Result<Inventory, CodecError> {
    let mut inventory = Inventory::default();
    if ctx.depth() != DecodeDepth::Full
        || !container
            .rse
            .segments
            .iter()
            .any(|segment| segment.kind == SegmentKind::Drawing(Family::Document))
    {
        return Ok(inventory);
    }
    for segment in &container.rse.segments {
        let SegmentKind::Drawing(family) = segment.kind else {
            continue;
        };
        inventory.segment_count += 1;
        if let SegmentMetaState::Parsed(meta) = &segment.meta {
            match references::Tables::read(ctx, segment.pair.token.key(), meta) {
                Ok(table) => {
                    ctx.charge_entities(1, "retain Inventor drawing reference tables")?;
                    inventory.reference_tables.push(table);
                }
                Err(error) => {
                    let detail = crate::issue_detail(error)?;
                    ctx.charge_entities(1, "retain Inventor drawing reference-table refusal")?;
                    let scope = format!("reference-tables-{}", segment.pair.token.as_str());
                    inventory.issues.push(StructuralIssueRecord {
                        id: format!("inventor:drawing:record-issue#{scope}"),
                        scope,
                        detail,
                    });
                }
            }
            let read = || -> Result<Vec<MetadataReference>, CodecError> {
                let section = &meta.tables.sections[8];
                let mut view = section.payload;
                let bounded = view
                    .counted(
                        u64::from(section.discriminator),
                        crate::layout::meta_reference_header::LEN
                            + crate::layout::meta_reference_pair::LEN,
                    )
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "drawing metadata reference count exceeds its payload",
                        )
                    })?;
                let mut entries = ctx.alloc_filled(
                    bounded.get(),
                    None,
                    "retain drawing metadata reference records",
                )?;
                for (index, target) in entries.iter_mut().enumerate() {
                    let ordinal = u32::try_from(index).map_err(|_| {
                        CodecError::malformed("drawing metadata reference ordinal exceeds u32")
                    })?;
                    ctx.charge_entities(1, "retain Inventor drawing metadata reference")?;
                    let mut start = view;
                    let entry = crate::records::references::Entry::read(&mut view)?;
                    let bytes = crate::reader::take(
                        &mut start,
                        entry.encoded_len(),
                        "drawing metadata reference payload",
                    )?;
                    let mut payload = ctx.alloc_filled(
                        bytes.len(),
                        0_u8,
                        "retain drawing metadata reference payload",
                    )?;
                    payload.copy_from_slice(bytes);
                    *target = Some(MetadataReference {
                        id: format!(
                            "inventor:drawing:metadata-reference#{}-{ordinal}",
                            segment.pair.token.as_str()
                        ),
                        segment_token: segment.pair.token.key().clone(),
                        ordinal,
                        entry,
                        payload: payload.into(),
                    });
                }
                entries
                    .into_iter()
                    .map(|entry| {
                        entry.ok_or_else(|| {
                            CodecError::malformed("drawing metadata reference entry was not read")
                        })
                    })
                    .collect()
            };
            match read() {
                Ok(references) => inventory.references.extend(references),
                Err(error) => {
                    let detail = crate::issue_detail(error)?;
                    ctx.charge_entities(1, "retain Inventor drawing metadata-reference refusal")?;
                    let scope = format!("metadata-references-{}", segment.pair.token.as_str());
                    inventory.issues.push(StructuralIssueRecord {
                        id: format!("inventor:drawing:record-issue#{scope}"),
                        scope,
                        detail,
                    });
                }
            }
        }
        let (Some(registry), SegmentBulkState::Framed(bulk)) = (segment.registry, &segment.bulk)
        else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        inventory.record_count += table.records.len();
        for frame in &table.records {
            if !records::recognizes(family, frame.type_id) {
                continue;
            }
            match Record::read(
                ctx,
                segment.pair.token.key(),
                family,
                registry.version_major,
                frame,
            ) {
                Ok(record) => {
                    ctx.charge_entities(1, "retain Inventor drawing record")?;
                    inventory.records.push(record);
                }
                Err(error) => {
                    let detail = crate::issue_detail(error)?;
                    ctx.charge_entities(1, "retain Inventor drawing record refusal")?;
                    let scope = format!("{}-{}", segment.pair.token.as_str(), frame.ordinal);
                    inventory.issues.push(StructuralIssueRecord {
                        id: format!("inventor:drawing:record-issue#{scope}"),
                        scope,
                        detail,
                    });
                }
            }
        }
    }
    if inventory.segment_count != 0 {
        let candidates = std::mem::take(&mut inventory.records);
        let source = cadmpeg_core::decode::unique_index(
            frames
                .iter()
                .map(|frame| ((frame.token.as_str(), frame.ordinal), frame)),
        );
        let mut eligible = Vec::new();
        for record in candidates {
            if record.operands_are_valid(|ordinal| {
                source
                    .get(&(record.segment_token.as_str(), ordinal))
                    .and_then(|frame| *frame)
                    .map(|frame| frame.type_id.as_str())
            }) {
                eligible.push(record);
            } else {
                inventory.refuse_references(ctx, &record)?;
            }
        }
        let mut admitted =
            ctx.alloc_filled(eligible.len(), false, "validate drawing record references")?;
        for (accepted, record) in admitted.iter_mut().zip(&eligible) {
            *accepted =
                references::record_links_are_valid(record, &inventory.reference_tables, &eligible);
        }
        for (record, accepted) in eligible.into_iter().zip(admitted) {
            if accepted {
                inventory.records.push(record);
            } else {
                inventory.refuse_references(ctx, &record)?;
            }
        }
    }
    Ok(inventory)
}

#[cfg(test)]
mod tests;
