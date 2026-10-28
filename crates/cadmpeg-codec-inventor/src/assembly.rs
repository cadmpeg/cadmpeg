// SPDX-License-Identifier: Apache-2.0
//! Typed assembly occurrence and placement records.

use cadmpeg_core::decode::unique_index;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointId, JointOperand, Occurrence, OccurrenceParent,
    PrototypeReference,
};
use cadmpeg_ir::transform::Transform;

use crate::compact_matrix::CompactMatrix;
use crate::native::ufrx::{ExternalReferenceRecord, UfrxOccurrenceRecord};
use crate::native::{
    AssemblyGroundingRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecord,
    DesignViewRepresentationRecord, DesignViewVisibilityOffRecord,
};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
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
const DESIGN_VIEW_REPRESENTATION_TYPE: [u8; 16] = [
    0x41, 0xa2, 0xc2, 0x9d, 0xd2, 0x11, 0xcc, 0x99, 0xa0, 0x00, 0x2a, 0xa9, 0x76, 0xd4, 0x56, 0xc9,
];
const VISIBILITY_OFF_TYPE: [u8; 16] = [
    0x21, 0x23, 0xc2, 0x71, 0xd2, 0x11, 0x87, 0xa9, 0xa0, 0x00, 0x2e, 0xa9, 0x76, 0xd4, 0x56, 0xc9,
];
const OCCURRENCE_GROUNDING_TYPE: [u8; 16] = [
    0x61, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const GROUNDED_FLAGS_BIT: u32 = 0x0200_0000;

#[derive(Debug)]
pub(crate) struct AssemblyInventory<'a> {
    pub(crate) occurrences: Vec<AssemblyOccurrence>,
    pub(crate) placements: Vec<AssemblyPlacement<'a>>,
    pub(crate) representations: Vec<DesignViewRepresentation<'a>>,
    pub(crate) visibility_off: Vec<DesignViewVisibilityOff<'a>>,
    pub(crate) groundings: Vec<OccurrenceGrounding<'a>>,
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

#[derive(Debug)]
pub(crate) struct DesignViewRepresentation<'a> {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) name: String,
    pub(crate) member_references: Vec<u32>,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug)]
pub(crate) struct DesignViewVisibilityOff<'a> {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) occurrence_path: Vec<u32>,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug)]
pub(crate) struct OccurrenceGrounding<'a> {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) next_reference: u32,
    pub(crate) flags: u32,
    pub(crate) suffix: View<'a>,
}

/// Records one more occurrence of `cause` in a non-zero tally.
pub(crate) fn count_unresolved<C: Ord>(
    counts: &mut BTreeMap<C, NonZeroUsize>,
    cause: C,
) -> Result<(), CodecError> {
    match counts.entry(cause) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(NonZeroUsize::MIN);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let count = entry.get().checked_add(1).ok_or_else(|| {
                CodecError::malformed("Inventor unresolved projection count overflows")
            })?;
            entry.insert(count);
        }
    }
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

/// The design-view and occurrence state records that qualify an occurrence.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct OccurrenceStates<'a> {
    pub(crate) active_design_view: Option<&'a str>,
    pub(crate) representations: &'a [DesignViewRepresentationRecord],
    pub(crate) visibility_off: &'a [DesignViewVisibilityOffRecord],
    pub(crate) groundings: &'a [AssemblyGroundingRecord],
}

#[derive(Debug)]
pub(crate) struct AssemblyProjection {
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) joints: Vec<AssemblyJoint>,
    /// Hidden occurrences below a root occurrence; they stay in native records.
    pub(crate) nested_hidden: usize,
    pub(crate) unresolved_placements: BTreeMap<UnresolvedCause, NonZeroUsize>,
}

/// Projects the current document's occurrence table without loading prototypes.
///
/// `UFRx` supplies document order and the external prototype join. `AmDc` proves
/// the occurrence identity, while `AmGraphics` supplies the placement. Referenced
/// assemblies remain one unresolved external prototype, so their internal trees
/// are not invented as children of this document.
///
/// The active design-view representation selects hidden occurrences through its
/// member visibility-off records. An occurrence grounding record flags a
/// grounded occurrence through the owning occurrence's references.
pub(crate) fn project_occurrences(
    ctx: &DecodeContext<'_>,
    ufrx_occurrences: &[UfrxOccurrenceRecord],
    external_references: &[ExternalReferenceRecord],
    assembly_occurrences: &[AssemblyOccurrenceRecord],
    assembly_placements: &[AssemblyPlacementRecord],
    states: OccurrenceStates<'_>,
) -> Result<AssemblyProjection, CodecError> {
    let occurrence_records = unique_index(
        assembly_occurrences
            .iter()
            .map(|record| (record.occurrence_id, record)),
    );
    let placements = unique_index(
        assembly_placements
            .iter()
            .map(|record| (record.occurrence_id, record)),
    );
    let hidden_paths = active_hidden_paths(
        states.active_design_view,
        states.representations,
        states.visibility_off,
    );
    let hidden_keys: HashSet<u32> = hidden_paths
        .iter()
        .filter_map(|path| match path {
            [key] => Some(*key),
            _ => None,
        })
        .collect();
    let nested_hidden = hidden_paths.iter().filter(|path| path.len() > 1).count();
    let grounding_records = unique_index(states.groundings.iter().map(|record| {
        (
            (record.segment_token.clone(), record.record_ordinal),
            record,
        )
    }));
    let mut emitted_ids = HashSet::new();
    let mut occurrences = Vec::new();
    let mut joints = Vec::new();
    let mut unresolved_placements = BTreeMap::new();

    for source in ufrx_occurrences {
        let Some(reference) =
            crate::native::ufrx::resolve_occurrence_reference(source, external_references)
        else {
            count_unresolved(
                &mut unresolved_placements,
                UnresolvedCause::ExternalReference,
            )?;
            continue;
        };
        let Some(occurrence_record) = occurrence_records
            .get(&source.occurrence_id)
            .and_then(Option::as_ref)
        else {
            count_unresolved(
                &mut unresolved_placements,
                UnresolvedCause::OccurrenceRecord,
            )?;
            continue;
        };
        if emitted_ids.contains(&source.occurrence_id) {
            count_unresolved(
                &mut unresolved_placements,
                UnresolvedCause::DuplicateOccurrence,
            )?;
            continue;
        }
        emitted_ids.insert(source.occurrence_id);

        let suppressed = reference.state[0] & SUPPRESSED_REFERENCE_STATE != 0;
        let hidden = hidden_keys.contains(&source.occurrence_key);
        let invisible = suppressed || hidden;
        let (transform, visible) = match placements
            .get(&source.occurrence_id)
            .and_then(Option::as_ref)
        {
            Some(placement) => {
                let source = placement.transform.checked_rows();
                let mut rows = [source[0], source[1], source[2]];
                let Some(transform) = (source[3].map(cadmpeg_ir::scalar::FiniteReal::get)
                    == [0.0, 0.0, 0.0, 1.0])
                .then(|| {
                    for row in &mut rows {
                        row[3] = cadmpeg_ir::scalar::FiniteReal::new(
                            row[3].get() * INVENTOR_LENGTH_TO_MILLIMETRES,
                        )?;
                    }
                    Some(Transform::from_finite_rows(rows))
                })
                .flatten() else {
                    count_unresolved(
                        &mut unresolved_placements,
                        UnresolvedCause::InvalidTransform,
                    )?;
                    continue;
                };
                (transform, invisible.then_some(false))
            }
            None if suppressed => (Transform::identity(), Some(false)),
            None => {
                count_unresolved(&mut unresolved_placements, UnresolvedCause::Placement)?;
                continue;
            }
        };

        ctx.charge_entities(1, "project Inventor occurrence")?;
        let id = OccurrenceId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "assembly", "instance"),
            source.occurrence_id,
        );
        occurrences.push(Occurrence {
            id: id.clone(),
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
        if let Some(grounding) = grounded_record(occurrence_record, &grounding_records) {
            ctx.charge_entities(1, "project Inventor grounded joint")?;
            let object = source
                .component_name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| source.occurrence_id.to_string());
            let mut joint = AssemblyJoint::grounded(
                JointId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "assembly", "joint"),
                    source.occurrence_id,
                ),
                JointConnector {
                    operand: JointOperand::occurrence(id, object, Vec::new()),
                    frame: transform,
                    detached: false,
                },
                None,
            );
            joint.native_ref = Some(grounding.id.clone());
            joints.push(joint);
        }
    }

    Ok(AssemblyProjection {
        occurrences,
        joints,
        nested_hidden,
        unresolved_placements,
    })
}

/// Resolves a one-based same-segment record reference to a record ordinal.
///
/// Bit 31 is a reference qualifier and does not change the index arithmetic.
/// A zero index is null.
fn record_reference_ordinal(reference: u32) -> Option<u32> {
    if reference == 0 {
        return None;
    }
    (reference & 0x7fff_ffff).checked_sub(1)
}

/// Collects the occurrence keys the active design-view representation hides.
///
/// Only the representation the `UFRxDoc` header names as the active design
/// view contributes. Member references that do not
/// select a visibility-off state record in the same segment hide nothing.
fn active_hidden_paths<'a>(
    active_design_view: Option<&str>,
    representations: &[DesignViewRepresentationRecord],
    visibility_off: &'a [DesignViewVisibilityOffRecord],
) -> Vec<&'a [u32]> {
    let Some(name) = active_design_view else {
        return Vec::new();
    };
    let matched: Vec<&DesignViewRepresentationRecord> = representations
        .iter()
        .filter(|representation| representation.name == name)
        .collect();
    let [representation] = matched.as_slice() else {
        return Vec::new();
    };
    let members: HashSet<(String, u32)> = representation
        .member_references
        .iter()
        .filter_map(|reference| {
            record_reference_ordinal(*reference)
                .map(|ordinal| (representation.segment_token.clone(), ordinal))
        })
        .collect();
    visibility_off
        .iter()
        .filter(|state| members.contains(&(state.segment_token.clone(), state.record_ordinal)))
        .map(|state| state.occurrence_path.as_slice())
        .collect()
}

/// Selects the grounding record that grounds an occurrence.
///
/// The occurrence's related references select its state records in the same
/// segment. Only a state record whose flags carry the grounded bit grounds.
fn grounded_record<'b>(
    occurrence: &AssemblyOccurrenceRecord,
    groundings: &HashMap<(String, u32), Option<&'b AssemblyGroundingRecord>>,
) -> Option<&'b AssemblyGroundingRecord> {
    occurrence
        .related_references
        .iter()
        .filter_map(|reference| {
            record_reference_ordinal(*reference)
                .map(|ordinal| (occurrence.segment_token.clone(), ordinal))
        })
        .filter_map(|key| groundings.get(&key).and_then(Option::as_ref).copied())
        .find(|grounding| grounding.flags & GROUNDED_FLAGS_BIT != 0)
}

pub(crate) fn inventory<'a>(
    ctx: &DecodeContext<'a>,
    document: &RseInventory<'a>,
) -> Result<AssemblyInventory<'a>, CodecError> {
    let mut occurrences = Vec::new();
    let mut placements = Vec::new();
    let mut representations = Vec::new();
    let mut visibility_off = Vec::new();
    let mut groundings = Vec::new();
    let mut issues = Vec::new();
    for segment in &document.segments {
        let relevant = matches!(
            segment.kind,
            SegmentKind::AmDc | SegmentKind::AmGraphics | SegmentKind::DesignView
        );
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
                let version = segment.registry.map_or(0, |join| join.version_major);
                parse_occurrence(ctx, record.payload, version).map(|mut occurrence| {
                    occurrence.segment_token = segment.pair.token.as_str().into();
                    occurrence.record_ordinal = record.ordinal;
                    occurrences.push(occurrence);
                })
            } else if segment.kind == SegmentKind::AmDc
                && record.type_id == OCCURRENCE_GROUNDING_TYPE
            {
                parse_grounding(record.payload).map(|mut grounding| {
                    grounding.segment_token = segment.pair.token.as_str().into();
                    grounding.record_ordinal = record.ordinal;
                    groundings.push(grounding);
                })
            } else if segment.kind == SegmentKind::AmGraphics
                && matches!(record.type_id, PLACEMENT_TYPE_CA | PLACEMENT_TYPE_B9)
            {
                parse_placement(ctx, record.payload).map(|mut placement| {
                    placement.segment_token = segment.pair.token.as_str().into();
                    placement.record_ordinal = record.ordinal;
                    placements.push(placement);
                })
            } else if segment.kind == SegmentKind::DesignView
                && record.type_id == DESIGN_VIEW_REPRESENTATION_TYPE
            {
                parse_design_view_representation(ctx, record.payload).map(|mut representation| {
                    representation.segment_token = segment.pair.token.as_str().into();
                    representation.record_ordinal = record.ordinal;
                    representations.push(representation);
                })
            } else if segment.kind == SegmentKind::DesignView
                && record.type_id == VISIBILITY_OFF_TYPE
            {
                parse_visibility_off(record.payload).map(|mut state| {
                    state.segment_token = segment.pair.token.as_str().into();
                    state.record_ordinal = record.ordinal;
                    visibility_off.push(state);
                })
            } else {
                continue;
            };
            if let Err(error) = result {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_entities(1, "admit Inventor assembly issue")?;
                issues.push(RecordIssue {
                    family: RecordIssueFamily::Assembly,
                    segment_token: segment.pair.token.key().clone(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(AssemblyInventory {
        occurrences,
        placements,
        representations,
        visibility_off,
        groundings,
        issues,
    })
}

fn parse_occurrence<'a>(
    ctx: &DecodeContext<'a>,
    payload: View<'a>,
    version: u8,
) -> Result<AssemblyOccurrence, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header_value = cursor.u32("occurrence header value")?;
    let header_id = cursor.u16("occurrence header id")?;
    let next_reference = cursor.u32("occurrence next reference")?;
    let flags = cursor.u32("occurrence flags")?;
    if version >= 28 {
        require(
            cursor.u32("occurrence flags extension")?,
            0,
            "occurrence flags extension",
        )?;
    }
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
    let mut related_references = Vec::new();
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

fn parse_design_view_representation<'a>(
    ctx: &DecodeContext<'_>,
    payload: View<'a>,
) -> Result<DesignViewRepresentation<'a>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let _header_value = cursor.u32("design-view representation header value")?;
    let _kind = cursor.u16("design-view representation kind")?;
    let name = cursor.utf16(ctx, "design-view representation name", 65_536)?;
    require(
        u32::from(cursor.u16("design-view representation member-list marker")?),
        2,
        "design-view representation member-list marker",
    )?;
    require(
        u32::from(cursor.u16("design-view representation member-list signature")?),
        0x3000,
        "design-view representation member-list signature",
    )?;
    let member_count = cursor.count32("design-view representation member count", 65_536)?;
    let mut member_references = Vec::new();
    if member_count != 0 {
        cursor.u32("design-view representation member-list metadata")?;
        cursor.u32("design-view representation member-list metadata")?;
        for _ in 0..member_count {
            member_references.push(cursor.u32("design-view representation member reference")?);
        }
    }
    let suffix = cursor.remaining("design-view representation suffix")?;
    Ok(DesignViewRepresentation {
        segment_token: String::new(),
        record_ordinal: 0,
        name,
        member_references,
        suffix,
    })
}

fn parse_visibility_off(payload: View<'_>) -> Result<DesignViewVisibilityOff<'_>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let _header_value = cursor.u32("design-view visibility header value")?;
    let _kind = cursor.u16("design-view visibility kind")?;
    require(
        u32::from(cursor.u16("design-view visibility member-list marker")?),
        2,
        "design-view visibility member-list marker",
    )?;
    require(
        u32::from(cursor.u16("design-view visibility member-list signature")?),
        0x3000,
        "design-view visibility member-list signature",
    )?;
    let member_count = cursor.count32("design-view visibility member count", 65_536)?;
    if member_count == 0 {
        return Err(CodecError::malformed(
            "Inventor design-view visibility record names no occurrence",
        ));
    }
    cursor.u32("design-view visibility member-list metadata")?;
    cursor.u32("design-view visibility member-list metadata")?;
    let mut occurrence_path = Vec::new();
    for _ in 0..member_count {
        occurrence_path.push(cursor.u32("design-view visibility occurrence key")?);
    }
    require(
        u32::from(cursor.u8("design-view visibility trailer")?),
        0,
        "design-view visibility trailer",
    )?;
    let suffix = cursor.remaining("design-view visibility suffix")?;
    Ok(DesignViewVisibilityOff {
        segment_token: String::new(),
        record_ordinal: 0,
        occurrence_path,
        suffix,
    })
}

fn parse_grounding(payload: View<'_>) -> Result<OccurrenceGrounding<'_>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header_value = cursor.u32("occurrence grounding header value")?;
    let header_id = cursor.u16("occurrence grounding header id")?;
    let next_reference = cursor.u32("occurrence grounding next reference")?;
    let flags = cursor.u32("occurrence grounding flags")?;
    let suffix = cursor.remaining("occurrence grounding suffix")?;
    Ok(OccurrenceGrounding {
        segment_token: String::new(),
        record_ordinal: 0,
        header_value,
        header_id,
        next_reference,
        flags,
        suffix,
    })
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
        crate::reader::utf16_text(
            ctx,
            &mut self.source,
            count,
            field,
            "retain Inventor assembly string",
        )
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
        let matrix = CompactMatrix::try_new(set, zero, |index| {
            let value = self.source.req_f64_le()?;
            cadmpeg_ir::scalar::FiniteReal::new(value).ok_or_else(|| {
                CodecError::malformed(format_args!("compact matrix[{index}] is not finite"))
            })
        })?;
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
        inventory, parse_design_view_representation, parse_grounding, parse_occurrence,
        parse_placement, parse_visibility_off, DESIGN_VIEW_REPRESENTATION_TYPE,
        OCCURRENCE_GROUNDING_TYPE, OCCURRENCE_TYPE, PLACEMENT_TYPE_CA, SUPPRESSED_REFERENCE_STATE,
        VISIBILITY_OFF_TYPE,
    };
    use crate::compact_matrix::CompactMatrix;
    use crate::native::ufrx::{ExternalReferenceRecord, UfrxOccurrenceRecord};
    use crate::native::{
        AssemblyGroundingRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecord,
        DesignViewRepresentationRecord, DesignViewVisibilityOffRecord,
    };
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_ir::transform::Transform;
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    #[test]
    fn assembly_utf16_text_decodes_and_advances() {
        let bytes = [1, 0, 0, 0, 0, 8];
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("test root fits input admission");
        let mut cursor = super::Cursor::new(root);
        assert_eq!(
            cursor.utf16(&ctx, "label", 256).expect("one UTF-16 unit"),
            "ࠀ"
        );
        assert_eq!(cursor.source.position(), bytes.len());
    }

    fn project_under_service(
        ufrx_occurrences: &[UfrxOccurrenceRecord],
        external_references: &[ExternalReferenceRecord],
        assembly_occurrences: &[AssemblyOccurrenceRecord],
        assembly_placements: &[AssemblyPlacementRecord],
    ) -> super::AssemblyProjection {
        project_under_service_full(
            ufrx_occurrences,
            external_references,
            assembly_occurrences,
            assembly_placements,
            super::OccurrenceStates::default(),
        )
    }

    fn project_under_service_full(
        ufrx_occurrences: &[UfrxOccurrenceRecord],
        external_references: &[ExternalReferenceRecord],
        assembly_occurrences: &[AssemblyOccurrenceRecord],
        assembly_placements: &[AssemblyPlacementRecord],
        states: super::OccurrenceStates<'_>,
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
            states,
        )
        .expect("projection fits service policy")
    }

    #[test]
    fn occurrence_projection_emits_the_resolved_occurrence() {
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
                super::OccurrenceStates::default(),
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

    struct InventoryCounts {
        occurrences: usize,
        placements: usize,
        representations: usize,
        visibility_off: usize,
        groundings: usize,
        issues: Vec<crate::record_issue::RecordIssue>,
    }

    fn inventory_with_record(
        kind: SegmentKind,
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<InventoryCounts, CodecError> {
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
        Ok(InventoryCounts {
            occurrences: result.occurrences.len(),
            placements: result.placements.len(),
            representations: result.representations.len(),
            visibility_off: result.visibility_off.len(),
            groundings: result.groundings.len(),
            issues: result.issues,
        })
    }

    #[test]
    fn assembly_record_forms_are_admitted() {
        for (kind, type_id, payload, _) in [
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
            (
                SegmentKind::DesignView,
                DESIGN_VIEW_REPRESENTATION_TYPE,
                design_view_representation_fixture("Default", &[0x8000_0003]),
                "admit Inventor design-view representation record",
            ),
            (
                SegmentKind::DesignView,
                VISIBILITY_OFF_TYPE,
                visibility_off_fixture(2),
                "admit Inventor design-view visibility record",
            ),
            (
                SegmentKind::AmDc,
                OCCURRENCE_GROUNDING_TYPE,
                grounding_fixture(0x0200_0000),
                "admit Inventor occurrence grounding record",
            ),
        ] {
            let admitted =
                inventory_with_record(kind.clone(), type_id, &payload, DecodePolicy::service())
                    .expect("assembly record is admitted");
            assert_eq!(
                admitted.occurrences
                    + admitted.placements
                    + admitted.representations
                    + admitted.visibility_off
                    + admitted.groundings,
                1
            );
            assert!(admitted.issues.is_empty());
        }
    }

    #[test]
    fn assembly_related_references_are_admitted() {
        let payload = occurrence_fixture(7, &[8]);
        assert_eq!(
            inventory_with_record(
                SegmentKind::AmDc,
                OCCURRENCE_TYPE,
                &payload,
                DecodePolicy::service()
            )
            .expect("related reference is admitted")
            .occurrences,
            1
        );
    }

    #[test]
    fn assembly_parse_issue_is_recorded() {
        assert_eq!(
            inventory_with_record(
                SegmentKind::AmDc,
                OCCURRENCE_TYPE,
                &[],
                DecodePolicy::service()
            )
            .expect("truncated occurrence becomes an issue")
            .issues
            .len(),
            1
        );
    }

    #[test]
    fn assembly_parse_issue_refuses_entity_limit_before_push() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(matches!(
            inventory_with_record(SegmentKind::AmDc, OCCURRENCE_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit Inventor assembly issue"
        ));
        assert_eq!(
            inventory_with_record(
                SegmentKind::AmDc,
                OCCURRENCE_TYPE,
                &[],
                DecodePolicy::service()
            )
            .expect("build the record inventory")
            .issues
            .len(),
            1
        );
    }

    #[test]
    fn frames_occurrence_identity_and_variable_related_references() {
        for related in [Vec::new(), vec![0x8000_0008]] {
            let bytes = occurrence_fixture(42, &related);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic occurrence fits policy");
            let occurrence = parse_occurrence(&ctx, root, 22).expect("synthetic occurrence parses");
            assert_eq!(occurrence.occurrence_id, 42);
            assert_eq!(occurrence.related_references, related);
            assert_eq!(occurrence.header_value, 6);
        }
    }

    #[test]
    fn a_segment_version_28_occurrence_stores_a_zero_after_its_flags() {
        let mut bytes = occurrence_fixture(42, &[]);
        bytes.splice(14..14, 0u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic occurrence fits policy");
        let occurrence = parse_occurrence(&ctx, root, 28).expect("version 28 occurrence parses");
        assert_eq!(occurrence.occurrence_id, 42);
        bytes[14] = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic occurrence fits policy");
        assert!(parse_occurrence(&ctx, root, 28).is_err());
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
    fn an_occurrence_joins_its_prototype_by_reference_id_and_document_id() {
        let copies = [
            external_reference_with_document_id(5, "b/other.ipt", [0, 0], &fixture_document_id(4)),
            external_reference_with_document_id(4, "a\\Part.ipt", [0, 0], &fixture_document_id(4)),
        ];
        let projection = project_under_service(
            &[ufrx_occurrence(4, 7, 0)],
            &copies,
            &[assembly_occurrence(7)],
            &[assembly_placement(7)],
        );
        let [projected] = projection.occurrences.as_slice() else {
            panic!("the referenced copy must be projected");
        };
        let PrototypeReference::External { document, .. } = &projected.prototype else {
            panic!("the prototype must remain external");
        };
        assert_eq!(
            (match &document {
                cadmpeg_ir::products::ExternalDocument::Path { path } => Some(path.as_str()),
                _ => None,
            }),
            Some("a\\Part.ipt")
        );

        // A reference whose document identifier differs from the occurrence's
        // first-section identifier, or no reference with the `reference_id`,
        // leaves the occurrence unresolved.
        for references in [
            vec![external_reference_with_document_id(
                4,
                "a/part.ipt",
                [0, 0],
                &fixture_document_id(5),
            )],
            vec![external_reference_with_document_id(
                5,
                "b/other.ipt",
                [0, 0],
                &fixture_document_id(4),
            )],
        ] {
            let projection = project_under_service(
                &[ufrx_occurrence(4, 7, 0)],
                &references,
                &[assembly_occurrence(7)],
                &[assembly_placement(7)],
            );
            assert!(projection.occurrences.is_empty());
            assert_eq!(
                projection.unresolved_placements,
                BTreeMap::from([(super::UnresolvedCause::ExternalReference, NonZeroUsize::MIN)])
            );
        }
    }

    #[test]
    fn path_identity_takes_precedence_on_external_prototypes() {
        let reference = external_reference_with_document_id(
            4,
            "components/part.ipt",
            [0, 0],
            &fixture_document_id(4),
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
        let expected_document_id = fixture_document_id(4);
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
    fn duplicated_occurrence_records_do_not_prove_an_occurrence() {
        let projection = project_under_service(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7), assembly_occurrence(7)],
            &[assembly_placement(7)],
        );

        assert!(projection.occurrences.is_empty());
        assert_eq!(
            projection.unresolved_placements,
            BTreeMap::from([(super::UnresolvedCause::OccurrenceRecord, NonZeroUsize::MIN)])
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

    /// The synthetic document identifier of external reference `reference_id`.
    fn fixture_document_id(reference_id: u32) -> String {
        cadmpeg_ir::native::bytes::NativeBytes::from(fixture_document(reference_id)).to_string()
    }

    fn fixture_document(reference_id: u32) -> [u8; 16] {
        let mut id = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0,
            0,
        ];
        id[14..].copy_from_slice(&u16::try_from(reference_id).expect("small id").to_be_bytes());
        id
    }

    /// A `UFRxDoc` occurrence of external reference `reference_id` whose
    /// `AmDc`/`AmGraphics` key is `occurrence_id`.
    fn ufrx_occurrence(
        reference_id: u32,
        occurrence_id: u32,
        ordinal: u32,
    ) -> UfrxOccurrenceRecord {
        UfrxOccurrenceRecord::try_from(crate::native::ufrx::UfrxOccurrenceRecordWire {
            id: format!("inventor:ufrx:occurrence#{ordinal}"),
            ordinal,
            reference_id,
            occurrence_id,
            occurrence_key: ordinal + 1,
            header_value: 0,
            title: Some("placed part".into()),
            header_padding_words: 0,
            prototype_document_id: Some(fixture_document(reference_id).into()),
            component_name: Some("part".into()),
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
        external_reference_with_document_id(
            reference_id,
            path,
            state,
            &fixture_document_id(reference_id),
        )
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
        push_u32(
            &mut bytes,
            u32::try_from(related.len()).expect("fixture value fits u32"),
        );
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

    fn design_view_representation_fixture(name: &str, members: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 2);
        push_utf16(&mut bytes, name);
        push_u16(&mut bytes, 2);
        push_u16(&mut bytes, 0x3000);
        push_u32(
            &mut bytes,
            u32::try_from(members.len()).expect("fixture value fits u32"),
        );
        if !members.is_empty() {
            push_u32(
                &mut bytes,
                u32::try_from(members.len()).expect("fixture value fits u32"),
            );
            push_u32(&mut bytes, 0x10);
            for member in members {
                push_u32(&mut bytes, *member);
            }
        }
        bytes
    }

    fn visibility_off_fixture(occurrence_key: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 5);
        push_u16(&mut bytes, 2);
        push_u16(&mut bytes, 0x3000);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 7);
        push_u32(&mut bytes, 8);
        push_u32(&mut bytes, occurrence_key);
        bytes.push(0);
        bytes
    }

    fn grounding_fixture(flags: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 0x24);
        push_u32(&mut bytes, 0x8000_001c);
        push_u32(&mut bytes, flags);
        bytes.extend_from_slice(&[0; 28]);
        bytes
    }

    #[test]
    fn design_view_representation_names_its_members() {
        let bytes = design_view_representation_fixture("Default", &[0x8000_0003, 0x8000_0004]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic representation fits policy");
        let representation =
            parse_design_view_representation(&ctx, root).expect("representation parses");
        assert_eq!(representation.name, "Default");
        assert_eq!(
            representation.member_references,
            vec![0x8000_0003, 0x8000_0004]
        );
    }

    #[test]
    fn design_view_representation_without_members_has_no_suffix() {
        let bytes = design_view_representation_fixture("Master", &[]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic representation fits policy");
        let representation =
            parse_design_view_representation(&ctx, root).expect("representation parses");
        assert_eq!(representation.name, "Master");
        assert!(representation.member_references.is_empty());
        assert!(representation.suffix.window().is_empty());
    }

    #[test]
    fn visibility_off_state_names_an_occurrence_key_path() {
        let bytes = visibility_off_fixture(2);
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic visibility state fits policy");
        let state = parse_visibility_off(root).expect("visibility state parses");
        assert_eq!(state.occurrence_path, vec![2]);

        let mut truncated = visibility_off_fixture(2);
        truncated.truncate(14);
        let (_, root) =
            DecodeContext::from_root_bytes(&truncated, &arena, &DecodePolicy::default())
                .expect("truncated visibility state fits policy");
        assert!(parse_visibility_off(root).is_err());
    }

    #[test]
    fn visibility_off_state_without_a_key_is_refused() {
        let mut bytes = visibility_off_fixture(2);
        bytes[10..14].copy_from_slice(&0u32.to_le_bytes());
        bytes.truncate(15);
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("empty visibility state fits policy");
        assert!(parse_visibility_off(root).is_err());
    }

    #[test]
    fn visibility_off_state_keeps_a_nested_key_path() {
        let mut bytes = visibility_off_fixture(2);
        bytes[10..14].copy_from_slice(&2u32.to_le_bytes());
        bytes.splice(26..26, 9u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("nested visibility state fits policy");
        let state = parse_visibility_off(root).expect("nested path parses");
        assert_eq!(state.occurrence_path, vec![2, 9]);
    }

    #[test]
    fn grounding_flags_carry_the_grounded_bit() {
        for (flags, grounded) in [
            (0x0200_0000, true),
            (0x0200_0200, true),
            (0x0000_0200, false),
            (0, false),
        ] {
            let bytes = grounding_fixture(flags);
            let arena = DecodeArena::new();
            let (_, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic grounding fits policy");
            let grounding = parse_grounding(root).expect("grounding parses");
            assert_eq!(grounding.flags & super::GROUNDED_FLAGS_BIT != 0, grounded);
            assert_eq!(grounding.suffix.window().len(), 28);
        }
    }

    fn design_view_representation_record(
        name: &str,
        record_ordinal: u32,
        members: &[u32],
    ) -> DesignViewRepresentationRecord {
        DesignViewRepresentationRecord {
            id: format!("inventor:design-view:representation#synthetic-{record_ordinal}"),
            segment_token: "synthetic".into(),
            record_ordinal,
            name: name.into(),
            member_references: members.to_vec(),
            suffix_len: 0,
            suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        }
    }

    fn visibility_off_record(
        record_ordinal: u32,
        occurrence_path: &[u32],
    ) -> DesignViewVisibilityOffRecord {
        DesignViewVisibilityOffRecord {
            id: format!("inventor:design-view:visibility#synthetic-{record_ordinal}"),
            segment_token: "synthetic".into(),
            record_ordinal,
            occurrence_path: occurrence_path.to_vec(),
            suffix_len: 0,
            suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        }
    }

    fn grounding_record(record_ordinal: u32, flags: u32) -> AssemblyGroundingRecord {
        AssemblyGroundingRecord {
            id: format!("inventor:assembly:grounding#synthetic-{record_ordinal}"),
            segment_token: "synthetic".into(),
            record_ordinal,
            header_value: 0,
            header_id: 0x24,
            next_reference: 0x8000_001c,
            flags,
            suffix_len: 0,
            suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        }
    }

    #[test]
    fn hidden_occurrence_in_active_design_view_is_not_visible() {
        let ufrx = [ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)];
        let reference = external_reference(4, "part.ipt", [0, 0]);
        let occurrences = [assembly_occurrence(7), assembly_occurrence(8)];
        let placements = [assembly_placement(7), assembly_placement(8)];
        let representations = [design_view_representation_record(
            "Default",
            1,
            &[0x8000_0003],
        )];
        let visibility = [visibility_off_record(2, &[2])];

        let projection = project_under_service_full(
            &ufrx,
            &[reference],
            &occurrences,
            &placements,
            super::OccurrenceStates {
                active_design_view: Some("Default"),
                representations: &representations,
                visibility_off: &visibility,
                ..Default::default()
            },
        );

        assert!(projection.unresolved_placements.is_empty());
        assert!(projection.joints.is_empty());
        assert_eq!(projection.occurrences.len(), 2);
        assert_eq!(projection.occurrences[0].visible, None);
        assert_eq!(projection.occurrences[1].visible, Some(false));
    }

    #[test]
    fn visibility_needs_the_active_design_view() {
        let ufrx = [ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)];
        let reference = external_reference(4, "part.ipt", [0, 0]);
        let occurrences = [assembly_occurrence(7), assembly_occurrence(8)];
        let placements = [assembly_placement(7), assembly_placement(8)];
        let representations = [design_view_representation_record(
            "Default",
            1,
            &[0x8000_0003],
        )];
        let visibility = [visibility_off_record(2, &[2])];

        for active in [None, Some("Other")] {
            let projection = project_under_service_full(
                &ufrx,
                std::slice::from_ref(&reference),
                &occurrences,
                &placements,
                super::OccurrenceStates {
                    active_design_view: active,
                    representations: &representations,
                    visibility_off: &visibility,
                    ..Default::default()
                },
            );
            assert!(projection.unresolved_placements.is_empty());
            assert_eq!(projection.occurrences.len(), 2);
            assert_eq!(projection.occurrences[0].visible, None);
            assert_eq!(projection.occurrences[1].visible, None);
        }
    }

    #[test]
    fn grounded_occurrence_emits_a_grounded_joint() {
        let mut occurrence = assembly_occurrence(7);
        occurrence.related_references = vec![0x8000_0009];
        let grounding = grounding_record(8, 0x0200_0200);

        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[occurrence],
            &[assembly_placement(7)],
            super::OccurrenceStates {
                groundings: &[grounding],
                ..Default::default()
            },
        );

        assert!(projection.unresolved_placements.is_empty());
        assert_eq!(projection.occurrences.len(), 1);
        assert_eq!(projection.occurrences[0].visible, None);
        let [joint] = projection.joints.as_slice() else {
            panic!("one grounded occurrence emits one joint");
        };
        assert!(matches!(
            joint.connectors().next().map(|connector| &connector.operand.container),
            Some(cadmpeg_ir::products::OperandContainer::Occurrence { occurrence, .. })
                if occurrence.as_str() == projection.occurrences[0].id.as_str()
        ));
        let connector = joint
            .connectors()
            .next()
            .expect("grounded joint has a connector");
        assert_eq!(connector.operand.object, "part");
        assert_eq!(
            joint.native_ref.as_deref(),
            Some("inventor:assembly:grounding#synthetic-8")
        );
    }

    #[test]
    fn ungrounded_occurrence_emits_no_joint() {
        let mut occurrence = assembly_occurrence(7);
        occurrence.related_references = vec![0x8000_0009];

        for flags in [0x0000_0200, 0] {
            let projection = project_under_service_full(
                &[ufrx_occurrence(4, 7, 0)],
                &[external_reference(4, "part.ipt", [0, 0])],
                std::slice::from_ref(&occurrence),
                &[assembly_placement(7)],
                super::OccurrenceStates {
                    groundings: &[grounding_record(8, flags)],
                    ..Default::default()
                },
            );
            assert!(projection.unresolved_placements.is_empty());
            assert_eq!(projection.occurrences.len(), 1);
            assert!(projection.joints.is_empty());
        }
    }

    #[test]
    fn grounding_without_occurrence_join_emits_no_joint() {
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7)],
            &[assembly_placement(7)],
            super::OccurrenceStates {
                groundings: &[grounding_record(9, 0x0200_0200)],
                ..Default::default()
            },
        );
        assert!(projection.unresolved_placements.is_empty());
        assert_eq!(projection.occurrences.len(), 1);
        assert!(projection.joints.is_empty());
    }

    #[test]
    fn a_nested_key_path_hides_no_root_occurrence() {
        let ufrx = [ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)];
        let projection = project_under_service_full(
            &ufrx,
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7), assembly_occurrence(8)],
            &[assembly_placement(7), assembly_placement(8)],
            super::OccurrenceStates {
                active_design_view: Some("Default"),
                representations: &[design_view_representation_record(
                    "Default",
                    1,
                    &[0x8000_0003],
                )],
                visibility_off: &[visibility_off_record(2, &[2, 9])],
                ..Default::default()
            },
        );
        assert_eq!(projection.occurrences.len(), 2);
        assert!(projection.occurrences.iter().all(|o| o.visible.is_none()));
        assert_eq!(projection.nested_hidden, 1);
    }

    #[test]
    fn a_state_record_outside_the_active_member_list_hides_nothing() {
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7), assembly_occurrence(8)],
            &[assembly_placement(7), assembly_placement(8)],
            super::OccurrenceStates {
                active_design_view: Some("Default"),
                representations: &[design_view_representation_record(
                    "Default",
                    1,
                    &[0x8000_0003],
                )],
                visibility_off: &[visibility_off_record(4, &[2])],
                ..Default::default()
            },
        );
        assert!(projection.occurrences.iter().all(|o| o.visible.is_none()));
    }

    #[test]
    fn two_representations_with_the_active_name_hide_nothing() {
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7), assembly_occurrence(8)],
            &[assembly_placement(7), assembly_placement(8)],
            super::OccurrenceStates {
                active_design_view: Some("Default"),
                representations: &[
                    design_view_representation_record("Default", 1, &[0x8000_0003]),
                    design_view_representation_record("Default", 2, &[0x8000_0003]),
                ],
                visibility_off: &[visibility_off_record(2, &[2])],
                ..Default::default()
            },
        );
        assert!(projection.occurrences.iter().all(|o| o.visible.is_none()));
    }

    #[test]
    fn a_hidden_occurrence_without_a_placement_stays_unresolved() {
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7)],
            &[],
            super::OccurrenceStates {
                active_design_view: Some("Default"),
                representations: &[design_view_representation_record(
                    "Default",
                    1,
                    &[0x8000_0003],
                )],
                visibility_off: &[visibility_off_record(2, &[1])],
                ..Default::default()
            },
        );
        assert!(projection.occurrences.is_empty());
        assert_eq!(
            projection.unresolved_placements,
            BTreeMap::from([(super::UnresolvedCause::Placement, NonZeroUsize::MIN)])
        );
    }

    #[test]
    fn grounding_joins_only_through_related_references() {
        let mut occurrence = assembly_occurrence(7);
        occurrence.child_reference = 0x8000_0009;
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[occurrence],
            &[assembly_placement(7)],
            super::OccurrenceStates {
                groundings: &[grounding_record(8, 0x0200_0200)],
                ..Default::default()
            },
        );
        assert!(projection.joints.is_empty());
    }

    #[test]
    fn the_hide_applies_only_in_the_view_that_lists_it() {
        let ufrx = [ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)];
        let occurrences = [assembly_occurrence(7), assembly_occurrence(8)];
        let placements = [assembly_placement(7), assembly_placement(8)];
        let representations = [
            design_view_representation_record("Default", 1, &[0x8000_0004, 0x8000_0005]),
            design_view_representation_record(
                "TestView",
                2,
                &[0x8000_0006, 0x8000_0007, 0x8000_0008],
            ),
        ];
        let visibility = [visibility_off_record(7, &[2])];
        for (active, hidden) in [
            (Some("TestView"), vec![None, Some(false)]),
            (Some("Default"), vec![None, None]),
        ] {
            let projection = project_under_service_full(
                &ufrx,
                &[external_reference(4, "part.ipt", [0, 0])],
                &occurrences,
                &placements,
                super::OccurrenceStates {
                    active_design_view: active,
                    representations: &representations,
                    visibility_off: &visibility,
                    ..Default::default()
                },
            );
            let visible: Vec<_> = projection.occurrences.iter().map(|o| o.visible).collect();
            assert_eq!(visible, hidden, "active view {active:?}");
        }
    }

    #[test]
    fn visibility_off_state_retains_bytes_after_its_trailer() {
        let mut bytes = visibility_off_fixture(2);
        bytes.extend_from_slice(&[7, 8, 9]);
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("extended visibility state fits policy");
        let state = parse_visibility_off(root).expect("extended visibility state parses");
        assert_eq!(state.occurrence_path, vec![2]);
        assert_eq!(state.suffix.window(), &[7, 8, 9]);
    }

    #[test]
    fn a_state_record_in_another_segment_is_not_a_member() {
        let mut other_segment = visibility_off_record(2, &[2]);
        other_segment.segment_token = "other".into();
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0), ufrx_occurrence(4, 8, 1)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7), assembly_occurrence(8)],
            &[assembly_placement(7), assembly_placement(8)],
            super::OccurrenceStates {
                active_design_view: Some("Default"),
                representations: &[design_view_representation_record(
                    "Default",
                    1,
                    &[0x8000_0003],
                )],
                visibility_off: &[other_segment],
                ..Default::default()
            },
        );
        assert!(projection.occurrences.iter().all(|o| o.visible.is_none()));
    }

    #[test]
    fn a_grounding_record_in_another_segment_grounds_nothing() {
        let mut occurrence = assembly_occurrence(7);
        occurrence.related_references = vec![0x8000_0009];
        let mut grounding = grounding_record(8, 0x0200_0200);
        grounding.segment_token = "other".into();
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[occurrence],
            &[assembly_placement(7)],
            super::OccurrenceStates {
                groundings: &[grounding],
                ..Default::default()
            },
        );
        assert!(projection.joints.is_empty());
    }

    #[test]
    fn two_representations_with_the_active_name_leave_no_active_view() {
        // Mirrors the specification: a name that selects no single record
        // hides no occurrence, including when the hide is a member of both.
        let projection = project_under_service_full(
            &[ufrx_occurrence(4, 7, 0)],
            &[external_reference(4, "part.ipt", [0, 0])],
            &[assembly_occurrence(7)],
            &[assembly_placement(7)],
            super::OccurrenceStates {
                active_design_view: Some("TestView"),
                representations: &[
                    design_view_representation_record("TestView", 1, &[0x8000_0003]),
                    design_view_representation_record("TestView", 2, &[0x8000_0003]),
                ],
                visibility_off: &[visibility_off_record(2, &[1])],
                ..Default::default()
            },
        );
        assert_eq!(projection.occurrences[0].visible, None);
    }
}
