// SPDX-License-Identifier: Apache-2.0
//! `AllFeatur` feature rows and their procedural projections.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::bytes::{contains, find_from, find_in};
use cadmpeg_core::decode::bounded_len;

use crate::psb;
use crate::scalar;

use super::helpers::decode_exact_scalars;

/// One byte-bounded positional `AllFeatur` row for a known model feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRow {
    /// Feature identifier decoded from the row prefix.
    pub feature_id: u32,
    /// Two-byte row header retained for downstream row-family dispatch.
    pub header: [u8; 2],
    /// Root `FeatDefs` schema class from the fixed row prefix.
    pub root_schema_class: Option<u32>,
    /// Absolute offset of the containing `AllFeatur` section. Replay state is
    /// scoped to this stream.
    pub stream_offset: usize,
    /// Row bytes after the compact feature identifier, ending before the next
    /// known feature row or at the end of the section.
    pub body: Vec<u8>,
    /// Byte offset of `body[0]` in the original stream.
    pub body_offset: usize,
    /// Byte offset of the feature identifier in the original stream.
    pub offset: usize,
}

/// One short-form scalar candidate from a class-913 round replay record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FeatureRoundReplayScalar {
    /// Owning feature identifier.
    pub(crate) feature_id: u32,
    /// Decoded short-form scalar value.
    pub(crate) value: f64,
    /// Absolute byte offset of the scalar in the source stream.
    pub(crate) offset: usize,
    /// Absolute byte offset of the enclosing `cr_flags_xar` record.
    pub(crate) record_offset: usize,
}

/// One labeled procedural-choice span inside a known feature row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureChoice {
    /// Owning feature row identifier.
    pub feature_id: u32,
    /// Procedural choice label without its NUL terminator.
    pub label: String,
    /// Named-record type byte when the label has an `e0` header.
    pub type_byte: Option<u8>,
    /// Exact bytes from the label terminator to the next choice span.
    pub payload: Vec<u8>,
    /// Byte offset of `payload[0]` in the original stream.
    pub payload_offset: usize,
    /// Byte offset of the choice header or bare label in the original stream.
    pub offset: usize,
}

/// Byte-declared wrapper around one procedural choice field value.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureFieldValue {
    /// No payload bytes follow the field header.
    Empty,
    /// One compact integer occupying the complete field payload.
    CompactInt(u32),
    /// An `f8` count followed by exactly that many compact integers.
    CompactIntArray(Vec<u32>),
    /// One canonical `f7` entity reference, optionally followed by `fb`.
    EntityReference {
        /// Walker-order entity identifier.
        entity_id: u32,
        /// Whether an `fb` terminator follows the identifier.
        terminated: bool,
    },
    /// An `f9 <dimensions> <count>` scalar-array wrapper and its undecoded body.
    ScalarArray {
        /// Scalar tuple dimensionality from the wrapper.
        dimensions: u32,
        /// Number of scalar tuples from the wrapper.
        count: u32,
        /// Exact scalar-body bytes after the wrapper header.
        body: Vec<u8>,
        /// Values when exactly `dimensions × count` defined scalar tokens
        /// consume the complete body.
        decoded_values: Option<Vec<f64>>,
    },
    /// Bytes whose enclosing field is known but whose wrapper is not.
    Raw(Vec<u8>),
}

/// One named field bounded inside a procedural choice span.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureChoiceField {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Owning procedural choice label.
    pub choice_label: String,
    /// Field name from its named-record header.
    pub name: String,
    /// Named-record type byte.
    pub type_byte: u8,
    /// Structurally decoded field-value wrapper.
    pub value: FeatureFieldValue,
    /// Byte offset of the named-record header in the original stream.
    pub offset: usize,
}

/// Generated-geometry namespace declared inside a feature row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureGeometryTableKind {
    /// `edg_id_tab_ptr` edge identifiers.
    EdgeIds,
    /// `lo_id_tab_ptr` loop identifiers.
    LoopIds,
    /// `bnd_type` boundary records.
    Boundaries,
    /// `used_bodies` body references.
    UsedBodies,
    /// `geom_lists` geometry-list references.
    GeometryLists,
    /// `dtm_id_tab` datum identifiers.
    DatumIds,
}

/// One typed generated-geometry table header owned by a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureGeometryTable {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Declared namespace kind.
    pub kind: FeatureGeometryTableKind,
    /// Declared entry count.
    pub count: u32,
    /// Entity-class identifier following the `f7` marker.
    pub entity_class: u32,
    /// Complete datum identifiers for a `dtm_id_tab`; other table bodies remain
    /// untyped.
    pub entry_ids: Option<Vec<u32>>,
    /// Byte offset of the field label in the original stream.
    pub offset: usize,
}

/// Namespace of IDs affected by a procedural feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffectedIdKind {
    /// `geoms_affected` geometry identifiers.
    Geometry,
    /// `edgs_affected` edge identifiers.
    Edges,
    /// `strong_parents` parent-feature identifiers.
    StrongParents,
    /// `parent_table` regeneration-parent feature identifiers.
    Parents,
    /// `contours` contour identifiers.
    Contours,
    /// `qlts_affected` quilt-entity identifiers.
    Quilts,
}

/// One complete affected-ID array owned by a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureAffectedIds {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Affected namespace.
    pub kind: AffectedIdKind,
    /// Declared compact identifiers in stored order.
    pub ids: Vec<u32>,
    /// Byte offset of the named field header in the original stream.
    pub offset: usize,
}

/// Whether an affected-array extent is present or inherited at its schema
/// position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayExtentSource {
    /// An `f8 <count>` opener occurs at this position.
    Explicit,
    /// The position omits `f8` and reuses the preceding extent in this schema
    /// stream.
    Inherited,
}

/// Geometry and edge operands recovered from a class-913 positional replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureReplayAffectedIds {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Geometry identifiers at the first affected-array schema position.
    pub geometry_ids: Vec<u32>,
    /// Edge identifiers at the second affected-array schema position.
    pub edge_ids: Vec<u32>,
    /// Encoding of the geometry-array extent.
    pub geometry_extent: ReplayExtentSource,
    /// Encoding of the edge-array extent.
    pub edge_extent: ReplayExtentSource,
    /// Byte offset of the replay anchor in the original stream.
    pub offset: usize,
}

/// Geometry, edge, and quilt operands recovered from a class-946 positional replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSurfaceMergeAffectedIds {
    /// Owning surface-merge feature identifier.
    pub feature_id: u32,
    /// Geometry identifiers at the first affected-array schema position.
    pub geometry_ids: Vec<u32>,
    /// Edge identifiers at the second affected-array schema position.
    pub edge_ids: Vec<u32>,
    /// Quilt identifiers at the third affected-array schema position.
    pub quilt_ids: Vec<u32>,
    /// Encoding of the geometry-array extent.
    pub geometry_extent: ReplayExtentSource,
    /// Encoding of the edge-array extent.
    pub edge_extent: ReplayExtentSource,
    /// Encoding of the quilt-array extent.
    pub quilt_extent: ReplayExtentSource,
    /// Byte offset of the replay anchor in the original stream.
    pub offset: usize,
}

/// Which named direction lane occurs in a loop-restoration record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopRestoreDirectionLane {
    /// `direction`.
    Primary,
    /// `direction2`.
    Secondary,
}

/// One named compact direction value in a loop-restoration record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureLoopRestoreDirection {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Primary or secondary direction lane.
    pub lane: LoopRestoreDirectionLane,
    /// Complete compact-integer value.
    pub value: u32,
    /// Byte offset of the named field header in the original stream.
    pub offset: usize,
}

/// One ordered feature-local loop identity from a complete `lo_hist` roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureLoopHistoryEntry {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Zero-based position in the feature's loop roster.
    pub ordinal: u32,
    /// Feature-local loop identifier.
    pub loop_id: u32,
    /// Four required row fields and the optional final field, in stored order.
    pub field_bytes: Vec<Vec<u8>>,
    /// Stored row boundary form.
    pub boundary: FeatureLoopHistoryBoundary,
    /// Byte offset of the loop identifier in the original stream.
    pub offset: usize,
    /// Byte offset immediately after the row, excluding a following named header.
    pub end_offset: usize,
}

/// Boundary form terminating one `lo_hist` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureLoopHistoryBoundary {
    /// Bare `e3` terminator.
    CompoundClose,
    /// `f1 f7 <reference> e3` terminator.
    ReferenceContinue(u32),
    /// `f2 f7 <reference> e3` terminator.
    ReferenceFinal(u32),
    /// The next named-record header bounds the final row.
    NamedRecord,
}

/// Angular termination selected by a rotational feature row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRevolutionExtentKind {
    /// Complete 360-degree travel.
    FullTurn,
}

/// One resolved rotational extent from an `AllFeatur` feature row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRevolutionExtent {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Resolved angular termination.
    pub kind: FeatureRevolutionExtentKind,
    /// Byte offset of the stored `angle_choice` value.
    pub offset: usize,
}

const CHOICE_LABELS: &[&[u8]] = &[
    b"blend_choice",
    b"depth_choice",
    b"angle_choice",
    b"pat_choice",
    b"round_choice",
    b"subsec_choice",
    b"sweep_choice",
    b"dome_choice",
    b"draft_choice",
    b"misc_choice",
];

pub(super) fn row_spans(payload: &[u8], feature_ids: &BTreeSet<u32>) -> Vec<(usize, usize, u32)> {
    // The raw section header is present when the caller passes the complete
    // section extent instead of the payload after `#<name>\n`.
    let section_header_end = if payload.first() == Some(&b'#') {
        payload
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|newline| newline + 1)
    } else {
        None
    };
    let mut starts = Vec::new();
    for offset in 0..payload.len() {
        let Ok((id, after)) = psb::reference_id(payload, offset) else {
            continue;
        };
        let prefix_end = after.saturating_add(16).min(payload.len());
        // Row-like identifiers inside a body are not boundaries. A compound
        // close is the only in-body boundary; the section header is the
        // corresponding boundary before the first row.
        let starts_at_row_boundary = offset == 0
            || section_header_end == Some(offset)
            || (offset > 0 && payload[offset - 1] == psb::token::COMPOUND_CLOSE);
        if starts_at_row_boundary
            && feature_ids.contains(&id)
            && payload.get(after..after + 2).is_some()
            && row_root_schema_class(payload, offset, prefix_end).is_some()
        {
            starts.push((offset, id));
        }
    }
    starts.sort_unstable();
    // One stream can expose the same feature identifier under conflicting
    // schema classes, but one identifier/class pair is one row.
    let mut seen_ids = BTreeSet::new();
    let mut seen_schema_classes = BTreeSet::new();
    let retained_starts: Vec<(usize, u32)> = starts
        .iter()
        .enumerate()
        .filter_map(|(index, &(start, id))| {
            let candidate_end = starts
                .get(index + 1)
                .map_or(payload.len(), |&(next, _)| next);
            let first_for_id = seen_ids.insert(id);
            let has_new_schema_class = row_root_schema_class(payload, start, candidate_end)
                .is_some_and(|schema_class| seen_schema_classes.insert((id, schema_class)));
            (first_for_id || has_new_schema_class).then_some((start, id))
        })
        .collect();
    retained_starts
        .iter()
        .enumerate()
        .map(|(index, &(start, id))| {
            let end = retained_starts
                .get(index + 1)
                .map_or(payload.len(), |&(next, _)| next);
            (start, end, id)
        })
        .collect()
}

/// Read the fixed-prefix root schema class from one candidate row span.
fn row_root_schema_class(payload: &[u8], start: usize, end: usize) -> Option<u32> {
    let (_, body_start) = psb::reference_id(payload, start).ok()?;
    let body = payload.get(body_start..end)?;
    body[..body.len().min(16)]
        .windows(2)
        .enumerate()
        .filter(|(relative, window)| *relative >= 2 && *window == [0xe3, 0xf6])
        .find_map(|(relative, _)| {
            let value_offset = body_start + relative + 2;
            let (value, after) = psb::compact_int(payload, value_offset);
            (after > value_offset && after < end && payload.get(after) == Some(&0xe1))
                .then_some(value)
        })
}

/// Decode positional `AllFeatur` rows whose identifiers exist in a decoded
/// model-feature namespace. Unknown feature-like byte sequences remain unclaimed.
pub fn rows(payload: &[u8], feature_ids: &BTreeSet<u32>) -> Vec<FeatureRow> {
    row_spans(payload, feature_ids)
        .into_iter()
        .filter_map(|(start, end, feature_id)| {
            let (_, body_start) = psb::reference_id(payload, start).ok()?;
            let body = payload.get(body_start..end)?;
            let header = payload.get(body_start..body_start + 2)?.try_into().ok()?;
            let root_schema_class = row_root_schema_class(payload, start, end);
            Some(FeatureRow {
                feature_id,
                header,
                root_schema_class,
                stream_offset: 0,
                body: body.to_vec(),
                body_offset: body_start,
                offset: start,
            })
        })
        .collect()
}

/// Decode the first short-form scalar in each bounded class-913 replay record.
///
/// The `f2 f7 80 a0` record is an unlabeled `cr_flags_xar` replay. Its record
/// boundary is the next `f3 f7 80 97 e2` `misc_choice` replay anchor. Within
/// the bounded record, `01 f6` ends the preceding compact fields and the first
/// `0x29` token is the replayed short-form scalar lane. Other `0x29` images in
/// the record are not assigned a field role.
pub(crate) fn round_replay_scalars(rows: &[FeatureRow]) -> Vec<FeatureRoundReplayScalar> {
    const CR_FLAGS_ANCHOR: &[u8] = &[0xf2, 0xf7, 0x80, 0xa0];
    const MISC_CHOICE_ANCHOR: &[u8] = &[0xf3, 0xf7, 0x80, 0x97, 0xe2];
    let mut result = Vec::new();
    for row in rows.iter().filter(|row| row.root_schema_class == Some(913)) {
        let record_ends = row
            .body
            .windows(MISC_CHOICE_ANCHOR.len())
            .enumerate()
            .filter_map(|(offset, bytes)| (bytes == MISC_CHOICE_ANCHOR).then_some(offset))
            .collect::<Vec<_>>();
        for (record_start, bytes) in row.body.windows(CR_FLAGS_ANCHOR.len()).enumerate() {
            if bytes != CR_FLAGS_ANCHOR {
                continue;
            }
            let Some(record_end) = record_ends
                .iter()
                .copied()
                .find(|offset| *offset > record_start)
            else {
                continue;
            };
            let Some(separator) = row.body[record_start + CR_FLAGS_ANCHOR.len()..record_end]
                .windows(2)
                .position(|bytes| bytes == [0x01, 0xf6])
                .map(|offset| record_start + CR_FLAGS_ANCHOR.len() + offset + 2)
            else {
                continue;
            };
            let Some(scalar_offset) = round_replay_short_scalar(&row.body, separator, record_end)
            else {
                continue;
            };
            let Some((value, scalar_end)) = scalar::decode(&row.body, scalar_offset) else {
                continue;
            };
            if scalar_end != scalar_offset + 3 || !value.is_finite() {
                continue;
            }
            result.push(FeatureRoundReplayScalar {
                feature_id: row.feature_id,
                value,
                offset: row.body_offset + scalar_offset,
                record_offset: row.body_offset + record_start,
            });
        }
    }
    result.sort_by_key(|candidate| candidate.offset);
    result
}

fn round_replay_short_scalar(body: &[u8], start: usize, end: usize) -> Option<usize> {
    let mut offset = start;
    while offset < end {
        if body.get(offset) == Some(&0x29)
            && scalar::decode(body, offset).is_some_and(|(value, scalar_end)| {
                scalar_end == offset + 3 && scalar_end <= end && value.is_finite()
            })
        {
            return Some(offset);
        }
        offset = round_replay_token_end(body, offset, end)?;
    }
    None
}

fn round_replay_token_end(body: &[u8], offset: usize, end: usize) -> Option<usize> {
    let head = *body.get(offset)?;
    let width = match head {
        0x19 | 0x28 | 0x32 | 0x37 | 0x41 => 8,
        0x31 | 0x4f | 0x90 | 0xd5 | 0xd7 => 7,
        0x18 => {
            let (_, compact_end) = psb::compact_int(body, offset + 1);
            compact_end.saturating_sub(offset).max(1)
        }
        _ => scalar::decode(body, offset)
            .map(|(_, scalar_end)| scalar_end.saturating_sub(offset))
            .or_else(|| psb::token_at(body, offset).map(|token| token.length))?,
    };
    let next = offset.checked_add(width)?;
    (width > 0 && next <= end).then_some(next)
}

/// Bound recognized procedural-choice labels within decoded feature rows.
pub fn choices(rows: &[FeatureRow]) -> Vec<FeatureChoice> {
    let mut result = Vec::new();
    for row in rows {
        let mut hits = Vec::new();
        for &label in CHOICE_LABELS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(label_offset) = find_from(&row.body, &needle, from) {
                let (header_offset, type_byte) = if label_offset >= 2
                    && row.body[label_offset - 2] == psb::token::NAMED_RECORD
                {
                    (label_offset - 2, Some(row.body[label_offset - 1]))
                } else {
                    (label_offset, None)
                };
                hits.push((header_offset, label_offset, label, type_byte));
                from = label_offset + label.len() + 1;
            }
        }
        hits.sort_by_key(|hit| hit.0);
        for (index, &(header, label_at, label, type_byte)) in hits.iter().enumerate() {
            let value = label_at + label.len() + 1;
            let end = hits.get(index + 1).map_or_else(
                || {
                    let post_choice = b"assoc_type\0";
                    row.body[value..]
                        .windows(post_choice.len() + 2)
                        .position(|window| {
                            window[0] == psb::token::NAMED_RECORD && window[2..] == post_choice[..]
                        })
                        .map_or(row.body.len(), |relative| value + relative)
                },
                |hit| hit.0,
            );
            result.push(FeatureChoice {
                feature_id: row.feature_id,
                label: String::from_utf8_lossy(label).into_owned(),
                type_byte,
                payload: row.body[value..end].to_vec(),
                payload_offset: row.body_offset + value,
                offset: row.body_offset + header,
            });
        }
    }
    result.sort_by_key(|choice| choice.offset);
    result
}

pub(crate) fn field_value(payload: &[u8]) -> FeatureFieldValue {
    if payload.is_empty() {
        return FeatureFieldValue::Empty;
    }
    if payload[0] == psb::token::SCALAR_BODY {
        let (dimensions, dimensions_end) = psb::compact_int(payload, 1);
        let (count, values_start) = psb::compact_int(payload, dimensions_end);
        let slot_count = usize::try_from(dimensions).ok().and_then(|dimensions| {
            usize::try_from(count)
                .ok()
                .and_then(|count| dimensions.checked_mul(count))
        });
        let Some(slot_count) = slot_count.filter(|slot_count| {
            dimensions_end > 1
                && values_start > dimensions_end
                && *slot_count
                    <= payload
                        .len()
                        .saturating_sub(values_start)
                        .saturating_mul(16)
                        .max(12)
        }) else {
            return FeatureFieldValue::Raw(payload.to_vec());
        };
        let cache = scalar::ScalarCache::from_section(payload);
        let decoded_values = decode_exact_scalars(&payload[values_start..], slot_count, &cache);
        return FeatureFieldValue::ScalarArray {
            dimensions,
            count,
            body: payload[values_start..].to_vec(),
            decoded_values,
        };
    }
    if payload[0] == psb::token::ENTITY_REF {
        if let Ok((entity_id, end)) = psb::reference_id(payload, 1) {
            let terminated = end + 1 == payload.len() && payload[end] == psb::token::ARRAY_CLOSE;
            if end == payload.len() || terminated {
                return FeatureFieldValue::EntityReference {
                    entity_id,
                    terminated,
                };
            }
        }
    }
    if payload[0] == psb::token::ARRAY_OPEN {
        let (count, mut cursor) = psb::compact_int(payload, 1);
        let mut values = Vec::new();
        for _ in 0..count {
            let (value, next) = psb::compact_int(payload, cursor);
            if next == cursor {
                return FeatureFieldValue::Raw(payload.to_vec());
            }
            values.push(value);
            cursor = next;
        }
        if cursor == payload.len()
            || cursor + 1 == payload.len() && payload[cursor] == psb::token::ARRAY_CLOSE
        {
            return FeatureFieldValue::CompactIntArray(values);
        }
    }
    let (value, end) = psb::compact_int(payload, 0);
    if end == payload.len() {
        FeatureFieldValue::CompactInt(value)
    } else {
        FeatureFieldValue::Raw(payload.to_vec())
    }
}

/// Decode named fields and their context-independent value wrappers inside
/// procedural choice spans.
pub fn choice_fields(choices: &[FeatureChoice]) -> Vec<FeatureChoiceField> {
    let mut fields = Vec::new();
    for choice in choices {
        let mut headers = Vec::new();
        for offset in 0..choice.payload.len().saturating_sub(2) {
            if choice.payload[offset] != psb::token::NAMED_RECORD {
                continue;
            }
            let Some(nul) = choice.payload[offset + 2..]
                .iter()
                .position(|&byte| byte == 0)
                .map(|relative| offset + 2 + relative)
            else {
                continue;
            };
            if choice.payload[offset + 2..nul]
                .iter()
                .all(u8::is_ascii_graphic)
            {
                headers.push((offset, nul + 1));
            }
        }
        for (index, &(header, value_start)) in headers.iter().enumerate() {
            let end = headers
                .get(index + 1)
                .map_or(choice.payload.len(), |hit| hit.0);
            if value_start > end {
                continue;
            }
            fields.push(FeatureChoiceField {
                feature_id: choice.feature_id,
                choice_label: choice.label.clone(),
                name: String::from_utf8_lossy(&choice.payload[header + 2..value_start - 1])
                    .into_owned(),
                type_byte: choice.payload[header + 1],
                value: field_value(&choice.payload[value_start..end]),
                offset: choice.payload_offset + header,
            });
        }
    }
    fields.sort_by_key(|field| field.offset);
    fields
}

/// Decode generated-geometry table headers from known feature rows.
pub fn geometry_tables(rows: &[FeatureRow]) -> Vec<FeatureGeometryTable> {
    const FIELDS: &[(&[u8], FeatureGeometryTableKind)] = &[
        (b"edg_id_tab_ptr", FeatureGeometryTableKind::EdgeIds),
        (b"lo_id_tab_ptr", FeatureGeometryTableKind::LoopIds),
        (b"bnd_type", FeatureGeometryTableKind::Boundaries),
        (b"used_bodies", FeatureGeometryTableKind::UsedBodies),
        (b"geom_lists", FeatureGeometryTableKind::GeometryLists),
        (b"dtm_id_tab", FeatureGeometryTableKind::DatumIds),
    ];
    let mut tables = Vec::new();
    let mut datum_class_by_stream = BTreeMap::<usize, u32>::new();
    for row in rows {
        for &(label, kind) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(offset) = find_from(&row.body, &needle, from) {
                from = offset + needle.len();
                let Some((count, entity_class, entry_ids)) =
                    geometry_table_at(&row.body, offset + needle.len(), kind)
                else {
                    continue;
                };
                tables.push(FeatureGeometryTable {
                    feature_id: row.feature_id,
                    kind,
                    count,
                    entity_class,
                    entry_ids,
                    offset: row.body_offset + offset,
                });
                if kind == FeatureGeometryTableKind::DatumIds {
                    datum_class_by_stream.insert(row.stream_offset, entity_class);
                }
            }
        }
        let Some(&entity_class) = datum_class_by_stream.get(&row.stream_offset) else {
            continue;
        };
        for cursor in 0..row.body.len() {
            let Some((count, entry_ids)) =
                positional_datum_geometry_table_at(&row.body, cursor, entity_class)
            else {
                continue;
            };
            tables.push(FeatureGeometryTable {
                feature_id: row.feature_id,
                kind: FeatureGeometryTableKind::DatumIds,
                count,
                entity_class,
                entry_ids: Some(entry_ids),
                offset: row.body_offset + cursor,
            });
        }
    }
    tables.sort_by_key(|table| table.offset);
    tables
}

fn positional_datum_geometry_table_at(
    body: &[u8],
    cursor: usize,
    entity_class: u32,
) -> Option<(u32, Vec<u32>)> {
    (body.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (count, after_count) = psb::compact_int(body, cursor + 1);
    (after_count > cursor + 1 && body.get(after_count) == Some(&psb::token::ENTITY_REF))
        .then_some(())?;
    let (stored_class, mut cursor) = psb::reference_id(body, after_count + 1).ok()?;
    (stored_class == entity_class).then_some(())?;
    if body.get(cursor) == Some(&psb::token::ARRAY_CLOSE) {
        cursor += 1;
    }
    if body.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }

    let capacity = bounded_len(u64::from(count), 1, body.len().saturating_sub(cursor))?;
    let entry_class = entity_class.checked_add(1)?;
    let mut entry_ids = Vec::with_capacity(capacity);
    for index in 0..count {
        if index == 0 {
            (body.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (stored_entry_class, after_class) = psb::reference_id(body, cursor + 1).ok()?;
            (stored_entry_class == entry_class).then_some(())?;
            cursor = after_class;
        } else {
            (body
                .get(cursor)
                .is_some_and(|byte| matches!(byte, 0xf1 | 0xf2)))
            .then_some(())?;
            (body.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (continuation_class, after_class) = psb::reference_id(body, cursor + 2).ok()?;
            (continuation_class == entity_class && body.get(after_class) == Some(&0xe2))
                .then_some(())?;
            cursor = after_class + 1;
        }
        let (entry_id, after_id) = psb::reference_id(body, cursor).ok()?;
        entry_ids.push(entry_id);
        cursor = after_id;
        if body.get(cursor) == Some(&0xf6) {
            cursor += 1;
        } else {
            let (_, after_dimension) = psb::reference_id(body, cursor).ok()?;
            cursor = after_dimension;
        }
    }
    Some((count, entry_ids))
}

fn geometry_table_at(
    body: &[u8],
    mut cursor: usize,
    kind: FeatureGeometryTableKind,
) -> Option<(u32, u32, Option<Vec<u32>>)> {
    if body
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1 | 0xf2))
    {
        cursor += 1;
    }
    if body.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return None;
    }
    let (count, after_count) = psb::compact_int(body, cursor + 1);
    if after_count == cursor + 1 || body.get(after_count) != Some(&psb::token::ENTITY_REF) {
        return None;
    }
    let (entity_class, mut after_class) = psb::reference_id(body, after_count + 1).ok()?;
    if body.get(after_class) == Some(&0xfb) {
        after_class += 1;
    }
    if body.get(after_class) == Some(&0xe2) {
        after_class += 1;
    }
    let entry_ids = if kind == FeatureGeometryTableKind::DatumIds {
        let mut entries = Vec::new();
        let mut entry_cursor = after_class;
        for _ in 0..count {
            const ENTRY: &[u8] = b"\xe0\x01dtm_id\0";
            if body.get(entry_cursor..entry_cursor + ENTRY.len()) != Some(ENTRY) {
                entries.clear();
                break;
            }
            let (entry, next) = psb::compact_int(body, entry_cursor + ENTRY.len());
            if next == entry_cursor + ENTRY.len() {
                entries.clear();
                break;
            }
            entries.push(entry);
            entry_cursor = next;
        }
        (entries.len() == usize::try_from(count).unwrap_or(usize::MAX)).then_some(entries)
    } else {
        None
    };
    Some((count, entity_class, entry_ids))
}

/// Decode complete named affected-ID arrays from known feature rows.
pub fn affected_ids(rows: &[FeatureRow]) -> Vec<FeatureAffectedIds> {
    const FIELDS: &[(&[u8], AffectedIdKind)] = &[
        (b"geoms_affected", AffectedIdKind::Geometry),
        (b"edgs_affected", AffectedIdKind::Edges),
        (b"strong_parents", AffectedIdKind::StrongParents),
        (b"parent_table", AffectedIdKind::Parents),
        (b"contours", AffectedIdKind::Contours),
        (b"qlts_affected", AffectedIdKind::Quilts),
    ];
    let mut result = Vec::new();
    for row in rows {
        for &(label, kind) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(label_offset) = find_from(&row.body, &needle, from) {
                from = label_offset + needle.len();
                if label_offset < 2
                    || row.body[label_offset - 2] != psb::token::NAMED_RECORD
                    || row.body.get(from) != Some(&psb::token::ARRAY_OPEN)
                {
                    continue;
                }
                let (count, mut cursor) = psb::compact_int(&row.body, from + 1);
                if cursor == from + 1 {
                    continue;
                }
                // Each id is a compact int of at least one byte, so the count
                // cannot exceed the unread bytes of the row body.
                let Some(capacity) =
                    bounded_len(u64::from(count), 1, row.body.len().saturating_sub(cursor))
                else {
                    continue;
                };
                let mut ids = Vec::with_capacity(capacity);
                for _ in 0..count {
                    let (id, next) = psb::compact_int(&row.body, cursor);
                    if next == cursor {
                        ids.clear();
                        break;
                    }
                    ids.push(id);
                    cursor = next;
                }
                if ids.len() == count as usize {
                    result.push(FeatureAffectedIds {
                        feature_id: row.feature_id,
                        kind,
                        ids,
                        offset: row.body_offset + label_offset - 2,
                    });
                }
            }
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

fn skip_replay_field_label(run: &[u8], cursor: usize, expected: &[u8]) -> Option<usize> {
    if run.get(cursor) != Some(&psb::token::NAMED_RECORD) {
        return Some(cursor);
    }
    let name_end = run
        .get(cursor + 2..)?
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| cursor + 2 + relative)?;
    (run.get(cursor + 2..name_end) == Some(expected)).then_some(name_end + 1)
}

fn replay_extent(
    run: &[u8],
    cursor: usize,
    field_name: &[u8],
    inherited: Option<u32>,
) -> Option<(u32, ReplayExtentSource, usize)> {
    let cursor = skip_replay_field_label(run, cursor, field_name)?;
    if run.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, after) = psb::compact_int(run, cursor + 1);
        (after > cursor + 1).then_some((count, ReplayExtentSource::Explicit, after))
    } else {
        inherited.map(|count| (count, ReplayExtentSource::Inherited, cursor))
    }
}

fn skip_replay_position_reference(run: &[u8], cursor: usize) -> Option<usize> {
    if run.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Some(cursor);
    }
    let (_, after) = psb::reference_id(run, cursor + 1).ok()?;
    (run.get(after) == Some(&psb::token::ARRAY_OPEN)).then_some(after)
}

fn replay_ids(run: &[u8], count: u32, mut cursor: usize) -> Option<(Vec<u32>, usize)> {
    // Each id is a compact int of at least one byte, so the count cannot exceed
    // the unread bytes of the run.
    bounded_len(u64::from(count), 1, run.len().saturating_sub(cursor))?;
    let mut ids = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (id, after) = psb::compact_int(run, cursor);
        if after == cursor {
            return None;
        }
        ids.push(id);
        cursor = after;
    }
    Some((ids, cursor))
}

struct ReplayAffectedPair {
    geometry_ids: Vec<u32>,
    edge_ids: Vec<u32>,
    geometry_extent: ReplayExtentSource,
    edge_extent: ReplayExtentSource,
    consumed: usize,
}

fn replay_affected_pair(run: &[u8], extents: [Option<u32>; 2]) -> Option<ReplayAffectedPair> {
    let (geometry_count, geometry_extent, cursor) =
        replay_extent(run, 0, b"geoms_affected", extents[0])?;
    let (geometry_ids, cursor) = replay_ids(run, geometry_count, cursor)?;
    let cursor = skip_replay_position_reference(run, cursor)?;
    let (edge_count, edge_extent, cursor) =
        replay_extent(run, cursor, b"edgs_affected", extents[1])?;
    let (edge_ids, cursor) = replay_ids(run, edge_count, cursor)?;
    Some(ReplayAffectedPair {
        geometry_ids,
        edge_ids,
        geometry_extent,
        edge_extent,
        consumed: cursor,
    })
}

fn explicit_replay_array(run: &[u8], opener: usize) -> Option<(Vec<u32>, usize)> {
    (run.get(opener) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (count, cursor) = psb::compact_int(run, opener + 1);
    (cursor > opener + 1).then_some(())?;
    replay_ids(run, count, cursor)
}

fn replay_entity_reference_end(bytes: &[u8], cursor: usize) -> Option<usize> {
    (bytes.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    psb::reference_id(bytes, cursor + 1)
        .ok()
        .map(|(_, after)| after)
}

fn replay_array_separator(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    if replay_entity_reference_end(bytes, 0) == Some(bytes.len()) {
        return true;
    }
    if bytes.first() == Some(&0xf0) {
        return replay_entity_reference_end(bytes, 1) == Some(bytes.len());
    }
    if bytes.first() != Some(&0xf1) {
        return false;
    }
    let Some(after_reference) = replay_entity_reference_end(bytes, 1) else {
        return false;
    };
    let Some(after_close) = bytes
        .get(after_reference..)
        .is_some_and(|tail| tail.starts_with(&[1, psb::token::COMPOUND_CLOSE]))
        .then_some(after_reference + 2)
    else {
        return false;
    };
    after_close == bytes.len()
        || replay_entity_reference_end(bytes, after_close) == Some(bytes.len())
}

fn replay_array_trailer(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes == [0xf5, 0x96, 0x92, 0x00] {
        return true;
    }
    if bytes.first() != Some(&0xf1) {
        return false;
    }
    replay_entity_reference_end(bytes, 1) == Some(bytes.len())
}

fn explicit_replay_pair_before_suffix(
    row: &FeatureRow,
    suffix: usize,
) -> Option<(ReplayAffectedPair, usize)> {
    let arrays = row.body[..suffix]
        .iter()
        .enumerate()
        .filter_map(|(opener, byte)| {
            (*byte == psb::token::ARRAY_OPEN)
                .then(|| explicit_replay_array(&row.body[..suffix], opener))
                .flatten()
                .map(|(ids, end)| (opener, ids, end))
        })
        .collect::<Vec<_>>();
    let [.., geometry, edges] = arrays.as_slice() else {
        return None;
    };
    let pair_prefix = match arrays.len() {
        2 => geometry.0 > 0 && row.body[geometry.0 - 1] == psb::token::COMPOUND_CLOSE,
        _ => {
            let preceding = &arrays[arrays.len() - 3];
            replay_array_separator(&row.body[preceding.2..geometry.0])
        }
    };
    pair_prefix.then_some(())?;
    replay_array_separator(&row.body[geometry.2..edges.0]).then_some(())?;
    replay_array_trailer(&row.body[edges.2..suffix]).then_some(())?;
    Some((
        ReplayAffectedPair {
            geometry_ids: geometry.1.clone(),
            edge_ids: edges.1.clone(),
            geometry_extent: ReplayExtentSource::Explicit,
            edge_extent: ReplayExtentSource::Explicit,
            consumed: suffix - geometry.0,
        },
        geometry.0,
    ))
}

fn unique_unanchored_replay_pair(
    row: &FeatureRow,
    extents: [Option<u32>; 2],
) -> Option<(ReplayAffectedPair, usize)> {
    let mut candidates = Vec::new();
    for suffix in row
        .body
        .windows(2)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0xe1, 0xe1]).then_some(offset))
    {
        let (row_id, after_id) = psb::compact_int(&row.body, suffix + 2);
        if after_id == suffix + 2 || row.body.get(after_id) != Some(&psb::token::COMPOUND_CLOSE) {
            continue;
        }
        let selector_start = if row.body.get(after_id + 1) == Some(&psb::token::COMPOUND_CLOSE) {
            after_id + 2
        } else if row.body.get(after_id + 1) == Some(&psb::token::ENTITY_REF) {
            let Ok((_, after_reference)) = psb::reference_id(&row.body, after_id + 2) else {
                continue;
            };
            if row.body.get(after_reference) != Some(&psb::token::COMPOUND_CLOSE) {
                continue;
            }
            after_reference + 1
        } else {
            continue;
        };
        let (_, after_selector) = psb::compact_int(&row.body, selector_start);
        let (repeated_row_id, after_repeated_id) = psb::compact_int(&row.body, after_selector);
        if after_selector == selector_start
            || after_repeated_id == after_selector
            || repeated_row_id != row_id
            || !matches!(
                row.body.get(after_repeated_id..after_repeated_id + 4),
                Some([0x00, 0xe1, 0x00, 0xe1 | psb::token::COMPOUND_CLOSE])
            )
        {
            continue;
        }
        if let Some(pair) = explicit_replay_pair_before_suffix(row, suffix) {
            candidates.push(pair);
            continue;
        }
        for start in 1..suffix {
            if row.body[start - 1] != psb::token::COMPOUND_CLOSE {
                continue;
            }
            let Some(pair) = replay_affected_pair(&row.body[start..suffix], extents) else {
                continue;
            };
            if pair.consumed == suffix - start {
                candidates.push((pair, start));
            }
        }
    }
    (candidates.len() == 1).then_some(())?;
    candidates.pop()
}

/// Decode the two affected-ID array positions in class-913 and class-914 replay rows.
///
/// Array extents are stateful within one `AllFeatur` stream and schema class.
/// An omitted `f8` opener reuses the preceding extent at the same array
/// position.
pub fn replay_affected_ids(rows: &[FeatureRow]) -> Vec<FeatureReplayAffectedIds> {
    const ANCHOR_PREFIX: &[u8] = &[0xf1, 0xf7, 0x42];
    const ANCHOR_SUFFIX: &[u8] = &[0x80, 0x01, 0xe3];
    const ANCHOR_LEN: usize = ANCHOR_PREFIX.len() + 1 + ANCHOR_SUFFIX.len();
    const TERMINATOR: &[u8] = &[0xf5, 0x96, 0x92];
    let mut result = Vec::new();
    let mut extents = BTreeMap::<(usize, u32), [Option<u32>; 2]>::new();
    for row in rows {
        let Some(schema_class @ (913 | 914)) = row.root_schema_class else {
            continue;
        };
        let anchor = row.body.windows(ANCHOR_LEN).rposition(|window| {
            window.starts_with(ANCHOR_PREFIX)
                && matches!(window[ANCHOR_PREFIX.len()], 0xc8 | 0xd8)
                && window.ends_with(ANCHOR_SUFFIX)
        });
        let state = extents
            .entry((row.stream_offset, schema_class))
            .or_default();
        let (pair, source_offset) = if let Some(anchor) = anchor {
            let run_start = anchor + ANCHOR_LEN;
            let Some(term) = find_from(&row.body, TERMINATOR, run_start) else {
                continue;
            };
            let run = &row.body[run_start..term];
            let Some(pair) = replay_affected_pair(run, *state) else {
                continue;
            };
            (pair, anchor)
        } else {
            let Some(pair) = unique_unanchored_replay_pair(row, *state) else {
                continue;
            };
            pair
        };
        let ReplayAffectedPair {
            geometry_ids,
            edge_ids,
            geometry_extent,
            edge_extent,
            ..
        } = pair;
        state[0] = Some(geometry_ids.len() as u32);
        state[1] = Some(edge_ids.len() as u32);
        result.push(FeatureReplayAffectedIds {
            feature_id: row.feature_id,
            geometry_ids,
            edge_ids,
            geometry_extent,
            edge_extent,
            offset: row.body_offset + source_offset,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}

fn unique_named_affected_ids(
    records: &[FeatureAffectedIds],
    feature_id: u32,
    kind: AffectedIdKind,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id && record.kind == kind);
    let ids = matches.next()?.ids.as_slice();
    matches
        .all(|record| record.ids.as_slice() == ids)
        .then_some(ids)
}

fn surface_merge_replay_suffix(bytes: &[u8]) -> bool {
    if bytes.get(..2) != Some(&[0xe1, 0xe1]) {
        return false;
    }
    let (row_id, after_row_id) = psb::compact_int(bytes, 2);
    if after_row_id == 2 || bytes.get(after_row_id) != Some(&psb::token::COMPOUND_CLOSE) {
        return false;
    }
    if bytes.get(after_row_id + 1) != Some(&psb::token::COMPOUND_CLOSE) {
        return false;
    }
    let selector = after_row_id + 2;
    let (_, after_selector) = psb::compact_int(bytes, selector);
    let (repeated_row_id, after_repeated_id) = psb::compact_int(bytes, after_selector);
    after_selector != selector
        && repeated_row_id == row_id
        && bytes.get(after_repeated_id..) == Some(&[0x00, 0xe1, 0x00, psb::token::COMPOUND_CLOSE])
}

fn positional_surface_merge_affected_ids(
    row: &FeatureRow,
    extents: [Option<u32>; 3],
) -> Option<FeatureSurfaceMergeAffectedIds> {
    const ANCHOR: &[u8] = &[0xf7, 0x80, 0x96];
    const QUILT_SEPARATOR: &[u8] = &[0xf0, 0xf7, 0x80, 0x99];
    let anchors = row
        .body
        .windows(ANCHOR.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == ANCHOR).then_some(offset))
        .collect::<Vec<_>>();
    let [anchor] = anchors.as_slice() else {
        return None;
    };
    let (_, cursor) = explicit_replay_array(&row.body, anchor + ANCHOR.len())?;
    if row.body.get(cursor..cursor + 2) != Some(&[0x01, psb::token::COMPOUND_CLOSE]) {
        return None;
    }
    let (geometry_count, geometry_extent, cursor) =
        replay_extent(&row.body, cursor + 2, b"geoms_affected", extents[0])?;
    let (geometry_ids, cursor) = replay_ids(&row.body, geometry_count, cursor)?;
    let (edge_count, edge_extent, cursor) =
        replay_extent(&row.body, cursor, b"edgs_affected", extents[1])?;
    let (edge_ids, cursor) = replay_ids(&row.body, edge_count, cursor)?;
    if row.body.get(cursor..cursor + QUILT_SEPARATOR.len()) != Some(QUILT_SEPARATOR) {
        return None;
    }
    let (quilt_count, quilt_extent, cursor) = replay_extent(
        &row.body,
        cursor + QUILT_SEPARATOR.len(),
        b"qlts_affected",
        extents[2],
    )?;
    let (quilt_ids, cursor) = replay_ids(&row.body, quilt_count, cursor)?;
    surface_merge_replay_suffix(row.body.get(cursor..)?).then_some(FeatureSurfaceMergeAffectedIds {
        feature_id: row.feature_id,
        geometry_ids,
        edge_ids,
        quilt_ids,
        geometry_extent,
        edge_extent,
        quilt_extent,
        offset: row.body_offset + anchor,
    })
}

/// Decode affected geometry, edge, and quilt arrays from class-946 replay rows.
///
/// Positional rows inherit an omitted array extent from the preceding
/// class-946 row in the same `AllFeatur` stream.
pub fn surface_merge_replay_affected_ids(
    rows: &[FeatureRow],
    named: &[FeatureAffectedIds],
) -> Vec<FeatureSurfaceMergeAffectedIds> {
    let mut result = Vec::new();
    let mut extents = BTreeMap::<usize, [Option<u32>; 3]>::new();
    for row in rows {
        if row.root_schema_class != Some(946) {
            continue;
        }
        let state = extents.entry(row.stream_offset).or_default();
        let named_arrays = [
            unique_named_affected_ids(named, row.feature_id, AffectedIdKind::Geometry),
            unique_named_affected_ids(named, row.feature_id, AffectedIdKind::Edges),
            unique_named_affected_ids(named, row.feature_id, AffectedIdKind::Quilts),
        ];
        if let [Some(geometry), Some(edges), Some(quilts)] = named_arrays {
            let (Ok(geometry_count), Ok(edge_count), Ok(quilt_count)) = (
                u32::try_from(geometry.len()),
                u32::try_from(edges.len()),
                u32::try_from(quilts.len()),
            ) else {
                continue;
            };
            *state = [Some(geometry_count), Some(edge_count), Some(quilt_count)];
            continue;
        }
        let Some(record) = positional_surface_merge_affected_ids(row, *state) else {
            continue;
        };
        let (Ok(geometry_count), Ok(edge_count), Ok(quilt_count)) = (
            u32::try_from(record.geometry_ids.len()),
            u32::try_from(record.edge_ids.len()),
            u32::try_from(record.quilt_ids.len()),
        ) else {
            continue;
        };
        *state = [Some(geometry_count), Some(edge_count), Some(quilt_count)];
        result.push(record);
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode named `direction` and `direction2` compact integers inside
/// `lo_restore` records.
pub fn loop_restore_directions(rows: &[FeatureRow]) -> Vec<FeatureLoopRestoreDirection> {
    const FIELDS: &[(&[u8], LoopRestoreDirectionLane)] = &[
        (b"direction", LoopRestoreDirectionLane::Primary),
        (b"direction2", LoopRestoreDirectionLane::Secondary),
    ];
    let mut result = Vec::new();
    for row in rows {
        for &(label, lane) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(label_offset) = find_from(&row.body, &needle, from) {
                from = label_offset + needle.len();
                if label_offset < 2
                    || row.body[label_offset - 2] != psb::token::NAMED_RECORD
                    || row.body[label_offset - 1] != 1
                    || !contains(&row.body[..label_offset - 2], b"lo_restore\0")
                {
                    continue;
                }
                let (value, after) = psb::compact_int(&row.body, from);
                if after == from {
                    continue;
                }
                result.push(FeatureLoopRestoreDirection {
                    feature_id: row.feature_id,
                    lane,
                    value,
                    offset: row.body_offset + label_offset - 2,
                });
            }
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode complete ordered `lo_hist` rosters paired with named loop tables.
pub fn loop_history_entries(
    rows: &[FeatureRow],
    geometry_tables: &[FeatureGeometryTable],
) -> Vec<FeatureLoopHistoryEntry> {
    const LABEL: &[u8] = b"\xe0\x01lo_hist\0";
    const RECORD_WIDTH: u32 = 6;
    let mut result = Vec::new();
    for table in geometry_tables
        .iter()
        .filter(|table| table.kind == FeatureGeometryTableKind::LoopIds)
    {
        let Some(row) = rows.iter().find(|row| {
            row.feature_id == table.feature_id
                && table.offset >= row.body_offset
                && table.offset < row.body_offset.saturating_add(row.body.len())
        }) else {
            continue;
        };
        let table_offset = table.offset - row.body_offset;
        let Some(label_offset) = find_from(&row.body, LABEL, table_offset) else {
            continue;
        };
        let label_stream_offset = row.body_offset + label_offset;
        if geometry_tables.iter().any(|other| {
            other.kind == FeatureGeometryTableKind::LoopIds
                && other.feature_id == table.feature_id
                && other.offset > table.offset
                && other.offset < label_stream_offset
        }) {
            continue;
        }
        let array_offset = label_offset + LABEL.len();
        if row.body.get(array_offset) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (width, roster_offset) = psb::compact_int(&row.body, array_offset + 1);
        if width != RECORD_WIDTH || roster_offset == array_offset + 1 {
            continue;
        }
        let Ok(count) = usize::try_from(table.count) else {
            continue;
        };
        let Some(entries) = loop_history_roster(&row.body, roster_offset, count) else {
            continue;
        };
        result.extend((0..table.count).zip(entries).map(|(ordinal, entry)| {
            FeatureLoopHistoryEntry {
                feature_id: row.feature_id,
                ordinal,
                loop_id: entry.loop_id,
                field_bytes: entry.field_bytes,
                boundary: entry.boundary,
                offset: row.body_offset + entry.offset,
                end_offset: row.body_offset + entry.end_offset,
            }
        }));
    }
    result.sort_by_key(|entry| entry.offset);
    result
}

pub(crate) fn loop_history_roster(
    body: &[u8],
    mut cursor: usize,
    count: usize,
) -> Option<Vec<ParsedLoopHistoryEntry>> {
    const FIELD_COUNT: usize = 4;
    (count > 0 && count <= body.len().saturating_sub(cursor) / 2).then_some(())?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let offset = cursor;
        let (loop_id, after_id) = psb::compact_int(body, cursor);
        (after_id > cursor && body[cursor] <= 0xbf).then_some(())?;
        cursor = after_id;
        let mut field_bytes = Vec::with_capacity(FIELD_COUNT + 1);
        for _ in 0..FIELD_COUNT {
            let token = psb::token_at(body, cursor)?;
            (!matches!(
                token.kind,
                psb::TokenKind::CompoundClose | psb::TokenKind::Truncated(_)
            ))
            .then_some(())?;
            field_bytes.push(
                body.get(cursor..cursor.checked_add(token.length)?)?
                    .to_vec(),
            );
            cursor = cursor.checked_add(token.length)?;
        }
        let boundary = if body.get(cursor) == Some(&0xe3) {
            cursor += 1;
            FeatureLoopHistoryBoundary::CompoundClose
        } else if body
            .get(cursor)
            .is_some_and(|byte| matches!(byte, 0xf1 | 0xf2))
        {
            let marker = body[cursor];
            (body.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (reference, after_reference) = psb::reference_id(body, cursor + 2).ok()?;
            (body.get(after_reference) == Some(&0xe3)).then_some(())?;
            cursor = after_reference + 1;
            if marker == 0xf1 {
                FeatureLoopHistoryBoundary::ReferenceContinue(reference)
            } else {
                FeatureLoopHistoryBoundary::ReferenceFinal(reference)
            }
        } else {
            (index + 1 == count).then_some(())?;
            let token = psb::token_at(body, cursor)?;
            if token.kind != psb::TokenKind::NamedRecord {
                (!matches!(
                    token.kind,
                    psb::TokenKind::CompoundClose | psb::TokenKind::Truncated(_)
                ))
                .then_some(())?;
                field_bytes.push(
                    body.get(cursor..cursor.checked_add(token.length)?)?
                        .to_vec(),
                );
                cursor = cursor.checked_add(token.length)?;
                matches!(
                    psb::token_at(body, cursor).map(|token| token.kind),
                    Some(psb::TokenKind::NamedRecord)
                )
                .then_some(())?;
            }
            FeatureLoopHistoryBoundary::NamedRecord
        };
        entries.push(ParsedLoopHistoryEntry {
            loop_id,
            field_bytes,
            boundary,
            offset,
            end_offset: cursor,
        });
    }
    Some(entries)
}

pub(crate) struct ParsedLoopHistoryEntry {
    pub(crate) loop_id: u32,
    pub(crate) field_bytes: Vec<Vec<u8>>,
    pub(crate) boundary: FeatureLoopHistoryBoundary,
    pub(crate) offset: usize,
    pub(crate) end_offset: usize,
}

/// Decode full-turn rotational termination from the positional
/// `param_choice_ptr` body of section-sweep feature rows.
pub fn revolution_extents(rows: &[FeatureRow]) -> Vec<FeatureRevolutionExtent> {
    const PARAMETER_CHOICE_PREFIX: &[u8] = &[0x83, 0xdf, 0xf6, 0xe3];
    const FULL_TURN_CHOICES: &[u8] = &[
        0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00, 0x00, 0x00,
    ];
    let mut result = Vec::new();
    for row in rows {
        if !matches!(row.root_schema_class, Some(916 | 917)) {
            continue;
        }
        let Some(schema_end) = (0..row.body.len().min(20)).find_map(|offset| {
            if row.body.get(offset..offset + 2) != Some(&[0xe3, 0xf6]) {
                return None;
            }
            let (schema_class, after) = psb::compact_int(&row.body, offset + 2);
            (Some(schema_class) == row.root_schema_class && row.body.get(after) == Some(&0xe1))
                .then_some(after + 1)
        }) else {
            continue;
        };
        if row.body.get(schema_end) != Some(&2) {
            continue;
        }
        let Some(choice_start) = find_in(
            &row.body,
            PARAMETER_CHOICE_PREFIX,
            schema_end + 1,
            row.body.len().min(64),
        )
        .map(|at| at + PARAMETER_CHOICE_PREFIX.len()) else {
            continue;
        };
        if row
            .body
            .get(choice_start..choice_start + FULL_TURN_CHOICES.len())
            != Some(FULL_TURN_CHOICES)
        {
            continue;
        }
        result.push(FeatureRevolutionExtent {
            feature_id: row.feature_id,
            kind: FeatureRevolutionExtentKind::FullTurn,
            offset: row.body_offset + choice_start + 2,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}
