// SPDX-License-Identifier: Apache-2.0
//! Typed assembly occurrence and placement records.

use crate::pmdc::unique_by;

use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::transform::Transform;

use crate::compact_matrix::CompactMatrix;
use crate::native::ufrx::{ExternalReferenceRecord, UfrxOccurrenceRecord};
use crate::native::{AssemblyOccurrenceRecord, AssemblyPlacementRecord};
use crate::record_issue::{admit_issue_detail, RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const SUPPRESSED_REFERENCE_STATE: u16 = 0x2000;
const INVENTOR_LENGTH_TO_MILLIMETRES: f64 = 10.0;

const OCCURRENCE_TYPE: [u8; 16] = [
    0x60, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const PLACEMENT_TYPE_CA: [u8; 16] = [
    0xa2, 0x63, 0x71, 0xca, 0xd0, 0x11, 0xb2, 0xd3, 0x00, 0x08, 0xbf, 0xbb, 0x21, 0xed, 0xdc, 0x09,
];
const PLACEMENT_TYPE_B9: [u8; 16] = [
    0x07, 0xd0, 0xd0, 0xb9, 0xd4, 0x11, 0x2d, 0x5f, 0x60, 0x00, 0xf8, 0x83, 0x0e, 0x73, 0xfc, 0xb0,
];

#[derive(Debug)]
pub(crate) struct AssemblyInventory<'a> {
    pub(crate) occurrences: Vec<AssemblyOccurrence>,
    pub(crate) placements: Vec<AssemblyPlacement<'a>>,
    pub(crate) issues: Vec<RecordIssue>,
}

#[derive(Debug)]
pub(crate) struct AssemblyOccurrence {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) next_reference: u32,
    pub(crate) flags: u32,
    pub(crate) owner_reference: u32,
    pub(crate) node_index: u32,
    pub(crate) state: [i32; 2],
    pub(crate) ordinal_key: u32,
    pub(crate) related_references: Vec<u32>,
    pub(crate) child_reference: u32,
    pub(crate) occurrence_id: u32,
}

#[derive(Debug)]
pub(crate) struct AssemblyPlacement<'a> {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) header_id: u16,
    pub(crate) owner_reference: u32,
    pub(crate) attribute_reference: u32,
    pub(crate) state: u8,
    pub(crate) transform_prefix: bool,
    pub(crate) transform: CompactMatrix,
    pub(crate) branch: u8,
    pub(crate) graphics_state: u8,
    pub(crate) occurrence_id: u32,
    pub(crate) graphics_index: u32,
    pub(crate) object_reference: u32,
    pub(crate) suffix: View<'a>,
}

/// Records one more occurrence of `cause` in a non-zero tally.
pub(crate) fn count_unresolved<C: Ord>(
    ctx: &DecodeContext<'_>,
    counts: &mut BTreeMap<C, NonZeroUsize>,
    cause: C,
) -> Result<(), CodecError> {
    if let Some(count) = counts.get_mut(&cause) {
        *count = count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "count unresolved Inventor projection cause",
                u64::MAX,
                u64::MAX,
            )
        })?;
        return Ok(());
    }
    ctx.charge_collection_items(1, "count unresolved Inventor projection cause")?;
    counts.insert(cause, NonZeroUsize::MIN);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum UnresolvedCause {
    ExternalReference,
    OccurrenceRecord,
    DuplicateOccurrence,
    InvalidTransform,
    Placement,
}

impl UnresolvedCause {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::ExternalReference => "external reference is missing or ambiguous",
            Self::OccurrenceRecord => "AmDc occurrence is missing or ambiguous",
            Self::DuplicateOccurrence => "occurrence ID is duplicated",
            Self::InvalidTransform => "placement transform is not affine after unit conversion",
            Self::Placement => "AmGraphics placement is missing or ambiguous",
        }
    }
}

#[derive(Debug)]
pub(crate) struct AssemblyProjection {
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) unresolved_placements: BTreeMap<UnresolvedCause, NonZeroUsize>,
}

/// Projects the current document's occurrence table without loading prototypes.
///
/// `UFRx` supplies document order and the external prototype join. `AmDc` proves
/// the occurrence identity, while `AmGraphics` supplies the placement. Referenced
/// assemblies remain one unresolved external prototype, so their internal trees
/// are not invented as children of this document.
pub(crate) fn project_occurrences(
    ctx: &DecodeContext<'_>,
    ufrx_occurrences: &[UfrxOccurrenceRecord],
    external_references: &[ExternalReferenceRecord],
    assembly_occurrences: &[AssemblyOccurrenceRecord],
    assembly_placements: &[AssemblyPlacementRecord],
) -> Result<AssemblyProjection, CodecError> {
    let references = unique_by(
        ctx,
        external_references,
        "index Inventor external references",
        |record| record.reference_id,
    )?;
    let occurrence_records = unique_by(
        ctx,
        assembly_occurrences,
        "index Inventor assembly occurrences",
        |record| record.occurrence_id,
    )?;
    let placements = unique_by(
        ctx,
        assembly_placements,
        "index Inventor assembly placements",
        |record| record.occurrence_id,
    )?;
    let mut emitted_ids = HashSet::new();
    let mut occurrences = Vec::new();
    let mut unresolved_placements = BTreeMap::new();

    for source in ufrx_occurrences {
        let Some(reference) = references.get(&source.file_reference_id) else {
            count_unresolved(
                ctx,
                &mut unresolved_placements,
                UnresolvedCause::ExternalReference,
            )?;
            continue;
        };
        if !occurrence_records.contains_key(&source.occurrence_id) {
            count_unresolved(
                ctx,
                &mut unresolved_placements,
                UnresolvedCause::OccurrenceRecord,
            )?;
            continue;
        }
        if emitted_ids.contains(&source.occurrence_id) {
            count_unresolved(
                ctx,
                &mut unresolved_placements,
                UnresolvedCause::DuplicateOccurrence,
            )?;
            continue;
        }
        ctx.charge_collection_items(1, "track Inventor emitted occurrence id")?;
        emitted_ids.insert(source.occurrence_id);

        let suppressed = reference.state[0] & SUPPRESSED_REFERENCE_STATE != 0;
        let (transform, visible) = match placements.get(&source.occurrence_id) {
            Some(placement) => {
                let source = placement.transform.rows();
                let mut rows = [source[0], source[1], source[2]];
                for row in &mut rows {
                    row[3] *= INVENTOR_LENGTH_TO_MILLIMETRES;
                }
                let Some(transform) = (source[3] == [0.0, 0.0, 0.0, 1.0])
                    .then(|| Transform::affine(rows))
                    .flatten()
                else {
                    count_unresolved(
                        ctx,
                        &mut unresolved_placements,
                        UnresolvedCause::InvalidTransform,
                    )?;
                    continue;
                };
                (transform, suppressed.then_some(false))
            }
            None if suppressed => (Transform::identity(), Some(false)),
            None => {
                count_unresolved(ctx, &mut unresolved_placements, UnresolvedCause::Placement)?;
                continue;
            }
        };

        ctx.charge_collection_items(1, "project Inventor occurrence")?;
        ctx.charge_entities(1, "project Inventor occurrence")?;
        ctx.charge_retained(
            ("inventor:assembly:instance#".len()
                + source.occurrence_id.max(1).ilog10() as usize
                + 1) as u64,
            "retain projected Inventor occurrence id",
        )?;
        ctx.charge_retained(
            reference.document_copy_len() as u64,
            "retain projected Inventor external document",
        )?;
        ctx.charge_retained(
            source.title.as_ref().map_or(0, String::len) as u64,
            "retain projected Inventor occurrence title",
        )?;
        ctx.charge_retained(
            source.id.len() as u64,
            "retain projected Inventor occurrence native reference",
        )?;
        occurrences.push(Occurrence {
            id: OccurrenceId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "assembly", "instance"),
                source.occurrence_id,
            ),
            prototype: PrototypeReference::External {
                document: reference.document(),
                object: None,
            },
            parent: OccurrenceParent::Root {},
            ordinal: source.ordinal,
            transform,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: source.title.clone().filter(|title| !title.is_empty()),
            visible,
            link: None,
            native_ref: Some(source.id.clone()),
        });
    }

    Ok(AssemblyProjection {
        occurrences,
        unresolved_placements,
    })
}

pub(crate) fn inventory<'a>(
    ctx: &DecodeContext<'a>,
    document: &RseInventory<'a>,
) -> Result<AssemblyInventory<'a>, CodecError> {
    let mut occurrences = Vec::new();
    let mut placements = Vec::new();
    let mut issues = Vec::new();
    for segment in &document.segments {
        let relevant = matches!(segment.kind, SegmentKind::AmDc | SegmentKind::AmGraphics);
        if !relevant {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let result = if segment.kind == SegmentKind::AmDc && record.type_id == OCCURRENCE_TYPE {
                parse_occurrence(ctx, record.payload).and_then(|mut occurrence| {
                    ctx.charge_collection_items(1, "admit Inventor assembly occurrence record")?;
                    ctx.charge_retained(
                        segment.pair.token.as_str().len() as u64,
                        "retain Inventor assembly occurrence token",
                    )?;
                    occurrence.segment_token = segment.pair.token.as_str().into();
                    occurrence.record_ordinal = record.ordinal;
                    occurrences.push(occurrence);
                    Ok(())
                })
            } else if segment.kind == SegmentKind::AmGraphics
                && matches!(record.type_id, PLACEMENT_TYPE_CA | PLACEMENT_TYPE_B9)
            {
                parse_placement(ctx, record.payload).and_then(|mut placement| {
                    ctx.charge_collection_items(1, "admit Inventor assembly placement record")?;
                    ctx.charge_retained(
                        segment.pair.token.as_str().len() as u64,
                        "retain Inventor assembly placement token",
                    )?;
                    placement.segment_token = segment.pair.token.as_str().into();
                    placement.record_ordinal = record.ordinal;
                    placements.push(placement);
                    Ok(())
                })
            } else {
                continue;
            };
            if let Err(error) = result {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_collection_items(1, "admit Inventor assembly issue")?;
                admit_issue_detail(ctx, &error, "retain Inventor assembly issue detail")?;
                ctx.charge_retained(
                    segment.pair.token.as_str().len() as u64,
                    "retain Inventor assembly issue token",
                )?;
                issues.push(RecordIssue {
                    family: RecordIssueFamily::Assembly,
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(AssemblyInventory {
        occurrences,
        placements,
        issues,
    })
}

fn parse_occurrence<'a>(
    ctx: &DecodeContext<'a>,
    payload: View<'a>,
) -> Result<AssemblyOccurrence, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header_value = cursor.u32("occurrence header value")?;
    let header_id = cursor.u16("occurrence header id")?;
    let next_reference = cursor.u32("occurrence next reference")?;
    let flags = cursor.u32("occurrence flags")?;
    let owner_reference = cursor.u32("occurrence owner reference")?;
    let node_index = cursor.u32("occurrence node index")?;
    let state = [
        cursor.i32("occurrence state")?,
        cursor.i32("occurrence state")?,
    ];
    require(
        cursor.u32("occurrence relation-list marker")?,
        0x3000_0002,
        "occurrence relation-list marker",
    )?;
    require(
        cursor.u32("occurrence relation-list count")?,
        0,
        "occurrence relation-list count",
    )?;
    let ordinal_key = cursor.u32("occurrence ordinal key")?;
    require(
        cursor.u32("occurrence related-list marker")?,
        0x3000_0002,
        "occurrence related-list marker",
    )?;
    let related_count = cursor.count32("occurrence related-list count", 65_536)?;
    ctx.charge_collection_items(
        related_count as u64,
        "admit Inventor occurrence related references",
    )?;
    let mut related_references = Vec::with_capacity(related_count);
    if related_count != 0 {
        cursor.u32("occurrence related-list metadata")?;
        cursor.u32("occurrence related-list metadata")?;
        for _ in 0..related_count {
            related_references.push(cursor.u32("occurrence related reference")?);
        }
    }
    let child_reference = cursor.u32("occurrence child reference")?;
    require(
        u32::from(cursor.u16("occurrence identity mode")?),
        0x0200,
        "occurrence identity mode",
    )?;
    let occurrence_id = cursor.u32("occurrence id")?;
    let label = cursor.utf16(ctx, "occurrence record label", 256)?;
    if label != "DCx" {
        return Err(CodecError::malformed(format_args!(
            "Inventor occurrence record label is {label:?}, expected \"DCx\""
        )));
    }
    require(
        u32::from(cursor.u16("occurrence trailer")?),
        1,
        "occurrence trailer",
    )?;
    if !cursor.remaining("occurrence suffix")?.window().is_empty() {
        return Err(CodecError::Malformed(
            "Inventor occurrence record has trailing bytes".into(),
        ));
    }
    Ok(AssemblyOccurrence {
        segment_token: String::new(),
        record_ordinal: 0,
        header_value,
        header_id,
        next_reference,
        flags,
        owner_reference,
        node_index,
        state,
        ordinal_key,
        related_references,
        child_reference,
        occurrence_id,
    })
}

fn parse_placement<'a>(
    ctx: &DecodeContext<'a>,
    payload: View<'a>,
) -> Result<AssemblyPlacement<'a>, CodecError> {
    let mut cursor = Cursor::new(payload);
    require(cursor.u32("placement prefix")?, 0, "placement prefix")?;
    let header_id = cursor.u16("placement header id")?;
    let owner_reference = cursor.u32("placement owner reference")?;
    let attribute_reference = cursor.u32("placement attribute reference")?;
    let state = cursor.u8("placement state")?;
    let (transform_prefix, transform) = cursor.transform()?;
    let branch = cursor.u8("placement branch")?;
    let graphics_state = cursor.u8("placement graphics state")?;
    let occurrence_id = cursor.u32("placement occurrence id")?;
    let label = cursor.utf16(ctx, "placement record label", 256)?;
    if label != "GRx" {
        return Err(CodecError::malformed(format_args!(
            "Inventor placement record label is {label:?}, expected \"GRx\""
        )));
    }
    require(
        u32::from(cursor.u16("placement invariant")?),
        1,
        "placement invariant",
    )?;
    let graphics_index = cursor.u32("placement graphics index")?;
    let object_reference = cursor.u32("placement object reference")?;
    let repeated_occurrence_id = cursor.u32("placement repeated occurrence id")?;
    require(
        repeated_occurrence_id,
        occurrence_id,
        "placement repeated occurrence id",
    )?;
    let suffix = cursor.remaining("placement suffix")?;
    Ok(AssemblyPlacement {
        segment_token: String::new(),
        record_ordinal: 0,
        header_id,
        owner_reference,
        attribute_reference,
        state,
        transform_prefix,
        transform,
        branch,
        graphics_state,
        occurrence_id,
        graphics_index,
        object_reference,
        suffix,
    })
}

fn require(actual: u32, expected: u32, field: &str) -> Result<(), CodecError> {
    if actual != expected {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} is {actual:#010x}, expected {expected:#010x}"
        )));
    }
    Ok(())
}

struct Cursor<'a> {
    source: View<'a>,
}

impl<'a> Cursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self { source }
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        crate::reader::u8(&mut self.source, field)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        crate::reader::u16(&mut self.source, field)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        crate::reader::u32(&mut self.source, field)
    }

    fn i32(&mut self, field: &'static str) -> Result<i32, CodecError> {
        crate::reader::i32(&mut self.source, field)
    }

    fn count32(&mut self, field: &'static str, maximum: usize) -> Result<usize, CodecError> {
        let value = usize::try_from(self.u32(field)?)
            .map_err(|_| CodecError::malformed(format_args!("Inventor {field} is too large")))?;
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "Inventor {field} value {value} exceeds {maximum}"
            )));
        }
        Ok(value)
    }

    fn utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
        maximum: usize,
    ) -> Result<String, CodecError> {
        let count = self.count32(field, maximum)?;
        let len = count.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor {field} length overflows"))
        })?;
        ctx.charge_retained(len as u64, "retain Inventor assembly string")?;
        self.source
            .utf16_le(count)
            .ok_or_else(|| CodecError::malformed(format_args!("Inventor {field} is not UTF-16")))
    }

    fn transform(&mut self) -> Result<(bool, CompactMatrix), CodecError> {
        let mut peek = self.source;
        let prefixed = peek.u32_le() == Some(0x0000_0203);
        if prefixed {
            self.source.skip(4).ok_or_else(|| {
                CodecError::Malformed("truncated Inventor placement transform prefix".into())
            })?;
        }
        let set = self.u16("placement transform set mask")?;
        let zero = self.u16("placement transform zero mask")?;
        let matrix = CompactMatrix::try_new(set, zero, |_| Ok(self.source.req_f64_le()?))?;
        Ok((prefixed, matrix))
    }

    fn remaining(&mut self, field: &'static str) -> Result<View<'a>, CodecError> {
        let view = self
            .source
            .child(self.source.position(), self.source.end())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("Inventor {field} range is invalid"))
            })?;
        self.source.seek(self.source.end()).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor {field} range is invalid"))
        })?;
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use crate::container::InventorContainer;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::primary_envelope_fixture;
    use crate::test_support::test_fixtures::push_u16;
    use crate::test_support::test_fixtures::push_u32;
    use crate::test_support::test_fixtures::push_utf16;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::products::PrototypeReference;

    use super::{
        inventory, parse_occurrence, parse_placement, OCCURRENCE_TYPE, PLACEMENT_TYPE_CA,
        SUPPRESSED_REFERENCE_STATE,
    };
    use crate::compact_matrix::CompactMatrix;
    use crate::native::ufrx::{ExternalReferenceRecord, UfrxOccurrenceRecord};
    use crate::native::{AssemblyOccurrenceRecord, AssemblyPlacementRecord};
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_ir::transform::Transform;
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    fn project_under_service(
        ufrx_occurrences: &[UfrxOccurrenceRecord],
        external_references: &[ExternalReferenceRecord],
        assembly_occurrences: &[AssemblyOccurrenceRecord],
        assembly_placements: &[AssemblyPlacementRecord],
    ) -> super::AssemblyProjection {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        super::project_occurrences(
            &ctx,
            ufrx_occurrences,
            external_references,
            assembly_occurrences,
            assembly_placements,
        )
        .expect("projection fits service policy")
    }

    #[test]
    fn occurrence_projection_refuses_collection_limit_before_output() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 7;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project_occurrences(
                &ctx,
                &[ufrx_occurrence(4, 7, 0)],
                &[external_reference(4, "part.ipt", [0, 0])],
                &[assembly_occurrence(7)],
                &[assembly_placement(7)],
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "project Inventor occurrence"
        ));
        assert_eq!(
            project_under_service(
                &[ufrx_occurrence(4, 7, 0)],
                &[external_reference(4, "part.ipt", [0, 0])],
                &[assembly_occurrence(7)],
                &[assembly_placement(7)],
            )
            .occurrences
            .len(),
            1
        );
    }

    #[test]
    fn occurrence_projection_refuses_entity_limit_before_creation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project_occurrences(
                &ctx,
                &[ufrx_occurrence(4, 7, 0)],
                &[external_reference(4, "part.ipt", [0, 0])],
                &[assembly_occurrence(7)],
                &[assembly_placement(7)],
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor occurrence"
        ));
        assert_eq!(
            project_under_service(
                &[ufrx_occurrence(4, 7, 0)],
                &[external_reference(4, "part.ipt", [0, 0])],
                &[assembly_occurrence(7)],
                &[assembly_placement(7)],
            )
            .occurrences
            .len(),
            1
        );
    }

    fn inventory_with_record(
        kind: SegmentKind,
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<
        (
            usize,
            usize,
            Vec<crate::record_issue::RecordIssue>,
            Option<String>,
        ),
        CodecError,
    > {
        let bytes = primary_envelope_fixture();
        let payload = payload.to_vec();
        let arena = DecodeArena::new();
        let (setup_ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("envelope view");
        let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
        let segment = &mut container.rse.segments[0];
        segment.kind = kind;
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            panic!("framed bulk fixture");
        };
        let RecordFrameState::Framed(table) = &mut bulk.records else {
            panic!("framed record fixture");
        };
        table.records[0].type_id = type_id;
        table.records[0].payload = View::over_retained(&payload);
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
        let result = inventory(&ctx, &container.rse)?;
        let token = result
            .occurrences
            .first()
            .map(|record| record.segment_token.clone())
            .or_else(|| {
                result
                    .placements
                    .first()
                    .map(|record| record.segment_token.clone())
            });
        Ok((
            result.occurrences.len(),
            result.placements.len(),
            result.issues,
            token,
        ))
    }

    #[test]
    fn assembly_record_forms_refuse_collection_limit_before_push() {
        for (kind, type_id, payload, operation) in [
            (
                SegmentKind::AmDc,
                OCCURRENCE_TYPE,
                occurrence_fixture(7, &[]),
                "admit Inventor assembly occurrence record",
            ),
            (
                SegmentKind::AmGraphics,
                PLACEMENT_TYPE_CA,
                placement_fixture(7, false, 0x8421, 0x7bde, &[]),
                "admit Inventor assembly placement record",
            ),
        ] {
            let admitted =
                inventory_with_record(kind.clone(), type_id, &payload, DecodePolicy::service())
                    .expect("assembly record is admitted");
            assert_eq!(admitted.0 + admitted.1, 1);
            assert!(admitted.2.is_empty());
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            assert!(matches!(
                inventory_with_record(kind, type_id, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
                        && limit.used == 0
            ));
        }
    }

    #[test]
    fn assembly_related_references_refuse_collection_limit_before_allocation() {
        let payload = occurrence_fixture(7, &[8]);
        assert_eq!(
            inventory_with_record(
                SegmentKind::AmDc,
                OCCURRENCE_TYPE,
                &payload,
                DecodePolicy::service()
            )
            .expect("related reference is admitted")
            .0,
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(SegmentKind::AmDc, OCCURRENCE_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor occurrence related references"
                    && limit.used == 0
        ));
    }

    #[test]
    fn assembly_parse_issue_refuses_collection_limit_before_push() {
        assert_eq!(
            inventory_with_record(
                SegmentKind::AmDc,
                OCCURRENCE_TYPE,
                &[],
                DecodePolicy::service()
            )
            .expect("truncated occurrence becomes an issue")
            .2
            .len(),
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(SegmentKind::AmDc, OCCURRENCE_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor assembly issue"
                    && limit.used == 0
        ));
    }

    #[test]
    fn assembly_record_tokens_refuse_retained_limit_before_copy() {
        for (kind, type_id, payload, operation) in [
            (
                SegmentKind::AmDc,
                OCCURRENCE_TYPE,
                occurrence_fixture(7, &[]),
                "retain Inventor assembly occurrence token",
            ),
            (
                SegmentKind::AmGraphics,
                PLACEMENT_TYPE_CA,
                placement_fixture(7, false, 0x8421, 0x7bde, &[]),
                "retain Inventor assembly placement token",
            ),
        ] {
            let admitted =
                inventory_with_record(kind.clone(), type_id, &payload, DecodePolicy::service())
                    .expect("assembly record is admitted");
            let token_len = admitted.3.as_deref().expect("record token").len();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = (6 + token_len - 1) as u64;
            assert!(matches!(
                inventory_with_record(kind, type_id, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                    && limit.used == 6
            ));
        }
    }

    #[test]
    fn assembly_issue_copies_refuse_retained_limits_before_creation() {
        let admitted = inventory_with_record(
            SegmentKind::AmDc,
            OCCURRENCE_TYPE,
            &[],
            DecodePolicy::service(),
        )
        .expect("truncated occurrence becomes an issue");
        let detail_len = admitted.2[0].detail.len();
        let token_len = admitted.2[0].segment_token.len();
        for (limit_bytes, operation, used) in [
            (detail_len - 1, "retain Inventor assembly issue detail", 0),
            (
                detail_len + token_len - 1,
                "retain Inventor assembly issue token",
                detail_len,
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit_bytes as u64;
            assert!(matches!(
                inventory_with_record(SegmentKind::AmDc, OCCURRENCE_TYPE, &[], policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == used as u64
            ));
        }
    }

    #[test]
    fn frames_occurrence_identity_and_variable_related_references() {
        for related in [Vec::new(), vec![0x8000_0008]] {
            let bytes = occurrence_fixture(42, &related);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic occurrence fits policy");
            let occurrence = parse_occurrence(&ctx, root).expect("synthetic occurrence parses");
            assert_eq!(occurrence.occurrence_id, 42);
            assert_eq!(occurrence.related_references, related);
            assert_eq!(occurrence.header_value, 6);
        }
    }

    #[test]
    fn expands_compact_identity_and_translation_placements() {
        let identity = placement_fixture(7, false, 0x8421, 0x7bde, &[]);
        let translated = placement_fixture(8, true, 0x8124, 0x7657, &[1.0, 2.0, 3.0]);
        for (bytes, expected, prefixed) in [
            (identity, [0.0; 3], false),
            (translated, [1.0, 2.0, 3.0], true),
        ] {
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic placement fits policy");
            let placement = parse_placement(&ctx, root).expect("synthetic placement parses");
            assert_eq!(placement.transform_prefix, prefixed);
            assert_eq!(placement.transform.rows()[0][3], expected[0]);
            assert_eq!(placement.transform.rows()[1][3], expected[1]);
            assert_eq!(placement.transform.rows()[2][3], expected[2]);
            assert_eq!(placement.transform.rows()[3][3], 1.0);
        }

        let rotation = placement_fixture(9, false, 0x8412, 0x7bef, &[]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&rotation, &arena, &DecodePolicy::default())
                .expect("synthetic rotation fits policy");
        let placement = parse_placement(&ctx, root).expect("synthetic rotation parses");
        assert_eq!(placement.transform.rows()[0][1], -1.0);
        assert_eq!(placement.transform.rows()[1][0], 1.0);
    }

    #[test]
    fn projects_external_occurrence_and_converts_translation_to_millimetres() {
        let ufrx = ufrx_occurrence(4, 7, 2);
        let reference = external_reference(4, "components/part.ipt", [0, 0]);
        let occurrence = assembly_occurrence(7);
        let mut placement = assembly_placement(7);
        let mut rows = placement.transform.rows();
        rows[0][3] = 1.25;
        rows[1][3] = -2.0;
        placement.transform =
            CompactMatrix::try_from_rows(0, 0, rows).expect("finite explicit matrix fixture");

        let projection = project_under_service(&[ufrx], &[reference], &[occurrence], &[placement]);

        assert!(projection.unresolved_placements.is_empty());
        let [projected] = projection.occurrences.as_slice() else {
            panic!("one occurrence must be projected");
        };
        assert_eq!(projected.ordinal, 2);
        assert_eq!(projected.transform.rows()[0][3], 12.5);
        assert_eq!(projected.transform.rows()[1][3], -20.0);
        let PrototypeReference::External { document, object } = &projected.prototype else {
            panic!("the persisted file reference must remain external");
        };
        assert_eq!(
            (match &document {
                cadmpeg_ir::products::ExternalDocument::Path { path } => Some(path.as_str()),
                _ => None,
            }),
            Some("components/part.ipt")
        );
        assert_eq!(
            (match &document {
                cadmpeg_ir::products::ExternalDocument::DocumentId { document_id } =>
                    Some(document_id.as_str()),
                _ => None,
            }),
            None
        );
        assert!(!matches!(
            document,
            cadmpeg_ir::products::ExternalDocument::Missing {}
        ));
        assert_eq!(object, &None);
    }

    #[test]
    fn projects_repeated_occurrences_with_distinct_stable_identity() {
        let ufrx = [ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)];
        let reference = external_reference(4, "components/part.ipt", [0, 0]);
        let occurrences = [assembly_occurrence(7), assembly_occurrence(8)];
        let mut first = assembly_placement(7);
        let mut rows = first.transform.rows();
        rows[0][3] = 1.0;
        first.transform =
            CompactMatrix::try_from_rows(0, 0, rows).expect("finite explicit matrix fixture");
        let mut second = assembly_placement(8);
        let mut rows = second.transform.rows();
        rows[0][3] = 2.0;
        second.transform =
            CompactMatrix::try_from_rows(0, 0, rows).expect("finite explicit matrix fixture");

        let projection = project_under_service(&ufrx, &[reference], &occurrences, &[first, second]);

        assert!(projection.unresolved_placements.is_empty());
        assert_eq!(projection.occurrences.len(), 2);
        assert_ne!(projection.occurrences[0].id, projection.occurrences[1].id);
        assert_ne!(
            projection.occurrences[0].transform,
            projection.occurrences[1].transform
        );
    }

    #[test]
    fn path_identity_takes_precedence_on_external_prototypes() {
        let reference = external_reference_with_document_id(
            4,
            "components/part.ipt",
            [0, 0],
            "00112233445566778899aabbccddeeff",
        );
        let projection = project_under_service(
            &[ufrx_occurrence(4, 7, 0)],
            &[reference],
            &[assembly_occurrence(7)],
            &[assembly_placement(7)],
        );

        let [projected] = projection.occurrences.as_slice() else {
            panic!("one occurrence must be projected");
        };
        let PrototypeReference::External { document, .. } = &projected.prototype else {
            panic!("the persisted document identity must remain external");
        };
        assert_eq!(
            (match &document {
                cadmpeg_ir::products::ExternalDocument::Path { path } => Some(path.as_str()),
                _ => None,
            }),
            Some("components/part.ipt")
        );
        assert_eq!(
            (match &document {
                cadmpeg_ir::products::ExternalDocument::DocumentId { document_id } =>
                    Some(document_id.as_str()),
                _ => None,
            }),
            None
        );
    }

    #[test]
    fn projects_suppressed_occurrence_without_graphics_placement() {
        let ufrx = ufrx_occurrence(4, 7, 0);
        let expected_document_id = "00112233445566778899aabbccddeeff".to_owned();
        let reference = external_reference_with_document_id(
            4,
            "",
            [SUPPRESSED_REFERENCE_STATE, 0],
            &expected_document_id,
        );
        let occurrence = assembly_occurrence(7);

        let projection = project_under_service(&[ufrx], &[reference], &[occurrence], &[]);

        assert!(projection.unresolved_placements.is_empty());
        let [projected] = projection.occurrences.as_slice() else {
            panic!("one suppressed occurrence must be projected");
        };
        assert_eq!(projected.transform, Transform::identity());
        assert_eq!(projected.visible, Some(false));
        let PrototypeReference::External { document, .. } = &projected.prototype else {
            panic!("the persisted document identity must remain external");
        };
        assert_eq!(
            (match &document {
                cadmpeg_ir::products::ExternalDocument::Path { path } => Some(path.as_str()),
                _ => None,
            }),
            None
        );
        assert_eq!(
            (match &document {
                cadmpeg_ir::products::ExternalDocument::DocumentId { document_id } =>
                    Some(document_id.as_str()),
                _ => None,
            }),
            Some(expected_document_id.as_str())
        );
    }

    #[test]
    fn reports_active_occurrence_without_placement() {
        let projection = project_under_service(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7)],
            &[],
        );

        assert!(projection.occurrences.is_empty());
        assert_eq!(
            projection.unresolved_placements,
            BTreeMap::from([(super::UnresolvedCause::Placement, NonZeroUsize::MIN)])
        );
    }

    fn ufrx_occurrence(
        file_reference_id: u32,
        occurrence_id: u32,
        ordinal: u32,
    ) -> UfrxOccurrenceRecord {
        UfrxOccurrenceRecord::try_from(crate::native::ufrx::UfrxOccurrenceRecordWire {
            id: format!("inventor:ufrx:occurrence#{ordinal}"),
            ordinal,
            end_string_flag: 0,
            file_reference_id,
            occurrence_id,
            header_value: 0,
            title: Some("placed part".into()),
            header_padding_words: 0,
            record_len: 1,
            record_sha256: "0".repeat(64),
        })
        .expect("valid native record fixture")
    }

    fn external_reference(
        reference_id: u32,
        path: &str,
        state: [u16; 2],
    ) -> ExternalReferenceRecord {
        external_reference_with_document_id(reference_id, path, state, &"0".repeat(32))
    }

    fn external_reference_with_document_id(
        reference_id: u32,
        path: &str,
        state: [u16; 2],
        document_id: &str,
    ) -> ExternalReferenceRecord {
        ExternalReferenceRecord::try_from(crate::native::ufrx::ExternalReferenceRecordWire {
            id: format!("inventor:ufrx:external-reference#{reference_id}"),
            ordinal: reference_id,
            path: path.into(),
            library_id: 0,
            library_name: String::new(),
            display_name: String::new(),
            state_groups: Vec::new(),
            state,
            document_id: Some(document_id.into()),
            database_id: "0".repeat(32),
            reference_id,
            occurrence_count: 1,
            version: 0,
            flags: 0,
        })
        .expect("valid reference fixture")
    }

    fn assembly_occurrence(occurrence_id: u32) -> AssemblyOccurrenceRecord {
        AssemblyOccurrenceRecord {
            id: format!("inventor:assembly:occurrence#{occurrence_id}"),
            segment_token: "synthetic".into(),
            record_ordinal: occurrence_id,
            header_value: 0,
            header_id: 0,
            next_reference: 0,
            flags: 0,
            owner_reference: 0,
            node_index: 0,
            state: [0, 0],
            ordinal_key: occurrence_id,
            related_references: Vec::new(),
            child_reference: 0,
            occurrence_id,
        }
    }

    fn assembly_placement(occurrence_id: u32) -> AssemblyPlacementRecord {
        AssemblyPlacementRecord::try_from(crate::native::AssemblyPlacementRecordWire {
            id: format!("inventor:assembly:placement#{occurrence_id}"),
            segment_token: "synthetic".into(),
            record_ordinal: occurrence_id,
            header_id: 0,
            owner_reference: 0,
            attribute_reference: 0,
            state: 0,
            transform_prefix: false,
            transform: CompactMatrix::try_from_rows(0, 0, Transform::identity().rows())
                .expect("finite explicit matrix fixture"),
            branch: 0,
            graphics_state: 0,
            occurrence_id,
            graphics_index: 0,
            object_reference: 0,
            suffix_len: 48,
            suffix_sha256: "0".repeat(64),
        })
        .expect("valid placement fixture")
    }

    fn occurrence_fixture(occurrence_id: u32, related: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 6);
        push_u16(&mut bytes, 31);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0x0200);
        push_u32(&mut bytes, 0x8000_0003);
        push_u32(&mut bytes, 9);
        push_u32(&mut bytes, u32::MAX);
        push_u32(&mut bytes, u32::MAX);
        push_u32(&mut bytes, 0x3000_0002);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 5);
        push_u32(&mut bytes, 0x3000_0002);
        push_u32(&mut bytes, related.len() as u32);
        if !related.is_empty() {
            push_u32(&mut bytes, 1);
            push_u32(&mut bytes, 0);
            for reference in related {
                push_u32(&mut bytes, *reference);
            }
        }
        push_u32(&mut bytes, 0x8000_004c);
        push_u16(&mut bytes, 0x0200);
        push_u32(&mut bytes, occurrence_id);
        push_utf16(&mut bytes, "DCx");
        push_u16(&mut bytes, 1);
        bytes
    }

    fn placement_fixture(
        occurrence_id: u32,
        prefixed: bool,
        set: u16,
        zero: u16,
        values: &[f64],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 0xa2);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        bytes.push(1);
        if prefixed {
            push_u32(&mut bytes, 0x0000_0203);
        }
        push_u16(&mut bytes, set);
        push_u16(&mut bytes, zero);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0);
        bytes.push(7);
        push_u32(&mut bytes, occurrence_id);
        push_utf16(&mut bytes, "GRx");
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, 9);
        push_u32(&mut bytes, 0x8000_000b);
        push_u32(&mut bytes, occurrence_id);
        bytes.extend_from_slice(&[0; 48]);
        bytes
    }
}
