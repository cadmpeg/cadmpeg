// SPDX-License-Identifier: Apache-2.0
//! Typed assembly occurrence and placement records.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{
    ExternalDocumentReference, ExternalResolution, Occurrence, OccurrenceParent, PrototypeReference,
};
use cadmpeg_ir::transform::Transform;

use crate::native::{
    AssemblyOccurrenceRecord, AssemblyPlacementRecord, ExternalReferenceRecord,
    UfrxOccurrenceRecord,
};
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
    pub(crate) issues: Vec<AssemblyRecordIssue>,
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
    pub(crate) transform_encoding: [u16; 2],
    pub(crate) transform: [[f64; 4]; 4],
    pub(crate) branch: u8,
    pub(crate) graphics_state: u8,
    pub(crate) occurrence_id: u32,
    pub(crate) graphics_index: u32,
    pub(crate) object_reference: u32,
    pub(crate) suffix: View<'a>,
}

struct CompactTransform {
    prefixed: bool,
    encoding: [u16; 2],
    matrix: [[f64; 4]; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssemblyRecordIssue {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

#[derive(Debug)]
pub(crate) struct AssemblyProjection {
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) unresolved_placements: usize,
}

/// Projects the current document's occurrence table without loading prototypes.
///
/// `UFRx` supplies document order and the external prototype join. `AmDc` proves
/// the occurrence identity, while `AmGraphics` supplies the placement. Referenced
/// assemblies remain one unresolved external prototype, so their internal trees
/// are not invented as children of this document.
pub(crate) fn project_occurrences(
    ufrx_occurrences: &[UfrxOccurrenceRecord],
    external_references: &[ExternalReferenceRecord],
    assembly_occurrences: &[AssemblyOccurrenceRecord],
    assembly_placements: &[AssemblyPlacementRecord],
) -> AssemblyProjection {
    let references = unique_by(external_references, |record| record.reference_id);
    let occurrence_records = unique_by(assembly_occurrences, |record| record.occurrence_id);
    let placements = unique_by(assembly_placements, |record| record.occurrence_id);
    let mut emitted_ids = HashSet::new();
    let mut occurrences = Vec::new();
    let mut unresolved_placements = 0;

    for source in ufrx_occurrences {
        let Some(reference) = references.get(&source.file_reference_id) else {
            unresolved_placements += 1;
            continue;
        };
        if !occurrence_records.contains_key(&source.occurrence_id)
            || !emitted_ids.insert(source.occurrence_id)
        {
            unresolved_placements += 1;
            continue;
        }

        let suppressed = reference.state[0] & SUPPRESSED_REFERENCE_STATE != 0;
        let (transform, visible) = match placements.get(&source.occurrence_id) {
            Some(placement) => {
                let mut rows = placement.transform;
                for row in rows.iter_mut().take(3) {
                    row[3] *= INVENTOR_LENGTH_TO_MILLIMETRES;
                }
                let transform = Transform { rows };
                if !transform.is_affine() {
                    unresolved_placements += 1;
                    continue;
                }
                (transform, suppressed.then_some(false))
            }
            None if suppressed => (Transform::identity(), Some(false)),
            None => {
                unresolved_placements += 1;
                continue;
            }
        };

        occurrences.push(Occurrence {
            id: OccurrenceId(format!(
                "inventor:assembly:instance#{}",
                source.occurrence_id
            )),
            prototype: external_prototype(reference),
            parent: OccurrenceParent::Root,
            ordinal: source.ordinal,
            transform,
            prototype_transform: Transform::identity(),
            scale: [1.0; 3],
            name: source.title.clone().filter(|title| !title.is_empty()),
            linked_subelements: Vec::new(),
            visible,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: None,
            native_ref: Some(source.id.clone()),
        });
    }

    AssemblyProjection {
        occurrences,
        unresolved_placements,
    }
}

fn unique_by<T, K>(records: &[T], key: impl Fn(&T) -> K) -> HashMap<K, &T>
where
    K: Eq + std::hash::Hash + Copy,
{
    let mut unique = HashMap::new();
    let mut duplicates = HashSet::new();
    for record in records {
        let key = key(record);
        if unique.insert(key, record).is_some() {
            duplicates.insert(key);
        }
    }
    for duplicate in duplicates {
        unique.remove(&duplicate);
    }
    unique
}

fn external_prototype(reference: &ExternalReferenceRecord) -> PrototypeReference {
    let path = (!reference.path.is_empty()).then(|| reference.path.clone());
    let document_id = reference
        .document_id
        .chars()
        .any(|character| character != '0')
        .then(|| reference.document_id.clone());
    if path.is_none() && document_id.is_none() {
        return PrototypeReference::Unresolved;
    }
    PrototypeReference::External {
        document: ExternalDocumentReference {
            path,
            document_id,
            resolution: ExternalResolution::Unresolved,
        },
        object: None,
    }
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
                parse_occurrence(ctx, record.payload).map(|mut occurrence| {
                    occurrence.segment_token = segment.pair.token.as_str().into();
                    occurrence.record_ordinal = record.ordinal;
                    occurrences.push(occurrence);
                })
            } else if segment.kind == SegmentKind::AmGraphics
                && matches!(record.type_id, PLACEMENT_TYPE_CA | PLACEMENT_TYPE_B9)
            {
                parse_placement(ctx, record.payload).map(|mut placement| {
                    placement.segment_token = segment.pair.token.as_str().into();
                    placement.record_ordinal = record.ordinal;
                    placements.push(placement);
                })
            } else {
                continue;
            };
            if let Err(error) = result {
                issues.push(AssemblyRecordIssue {
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    ctx.charge_collection_items(
        occurrences
            .len()
            .saturating_add(placements.len())
            .saturating_add(issues.len()) as u64,
        "admit Inventor assembly records",
    )?;
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
    let compact_transform = cursor.transform()?;
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
        transform_prefix: compact_transform.prefixed,
        transform_encoding: compact_transform.encoding,
        transform: compact_transform.matrix,
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

    #[allow(dead_code)] // Retained for framed RSe walks that still use the helper.
    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], CodecError> {
        Ok(self
            .source
            .req_take(len)
            .map_err(|error| error.during(field))?)
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        Ok(self.source.req_u8().map_err(|error| error.during(field))?)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        Ok(self
            .source
            .req_u16_le()
            .map_err(|error| error.during(field))?)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        Ok(self
            .source
            .req_u32_le()
            .map_err(|error| error.during(field))?)
    }

    fn i32(&mut self, field: &'static str) -> Result<i32, CodecError> {
        Ok(self
            .source
            .req_i32_le()
            .map_err(|error| error.during(field))?)
    }

    fn f64(&mut self, field: &'static str) -> Result<f64, CodecError> {
        let value = self
            .source
            .req_f64_le()
            .map_err(|error| error.during(field))?;
        if !value.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "Inventor {field} is not finite"
            )));
        }
        Ok(value)
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
        ctx.charge_retained(len as u64, "retain Inventor assembly string", None)?;
        self.source
            .utf16_le(count)
            .ok_or_else(|| CodecError::malformed(format_args!("Inventor {field} is not UTF-16")))
    }

    fn transform(&mut self) -> Result<CompactTransform, CodecError> {
        let mut peek = self.source;
        let prefixed = peek.u32_le() == Some(0x0000_0203);
        if prefixed {
            self.source.skip(4).ok_or_else(|| {
                CodecError::Malformed("truncated Inventor placement transform prefix".into())
            })?;
        }
        let set = self.u16("placement transform set mask")?;
        let zero = self.u16("placement transform zero mask")?;
        let mut rows = [[0.0; 4]; 4];
        for (index, value) in rows.iter_mut().flatten().enumerate() {
            let bit = 1_u16 << index;
            *value = if zero & bit == 0 {
                if set & bit == 0 {
                    self.f64("placement transform value")?
                } else {
                    1.0
                }
            } else if set & bit == 0 {
                0.0
            } else {
                -1.0
            };
        }
        Ok(CompactTransform {
            prefixed,
            encoding: [set, zero],
            matrix: rows,
        })
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
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_ir::products::{ExternalResolution, PrototypeReference};

    use super::*;

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
            assert_eq!(placement.transform[0][3], expected[0]);
            assert_eq!(placement.transform[1][3], expected[1]);
            assert_eq!(placement.transform[2][3], expected[2]);
            assert_eq!(placement.transform[3][3], 1.0);
        }

        let rotation = placement_fixture(9, false, 0x8412, 0x7bef, &[]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&rotation, &arena, &DecodePolicy::default())
                .expect("synthetic rotation fits policy");
        let placement = parse_placement(&ctx, root).expect("synthetic rotation parses");
        assert_eq!(placement.transform[0][1], -1.0);
        assert_eq!(placement.transform[1][0], 1.0);
    }

    #[test]
    fn projects_external_occurrence_and_converts_translation_to_millimetres() {
        let ufrx = ufrx_occurrence(4, 7, 2);
        let reference = external_reference(4, "components/part.ipt", [0, 0]);
        let occurrence = assembly_occurrence(7);
        let mut placement = assembly_placement(7);
        placement.transform[0][3] = 1.25;
        placement.transform[1][3] = -2.0;

        let projection = project_occurrences(&[ufrx], &[reference], &[occurrence], &[placement]);

        assert_eq!(projection.unresolved_placements, 0);
        let [projected] = projection.occurrences.as_slice() else {
            panic!("one occurrence must be projected");
        };
        assert_eq!(projected.ordinal, 2);
        assert_eq!(projected.transform.rows[0][3], 12.5);
        assert_eq!(projected.transform.rows[1][3], -20.0);
        let PrototypeReference::External { document, object } = &projected.prototype else {
            panic!("the persisted file reference must remain external");
        };
        assert_eq!(document.path.as_deref(), Some("components/part.ipt"));
        assert_eq!(document.document_id, None);
        assert_eq!(document.resolution, ExternalResolution::Unresolved);
        assert_eq!(object, &None);
    }

    #[test]
    fn projects_repeated_occurrences_with_distinct_stable_identity() {
        let ufrx = [ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)];
        let reference = external_reference(4, "components/part.ipt", [0, 0]);
        let occurrences = [assembly_occurrence(7), assembly_occurrence(8)];
        let mut first = assembly_placement(7);
        first.transform[0][3] = 1.0;
        let mut second = assembly_placement(8);
        second.transform[0][3] = 2.0;

        let projection = project_occurrences(&ufrx, &[reference], &occurrences, &[first, second]);

        assert_eq!(projection.unresolved_placements, 0);
        assert_eq!(projection.occurrences.len(), 2);
        assert_ne!(projection.occurrences[0].id, projection.occurrences[1].id);
        assert_ne!(
            projection.occurrences[0].transform,
            projection.occurrences[1].transform
        );
    }

    #[test]
    fn preserves_path_and_document_id_on_external_prototypes() {
        let mut reference = external_reference(4, "components/part.ipt", [0, 0]);
        reference.document_id = "00112233445566778899aabbccddeeff".into();
        let projection = project_occurrences(
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
        assert_eq!(document.path.as_deref(), Some("components/part.ipt"));
        assert_eq!(
            document.document_id.as_deref(),
            Some("00112233445566778899aabbccddeeff")
        );
    }

    #[test]
    fn projects_suppressed_occurrence_without_graphics_placement() {
        let ufrx = ufrx_occurrence(4, 7, 0);
        let mut reference = external_reference(4, "", [SUPPRESSED_REFERENCE_STATE, 0]);
        reference.document_id = "00112233445566778899aabbccddeeff".into();
        let expected_document_id = reference.document_id.clone();
        let occurrence = assembly_occurrence(7);

        let projection = project_occurrences(&[ufrx], &[reference], &[occurrence], &[]);

        assert_eq!(projection.unresolved_placements, 0);
        let [projected] = projection.occurrences.as_slice() else {
            panic!("one suppressed occurrence must be projected");
        };
        assert_eq!(projected.transform, Transform::identity());
        assert_eq!(projected.visible, Some(false));
        let PrototypeReference::External { document, .. } = &projected.prototype else {
            panic!("the persisted document identity must remain external");
        };
        assert_eq!(document.path, None);
        assert_eq!(
            document.document_id.as_deref(),
            Some(expected_document_id.as_str())
        );
    }

    #[test]
    fn reports_active_occurrence_without_placement() {
        let projection = project_occurrences(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7)],
            &[],
        );

        assert!(projection.occurrences.is_empty());
        assert_eq!(projection.unresolved_placements, 1);
    }

    fn ufrx_occurrence(
        file_reference_id: u32,
        occurrence_id: u32,
        ordinal: u32,
    ) -> UfrxOccurrenceRecord {
        UfrxOccurrenceRecord {
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
        }
    }

    fn external_reference(
        reference_id: u32,
        path: &str,
        state: [u16; 2],
    ) -> ExternalReferenceRecord {
        ExternalReferenceRecord {
            id: format!("inventor:ufrx:external-reference#{reference_id}"),
            ordinal: reference_id,
            path: path.into(),
            library_id: 0,
            library_name: String::new(),
            display_name: String::new(),
            state_groups: Vec::new(),
            state,
            document_id: "0".repeat(32),
            database_id: "0".repeat(32),
            reference_id,
            occurrence_count: 1,
            version: 0,
            flags: 0,
        }
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
        AssemblyPlacementRecord {
            id: format!("inventor:assembly:placement#{occurrence_id}"),
            segment_token: "synthetic".into(),
            record_ordinal: occurrence_id,
            header_id: 0,
            owner_reference: 0,
            attribute_reference: 0,
            state: 0,
            transform_prefix: false,
            transform_encoding: [0, 0],
            transform: Transform::identity().rows,
            branch: 0,
            graphics_state: 0,
            occurrence_id,
            graphics_index: 0,
            object_reference: 0,
            suffix_len: 0,
            suffix_sha256: "0".repeat(64),
        }
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

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        push_u32(bytes, units.len() as u32);
        for unit in units {
            push_u16(bytes, unit);
        }
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
