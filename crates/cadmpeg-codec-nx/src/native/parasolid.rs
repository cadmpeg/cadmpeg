// SPDX-License-Identifier: Apache-2.0
//! Parasolid source-record extractors and their record types.

#[allow(clippy::wildcard_imports)]
use super::*;

use crate::deltas::Census;

use super::substrate::{ParsedStreams, StreamView};

use std::collections::{BTreeMap, BTreeSet};

/// One complete Parasolid GROUP record with its source and owning-partition scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidGroupRecord {
    /// Globally unique source-record identity.
    pub id: String,
    /// Stream containing the exact serialized GROUP record.
    pub stream_ordinal: u32,
    /// `partition` or `deltas` source classification.
    pub stream_kind: String,
    /// Partition whose local node-id namespace owns this GROUP.
    ///
    /// An unpaired deltas stream retains the record without assigning a
    /// partition namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_stream_ordinal: Option<u32>,
    /// Stream-local XMT identity.
    pub xmt: u32,
    /// Partition-local kernel node identity.
    pub node_id: u32,
    /// Ordered GROUP references without their framing status bytes.
    pub references: Vec<u32>,
    /// Selector between the four leading references and the linked reference.
    pub selector: u8,
    /// Status byte following the linked reference.
    pub linked_reference_status: u8,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// GROUP tag offset in the inflated source stream.
    pub inflated_offset: u64,
}

/// One topology member in a fully closed current Parasolid GROUP chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidGroupMember {
    /// Globally unique membership identity.
    pub id: String,
    /// Partition whose local XMT and node namespaces own the chain.
    pub partition_stream_ordinal: u32,
    /// Current GROUP record XMT identity.
    pub group_xmt: u32,
    /// Current GROUP kernel node identity.
    pub group_node_id: u32,
    /// Zero-based member order from the list head to tail.
    pub ordinal: u32,
    /// `TYPE_91` list-record XMT identity.
    pub list_record_xmt: u32,
    /// Member record XMT identity.
    pub member_xmt: u32,
    /// Parasolid topology family of the member record.
    pub member_family: String,
    /// Kernel node identity when the member family carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_node_id: Option<u32>,
    /// Current semantic XMT identity selected by unique family and kernel node identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_member_xmt: Option<u32>,
}

/// Retain GROUP records from partition streams and raw deltas overlays.
///
/// Deltas records use the partition pairing already selected for topology
/// reconstruction. A record in an unpaired deltas stream remains exact native
/// evidence but has no partition-local namespace assignment.
pub(crate) fn parasolid_group_records(
    streams: &[Stream],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
    deltas_records: &[ParasolidDeltasRecord],
) -> Vec<ParasolidGroupRecord> {
    let paired_partition = delta_pairs
        .iter()
        .flat_map(|(partition, deltas)| {
            deltas
                .iter()
                .filter_map(move |delta| Some((*delta, u32::try_from(*partition).ok()?)))
        })
        .collect::<BTreeMap<_, _>>();
    let mut groups = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if stream.kind != crate::parasolid::StreamKind::Partition {
            continue;
        }
        let Ok(stream_ordinal_u32) = u32::try_from(stream_ordinal) else {
            continue;
        };
        for record in crate::deltas::walk(&stream.inflated)
            .records
            .into_iter()
            .filter(|record| crate::deltas::record_family_name(record) == Some("GROUP"))
        {
            let Some(node_id) = record.node_id else {
                continue;
            };
            let Some(controls) = crate::deltas::group_controls(&record) else {
                continue;
            };
            groups.push(ParasolidGroupRecord {
                id: format!(
                    "nx:s{stream_ordinal}:parasolid-group#{}-{}",
                    record.offset, record.xmt
                ),
                stream_ordinal: stream_ordinal_u32,
                stream_kind: stream.kind.label().to_string(),
                partition_stream_ordinal: Some(stream_ordinal_u32),
                xmt: record.xmt,
                node_id,
                references: record.references,
                selector: controls.selector,
                linked_reference_status: controls.linked_reference_status,
                byte_len: (record.end - record.offset) as u64,
                inflated_offset: record.offset as u64,
            });
        }
    }
    for record in deltas_records
        .iter()
        .filter(|record| record.family == "GROUP")
    {
        let Some(node_id) = record.node_id else {
            continue;
        };
        let (Some(selector), Some(linked_reference_status)) =
            (record.group_selector, record.group_linked_reference_status)
        else {
            continue;
        };
        groups.push(ParasolidGroupRecord {
            id: record.id.replacen("deltas-record", "parasolid-group", 1),
            stream_ordinal: record.stream_ordinal,
            stream_kind: "deltas".to_string(),
            partition_stream_ordinal: usize::try_from(record.stream_ordinal)
                .ok()
                .and_then(|delta| paired_partition.get(&delta).copied()),
            xmt: record.xmt,
            node_id,
            references: record.references.clone(),
            selector,
            linked_reference_status,
            byte_len: record.byte_len,
            inflated_offset: record.inflated_offset,
        });
    }
    groups.sort_by_key(|group| (group.stream_ordinal, group.inflated_offset));
    groups
}

fn is_group_member_family(family: &str) -> bool {
    matches!(
        family,
        "BODY" | "SHELL" | "FACE" | "LOOP" | "FIN" | "EDGE" | "VERTEX" | "REGION"
    )
}

fn group_members_from_records(
    partition_stream_ordinal: u32,
    records: &[crate::deltas::Record],
) -> Vec<ParasolidGroupMember> {
    let mut records_by_xmt = BTreeMap::<u32, Vec<&crate::deltas::Record>>::new();
    for record in records {
        records_by_xmt.entry(record.xmt).or_default().push(record);
    }
    let unique_record = |xmt| match records_by_xmt.get(&xmt).map(Vec::as_slice) {
        Some([record]) => Some(*record),
        _ => None,
    };
    let mut groups_by_node = BTreeMap::<u32, Vec<&crate::deltas::Record>>::new();
    for record in records
        .iter()
        .filter(|record| crate::deltas::record_family_name(record) == Some("GROUP"))
    {
        let Some(node_id) = record.node_id else {
            continue;
        };
        groups_by_node.entry(node_id).or_default().push(record);
    }
    let mut members = Vec::new();
    for groups in groups_by_node.values() {
        let [group] = groups.as_slice() else {
            continue;
        };
        let Some(&tail) = group.references.get(4) else {
            continue;
        };
        let mut reverse_chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = tail;
        let mut expected_next = 1;
        let mut complete = true;
        while current != 1 {
            if !seen.insert(current) {
                complete = false;
                break;
            }
            let Some(list_record) = unique_record(current) else {
                complete = false;
                break;
            };
            if crate::deltas::record_family_name(list_record) != Some("TYPE_91")
                || list_record.references.len() != 6
                || list_record.references[0] != group.xmt
                || list_record.references[5] != expected_next
            {
                complete = false;
                break;
            }
            let member_xmt = list_record.references[1];
            let Some(member_record) = unique_record(member_xmt) else {
                complete = false;
                break;
            };
            let Some(member_family) = crate::deltas::record_family_name(member_record) else {
                complete = false;
                break;
            };
            if !is_group_member_family(member_family) {
                complete = false;
                break;
            }
            reverse_chain.push((current, member_xmt, member_family, member_record.node_id));
            expected_next = current;
            current = list_record.references[4];
        }
        if !complete || reverse_chain.is_empty() {
            continue;
        }
        reverse_chain.reverse();
        let Some(group_node_id) = group.node_id else {
            continue;
        };
        members.extend(reverse_chain.into_iter().enumerate().filter_map(
            |(ordinal, (list_record_xmt, member_xmt, member_family, member_node_id))| {
                Some(ParasolidGroupMember {
                    id: format!(
                        "nx:s{partition_stream_ordinal}:parasolid-group-member#{group_node_id}-{}-{ordinal}",
                        group.xmt
                    ),
                    partition_stream_ordinal,
                    group_xmt: group.xmt,
                    group_node_id,
                    ordinal: u32::try_from(ordinal).ok()?,
                    list_record_xmt,
                    member_xmt,
                    member_family: member_family.to_string(),
                    member_node_id,
                    current_member_xmt: None,
                })
            },
        ));
    }
    members
}

fn apply_group_state_events(records: &mut BTreeMap<u32, crate::deltas::Record>, bytes: &[u8]) {
    enum Event {
        Record(crate::deltas::Record),
        Tombstone(u32),
    }
    let census = crate::deltas::walk(bytes);
    let mut events = census
        .records
        .into_iter()
        .map(|record| (record.offset, Event::Record(record)))
        .chain(
            census
                .tombstones
                .into_iter()
                .map(|tombstone| (tombstone.offset, Event::Tombstone(tombstone.xmt))),
        )
        .collect::<Vec<_>>();
    events.sort_by_key(|(offset, _)| *offset);
    for (_, event) in events {
        match event {
            Event::Record(record) => {
                records.insert(record.xmt, record);
            }
            Event::Tombstone(xmt) => {
                records.remove(&xmt);
            }
        }
    }
}

/// Resolve current GROUP membership from partition and ordered deltas events.
pub(crate) fn parasolid_group_members(
    streams: &[Stream],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
    parsed: &ParsedStreams<'_>,
) -> Vec<ParasolidGroupMember> {
    let mut members = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind == crate::parasolid::StreamKind::Partition)
        .filter_map(|(stream_ordinal, stream)| {
            let stream_ordinal_u32 = u32::try_from(stream_ordinal).ok()?;
            let mut current = BTreeMap::new();
            apply_group_state_events(&mut current, &stream.inflated);
            for delta in delta_pairs.get(&stream_ordinal).into_iter().flatten() {
                apply_group_state_events(&mut current, &streams.get(*delta)?.inflated);
            }
            Some((
                stream_ordinal_u32,
                current.into_values().collect::<Vec<_>>(),
            ))
        })
        .flat_map(|(stream_ordinal, records)| group_members_from_records(stream_ordinal, &records))
        .collect::<Vec<_>>();
    for member in &mut members {
        let (Some(kind), Some(node_id), Ok(partition)) = (
            group_member_kind(&member.member_family),
            member.member_node_id,
            usize::try_from(member.partition_stream_ordinal),
        ) else {
            continue;
        };
        let graph = parsed.stream(partition).view_for_geometry().graph.as_ref();
        member.current_member_xmt = resolved_current_member_xmt(graph, member, kind, node_id);
    }
    members
}

fn group_member_kind(family: &str) -> Option<u8> {
    Some(match family {
        "BODY" => 12,
        "SHELL" => 13,
        "FACE" => 14,
        "LOOP" => 15,
        "FIN" => 17,
        "EDGE" => 16,
        "VERTEX" => 18,
        "REGION" => 19,
        _ => return None,
    })
}

/// Resolve a GROUP member against the current merged topology graph.
///
/// The member XMT is the identity selected by the current GROUP chain, so it
/// is the primary lookup key. A node-ID lookup is retained as a guarded
/// compatibility path for a delta revision that changes the XMT while
/// preserving the kernel node identity. Both paths require the expected
/// topology family and the serialized node identity to agree.
fn resolved_current_member_xmt(
    graph: &crate::topology::Graph,
    member: &ParasolidGroupMember,
    kind: u8,
    node_id: u32,
) -> Option<u32> {
    graph
        .get(kind, member.member_xmt)
        .filter(|node| node.node_id() == Some(node_id))
        .map(|node| node.xmt)
        .or_else(|| graph.unique_xmt_by_node_id(kind, node_id))
}

/// One completely bounded record in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidDeltasRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Stable Parasolid record-family name.
    pub family: String,
    /// Numeric Parasolid node type.
    pub kind: u16,
    /// Stream-local XMT identity.
    pub xmt: u32,
    /// Kernel node identity when serialized by this family.
    pub node_id: Option<u32>,
    /// Ordered decoded XMT references.
    pub references: Vec<u32>,
    /// GROUP selector byte when this is a GROUP record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_selector: Option<u8>,
    /// GROUP linked-reference status when this is a GROUP record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_linked_reference_status: Option<u8>,
    /// Model-space point in Parasolid metres when serialized by this family.
    pub position: Option<[f64; 3]>,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// One compact deletion in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasTombstone {
    /// Globally unique event identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Stable Parasolid record-family name.
    pub family: String,
    /// Numeric Parasolid node type.
    pub kind: u16,
    /// Stream-local deleted XMT identity.
    pub xmt: u32,
    /// Exact compact tombstone length.
    pub byte_len: u64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// BODY revision envelope in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasBodyRevision {
    /// Globally unique revision identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local BODY XMT identity.
    pub xmt: u32,
    /// Monotonic kernel revision identity.
    pub node_id: u32,
    /// Eight ordered BODY references.
    pub references: [u32; 8],
    /// Exact complete envelope length.
    pub byte_len: u64,
    /// Exact validated prefix length.
    pub prefix_byte_len: u64,
    /// Exact bounded state-tail length.
    pub state_tail_byte_len: u64,
    /// SHA-256 of the exact bounded state-tail bytes.
    pub state_tail_sha256: String,
    /// BODY tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Parasolid transmit header at the start of a deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasTransmitHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Printable transmit-file description.
    pub description: String,
    /// Declared Parasolid schema token.
    pub schema: String,
    /// Consecutive stream-local header identities.
    pub references: [u32; 2],
    /// Exact header byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact header bytes.
    pub sha256: String,
    /// First header byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Null references at the boundary of a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasTerminalNullReferences {
    /// Globally unique trailer identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Ordered null XMT references.
    pub references: Vec<u32>,
    /// Exact trailer byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact trailer bytes.
    pub sha256: String,
    /// First trailer byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Count-selected numeric lane following one deltas `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidDeltasTermUseNumericTail {
    /// Globally unique numeric-tail identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// XMT identity of the owning `term_use` record.
    pub term_use_xmt: u32,
    /// Serialized endpoint count selecting the numeric-tail cardinality.
    pub term_use_count: u32,
    /// Ordered finite binary64 values without assigned semantic roles.
    pub values: Vec<f64>,
    /// Exact numeric-tail byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact numeric-tail bytes.
    pub sha256: String,
    /// First numeric byte following the complete `term_use` record.
    pub inflated_offset: u64,
}

/// Maximal deltas gap composed entirely of typed stream-local references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasTaggedReferenceLane {
    /// Globally unique reference-lane identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Ordered `(Parasolid record kind, XMT identity)` references.
    pub references: Vec<(u16, u32)>,
    /// Exact reference-lane byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact reference-lane bytes.
    pub sha256: String,
    /// First byte of the first tagged reference.
    pub inflated_offset: u64,
}

/// Framed reference/type map in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasReferenceTypeMap {
    /// Globally unique map identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Ordered `(XMT identity, Parasolid type code)` entries.
    pub entries: Vec<(u32, u16)>,
    /// Type code of the optional terminal map target.
    pub target_kind: Option<u16>,
    /// Exact map byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact map bytes.
    pub sha256: String,
    /// First map byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// One four-reference frame in a Parasolid deltas state packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasReferenceStateFrame {
    /// Four ordered stream-local XMT references.
    pub references: [u32; 4],
    /// Five ordered big-endian state words.
    pub state_words: [u32; 5],
    /// Terminal serialized state byte.
    pub state_byte: u8,
}

/// Reference-state packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasReferenceStatePacket {
    /// Globally unique packet identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Ordered packet frames.
    pub frames: Vec<ParasolidDeltasReferenceStateFrame>,
    /// Whether the packet ends with `ref(1)[3], u32(1)`.
    pub terminal: bool,
    /// Exact packet byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    pub sha256: String,
    /// First packet byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Schema reference preamble in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasSchemaReferencePreamble {
    /// Globally unique preamble identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Repeated serialized identity.
    pub identity: u16,
    /// Two consecutive non-null stream-local XMT references.
    pub references: [u32; 2],
    /// Non-null state-lane reference between null sentinels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_reference: Option<u32>,
    /// Four ordered big-endian state words.
    pub state_words: [u32; 4],
    /// Serialized state count.
    pub count: u16,
    /// Ordered `(Parasolid record kind, XMT identity)` entries.
    pub entries: Vec<(u16, u32)>,
    /// Terminal serialized state value.
    pub terminal_value: u16,
    /// Exact preamble byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact preamble bytes.
    pub sha256: String,
    /// First preamble byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Reference-marker packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasReferenceMarkerPacket {
    /// Globally unique packet identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Non-null stream-local XMT reference.
    pub reference: u32,
    /// Serialized marker byte.
    pub marker: u8,
    /// Exact packet byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    pub sha256: String,
    /// First packet byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Single-byte type-150 state packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidDeltasType150StatePacket {
    /// Globally unique packet identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Five ordered stream-local XMT references.
    pub references: [u32; 5],
    /// Serialized state discriminator.
    pub marker: u8,
    /// Nine finite binary64 state values.
    pub values: [f64; 9],
    /// Exact packet byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    pub sha256: String,
    /// First packet byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Body of an inline schema declaration in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema", rename_all = "snake_case")]
pub enum ParasolidDeltasInlineSchemaFields {
    /// Type 12 `BODY` schema header without following instance state.
    BodyHeader,
    /// REGION declaration state.
    Region {
        xmt: u32,
        state_word: u32,
        references: [u32; 4],
    },
    /// `ATTDEF_LIST` declaration state.
    AttdefList {
        xmt: u32,
        slot_count: u32,
        active_count: u32,
        references: Vec<u32>,
    },
    /// Type 70 declaration state.
    Type70 {
        xmt: u32,
        node_id: u32,
        references: [u32; 4],
        count: u16,
        trailing_reference: u32,
    },
    /// Type 100 declaration and its precision state.
    Type100 {
        xmt: u32,
        references: [u32; 3],
        transform: [f64; 13],
    },
    /// Type 101 declaration and its schema-bound instance state.
    Type101 {
        references: [u32; 4],
        anchor_reference: Option<u32>,
        state_words: [u32; 3],
        terminal_value: u64,
    },
    /// Type 101 declaration with the compact fixed state.
    Type101Compact,
    /// Type 38 intersection-data declaration state.
    Type38 {
        xmt: u32,
        node_id: u32,
        leading_references: [u32; 5],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leading_statuses: Option<[u8; 5]>,
        marker: u8,
        linked_references: Vec<u32>,
        state_references: Vec<u32>,
        numeric_values: Option<[f64; 11]>,
    },
    /// Type 41 term-use declaration state.
    Type41 {
        reference: u32,
        numeric_values: [f64; 11],
    },
}

/// Inline schema declaration in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidDeltasInlineSchemaDeclaration {
    /// Globally unique declaration identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Schema-specific declaration body.
    #[serde(flatten)]
    pub fields: ParasolidDeltasInlineSchemaFields,
    /// Exact declaration byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact declaration bytes.
    pub sha256: String,
    /// First declaration byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Schema-bound type-12 `BODY` instance-state fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum ParasolidDeltasInlineBodyStateFields {
    /// Compact reference form followed by status zero.
    Compact {
        /// Non-null stream-local XMT reference.
        reference: u32,
    },
    /// Revision form with a bounded opaque state tail.
    Revision {
        /// Monotonic kernel revision identity.
        node_id: u32,
        /// Eight ordered status-framed XMT references.
        references: [u32; 8],
        /// Exact state bytes following the reference prefix.
        state_bytes: Vec<u8>,
    },
}

/// Schema-bound type-12 `BODY` instance state in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasInlineBodyState {
    /// Globally unique state identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Serialized state form.
    pub fields: ParasolidDeltasInlineBodyStateFields,
    /// Exact state byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact state bytes.
    pub sha256: String,
    /// First state byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Maximal inflated-stream span outside every admitted deltas event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasResidualSpan {
    /// Globally unique residual-span identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Exact residual byte length.
    pub byte_len: u64,
    /// SHA-256 of the residual bytes.
    pub sha256: String,
    /// First residual byte offset in the inflated stream.
    pub inflated_offset: u64,
}

pub(crate) struct ParasolidDeltasEvents {
    pub(crate) transmit_headers: Vec<ParasolidDeltasTransmitHeader>,
    pub(crate) terminal_null_references: Vec<ParasolidDeltasTerminalNullReferences>,
    pub(crate) records: Vec<ParasolidDeltasRecord>,
    pub(crate) tombstones: Vec<ParasolidDeltasTombstone>,
    pub(crate) body_revisions: Vec<ParasolidDeltasBodyRevision>,
    pub(crate) term_use_numeric_tails: Vec<ParasolidDeltasTermUseNumericTail>,
    pub(crate) tagged_reference_lanes: Vec<ParasolidDeltasTaggedReferenceLane>,
    pub(crate) reference_type_maps: Vec<ParasolidDeltasReferenceTypeMap>,
    pub(crate) reference_state_packets: Vec<ParasolidDeltasReferenceStatePacket>,
    pub(crate) schema_reference_preambles: Vec<ParasolidDeltasSchemaReferencePreamble>,
    pub(crate) reference_marker_packets: Vec<ParasolidDeltasReferenceMarkerPacket>,
    pub(crate) type_150_state_packets: Vec<ParasolidDeltasType150StatePacket>,
    pub(crate) inline_schema_declarations: Vec<ParasolidDeltasInlineSchemaDeclaration>,
    pub(crate) inline_body_states: Vec<ParasolidDeltasInlineBodyState>,
    pub(crate) residual_spans: Vec<ParasolidDeltasResidualSpan>,
}

/// Retain every completely bounded event in every Parasolid deltas stream.
#[cfg(test)]
pub(crate) fn parasolid_deltas_events(streams: &[Stream]) -> ParasolidDeltasEvents {
    let delta_censuses = streams
        .iter()
        .map(|stream| {
            (stream.kind == crate::parasolid::StreamKind::Deltas)
                .then(|| crate::deltas::walk(&stream.inflated))
        })
        .collect();
    parasolid_deltas_events_with_censuses(streams, delta_censuses)
}

/// Retain deltas events from censuses produced by the shared decode substrate.
///
/// The function consumes the census vector after semantic construction has
/// finished, so the large record walk is performed once and its owned records
/// are moved directly into native output.
pub(crate) fn parasolid_deltas_events_with_censuses(
    streams: &[Stream],
    mut delta_censuses: Vec<Option<Census>>,
) -> ParasolidDeltasEvents {
    let mut events = ParasolidDeltasEvents {
        transmit_headers: Vec::new(),
        terminal_null_references: Vec::new(),
        records: Vec::new(),
        tombstones: Vec::new(),
        body_revisions: Vec::new(),
        term_use_numeric_tails: Vec::new(),
        tagged_reference_lanes: Vec::new(),
        reference_type_maps: Vec::new(),
        reference_state_packets: Vec::new(),
        schema_reference_preambles: Vec::new(),
        reference_marker_packets: Vec::new(),
        type_150_state_packets: Vec::new(),
        inline_schema_declarations: Vec::new(),
        inline_body_states: Vec::new(),
        residual_spans: Vec::new(),
    };
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if stream.kind != crate::parasolid::StreamKind::Deltas {
            continue;
        }
        let census = delta_censuses
            .get_mut(stream_ordinal)
            .and_then(Option::take)
            .unwrap_or_else(|| crate::deltas::walk(&stream.inflated));
        let mut residual_start = 0;
        for (covered_start, covered_end) in census.covered_spans() {
            if residual_start < covered_start {
                push_deltas_residual_span(
                    &mut events.residual_spans,
                    stream_ordinal,
                    &stream.inflated,
                    residual_start,
                    covered_start,
                );
            }
            residual_start = residual_start.max(covered_end);
        }
        if residual_start < stream.inflated.len() {
            push_deltas_residual_span(
                &mut events.residual_spans,
                stream_ordinal,
                &stream.inflated,
                residual_start,
                stream.inflated.len(),
            );
        }
        if let Some(header) = census.transmit_header {
            let bytes = &stream.inflated[..header.end];
            events.transmit_headers.push(ParasolidDeltasTransmitHeader {
                id: format!("nx:s{stream_ordinal}:deltas-transmit-header#0"),
                stream_ordinal: stream_ordinal as u32,
                description: header.description,
                schema: header.schema,
                references: header.references,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                inflated_offset: 0,
            });
        }
        if let Some(trailer) = census.terminal_null_references {
            let bytes = &stream.inflated[trailer.offset..trailer.end];
            events
                .terminal_null_references
                .push(ParasolidDeltasTerminalNullReferences {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-terminal-null-references#{}",
                        trailer.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    references: std::iter::repeat_n(1_u32, trailer.count.into()).collect(),
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: trailer.offset as u64,
                });
        }
        for record in census.records {
            let family = crate::deltas::record_family_name(&record)
                .expect("the deltas walker admits only named record families");
            let group_controls = crate::deltas::group_controls(&record);
            events.records.push(ParasolidDeltasRecord {
                id: format!(
                    "nx:s{stream_ordinal}:deltas-record#{}-{}",
                    record.offset, record.xmt
                ),
                stream_ordinal: stream_ordinal as u32,
                family: family.to_string(),
                kind: record.kind,
                xmt: record.xmt,
                node_id: record.node_id,
                references: record.references,
                group_selector: group_controls.map(|controls| controls.selector),
                group_linked_reference_status: group_controls
                    .map(|controls| controls.linked_reference_status),
                position: record.position,
                byte_len: (record.end - record.offset) as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for tombstone in census.tombstones {
            let family = crate::deltas::family_name(tombstone.kind)
                .expect("the deltas walker admits only named tombstone families");
            events.tombstones.push(ParasolidDeltasTombstone {
                id: format!(
                    "nx:s{stream_ordinal}:deltas-tombstone#{}-{}",
                    tombstone.offset, tombstone.xmt
                ),
                stream_ordinal: stream_ordinal as u32,
                family: family.to_string(),
                kind: tombstone.kind,
                xmt: tombstone.xmt,
                byte_len: 6,
                inflated_offset: tombstone.offset as u64,
            });
        }
        for revision in census.body_revisions {
            let state_tail = &stream.inflated[revision.prefix_end..revision.end];
            events.body_revisions.push(ParasolidDeltasBodyRevision {
                id: format!(
                    "nx:s{stream_ordinal}:deltas-body-revision#{}-{}",
                    revision.offset, revision.node_id
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: revision.xmt,
                node_id: revision.node_id,
                references: revision.references,
                byte_len: (revision.end - revision.offset) as u64,
                prefix_byte_len: (revision.prefix_end - revision.offset) as u64,
                state_tail_byte_len: state_tail.len() as u64,
                state_tail_sha256: cadmpeg_ir::hash::sha256_hex(state_tail),
                inflated_offset: revision.offset as u64,
            });
        }
        for tail in census.term_use_numeric_tails {
            let bytes = &stream.inflated[tail.offset..tail.end];
            events
                .term_use_numeric_tails
                .push(ParasolidDeltasTermUseNumericTail {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-term-use-tail#{}-{}",
                        tail.offset, tail.term_use_xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    term_use_xmt: tail.term_use_xmt,
                    term_use_count: tail.term_use_count,
                    values: tail.values,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: tail.offset as u64,
                });
        }
        for lane in census.tagged_reference_lanes {
            let bytes = &stream.inflated[lane.offset..lane.end];
            events
                .tagged_reference_lanes
                .push(ParasolidDeltasTaggedReferenceLane {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-tagged-reference-lane#{}",
                        lane.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    references: lane.references,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: lane.offset as u64,
                });
        }
        for map in census.reference_type_maps {
            let bytes = &stream.inflated[map.offset..map.end];
            events
                .reference_type_maps
                .push(ParasolidDeltasReferenceTypeMap {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-reference-type-map#{}",
                        map.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    entries: map.entries,
                    target_kind: map.target_kind,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: map.offset as u64,
                });
        }
        for packet in census.reference_state_packets {
            let bytes = &stream.inflated[packet.offset..packet.end];
            events
                .reference_state_packets
                .push(ParasolidDeltasReferenceStatePacket {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-reference-state#{}",
                        packet.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    frames: packet
                        .frames
                        .into_iter()
                        .map(|frame| ParasolidDeltasReferenceStateFrame {
                            references: frame.references,
                            state_words: frame.state_words,
                            state_byte: frame.state_byte,
                        })
                        .collect(),
                    terminal: packet.terminal,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: packet.offset as u64,
                });
        }
        for preamble in census.schema_reference_preambles {
            let bytes = &stream.inflated[preamble.offset..preamble.end];
            events
                .schema_reference_preambles
                .push(ParasolidDeltasSchemaReferencePreamble {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-schema-reference-preamble#{}",
                        preamble.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    identity: preamble.identity,
                    references: preamble.references,
                    state_reference: (preamble.state_references != [1; 3])
                        .then_some(preamble.state_references[1]),
                    state_words: preamble.state_words,
                    count: preamble.count,
                    entries: preamble.entries,
                    terminal_value: preamble.terminal_value,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: preamble.offset as u64,
                });
        }
        for packet in census.reference_marker_packets {
            let bytes = &stream.inflated[packet.offset..packet.end];
            events
                .reference_marker_packets
                .push(ParasolidDeltasReferenceMarkerPacket {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-reference-marker#{}",
                        packet.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    reference: packet.reference,
                    marker: packet.marker,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: packet.offset as u64,
                });
        }
        for packet in census.type_150_state_packets {
            let bytes = &stream.inflated[packet.offset..packet.end];
            events
                .type_150_state_packets
                .push(ParasolidDeltasType150StatePacket {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-type-150-state#{}",
                        packet.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    references: packet.references,
                    marker: packet.marker,
                    values: packet.values,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: packet.offset as u64,
                });
        }
        for declaration in census.inline_schema_declarations {
            let bytes = &stream.inflated[declaration.offset..declaration.end];
            let fields = match declaration.fields {
                crate::deltas::InlineSchemaFields::BodyHeader => {
                    ParasolidDeltasInlineSchemaFields::BodyHeader
                }
                crate::deltas::InlineSchemaFields::Region {
                    xmt,
                    state_word,
                    references,
                } => ParasolidDeltasInlineSchemaFields::Region {
                    xmt,
                    state_word,
                    references,
                },
                crate::deltas::InlineSchemaFields::AttdefList {
                    xmt,
                    slot_count,
                    active_count,
                    references,
                } => ParasolidDeltasInlineSchemaFields::AttdefList {
                    xmt,
                    slot_count,
                    active_count,
                    references,
                },
                crate::deltas::InlineSchemaFields::Type70 {
                    xmt,
                    node_id,
                    references,
                    count,
                    trailing_reference,
                } => ParasolidDeltasInlineSchemaFields::Type70 {
                    xmt,
                    node_id,
                    references,
                    count,
                    trailing_reference,
                },
                crate::deltas::InlineSchemaFields::Type100 {
                    xmt,
                    references,
                    transform,
                } => ParasolidDeltasInlineSchemaFields::Type100 {
                    xmt,
                    references,
                    transform,
                },
                crate::deltas::InlineSchemaFields::Type101 {
                    references,
                    anchor_reference,
                    state_words,
                    terminal_value,
                } => ParasolidDeltasInlineSchemaFields::Type101 {
                    references,
                    anchor_reference,
                    state_words,
                    terminal_value,
                },
                crate::deltas::InlineSchemaFields::Type101Compact => {
                    ParasolidDeltasInlineSchemaFields::Type101Compact
                }
                crate::deltas::InlineSchemaFields::Type38 {
                    xmt,
                    node_id,
                    leading_references,
                    leading_statuses,
                    marker,
                    linked_references,
                    state_references,
                    numeric_values,
                } => ParasolidDeltasInlineSchemaFields::Type38 {
                    xmt,
                    node_id,
                    leading_references,
                    leading_statuses: (leading_statuses != [1; 5]).then_some(leading_statuses),
                    marker,
                    linked_references,
                    state_references,
                    numeric_values,
                },
                crate::deltas::InlineSchemaFields::Type41 {
                    reference,
                    numeric_values,
                } => ParasolidDeltasInlineSchemaFields::Type41 {
                    reference,
                    numeric_values,
                },
            };
            events
                .inline_schema_declarations
                .push(ParasolidDeltasInlineSchemaDeclaration {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-inline-schema#{}",
                        declaration.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    fields,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: declaration.offset as u64,
                });
        }
        for state in census.inline_body_states {
            let bytes = &stream.inflated[state.offset..state.end];
            let fields = match state.fields {
                crate::deltas::InlineBodyStateFields::Compact { reference } => {
                    ParasolidDeltasInlineBodyStateFields::Compact { reference }
                }
                crate::deltas::InlineBodyStateFields::Revision {
                    node_id,
                    references,
                    state_bytes,
                } => ParasolidDeltasInlineBodyStateFields::Revision {
                    node_id,
                    references,
                    state_bytes,
                },
            };
            events
                .inline_body_states
                .push(ParasolidDeltasInlineBodyState {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-inline-body-state#{}",
                        state.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    fields,
                    byte_len: bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                    inflated_offset: state.offset as u64,
                });
        }
    }
    events
        .transmit_headers
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .terminal_null_references
        .sort_by(|left, right| left.id.cmp(&right.id));
    events.records.sort_by(|left, right| left.id.cmp(&right.id));
    events
        .tombstones
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .body_revisions
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .term_use_numeric_tails
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .tagged_reference_lanes
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .reference_type_maps
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .reference_state_packets
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .schema_reference_preambles
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .reference_marker_packets
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .type_150_state_packets
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .inline_schema_declarations
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .inline_body_states
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .residual_spans
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
}

fn push_deltas_residual_span(
    residual_spans: &mut Vec<ParasolidDeltasResidualSpan>,
    stream_ordinal: usize,
    bytes: &[u8],
    start: usize,
    end: usize,
) {
    let residual = &bytes[start..end];
    residual_spans.push(ParasolidDeltasResidualSpan {
        id: format!("nx:s{stream_ordinal}:deltas-residual#{start}"),
        stream_ordinal: stream_ordinal as u32,
        byte_len: residual.len() as u64,
        sha256: cadmpeg_ir::hash::sha256_hex(residual),
        inflated_offset: start as u64,
    });
}

/// Shared skeleton for Parasolid record families read from the cached per-stream
/// record view. It owns the stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}`
/// identity, and the sort by identity; each family supplies only its cached row
/// slice and its record constructor.
pub(crate) trait ParasolidStreamRecords {
    /// Cached row type read from the stream's record [`StreamView`].
    type Row: Copy;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// The cached rows of one stream's record view.
    fn rows(view: &StreamView) -> &[Self::Row];
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and cached row.
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

/// Run the cached-view record skeleton for one family: map every cached row of
/// every stream to a record, then sort by identity. Non-Parasolid streams hold
/// empty views, so no per-stream guard is needed.
pub(crate) fn per_parasolid_stream<P: ParasolidStreamRecords>(
    parsed: &ParsedStreams,
) -> Vec<P::Record> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        for row in P::rows(stream.view_for_records()) {
            let id = format!("nx:s{stream_ordinal}:{}#{}", P::ID_STEM, P::xmt(row));
            records.push(P::record(id, stream_ordinal as u32, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    records
}

/// Shared skeleton for Parasolid record families scanned fresh from each
/// Parasolid stream's inflated bytes. It owns the `is_parasolid()` guard, the
/// stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}` identity, and the sort; each
/// family supplies only its scanner and its record constructor.
pub(crate) trait ParasolidScanRecords {
    /// Scanned row type produced from the inflated stream bytes.
    type Row;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// Scan one inflated Parasolid stream into its rows.
    fn scan(bytes: &[u8]) -> Vec<Self::Row>;
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and scanned row.
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

/// Run the fresh-scan record skeleton for one family: scan every Parasolid
/// stream, map each scanned row to a record, then sort by identity.
pub(crate) fn per_parasolid_scan<P: ParasolidScanRecords>(streams: &[Stream]) -> Vec<P::Record> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for row in P::scan(&stream.inflated) {
            let id = format!("nx:s{stream_ordinal}:{}#{}", P::ID_STEM, P::xmt(&row));
            records.push(P::record(id, stream_ordinal as u32, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    records
}

/// Complete typed source record for one Parasolid offset surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidOffsetSurfaceRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the offset surface.
    pub xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    pub discriminator: char,
    /// Serialized true-offset flag.
    pub true_offset: bool,
    /// Cross-reference index of the support surface.
    pub support_xmt: u32,
    /// Signed offset distance in millimetres.
    pub distance: f64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid offset surfaces.
pub(crate) fn parasolid_offset_surface_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidOffsetSurfaceRecord> {
    per_parasolid_stream::<ParasolidOffsetSurfaceRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidOffsetSurfaceRecord {
    type Row = crate::topology::OffsetSurface;
    type Record = ParasolidOffsetSurfaceRecord;
    const ID_STEM: &'static str = "offset-surface-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.offset_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidOffsetSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            discriminator: row.discriminator,
            true_offset: row.true_offset,
            support_xmt: row.support,
            distance: row.distance,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid trimmed curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidTrimmedCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the trimmed curve.
    pub xmt: u32,
    /// Cross-reference index of the basis curve.
    pub basis_xmt: u32,
    /// Stored start and end points in millimetres.
    pub points: [[f64; 3]; 2],
    /// Stored start and end parameters in basis-curve units.
    pub parameters: [f64; 2],
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid trimmed curves.
pub(crate) fn parasolid_trimmed_curve_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidTrimmedCurveRecord> {
    per_parasolid_stream::<ParasolidTrimmedCurveRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidTrimmedCurveRecord {
    type Row = crate::topology::TrimmedCurve;
    type Record = ParasolidTrimmedCurveRecord;
    const ID_STEM: &'static str = "trimmed-curve-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.trimmed_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidTrimmedCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            basis_xmt: row.basis,
            points: row.points,
            parameters: row.parameters,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidSurfaceCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the surface curve.
    pub xmt: u32,
    /// Cross-reference index of the support surface.
    pub surface_xmt: u32,
    /// Cross-reference index of the parameter-space B-curve.
    pub pcurve_xmt: u32,
    /// Nullable cross-reference index of the original model-space curve.
    pub original_curve_xmt: u32,
    /// Serialized tolerance to the original curve in Parasolid metres.
    pub tolerance_to_original: f64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid surface curves.
pub(crate) fn parasolid_surface_curve_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidSurfaceCurveRecord> {
    per_parasolid_stream::<ParasolidSurfaceCurveRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidSurfaceCurveRecord {
    type Row = crate::topology::SurfaceCurve;
    type Record = ParasolidSurfaceCurveRecord;
    const ID_STEM: &'static str = "surface-curve-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.surface_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidSurfaceCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            surface_xmt: row.surface,
            pcurve_xmt: row.pcurve,
            original_curve_xmt: row.original,
            tolerance_to_original: row.tolerance,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid blend-bound bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidBlendBoundRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the bridge.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Zero- or one-valued blend boundary index.
    pub boundary_index: u32,
    /// Cross-reference index of the blend surface.
    pub blend_surface_xmt: u32,
    /// Serialized partition/deltas and direct/escaped framing.
    pub framing: crate::intersection::BlendBoundFraming,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid blend-bound bridges.
pub fn parasolid_blend_bound_records(streams: &[Stream]) -> Vec<ParasolidBlendBoundRecord> {
    per_parasolid_scan::<ParasolidBlendBoundRecord>(streams)
}

impl ParasolidScanRecords for ParasolidBlendBoundRecord {
    type Row = crate::intersection::BlendBound;
    type Record = ParasolidBlendBoundRecord;
    const ID_STEM: &'static str = "blend-bound-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::blend_bounds(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidBlendBoundRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            header_references: row.header_references,
            sense: row.sense,
            boundary_index: row.boundary_index,
            blend_surface_xmt: row.blend_surface,
            framing: row.framing,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidTermUseRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    pub xmt: u32,
    /// Serialized leading count.
    pub count: u32,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    pub form: String,
    /// Endpoint position in millimetres.
    pub point: [f64; 3],
    /// Serialized record framing.
    pub framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid `term_use` endpoints.
pub fn parasolid_term_use_records(streams: &[Stream]) -> Vec<ParasolidTermUseRecord> {
    per_parasolid_scan::<ParasolidTermUseRecord>(streams)
}

impl ParasolidScanRecords for ParasolidTermUseRecord {
    type Row = crate::intersection::TermUse;
    type Record = ParasolidTermUseRecord;
    const ID_STEM: &'static str = "term-use-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::term_use_records(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidTermUseRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            count: row.count,
            form: String::from_utf8_lossy(&row.form).into_owned(),
            point: [row.point.x, row.point.y, row.point.z],
            framing: row.framing,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid support-UV values array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidSupportUvRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the values array.
    pub xmt: u32,
    /// Serialized scalar count.
    pub count: u32,
    /// Tuple-packing marker (`2`, `3`, or `4`).
    pub marker: u8,
    /// Ordered serialized scalar values.
    pub values: Vec<f64>,
    /// Serialized record framing.
    pub framing: crate::intersection::SupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid support-UV arrays.
pub fn parasolid_support_uv_records(streams: &[Stream]) -> Vec<ParasolidSupportUvRecord> {
    per_parasolid_scan::<ParasolidSupportUvRecord>(streams)
}

impl ParasolidScanRecords for ParasolidSupportUvRecord {
    type Row = crate::intersection::SupportUvRecord;
    type Record = ParasolidSupportUvRecord;
    const ID_STEM: &'static str = "support-uv-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::support_uv_records(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidSupportUvRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            count: row.count,
            marker: row.marker,
            values: row.values,
            framing: row.framing,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one physical Parasolid `CHART_s` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidChartRecord {
    /// Globally unique physical-record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the chart.
    pub xmt: u32,
    /// Serialized leading point count.
    pub count: u32,
    /// Base chart parameter.
    pub base_parameter: f64,
    /// Chord-to-parameter scale.
    pub base_scale: f64,
    /// Redundant serialized chart count.
    pub chart_count: u32,
    /// Chordal error in Parasolid metres.
    pub chordal_error: f64,
    /// Angular error in radians.
    pub angular_error: f64,
    /// Two serialized missing-parameter sentinels.
    pub parameter_errors: [f64; 2],
    /// Model-space chart points in millimetres.
    pub points: Vec<[f64; 3]>,
    /// Native ext11 parameters, when present.
    pub native_parameters: Option<Vec<f64>>,
    /// Two ordered ext11 support-UV lanes.
    pub ext_support_uv: [Option<Vec<[f64; 2]>>; 2],
    /// Hvec point layout.
    pub point_layout: crate::intersection::ChartPointLayout,
    /// Serialized record framing.
    pub framing: crate::intersection::ChartFraming,
    /// Type-tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode every complete physical Parasolid chart source record.
pub fn parasolid_chart_records(streams: &[Stream]) -> Vec<ParasolidChartRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let Some(point_layout) = stream.kind.chart_point_layout() else {
            continue;
        };
        for chart in crate::intersection::chart_source_records(&stream.inflated, point_layout) {
            records.push(ParasolidChartRecord {
                id: format!(
                    "nx:s{stream_ordinal}:chart-record#{}-{}",
                    chart.xmt, chart.pos
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: chart.xmt,
                count: chart.count,
                base_parameter: chart.base_parameter,
                base_scale: chart.base_scale,
                chart_count: chart.chart_count,
                chordal_error: chart.chordal_error,
                angular_error: chart.angular_error,
                parameter_errors: chart.parameter_errors,
                points: chart
                    .points
                    .into_iter()
                    .map(|point| [point.x, point.y, point.z])
                    .collect(),
                native_parameters: chart.native_parameters,
                ext_support_uv: chart.ext_support_uv,
                point_layout: chart.point_layout,
                framing: chart.framing,
                inflated_offset: chart.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid surface-intersection curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidIntersectionRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the construction.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Six ordered support and witness references.
    pub construction_references: [u32; 6],
    /// Whether the record uses the single-byte delta-twin tag.
    pub delta_twin: bool,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for retained intersection constructions.
pub(crate) fn parasolid_intersection_records(
    parsed: &ParsedStreams<'_>,
) -> Vec<ParasolidIntersectionRecord> {
    per_parasolid_stream::<ParasolidIntersectionRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidIntersectionRecord {
    type Row = crate::topology::CompositeCurve;
    type Record = ParasolidIntersectionRecord;
    const ID_STEM: &'static str = "intersection-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.intersections.source_constructions
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidIntersectionRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            header_references: row.header_references,
            sense: row.sense,
            construction_references: row.references,
            delta_twin: row.delta_twin,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed type-56 rolling-ball blend-surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidBlendSurfaceRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local `BLEND_SURF` identity.
    pub xmt: u32,
    /// Ordered support-surface identities.
    pub support_xmts: [u32; 2],
    /// Ball-centre spine identity; `1` is the null reference.
    pub spine_xmt: u32,
    /// Signed support offsets in model millimetres.
    pub offsets: [f64; 2],
    /// Dimensionless support thumb weights.
    pub thumb_weights: [f64; 2],
    /// Offset of the type tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Named Parasolid attribute class declared in one inflated body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local definition record identity.
    pub xmt: u32,
    /// Stream-local next-definition identity; `1` is null.
    pub next_definition_xmt: u32,
    /// Stream-local type-79 identifier identity.
    pub identifier_xmt: u32,
    /// Offset of the resolved type-79 identifier in the inflated stream.
    pub identifier_inflated_offset: u64,
    /// Exact printable attribute class name.
    pub name: String,
    /// Numeric attribute type identifier.
    pub type_id: u32,
    /// Ordered actions for the eight logged event families.
    pub action_codes: [u8; 8],
    /// Stream-local field-name-list identity; `1` is null.
    pub field_names_xmt: u32,
    /// Ordered legal-owner flags.
    pub legal_owner_flags: [u8; 16],
    /// Declared number of fields.
    pub field_count: u32,
    /// One serialized code for every declared field.
    pub field_codes: Vec<u8>,
    /// Offset of the declaration in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-99 field-name reference record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidFieldNamesRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered stream-local character or Unicode value references.
    pub name_xmts: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Complete type-80 declaration-to-field-name-list relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeFieldNames {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-80 declaration.
    pub attribute_definition: String,
    /// Uniquely resolved type-99 field-name record.
    pub field_names_record: String,
    /// Ordered uniquely resolved type-84 or type-98 records.
    pub value_records: Vec<String>,
    /// Ordered exact field names.
    pub names: Vec<String>,
}

/// Explicit topology-record ownership of one Parasolid attribute list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeListReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Parasolid topology record type.
    pub topology_type: u8,
    /// Stream-local topology-record identity.
    pub topology_xmt: u32,
    /// Stream-local attribute-list identity.
    pub attribute_list_xmt: u32,
    /// Uniquely resolved type-81 attribute-list record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute_list_record: Option<String>,
    /// Offset of the attribute-list field in the inflated stream.
    pub inflated_offset: u64,
}

/// Framed Parasolid type-81 entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51Record {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact record flags.
    pub flags: u32,
    /// Serialized sequence value.
    pub sequence: u32,
    /// Stream-local type-80 attribute-definition identity.
    pub definition_xmt: u32,
    /// Five fixed leading stream-local references.
    pub leading_references: [u32; 5],
    /// Variable trailing stream-local references counted by `flags`.
    pub trailing_references: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Self-framed printable Parasolid type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity54StringRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact nonempty printable value.
    pub value: String,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-82 unsigned-integer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity52IntegerRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian unsigned values.
    pub values: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-83 finite binary64 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntity53DoubleRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite big-endian binary64 values.
    pub values: Vec<f64>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Parasolid vector-shaped attribute-value family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidVectorValueKind {
    /// Type-85 point values.
    Points,
    /// Type-86 free-vector values.
    Vectors,
    /// Type-89 direction values.
    Directions,
}

/// Counted Parasolid type-85, type-86, or type-89 vector-shaped value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntityVectorRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Exact value family.
    pub kind: ParasolidVectorValueKind,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite xyz values.
    pub values: Vec<[f64; 3]>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-87 axis-value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntity57AxisRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered axes, each retaining its two serialized xyz vectors.
    pub values: Vec<[[f64; 3]; 2]>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-88 tag-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity58TagRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered exact tag values.
    pub values: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-98 Unicode-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity62UnicodeRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered exact big-endian UTF-16 code units.
    pub code_units: Vec<u16>,
    /// Validated Unicode scalar string.
    pub value: String,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Attribute-value records discovered by one pass over the Parasolid streams.
pub(crate) struct ParasolidEntityValueRecords {
    pub(crate) integers: Vec<ParasolidEntity52IntegerRecord>,
    pub(crate) doubles: Vec<ParasolidEntity53DoubleRecord>,
    pub(crate) strings: Vec<ParasolidEntity54StringRecord>,
    pub(crate) vectors: Vec<ParasolidEntityVectorRecord>,
    pub(crate) axes: Vec<ParasolidEntity57AxisRecord>,
    pub(crate) tags: Vec<ParasolidEntity58TagRecord>,
    pub(crate) unicode: Vec<ParasolidEntity62UnicodeRecord>,
}

/// Numeric value-record family referenced by a type-81 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidEntity51NumericKind {
    /// Type-82 unsigned-integer lane.
    UnsignedIntegers,
    /// Type-83 binary64 lane.
    Doubles,
}

/// Exact type-81 reference to one uniquely resolved numeric value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51NumericUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Numeric record family.
    pub kind: ParasolidEntity51NumericKind,
    /// Uniquely resolved numeric record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Exact type-81 reference to a uniquely resolved type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51StringUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Uniquely resolved type-84 string record.
    pub string_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Exact type-81 reference to one uniquely resolved structured value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51StructuredUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Structured value-record family.
    pub kind: ParasolidAttributeFieldValueKind,
    /// Uniquely resolved structured value record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Resolved registered class of one Parasolid type-81 attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Stream-local XMT of the matched type-80 definition.
    pub definition_xmt: u32,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
}

/// Value-record family assigned to one declared Parasolid attribute field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidAttributeFieldValueKind {
    /// Type-82 integer values.
    UnsignedIntegers,
    /// Type-83 binary64 values.
    Doubles,
    /// Type-84 character values.
    String,
    /// Type-85 point values.
    Points,
    /// Type-86 vector values.
    Vectors,
    /// Type-87 axis values.
    Axes,
    /// Type-88 tag values.
    Tags,
    /// Type-89 direction values.
    Directions,
    /// Type-98 Unicode values.
    Unicode,
}

/// One uniquely typed type-81 field reference joined to its type-80 declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeFieldUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Resolved class relation for the attribute instance.
    pub attribute_class_use: String,
    /// Type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
    /// Zero-based position in the type-80 field declaration.
    pub field_ordinal: u32,
    /// Declared type-80 field code.
    pub field_code: u8,
    /// Zero-based position in the complete type-81 reference lane.
    pub reference_ordinal: u32,
    /// Resolved value-record family.
    pub value_kind: ParasolidAttributeFieldValueKind,
    /// Type-81-to-value relation carrying this field.
    pub value_use: String,
    /// Uniquely resolved value record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Resolved class of one topology-owned Parasolid attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning topology-to-attribute relation.
    pub topology_attribute_reference: String,
    /// Topology-owned type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Resolved class relation for the attribute instance.
    pub attribute_class_use: String,
    /// Stream-local XMT of the matched type-80 definition.
    pub definition_xmt: u32,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
}

/// Retain named attribute-class declarations from all Parasolid streams.
pub fn parasolid_attribute_definitions(streams: &[Stream]) -> Vec<ParasolidAttributeDefinition> {
    streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::attribute_definitions(&stream.inflated)
                .into_iter()
                .map(move |definition| ParasolidAttributeDefinition {
                    id: format!(
                        "nx:s{stream_ordinal}:attribute-definition#{}",
                        definition.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: definition.xmt,
                    next_definition_xmt: definition.next_definition_xmt,
                    identifier_xmt: definition.identifier_xmt,
                    identifier_inflated_offset: definition.identifier_offset as u64,
                    name: definition.name.to_string(),
                    type_id: definition.type_id,
                    action_codes: definition.action_codes,
                    field_names_xmt: definition.field_names_xmt,
                    legal_owner_flags: definition.legal_owner_flags,
                    field_count: definition.field_count,
                    field_codes: definition.field_codes.to_vec(),
                    inflated_offset: definition.offset as u64,
                })
        })
        .collect()
}

/// Decode every counted type-99 attribute field-name record.
pub fn parasolid_field_names_records(streams: &[Stream]) -> Vec<ParasolidFieldNamesRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::field_names_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidFieldNamesRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:field-names#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    name_xmts: record.name_xmts,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Resolve complete type-80 field-name lists through type-99 and character records.
pub fn parasolid_attribute_field_names(
    definitions: &[ParasolidAttributeDefinition],
    field_names: &[ParasolidFieldNamesRecord],
    strings: &[ParasolidEntity54StringRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Vec<ParasolidAttributeFieldNames> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u32), Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_identity
            .entry((definition.stream_ordinal, definition.xmt))
            .or_default()
            .push(definition);
    }
    let mut lists = BTreeMap::<(u32, u32), Vec<&ParasolidFieldNamesRecord>>::new();
    for list in field_names {
        lists
            .entry((list.stream_ordinal, list.xmt))
            .or_default()
            .push(list);
    }
    let mut names_by_xmt = BTreeMap::<(u32, u32), Vec<(&str, &str)>>::new();
    for string in strings {
        names_by_xmt
            .entry((string.stream_ordinal, string.xmt))
            .or_default()
            .push((string.id.as_str(), string.value.as_str()));
    }
    for value in unicode {
        names_by_xmt
            .entry((value.stream_ordinal, value.xmt))
            .or_default()
            .push((value.id.as_str(), value.value.as_str()));
    }
    let mut relations = definitions_by_identity
        .values()
        .filter_map(|definitions| {
            let [definition] = definitions.as_slice() else {
                return None;
            };
            Some(*definition)
        })
        .filter(|definition| definition.field_names_xmt > 1)
        .filter_map(|definition| {
            let [list] = lists
                .get(&(definition.stream_ordinal, definition.field_names_xmt))?
                .as_slice()
            else {
                return None;
            };
            (list.name_xmts.len() == definition.field_codes.len()).then_some(())?;
            let resolved = list
                .name_xmts
                .iter()
                .map(|xmt| {
                    let [name] = names_by_xmt
                        .get(&(definition.stream_ordinal, *xmt))?
                        .as_slice()
                    else {
                        return None;
                    };
                    Some(*name)
                })
                .collect::<Option<Vec<_>>>()?;
            Some(ParasolidAttributeFieldNames {
                id: format!(
                    "nx:s{}:attribute-field-names#{}",
                    definition.stream_ordinal, definition.xmt
                ),
                stream_ordinal: definition.stream_ordinal,
                attribute_definition: definition.id.clone(),
                field_names_record: list.id.clone(),
                value_records: resolved.iter().map(|(id, _)| (*id).to_string()).collect(),
                names: resolved
                    .iter()
                    .map(|(_, value)| (*value).to_string())
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    relations.sort_by(|first, second| first.id.cmp(&second.id));
    relations
}

/// Retain complete typed rolling-ball blend records from all Parasolid streams.
pub(crate) fn parasolid_blend_surface_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidBlendSurfaceRecord> {
    per_parasolid_stream::<ParasolidBlendSurfaceRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidBlendSurfaceRecord {
    type Row = crate::topology::BlendSurface;
    type Record = ParasolidBlendSurfaceRecord;
    const ID_STEM: &'static str = "blend-surface-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.blend_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidBlendSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            support_xmts: row.supports,
            spine_xmt: row.spine,
            offsets: row.offsets,
            thumb_weights: row.thumb_weights,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Retain every non-null topology-to-attribute-list reference.
pub(crate) fn parasolid_topology_attribute_list_references(
    parsed: &ParsedStreams,
    entity_records: &[ParasolidEntity51Record],
) -> Vec<ParasolidTopologyAttributeListReference> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for record in entity_records {
        records_by_identity
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push(record.id.as_str());
    }
    let mut references = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        let graph = &stream.view_for_records().graph;
        for topology_type in [13, 14, 15, 16, 17, 18] {
            for node in graph.of_kind(topology_type) {
                let attribute_list_xmt = match topology_type {
                    13 => node.shell_fields().map(|fields| fields.attributes),
                    14 => node.face_fields().map(|fields| fields.attributes),
                    15 => node.loop_fields().map(|fields| fields.attributes),
                    16 => node.edge_fields().map(|fields| fields.attributes),
                    17 => node.fin_fields().map(|fields| fields.attributes),
                    18 => node.vertex_fields().map(|fields| fields.attributes),
                    _ => unreachable!("bounded topology family"),
                };
                let Some(attribute_list_xmt) = attribute_list_xmt.filter(|value| *value > 1) else {
                    continue;
                };
                let Some(inflated_offset) = node.attribute_field_offset() else {
                    continue;
                };
                references.push(ParasolidTopologyAttributeListReference {
                    id: format!(
                        "nx:s{stream_ordinal}:topology-attribute-list-reference#{topology_type}-{}",
                        node.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    topology_type,
                    topology_xmt: node.xmt,
                    attribute_list_xmt,
                    attribute_list_record: records_by_identity
                        .get(&(stream_ordinal as u32, attribute_list_xmt))
                        .and_then(|records| {
                            let [record] = records.as_slice() else {
                                return None;
                            };
                            Some((*record).to_string())
                        }),
                    inflated_offset: inflated_offset as u64,
                });
            }
        }
    }
    references
}

/// Decode every framed type-81 entity/attribute-list record.
pub fn parasolid_entity_51_records(streams: &[Stream]) -> Vec<ParasolidEntity51Record> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_51_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity51Record {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-51#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    flags: record.flags,
                    sequence: record.sequence,
                    definition_xmt: record.definition_xmt,
                    leading_references: record.leading_references,
                    trailing_references: record.trailing_references,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode value records from their retained deltas or attribute owners.
pub(crate) fn parasolid_entity_value_records(
    streams: &[Stream],
    deltas_records: &[ParasolidDeltasRecord],
) -> ParasolidEntityValueRecords {
    let mut records = ParasolidEntityValueRecords {
        integers: Vec::new(),
        doubles: Vec::new(),
        strings: Vec::new(),
        vectors: Vec::new(),
        axes: Vec::new(),
        tags: Vec::new(),
        unicode: Vec::new(),
    };
    for (stream_ordinal, stream) in streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
    {
        let owned_offsets = match stream.kind {
            StreamKind::Deltas => deltas_records
                .iter()
                .filter_map(|record| {
                    (record.stream_ordinal == stream_ordinal as u32
                        && matches!(record.kind, 82..=89 | 98))
                    .then(|| usize::try_from(record.inflated_offset).ok())
                    .flatten()
                })
                .collect::<Vec<_>>(),
            StreamKind::Partition | StreamKind::Plain => {
                crate::parasolid::referenced_value_record_offsets(&stream.inflated)
            }
            StreamKind::Preview => unreachable!("preview streams were filtered out"),
        };
        let values = crate::parasolid::entity_value_records_at(&stream.inflated, owned_offsets);
        for record in values.integers {
            records.integers.push(ParasolidEntity52IntegerRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-52-integers#{}-{}",
                    record.xmt, record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.values,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.doubles {
            records.doubles.push(ParasolidEntity53DoubleRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-53-doubles#{}-{}",
                    record.xmt, record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.values,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.strings {
            records.strings.push(ParasolidEntity54StringRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-54-string#{}-{}",
                    record.xmt, record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                value: record.value.to_string(),
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        let mut retain_vector = |kind, family: &str, xmt, offset, byte_len, values| {
            records.vectors.push(ParasolidEntityVectorRecord {
                id: format!("nx:s{stream_ordinal}:entity-{family}#{xmt}-{offset}"),
                stream_ordinal: stream_ordinal as u32,
                kind,
                xmt,
                values,
                byte_len: byte_len as u64,
                inflated_offset: offset as u64,
            });
        };
        for record in values.points {
            retain_vector(
                ParasolidVectorValueKind::Points,
                "55-points",
                record.xmt,
                record.offset,
                record.byte_len,
                record.values,
            );
        }
        for record in values.vectors {
            retain_vector(
                ParasolidVectorValueKind::Vectors,
                "56-vectors",
                record.xmt,
                record.offset,
                record.byte_len,
                record.values,
            );
        }
        for record in values.directions {
            retain_vector(
                ParasolidVectorValueKind::Directions,
                "59-directions",
                record.xmt,
                record.offset,
                record.byte_len,
                record.values,
            );
        }
        for record in values.axes {
            records.axes.push(ParasolidEntity57AxisRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-57-axes#{}-{}",
                    record.xmt, record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.values,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.tags {
            records.tags.push(ParasolidEntity58TagRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-58-tags#{}-{}",
                    record.xmt, record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.values,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.unicode {
            records.unicode.push(ParasolidEntity62UnicodeRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-62-unicode#{}-{}",
                    record.xmt, record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                code_units: record.code_units,
                value: record.value,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
    }
    records
        .integers
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .doubles
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .strings
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .vectors
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .axes
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .tags
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .unicode
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Join type-81 reference slots to unique same-stream numeric value records.
pub fn parasolid_entity_51_numeric_uses(
    entities: &[ParasolidEntity51Record],
    integers: &[ParasolidEntity52IntegerRecord],
    doubles: &[ParasolidEntity53DoubleRecord],
) -> Vec<ParasolidEntity51NumericUse> {
    let mut values = BTreeMap::<(u32, u32), Vec<(ParasolidEntity51NumericKind, &str)>>::new();
    for record in integers {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::UnsignedIntegers, &record.id));
    }
    for record in doubles {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::Doubles, &record.id));
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (trailing_ordinal, referenced_xmt) in
            entity.trailing_references.iter().copied().enumerate()
        {
            let reference_ordinal = trailing_ordinal + 5;
            let Some([(kind, value_record)]) = values
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51NumericUse {
                id: format!(
                    "nx:s{}:entity-51-numeric-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                kind: *kind,
                value_record: (*value_record).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join type-81 reference slots to unique same-stream type-84 strings.
pub fn parasolid_entity_51_string_uses(
    entities: &[ParasolidEntity51Record],
    strings: &[ParasolidEntity54StringRecord],
) -> Vec<ParasolidEntity51StringUse> {
    let mut strings_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for string in strings {
        strings_by_identity
            .entry((string.stream_ordinal, string.xmt))
            .or_default()
            .push(string.id.as_str());
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (trailing_ordinal, referenced_xmt) in
            entity.trailing_references.iter().copied().enumerate()
        {
            let reference_ordinal = trailing_ordinal + 5;
            let Some([string]) = strings_by_identity
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51StringUse {
                id: format!(
                    "nx:s{}:entity-51-string-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                string_record: (*string).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join type-81 reference slots to unique same-stream structured value records.
pub fn parasolid_entity_51_structured_uses(
    entities: &[ParasolidEntity51Record],
    vectors: &[ParasolidEntityVectorRecord],
    axes: &[ParasolidEntity57AxisRecord],
    tags: &[ParasolidEntity58TagRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Vec<ParasolidEntity51StructuredUse> {
    let mut values = BTreeMap::<(u32, u32), Vec<(ParasolidAttributeFieldValueKind, &str)>>::new();
    for record in vectors {
        let kind = match record.kind {
            ParasolidVectorValueKind::Points => ParasolidAttributeFieldValueKind::Points,
            ParasolidVectorValueKind::Vectors => ParasolidAttributeFieldValueKind::Vectors,
            ParasolidVectorValueKind::Directions => ParasolidAttributeFieldValueKind::Directions,
        };
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((kind, record.id.as_str()));
    }
    for (kind, stream_ordinal, xmt, id) in axes
        .iter()
        .map(|record| {
            (
                ParasolidAttributeFieldValueKind::Axes,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        })
        .chain(tags.iter().map(|record| {
            (
                ParasolidAttributeFieldValueKind::Tags,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        }))
        .chain(unicode.iter().map(|record| {
            (
                ParasolidAttributeFieldValueKind::Unicode,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        }))
    {
        values
            .entry((stream_ordinal, xmt))
            .or_default()
            .push((kind, id));
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (trailing_ordinal, referenced_xmt) in
            entity.trailing_references.iter().copied().enumerate()
        {
            let reference_ordinal = trailing_ordinal + 5;
            let Some([(kind, value_record)]) = values
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51StructuredUse {
                id: format!(
                    "nx:s{}:entity-51-structured-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                kind: *kind,
                value_record: (*value_record).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve topology-owned attribute instances through their type-80 definition.
pub fn parasolid_topology_attribute_class_uses(
    topology_references: &[ParasolidTopologyAttributeListReference],
    class_uses: &[ParasolidAttributeClassUse],
) -> Vec<ParasolidTopologyAttributeClassUse> {
    let mut class_uses_by_entity = BTreeMap::<&str, Vec<&ParasolidAttributeClassUse>>::new();
    for class_use in class_uses {
        class_uses_by_entity
            .entry(class_use.entity_51_record.as_str())
            .or_default()
            .push(class_use);
    }
    let mut uses = Vec::new();
    for reference in topology_references {
        let Some(entity_id) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        let Some([class_use]) = class_uses_by_entity.get(entity_id).map(Vec::as_slice) else {
            continue;
        };
        uses.push(ParasolidTopologyAttributeClassUse {
            id: format!(
                "nx:s{}:topology-attribute-class-use#{}-{}",
                reference.stream_ordinal, reference.topology_type, reference.topology_xmt
            ),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: class_use.entity_51_record.clone(),
            attribute_class_use: class_use.id.clone(),
            definition_xmt: class_use.definition_xmt,
            attribute_definition: class_use.attribute_definition.clone(),
        });
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve every type-81 attribute instance through its type-80 definition reference.
pub fn parasolid_attribute_class_uses(
    entities: &[ParasolidEntity51Record],
    definitions: &[ParasolidAttributeDefinition],
) -> Vec<ParasolidAttributeClassUse> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u32), Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_identity
            .entry((definition.stream_ordinal, definition.xmt))
            .or_default()
            .push(definition);
    }
    let mut uses = entities
        .iter()
        .filter_map(|entity| {
            let definition_xmt = entity.definition_xmt;
            let [definition] = definitions_by_identity
                .get(&(entity.stream_ordinal, definition_xmt))?
                .as_slice()
            else {
                return None;
            };
            Some(ParasolidAttributeClassUse {
                id: format!(
                    "nx:s{}:attribute-class-use#{}-{}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                definition_xmt,
                attribute_definition: definition.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Assign uniquely resolved attribute values to declared type-80 fields.
pub fn parasolid_attribute_field_uses(
    class_uses: &[ParasolidAttributeClassUse],
    definitions: &[ParasolidAttributeDefinition],
    numeric_uses: &[ParasolidEntity51NumericUse],
    string_uses: &[ParasolidEntity51StringUse],
    structured_uses: &[ParasolidEntity51StructuredUse],
) -> Vec<ParasolidAttributeFieldUse> {
    let mut classes = BTreeMap::<&str, Vec<&ParasolidAttributeClassUse>>::new();
    for class_use in class_uses {
        classes
            .entry(class_use.entity_51_record.as_str())
            .or_default()
            .push(class_use);
    }
    let mut definitions_by_id = BTreeMap::<&str, Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_id
            .entry(definition.id.as_str())
            .or_default()
            .push(definition);
    }
    let mut candidates = BTreeMap::<(&str, u32), Vec<_>>::new();
    for numeric_use in numeric_uses {
        let value_kind = match numeric_use.kind {
            ParasolidEntity51NumericKind::UnsignedIntegers => {
                ParasolidAttributeFieldValueKind::UnsignedIntegers
            }
            ParasolidEntity51NumericKind::Doubles => ParasolidAttributeFieldValueKind::Doubles,
        };
        candidates
            .entry((
                numeric_use.entity_51_record.as_str(),
                numeric_use.reference_ordinal,
            ))
            .or_default()
            .push((
                numeric_use.stream_ordinal,
                value_kind,
                numeric_use.id.as_str(),
                numeric_use.value_record.as_str(),
                numeric_use.inflated_offset,
            ));
    }
    for string_use in string_uses {
        candidates
            .entry((
                string_use.entity_51_record.as_str(),
                string_use.reference_ordinal,
            ))
            .or_default()
            .push((
                string_use.stream_ordinal,
                ParasolidAttributeFieldValueKind::String,
                string_use.id.as_str(),
                string_use.string_record.as_str(),
                string_use.inflated_offset,
            ));
    }
    for structured_use in structured_uses {
        candidates
            .entry((
                structured_use.entity_51_record.as_str(),
                structured_use.reference_ordinal,
            ))
            .or_default()
            .push((
                structured_use.stream_ordinal,
                structured_use.kind,
                structured_use.id.as_str(),
                structured_use.value_record.as_str(),
                structured_use.inflated_offset,
            ));
    }
    let mut uses = candidates
        .into_iter()
        .filter_map(|((entity_51_record, reference_ordinal), candidates)| {
            let [(stream_ordinal, value_kind, value_use, value_record, inflated_offset)] =
                candidates.as_slice()
            else {
                return None;
            };
            let [class_use] = classes.get(entity_51_record)?.as_slice() else {
                return None;
            };
            if class_use.stream_ordinal != *stream_ordinal {
                return None;
            }
            let [definition] = definitions_by_id
                .get(class_use.attribute_definition.as_str())?
                .as_slice()
            else {
                return None;
            };
            let field_ordinal = reference_ordinal.checked_sub(5)?;
            let field_code = *definition.field_codes.get(field_ordinal as usize)?;
            matches!(
                (field_code, value_kind),
                (1, ParasolidAttributeFieldValueKind::UnsignedIntegers)
                    | (2, ParasolidAttributeFieldValueKind::Doubles)
                    | (3, ParasolidAttributeFieldValueKind::String)
                    | (4, ParasolidAttributeFieldValueKind::Points)
                    | (5, ParasolidAttributeFieldValueKind::Vectors)
                    | (6, ParasolidAttributeFieldValueKind::Directions)
                    | (7, ParasolidAttributeFieldValueKind::Axes)
                    | (8, ParasolidAttributeFieldValueKind::Tags)
                    | (10, ParasolidAttributeFieldValueKind::Unicode)
            )
            .then_some(())?;
            let (_, class_key) = class_use.id.rsplit_once('#')?;
            Some(ParasolidAttributeFieldUse {
                id: format!("nx:s{stream_ordinal}:attribute-field-use#{class_key}-{field_ordinal}"),
                stream_ordinal: *stream_ordinal,
                attribute_class_use: class_use.id.clone(),
                entity_51_record: entity_51_record.to_string(),
                attribute_definition: class_use.attribute_definition.clone(),
                field_ordinal,
                field_code,
                reference_ordinal,
                value_kind: *value_kind,
                value_use: (*value_use).to_string(),
                value_record: (*value_record).to_string(),
                inflated_offset: *inflated_offset,
            })
        })
        .collect::<Vec<_>>();
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Whether a concrete topology-owned attribute field lacks its exact value relation.
pub fn parasolid_topology_attribute_fields_have_untransferred_values(
    definitions: &[ParasolidAttributeDefinition],
    entities: &[ParasolidEntity51Record],
    field_uses: &[ParasolidAttributeFieldUse],
    topology_class_uses: &[ParasolidTopologyAttributeClassUse],
) -> bool {
    let mut definitions_by_id = BTreeMap::<&str, Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_id
            .entry(definition.id.as_str())
            .or_default()
            .push(definition);
    }
    let mut fields_by_identity = BTreeMap::<(&str, u32), Vec<&ParasolidAttributeFieldUse>>::new();
    for field_use in field_uses {
        fields_by_identity
            .entry((field_use.entity_51_record.as_str(), field_use.field_ordinal))
            .or_default()
            .push(field_use);
    }

    let mut entities_by_id = BTreeMap::<&str, Vec<&ParasolidEntity51Record>>::new();
    for entity in entities {
        entities_by_id
            .entry(entity.id.as_str())
            .or_default()
            .push(entity);
    }

    topology_class_uses.iter().any(|topology_class_use| {
        let entity_id = topology_class_use.entity_51_record.as_str();
        let Some([entity]) = entities_by_id.get(entity_id).map(Vec::as_slice) else {
            return true;
        };
        let Some([definition]) = definitions_by_id
            .get(topology_class_use.attribute_definition.as_str())
            .map(Vec::as_slice)
        else {
            return true;
        };
        definition
            .field_codes
            .iter()
            .enumerate()
            .any(|(field_ordinal, field_code)| {
                // Field code 0 is ignored. Pointer fields (code 9) are always
                // transmitted empty and therefore have no value relation.
                if matches!(field_code, 0 | 9) {
                    return false;
                }
                let Some(&referenced_xmt) = entity.trailing_references.get(field_ordinal) else {
                    return true;
                };
                if referenced_xmt == 1 {
                    return false;
                }
                let Ok(field_ordinal) = u32::try_from(field_ordinal) else {
                    return true;
                };
                !matches!(
                    fields_by_identity
                        .get(&(entity.id.as_str(), field_ordinal))
                        .map(Vec::as_slice),
                    Some([_])
                )
            })
    })
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use std::io::{Cursor, Write};

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use crate::parasolid::Stream;
    use crate::test_support::many_face_partition_stream;
    use crate::topology::Graph;

    fn group_record(xmt: u16, node_id: u32, linked_reference: u16) -> Vec<u8> {
        let mut bytes = vec![0, 90];
        bytes.extend_from_slice(&xmt.to_be_bytes());
        bytes.extend_from_slice(&node_id.to_be_bytes());
        for reference in [3u16, 4, 5, 6] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(4);
        bytes.extend_from_slice(&linked_reference.to_be_bytes());
        bytes.push(0);
        bytes
    }

    fn stream(kind: StreamKind, schema: &str, inflated: Vec<u8>) -> Stream {
        Stream {
            file_offset: 0,
            consumed: 0,
            inflated,
            kind,
            schema: Some(schema.to_string()),
        }
    }

    #[test]
    fn native_value_records_use_only_ledger_owned_offsets() {
        let mut outer = vec![0x00, 0x52];
        outer.extend_from_slice(&4u32.to_be_bytes());
        outer.extend_from_slice(&10u16.to_be_bytes());
        outer.extend_from_slice(&[0x00, 0x53]);
        outer.extend_from_slice(&1u32.to_be_bytes());
        outer.extend_from_slice(&20u16.to_be_bytes());
        outer.extend_from_slice(&0.25f64.to_be_bytes());

        let streams = [stream(StreamKind::Deltas, "SCH_TEST", outer)];
        let events = super::parasolid_deltas_events(&streams);
        let records = super::parasolid_entity_value_records(&streams, &events.records);

        assert_eq!(records.integers.len(), 1);
        assert_eq!(records.integers[0].values.len(), 4);
        assert!(records.doubles.is_empty());
    }

    fn record(
        kind: u16,
        xmt: u32,
        node_id: Option<u32>,
        references: Vec<u32>,
    ) -> crate::deltas::Record {
        crate::deltas::Record {
            kind,
            xmt,
            node_id,
            references,
            position: None,
            canonical_bytes: if kind == 90 { vec![0, 90] } else { Vec::new() },
            offset: 0,
            end: 1,
        }
    }

    #[test]
    fn group_members_follow_complete_bidirectional_type_91_chain() {
        let group = record(90, 10, Some(7), vec![3, 4, 5, 6, 30]);
        let tail = record(91, 30, None, vec![10, 100, 3, 4, 20, 1]);
        let head = record(91, 20, None, vec![10, 101, 3, 4, 1, 30]);
        let tail_member = record(14, 100, Some(50), Vec::new());
        let head_member = record(16, 101, Some(51), Vec::new());
        let records = [group, tail, head, tail_member, head_member];

        let members = super::group_members_from_records(4, &records);

        assert_eq!(members.len(), 2);
        assert_eq!(members[0].list_record_xmt, 20);
        assert_eq!(members[0].member_family, "EDGE");
        assert_eq!(members[0].member_node_id, Some(51));
        assert_eq!(members[0].current_member_xmt, None);
        assert_eq!(members[1].list_record_xmt, 30);
        assert_eq!(members[1].member_family, "FACE");
        assert_eq!(members[1].member_node_id, Some(50));

        let mut broken = records;
        broken[2].references[5] = 99;
        assert!(super::group_members_from_records(4, &broken).is_empty());
    }

    #[test]
    fn group_member_xmt_is_checked_before_node_identity_fallback() {
        let graph = Graph::parse(&many_face_partition_stream(1_000));
        let member = ParasolidGroupMember {
            id: "member".into(),
            partition_stream_ordinal: 4,
            group_xmt: 10,
            group_node_id: 7,
            ordinal: 0,
            list_record_xmt: 20,
            member_xmt: 300,
            member_family: "FACE".into(),
            member_node_id: Some(1_000),
            current_member_xmt: None,
        };

        assert_eq!(
            super::resolved_current_member_xmt(&graph, &member, 14, 1_000),
            Some(300)
        );
        assert_eq!(
            super::resolved_current_member_xmt(
                &graph,
                &ParasolidGroupMember {
                    member_xmt: 999,
                    ..member.clone()
                },
                14,
                1_000,
            ),
            Some(300)
        );
        assert_eq!(
            super::resolved_current_member_xmt(&graph, &member, 14, 2_000),
            None
        );
    }

    #[test]
    fn group_records_keep_equal_node_ids_in_distinct_partition_scopes() {
        let streams = [
            stream(StreamKind::Partition, "SCH_TEST", group_record(10, 7, 8)),
            stream(StreamKind::Partition, "SCH_TEST", group_record(11, 7, 9)),
        ];

        let groups = super::parasolid_group_records(&streams, &BTreeMap::new(), &[]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].node_id, groups[1].node_id);
        assert_eq!(groups[0].partition_stream_ordinal, Some(0));
        assert_eq!(groups[1].partition_stream_ordinal, Some(1));
        assert_eq!(groups[0].selector, 4);
        assert_eq!(groups[0].linked_reference_status, 0);
        assert_ne!(groups[0].id, groups[1].id);
    }

    #[test]
    fn group_records_assign_only_paired_deltas_to_a_partition_scope() {
        let streams = [
            stream(StreamKind::Partition, "SCH_TEST", group_record(10, 7, 8)),
            stream(StreamKind::Deltas, "SCH_TEST", group_record(11, 8, 9)),
            stream(StreamKind::Deltas, "SCH_OTHER", group_record(12, 9, 10)),
        ];
        let events = super::parasolid_deltas_events(&streams);
        let pairs = BTreeMap::from([(0, vec![1])]);

        let groups = super::parasolid_group_records(&streams, &pairs, &events.records);

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].partition_stream_ordinal, Some(0));
        assert_eq!(groups[1].partition_stream_ordinal, Some(0));
        assert_eq!(groups[2].partition_stream_ordinal, None);
        assert_eq!(groups[1].stream_kind, "deltas");
    }

    fn deltas_type_45(xmt: u16) -> Vec<u8> {
        let mut bytes = 45u16.to_be_bytes().to_vec();
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&xmt.to_be_bytes());
        bytes.extend_from_slice(&1.25f64.to_be_bytes());
        bytes.extend_from_slice(&2.5f64.to_be_bytes());
        bytes
    }

    #[test]
    fn deltas_events_retain_bounded_records_tombstones_and_revisions() {
        let mut bytes = vec![0xaa, 0xbb, 0xcc, 0, 12, 0, 3];
        bytes.extend_from_slice(&9u32.to_be_bytes());
        for reference in [2u16, 3, 4, 5, 6, 7, 8, 9] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        let revision_prefix_end = bytes.len();
        let revision_state_tail = [0xde, 0xad, 0xbe, 0xef];
        bytes.extend_from_slice(&revision_state_tail);
        let type_45_offset = bytes.len();
        bytes.extend_from_slice(&45u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&1.25f64.to_be_bytes());
        bytes.extend_from_slice(&2.5f64.to_be_bytes());
        bytes.extend_from_slice(&[0xdd, 0xee]);
        let tombstone_offset = bytes.len();
        bytes.extend_from_slice(&[0, 29, 0, 11, 0, 1]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes,
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let census = crate::deltas::walk(&streams[0].inflated);
        let events = super::parasolid_deltas_events_with_censuses(&streams, vec![Some(census)]);

        assert_eq!(events.body_revisions.len(), 1);
        assert_eq!(events.body_revisions[0].xmt, 3);
        assert_eq!(events.body_revisions[0].node_id, 9);
        assert_eq!(
            events.body_revisions[0].references,
            [2, 3, 4, 5, 6, 7, 8, 9]
        );
        assert_eq!(events.body_revisions[0].prefix_byte_len, 32);
        assert_eq!(events.body_revisions[0].state_tail_byte_len, 4);
        assert_eq!(events.body_revisions[0].byte_len, 36);
        assert_eq!(
            events.body_revisions[0].state_tail_sha256,
            cadmpeg_ir::hash::sha256_hex(&revision_state_tail)
        );
        assert_eq!(
            events.body_revisions[0].inflated_offset + events.body_revisions[0].prefix_byte_len,
            revision_prefix_end as u64
        );
        assert_eq!(events.records.len(), 1);
        assert_eq!(events.records[0].family, "TYPE_45");
        assert_eq!(events.records[0].xmt, 10);
        assert_eq!(events.records[0].inflated_offset, type_45_offset as u64);
        assert_eq!(events.records[0].byte_len, 24);
        assert_eq!(events.tombstones.len(), 1);
        assert_eq!(events.tombstones[0].family, "POINT");
        assert_eq!(events.tombstones[0].xmt, 11);
        assert_eq!(events.tombstones[0].byte_len, 6);
        assert_eq!(
            events.tombstones[0].inflated_offset,
            tombstone_offset as u64
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].inflated_offset, 0);
        assert_eq!(events.residual_spans[0].byte_len, 3);
        assert_eq!(
            events.residual_spans[0].sha256,
            cadmpeg_ir::hash::sha256_hex(&[0xaa, 0xbb, 0xcc])
        );
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            (type_45_offset + 24) as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_typed_term_use_numeric_tails_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend_from_slice(&41u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&20u16.to_be_bytes());
        bytes.extend_from_slice(b"L?");
        for coordinate in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&coordinate.to_be_bytes());
        }
        let tail_offset = bytes.len();
        for ordinal in 0..8 {
            bytes.extend_from_slice(&(ordinal as f64 + 0.5).to_be_bytes());
        }
        bytes.extend_from_slice(&[0xcc, 0xdd, 0xee]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes,
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.term_use_numeric_tails.len(), 1);
        let tail = &events.term_use_numeric_tails[0];
        assert_eq!(tail.term_use_xmt, 20);
        assert_eq!(tail.term_use_count, 1);
        assert_eq!(tail.values.len(), 8);
        assert_eq!(tail.byte_len, 64);
        assert_eq!(tail.inflated_offset, tail_offset as u64);
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            (tail_offset + 64) as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 3);
    }

    #[test]
    fn deltas_events_subtract_tagged_reference_lanes_from_residuals() {
        let mut bytes = [0xaa, 0xbb, 0xcc].to_vec();
        bytes.extend(deltas_type_45(10));
        let lane_offset = bytes.len();
        bytes.extend([
            0x00, 0x4f, 0x00, 0x0a, // direct type-79 reference
            0x00, 0x50, 0xff, 0xff, 0x00, 0x01, // extended type-80 reference
        ]);
        let lane_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xdd, 0xee]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.tagged_reference_lanes.len(), 1);
        let lane = &events.tagged_reference_lanes[0];
        assert_eq!(lane.references, [(79, 10), (80, 32_768)]);
        assert_eq!(lane.byte_len, 10);
        assert_eq!(lane.inflated_offset, lane_offset as u64);
        assert_eq!(
            lane.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[lane_offset..lane_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].inflated_offset, 0);
        assert_eq!(events.residual_spans[0].byte_len, 3);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_transmit_headers_from_residuals() {
        let description = b": TRANSMIT FILE (deltas) created by modeller version 3501171";
        let schema = b"SCH_3501171_35102_13006";
        let mut bytes = b"PS".to_vec();
        bytes.extend_from_slice(&(description.len() as u32).to_be_bytes());
        bytes.extend_from_slice(description);
        bytes.extend_from_slice(&(schema.len() as u32).to_be_bytes());
        bytes.extend_from_slice(schema);
        bytes.extend_from_slice(&[
            0, 0xe7, 0, 0, 0, 0, 0, 3, 0xff, 0x04, 0x27, 0x04, 0x28, 0, 0,
        ]);
        let header_end = bytes.len();
        bytes.extend([0xaa, 0xbb]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: Some("SCH_3501171_35102_13006".to_string()),
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.transmit_headers.len(), 1);
        let header = &events.transmit_headers[0];
        assert_eq!(header.id, "nx:s0:deltas-transmit-header#0");
        assert_eq!(header.description.as_bytes(), description);
        assert_eq!(header.schema.as_bytes(), schema);
        assert_eq!(header.references, [1063, 1064]);
        assert_eq!(header.byte_len, header_end as u64);
        assert_eq!(
            header.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[..header_end])
        );
        assert_eq!(events.residual_spans.len(), 1);
        assert_eq!(events.residual_spans[0].inflated_offset, header_end as u64);
        assert_eq!(events.residual_spans[0].byte_len, 2);
    }

    #[test]
    fn deltas_events_retain_terminal_null_references() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        let trailer_offset = bytes.len();
        bytes.extend_from_slice(&[0, 1, 0, 1, 0, 1, 0, 1]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.terminal_null_references.len(), 1);
        let trailer = &events.terminal_null_references[0];
        assert_eq!(trailer.references, [1; 4]);
        assert_eq!(trailer.byte_len, 8);
        assert_eq!(trailer.inflated_offset, trailer_offset as u64);
        assert_eq!(
            trailer.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[trailer_offset..])
        );
        assert_eq!(events.residual_spans.len(), 1);
        assert_eq!(events.residual_spans[0].inflated_offset, 0);
        assert_eq!(events.residual_spans[0].byte_len, trailer_offset as u64);
    }

    #[test]
    fn deltas_events_subtract_reference_type_maps_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let map_offset = bytes.len();
        bytes.extend([
            0, 1, 0, 1, 0xe3, 0xbf, 0, 1, 0, 81, 0, 3, 0, 100, 0, 1, 0, 0, 0, 55,
        ]);
        let map_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.reference_type_maps.len(), 1);
        let map = &events.reference_type_maps[0];
        assert_eq!(map.entries, [(40_000, 81), (3, 100)]);
        assert_eq!(map.target_kind, Some(55));
        assert_eq!(map.byte_len, 20);
        assert_eq!(map.inflated_offset, map_offset as u64);
        assert_eq!(
            map.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[map_offset..map_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_reference_state_packets_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let packet_offset = bytes.len();
        bytes.extend([0, 1, 0, 1, 0, 4]);
        for reference in [2u16, 3, 4, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&1u16.to_be_bytes());
        for word in [34u32, 6, 11, 22_362, 1] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes.push(65);
        let packet_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd, 0xee]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.reference_state_packets.len(), 1);
        let packet = &events.reference_state_packets[0];
        assert_eq!(
            packet.frames,
            [ParasolidDeltasReferenceStateFrame {
                references: [2, 3, 4, 1],
                state_words: [34, 6, 11, 22_362, 1],
                state_byte: 65,
            }]
        );
        assert!(!packet.terminal);
        assert_eq!(packet.byte_len, 37);
        assert_eq!(packet.inflated_offset, packet_offset as u64);
        assert_eq!(
            packet.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[packet_offset..packet_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 3);
    }

    #[test]
    fn deltas_events_retain_schema_reference_preambles() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let preamble_offset = bytes.len();
        bytes.extend_from_slice(&300u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.push(0xff);
        for reference in [2u16, 3, 1, 1, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for state_word in [2u32, 0, 1, 55] {
            bytes.extend_from_slice(&state_word.to_be_bytes());
        }
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&300u16.to_be_bytes());
        for reference in [1u16, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&5u16.to_be_bytes());
        for (kind, reference) in [(81u16, 4u16), (82, 5), (81, 6)] {
            bytes.extend_from_slice(&kind.to_be_bytes());
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&82u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&9u16.to_be_bytes());
        let preamble_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.schema_reference_preambles.len(), 1);
        let preamble = &events.schema_reference_preambles[0];
        assert_eq!(preamble.identity, 300);
        assert_eq!(preamble.references, [2, 3]);
        assert_eq!(preamble.state_reference, None);
        assert_eq!(preamble.state_words, [2, 0, 1, 55]);
        assert_eq!(preamble.count, 5);
        assert_eq!(preamble.entries, [(81, 4), (82, 5), (81, 6)]);
        assert_eq!(preamble.terminal_value, 9);
        assert_eq!(preamble.inflated_offset, preamble_offset as u64);
        assert_eq!(preamble.byte_len, (preamble_end - preamble_offset) as u64);
        assert_eq!(
            preamble.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[preamble_offset..preamble_end])
        );
    }

    #[test]
    fn deltas_events_subtract_reference_marker_packets_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let packet_offset = bytes.len();
        bytes.extend([0, 9, 1, 0, 1, 1, 0x53, 0, 1, 1]);
        let packet_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.reference_marker_packets.len(), 1);
        let packet = &events.reference_marker_packets[0];
        assert_eq!(packet.reference, 9);
        assert_eq!(packet.marker, 0x53);
        assert_eq!(packet.byte_len, 10);
        assert_eq!(packet.inflated_offset, packet_offset as u64);
        assert_eq!(
            packet.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[packet_offset..packet_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_inline_schema_declarations_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let declaration_offset = bytes.len();
        bytes.extend([
            0x00, 0x13, 0x09, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x05, 0x66, 0x72, 0x61,
            0x6d, 0x65, 0x00, 0xe6, 0x00, 0x01, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72,
            0x00, 0x0c, 0x00, 0x01, 0x5a,
        ]);
        bytes.extend_from_slice(&11u16.to_be_bytes());
        bytes.extend_from_slice(&5u32.to_be_bytes());
        for reference in [1u16, 3, 1, 9] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        let declaration_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.inline_schema_declarations.len(), 1);
        let declaration = &events.inline_schema_declarations[0];
        assert_eq!(
            declaration.fields,
            ParasolidDeltasInlineSchemaFields::Region {
                xmt: 11,
                state_word: 5,
                references: [1, 3, 1, 9],
            }
        );
        assert_eq!(declaration.byte_len, 51);
        assert_eq!(declaration.inflated_offset, declaration_offset as u64);
        assert_eq!(
            declaration.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[declaration_offset..declaration_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;

    use cadmpeg_ir::Exactness;

    use crate::container;
    use crate::parasolid::{self, StreamKind};
    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    #[test]
    fn attribute_value_uses_are_assigned_to_compatible_declared_fields() {
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 2,
            xmt: 9,
            next_definition_xmt: 1,
            identifier_xmt: 10,
            identifier_inflated_offset: 32,
            name: "CLASS".into(),
            type_id: 8000,
            action_codes: [0; 8],
            field_names_xmt: 1,
            legal_owner_flags: [0; 16],
            field_count: 3,
            field_codes: vec![1, 2, 3],
            inflated_offset: 40,
        };
        let class_use = ParasolidAttributeClassUse {
            id: "nx:s2:attribute-class-use#class-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: 9,
            attribute_definition: "definition".into(),
        };
        let numeric_use = ParasolidEntity51NumericUse {
            id: "numeric-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            reference_ordinal: 5,
            referenced_xmt: 12,
            kind: ParasolidEntity51NumericKind::UnsignedIntegers,
            value_record: "integers".into(),
            inflated_offset: 48,
        };
        let double_use = ParasolidEntity51NumericUse {
            id: "double-use".into(),
            reference_ordinal: 6,
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: "doubles".into(),
            ..numeric_use.clone()
        };
        let string_use = ParasolidEntity51StringUse {
            id: "string-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            reference_ordinal: 7,
            referenced_xmt: 14,
            string_record: "string".into(),
            inflated_offset: 48,
        };

        let uses = parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[numeric_use.clone(), double_use],
            std::slice::from_ref(&string_use),
            &[],
        );

        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].id, "nx:s2:attribute-field-use#class-use-0");
        assert_eq!(
            uses[0].attribute_class_use,
            "nx:s2:attribute-class-use#class-use"
        );
        assert_eq!(uses[0].attribute_definition, "definition");
        assert_eq!(uses[0].field_ordinal, 0);
        assert_eq!(uses[0].field_code, 1);
        assert_eq!(uses[0].reference_ordinal, 5);
        assert_eq!(
            uses[0].value_kind,
            ParasolidAttributeFieldValueKind::UnsignedIntegers
        );
        assert_eq!(uses[0].value_use, "numeric-use");
        assert_eq!(uses[0].value_record, "integers");
        assert_eq!(uses[1].field_ordinal, 1);
        assert_eq!(uses[1].field_code, 2);
        assert_eq!(
            uses[1].value_kind,
            ParasolidAttributeFieldValueKind::Doubles
        );
        assert_eq!(uses[1].value_record, "doubles");
        assert_eq!(uses[2].field_ordinal, 2);
        assert_eq!(uses[2].field_code, 3);
        assert_eq!(uses[2].value_kind, ParasolidAttributeFieldValueKind::String);
        assert_eq!(uses[2].value_record, "string");

        let duplicate = ParasolidAttributeClassUse {
            id: "duplicate".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: 11,
            attribute_definition: "other-definition".into(),
        };
        assert!(parasolid_attribute_field_uses(
            &[class_use.clone(), duplicate.clone()],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[],
            &[],
        )
        .is_empty());

        let wrong_stream = ParasolidAttributeClassUse {
            stream_ordinal: 3,
            ..duplicate
        };
        assert!(parasolid_attribute_field_uses(
            &[wrong_stream],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[],
            &[],
        )
        .is_empty());

        let mismatched = ParasolidEntity51NumericUse {
            kind: ParasolidEntity51NumericKind::Doubles,
            ..numeric_use.clone()
        };
        assert!(parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[mismatched],
            &[],
            &[],
        )
        .is_empty());

        let ambiguous_string = ParasolidEntity51StringUse {
            reference_ordinal: 5,
            ..string_use
        };
        assert!(parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[ambiguous_string],
            &[],
        )
        .is_empty());
    }

    #[test]
    fn structured_value_uses_require_one_same_stream_family() {
        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 2,
            xmt: 10,
            flags: 1,
            sequence: 0,
            definition_xmt: 9,
            leading_references: [1; 5],
            trailing_references: vec![12],
            byte_len: 32,
            inflated_offset: 40,
        };
        let point = ParasolidEntityVectorRecord {
            id: "point".into(),
            stream_ordinal: 2,
            kind: ParasolidVectorValueKind::Points,
            xmt: 12,
            values: vec![[1.0, 2.0, 3.0]],
            byte_len: 36,
            inflated_offset: 80,
        };
        let uses = parasolid_entity_51_structured_uses(
            std::slice::from_ref(&entity),
            std::slice::from_ref(&point),
            &[],
            &[],
            &[],
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].reference_ordinal, 5);
        assert_eq!(uses[0].kind, ParasolidAttributeFieldValueKind::Points);
        assert_eq!(uses[0].value_record, "point");

        let colliding_tag = ParasolidEntity58TagRecord {
            id: "tag".into(),
            stream_ordinal: 2,
            xmt: 12,
            values: vec![7],
            byte_len: 16,
            inflated_offset: 90,
        };
        assert!(parasolid_entity_51_structured_uses(
            std::slice::from_ref(&entity),
            std::slice::from_ref(&point),
            &[],
            std::slice::from_ref(&colliding_tag),
            &[],
        )
        .is_empty());

        let other_stream = ParasolidEntityVectorRecord {
            stream_ordinal: 3,
            ..point
        };
        assert!(
            parasolid_entity_51_structured_uses(&[entity], &[other_stream], &[], &[], &[],)
                .is_empty()
        );
    }

    #[test]
    fn structured_value_families_match_only_their_declared_field_codes() {
        let kinds = [
            ParasolidAttributeFieldValueKind::Points,
            ParasolidAttributeFieldValueKind::Vectors,
            ParasolidAttributeFieldValueKind::Directions,
            ParasolidAttributeFieldValueKind::Axes,
            ParasolidAttributeFieldValueKind::Tags,
            ParasolidAttributeFieldValueKind::Unicode,
        ];
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 2,
            xmt: 9,
            next_definition_xmt: 1,
            identifier_xmt: 10,
            identifier_inflated_offset: 32,
            name: "CLASS".into(),
            type_id: 8000,
            action_codes: [0; 8],
            field_names_xmt: 1,
            legal_owner_flags: [0; 16],
            field_count: 6,
            field_codes: vec![4, 5, 6, 7, 8, 10],
            inflated_offset: 40,
        };
        let class_use = ParasolidAttributeClassUse {
            id: "nx:s2:attribute-class-use#class-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: 9,
            attribute_definition: definition.id.clone(),
        };
        let structured = kinds
            .iter()
            .enumerate()
            .map(|(ordinal, kind)| ParasolidEntity51StructuredUse {
                id: format!("use-{ordinal}"),
                stream_ordinal: 2,
                entity_51_record: "entity".into(),
                reference_ordinal: u32::try_from(ordinal).expect("test ordinal fits u32") + 5,
                referenced_xmt: u32::try_from(ordinal).expect("test ordinal fits u32") + 20,
                kind: *kind,
                value_record: format!("value-{ordinal}"),
                inflated_offset: 48,
            })
            .collect::<Vec<_>>();
        let uses = parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[],
            &[],
            &structured,
        );
        assert_eq!(
            uses.iter().map(|use_| use_.value_kind).collect::<Vec<_>>(),
            kinds
        );

        let mut mismatched = structured;
        mismatched[0].kind = ParasolidAttributeFieldValueKind::Vectors;
        let uses =
            parasolid_attribute_field_uses(&[class_use], &[definition], &[], &[], &mismatched);
        assert_eq!(uses.len(), 5);
        assert!(uses.iter().all(|use_| use_.field_ordinal != 0));
    }

    #[test]
    fn attribute_loss_requires_concrete_unresolved_references() {
        let definition = |field_names_xmt, field_codes: Vec<u8>| ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 0,
            xmt: 20,
            next_definition_xmt: 1,
            identifier_xmt: 21,
            identifier_inflated_offset: 10,
            name: "CLASS".into(),
            type_id: 8000,
            action_codes: [0; 8],
            field_names_xmt,
            legal_owner_flags: [0; 16],
            field_count: u32::try_from(field_codes.len()).expect("test field count fits u32"),
            field_codes,
            inflated_offset: 20,
        };

        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 0,
            xmt: 30,
            flags: 1,
            sequence: 0,
            definition_xmt: 20,
            leading_references: [1; 5],
            trailing_references: vec![40],
            byte_len: 32,
            inflated_offset: 30,
        };
        let class_use = ParasolidAttributeClassUse {
            id: "class-use".into(),
            stream_ordinal: 0,
            entity_51_record: entity.id.clone(),
            definition_xmt: 20,
            attribute_definition: "definition".into(),
        };
        let field_use = ParasolidAttributeFieldUse {
            id: "field-use".into(),
            stream_ordinal: 0,
            attribute_class_use: class_use.id.clone(),
            entity_51_record: entity.id.clone(),
            attribute_definition: "definition".into(),
            field_ordinal: 0,
            field_code: 4,
            reference_ordinal: 5,
            value_kind: ParasolidAttributeFieldValueKind::Points,
            value_use: "point-use".into(),
            value_record: "points".into(),
            inflated_offset: 30,
        };
        let topology_reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 0,
            topology_type: 14,
            topology_xmt: 50,
            attribute_list_xmt: entity.xmt,
            attribute_list_record: Some(entity.id.clone()),
            inflated_offset: 28,
        };
        let topology_class_use = ParasolidTopologyAttributeClassUse {
            id: "topology-class-use".into(),
            topology_attribute_reference: topology_reference.id.clone(),
            entity_51_record: entity.id.clone(),
            attribute_class_use: class_use.id.clone(),
            definition_xmt: 20,
            attribute_definition: "definition".into(),
        };

        // An unused declaration carries no value-loss evidence.
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                &[],
                &[],
                &[],
            )
        );
        // A non-null instance reference must have exactly one resolved field use.
        assert!(
            parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                std::slice::from_ref(&entity),
                std::slice::from_ref(&field_use),
                std::slice::from_ref(&topology_class_use),
            )
        );
        // Null values and always-empty pointer fields require no value relation.
        let mut null_entity = entity.clone();
        null_entity.trailing_references[0] = 1;
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                &[null_entity],
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![9])],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );

        let named_definition = definition(22, vec![4]);
        // An unresolved optional field-name list uses the specification's
        // deterministic ordinal/code fallback and does not lose the value.
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                std::slice::from_ref(&named_definition),
                std::slice::from_ref(&entity),
                std::slice::from_ref(&field_use),
                std::slice::from_ref(&topology_class_use),
            )
        );
        assert!(
            parasolid_topology_attribute_fields_have_untransferred_values(
                &[named_definition],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );
    }

    #[test]
    fn attribute_field_names_require_complete_unambiguous_same_stream_relations() {
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: 20,
            next_definition_xmt: 1,
            identifier_xmt: 21,
            identifier_inflated_offset: 10,
            name: "CLASS".into(),
            type_id: 8000,
            action_codes: [0; 8],
            field_names_xmt: 25,
            legal_owner_flags: [0; 16],
            field_count: 3,
            field_codes: vec![2, 1, 1],
            inflated_offset: 20,
        };
        let list = ParasolidFieldNamesRecord {
            id: "field-names".into(),
            stream_ordinal: 3,
            xmt: 25,
            name_xmts: vec![28, 29, 30],
            byte_len: 15,
            inflated_offset: 30,
        };
        let strings = [28, 30].map(|xmt| ParasolidEntity54StringRecord {
            id: format!("string-{xmt}"),
            stream_ordinal: 3,
            xmt,
            value: (xmt - 27).to_string(),
            byte_len: 10,
            inflated_offset: u64::from(xmt),
        });
        let unicode = ParasolidEntity62UnicodeRecord {
            id: "unicode-29".into(),
            stream_ordinal: 3,
            xmt: 29,
            code_units: vec![0x03bc],
            value: "μ".into(),
            byte_len: 12,
            inflated_offset: 29,
        };

        let relations = parasolid_attribute_field_names(
            std::slice::from_ref(&definition),
            std::slice::from_ref(&list),
            &strings,
            std::slice::from_ref(&unicode),
        );
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].names, ["1", "μ", "3"]);

        let mut incomplete = list.clone();
        incomplete.name_xmts.pop();
        assert!(parasolid_attribute_field_names(
            std::slice::from_ref(&definition),
            &[incomplete],
            &strings,
            std::slice::from_ref(&unicode),
        )
        .is_empty());
        assert!(parasolid_attribute_field_names(
            &[definition.clone(), definition.clone()],
            std::slice::from_ref(&list),
            &strings,
            std::slice::from_ref(&unicode),
        )
        .is_empty());

        let ambiguous = ParasolidEntity62UnicodeRecord {
            xmt: 28,
            ..unicode.clone()
        };
        assert!(parasolid_attribute_field_names(
            std::slice::from_ref(&definition),
            &[ParasolidFieldNamesRecord {
                id: "field-names".into(),
                stream_ordinal: 3,
                xmt: 25,
                name_xmts: vec![28, 29, 30],
                byte_len: 15,
                inflated_offset: 30,
            }],
            &strings,
            &[unicode, ambiguous],
        )
        .is_empty());
    }

    #[test]
    fn deltas_events_subtract_type_150_state_packets_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let packet_offset = bytes.len();
        bytes.push(150);
        for (reference, status) in [(1u16, 1), (3, 1), (6_192, 0), (6_193, 1), (6_194, 0)] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(status);
        }
        bytes.push(0x2b);
        let values: [f64; 9] = [-0.025, -0.05, 0.25, 0.0, 1.0, 0.0, 0.0, -0.0, 1.0];
        for value in values {
            bytes.extend(value.to_be_bytes());
        }
        let packet_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            kind: StreamKind::Deltas,
            schema: None,
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.type_150_state_packets.len(), 1);
        let packet = &events.type_150_state_packets[0];
        assert_eq!(packet.references, [1, 3, 6_192, 6_193, 6_194]);
        assert_eq!(packet.marker, 0x2b);
        assert_eq!(packet.values, values);
        assert_eq!(packet.inflated_offset, packet_offset as u64);
        assert_eq!(packet.byte_len, (packet_end - packet_offset) as u64);
        assert_eq!(
            packet.sha256,
            cadmpeg_ir::hash::sha256_hex(&bytes[packet_offset..packet_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn topology_retains_entity_attribute_list_references() {
        let mut stream = topology_partition_stream();
        for (kind, attribute) in [(14, 41), (15, 42), (17, 43), (16, 44), (18, 45)] {
            let at = stream
                .windows(2)
                .position(|window| window == [0, kind])
                .expect("topology record");
            put_ref(&mut stream, at + if kind == 17 { 4 } else { 8 }, attribute);
        }
        stream.extend_from_slice(&[0, 0x51]);
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&41u16.to_be_bytes());
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in [4u16, 1, 1, 1, 1, 42] {
            stream.extend_from_slice(&reference.to_be_bytes());
        }
        stream.extend_from_slice(&[0, 0x54]);
        stream.extend_from_slice(&8u32.to_be_bytes());
        stream.extend_from_slice(&42u16.to_be_bytes());
        stream.extend_from_slice(b"deadbeef\0");

        let graph = crate::topology::Graph::parse(&stream);
        assert_eq!(
            graph
                .get(14, 4)
                .expect("required invariant")
                .face_fields()
                .expect("required invariant")
                .attributes,
            41
        );
        assert_eq!(
            graph
                .get(15, 5)
                .expect("required invariant")
                .loop_fields()
                .expect("required invariant")
                .attributes,
            42
        );
        assert_eq!(
            graph
                .get(17, 7)
                .expect("required invariant")
                .fin_fields()
                .expect("required invariant")
                .attributes,
            43
        );
        assert_eq!(
            graph
                .get(16, 8)
                .expect("required invariant")
                .edge_fields()
                .expect("required invariant")
                .attributes,
            44
        );
        assert_eq!(
            graph
                .get(18, 10)
                .expect("required invariant")
                .vertex_fields()
                .expect("required invariant")
                .attributes,
            45
        );

        let result = NxCodec
            .decode(
                &mut Cursor::new(prt_with_partition(&stream)),
                &DecodeOptions::default(),
            )
            .expect("required invariant");
        let references = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidTopologyAttributeListReference>(
                "parasolid_topology_attribute_list_references",
            )
            .expect("required invariant");
        assert_eq!(references.len(), 5);
        assert_eq!(references[0].topology_type, 14);
        assert_eq!(references[0].topology_xmt, 4);
        assert_eq!(references[0].attribute_list_xmt, 41);
        assert!(references[0].attribute_list_record.is_some());
        assert_eq!(result.ir().model.attributes.len(), 1);
        assert_eq!(
            result.ir().model.attributes[0].target,
            cadmpeg_ir::attributes::AttributeTarget::Face(cadmpeg_ir::ids::FaceId(
                "nx:s0:face#4".into()
            ))
        );
        assert_eq!(
            result.ir().model.attributes[0].name,
            "parasolid_type_84_reference_5"
        );
        assert_eq!(
            result.ir().model.attributes[0].values,
            [cadmpeg_ir::attributes::AttributeValue::String(
                "deadbeef".into()
            )]
        );
    }

    #[test]
    fn topology_attribute_class_uses_resolve_type_80_definitions_by_xmt() {
        use super::{
            ParasolidAttributeDefinition, ParasolidEntity51Record,
            ParasolidTopologyAttributeListReference,
        };

        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: 34,
            next_definition_xmt: 1,
            identifier_xmt: 35,
            identifier_inflated_offset: 80,
            name: "UG2/PMARK_ATTRIBUTE".into(),
            type_id: 9000,
            action_codes: [0; 8],
            field_names_xmt: 1,
            legal_owner_flags: [0; 16],
            field_count: 1,
            field_codes: vec![1],
            inflated_offset: 100,
        };
        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 3,
            xmt: 50,
            flags: 1,
            sequence: 7,
            definition_xmt: 34,
            leading_references: [60, 61, 1, 62, 63],
            trailing_references: vec![64],
            byte_len: 26,
            inflated_offset: 200,
        };
        let reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 3,
            topology_type: 14,
            topology_xmt: 60,
            attribute_list_xmt: 50,
            attribute_list_record: Some(entity.id.clone()),
            inflated_offset: 300,
        };

        let instance_uses = super::parasolid_attribute_class_uses(
            std::slice::from_ref(&entity),
            std::slice::from_ref(&definition),
        );
        assert_eq!(instance_uses.len(), 1);
        assert_eq!(instance_uses[0].entity_51_record, entity.id);
        assert_eq!(instance_uses[0].definition_xmt, 34);
        assert_eq!(instance_uses[0].attribute_definition, definition.id);

        let uses = super::parasolid_topology_attribute_class_uses(
            std::slice::from_ref(&reference),
            &instance_uses,
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].attribute_class_use, instance_uses[0].id);
        assert_eq!(uses[0].definition_xmt, 34);
        assert_eq!(uses[0].attribute_definition, definition.id);
        assert!(super::parasolid_topology_attribute_class_uses(
            std::slice::from_ref(&reference),
            &[instance_uses[0].clone(), instance_uses[0].clone()],
        )
        .is_empty());

        let mut invalid = entity;
        invalid.definition_xmt = 33;
        assert!(super::parasolid_attribute_class_uses(
            std::slice::from_ref(&invalid),
            std::slice::from_ref(&definition),
        )
        .is_empty());
        assert!(super::parasolid_topology_attribute_class_uses(
            &[reference],
            &super::parasolid_attribute_class_uses(&[invalid], &[definition]),
        )
        .is_empty());
    }

    #[test]
    fn entity_51_value_uses_exclude_fixed_leading_references() {
        use super::{
            ParasolidEntity51Record, ParasolidEntity52IntegerRecord, ParasolidEntity54StringRecord,
        };

        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 3,
            xmt: 50,
            flags: 2,
            sequence: 7,
            definition_xmt: 34,
            leading_references: [60, 61, 70, 71, 72],
            trailing_references: vec![70, 71],
            byte_len: 28,
            inflated_offset: 200,
        };
        let integers = [ParasolidEntity52IntegerRecord {
            id: "integers".into(),
            stream_ordinal: 3,
            xmt: 70,
            values: vec![1],
            byte_len: 12,
            inflated_offset: 300,
        }];
        let strings = [ParasolidEntity54StringRecord {
            id: "string".into(),
            stream_ordinal: 3,
            xmt: 71,
            value: "value".into(),
            byte_len: 14,
            inflated_offset: 400,
        }];

        let numeric_uses =
            super::parasolid_entity_51_numeric_uses(std::slice::from_ref(&entity), &integers, &[]);
        assert_eq!(numeric_uses.len(), 1);
        assert_eq!(numeric_uses[0].reference_ordinal, 5);
        assert_eq!(numeric_uses[0].referenced_xmt, 70);

        let string_uses =
            super::parasolid_entity_51_string_uses(std::slice::from_ref(&entity), &strings);
        assert_eq!(string_uses.len(), 1);
        assert_eq!(string_uses[0].reference_ordinal, 6);
        assert_eq!(string_uses[0].referenced_xmt, 71);
    }

    #[test]
    fn parasolid_attribute_definition_requires_declared_printable_name_and_field_record() {
        let mut bytes = vec![0xaa, 0x00, 0x4f, 0xff];
        bytes.extend_from_slice(&16u32.to_be_bytes());
        bytes.extend_from_slice(&0x012au16.to_be_bytes());
        bytes.extend_from_slice(b"SDL/TYSA_DENSITY");
        bytes.extend_from_slice(&[0x00, 0x50, 0x00, 0x00, 0x00, 0x01]);
        bytes.extend_from_slice(&0x012bu16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0x012au16.to_be_bytes());
        bytes.extend_from_slice(&9000u32.to_be_bytes());
        bytes.extend_from_slice(&[0, 1, 2, 3, 4, 5, 6, 0]);
        bytes.extend_from_slice(&0x0030u16.to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]);
        bytes.push(2);
        let definitions = crate::parasolid::attribute_definitions(&bytes);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].offset, 26);
        assert_eq!(definitions[0].xmt, 0x12b);
        assert_eq!(definitions[0].identifier_xmt, 0x12a);
        assert_eq!(definitions[0].identifier_offset, 1);
        assert_eq!(definitions[0].name, "SDL/TYSA_DENSITY");
        assert_eq!(definitions[0].next_definition_xmt, 1);
        assert_eq!(definitions[0].type_id, 9000);
        assert_eq!(definitions[0].action_codes, [0, 1, 2, 3, 4, 5, 6, 0]);
        assert_eq!(definitions[0].field_names_xmt, 0x30);
        assert_eq!(definitions[0].legal_owner_flags[4], 1);
        assert_eq!(definitions[0].legal_owner_flags[12], 1);
        assert_eq!(definitions[0].field_count, 1);
        assert_eq!(definitions[0].field_codes, [2]);

        let truncated = &bytes[..bytes.len() - 1];
        assert!(crate::parasolid::attribute_definitions(truncated).is_empty());

        let mut duplicate_identifier = bytes.clone();
        duplicate_identifier.splice(26..26, bytes[1..26].iter().copied());
        assert!(crate::parasolid::attribute_definitions(&duplicate_identifier).is_empty());

        bytes[42] = 7;
        assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
        bytes[42] = 0;
        bytes[52] = 2;
        assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
        bytes[52] = 0;
        bytes[20] = 0;
        assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
    }

    #[test]
    fn decode_preserves_offset_status_without_assigning_parameter_sense() {
        for discriminator in ['V', 'I', 'U'] {
            for true_offset in [false, true] {
                let mut stream = offset_surface_topology_partition_stream();
                let offset_record = stream.len() - 31;
                stream[offset_record + 19] = discriminator as u8;
                stream[offset_record + 20] = u8::from(true_offset);
                let mut cur = Cursor::new(prt_with_partition(&stream));
                let result = NxCodec
                    .decode(&mut cur, &DecodeOptions::default())
                    .expect("required invariant");

                let procedural = result
                    .ir()
                    .model
                    .procedural_surfaces
                    .first()
                    .expect("offset surface");
                let ProceduralSurfaceDefinition::Offset {
                    support,
                    distance,
                    u_sense,
                    v_sense,
                    extension_flags,
                    ..
                } = &procedural.definition
                else {
                    panic!("offset definition");
                };
                assert_eq!(*distance, 2.5);
                assert_eq!(*u_sense, None);
                assert_eq!(*v_sense, None);
                assert!(extension_flags.is_empty());
                assert_ne!(procedural.surface, *support);
                assert_eq!(result.ir().model.faces[0].surface, procedural.surface);
                let records = result
                    .ir()
                    .native
                    .namespace("nx")
                    .expect("required invariant")
                    .arena_as::<super::ParasolidOffsetSurfaceRecord>(
                        "parasolid_offset_surface_records",
                    )
                    .expect("required invariant");
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].discriminator, discriminator);
                assert_eq!(records[0].true_offset, true_offset);
                assert_eq!(records[0].support_xmt, 6);
                assert_eq!(records[0].distance, 2.5);
                let carrier = result
                    .ir()
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == procedural.surface)
                    .expect("offset carrier");
                assert_eq!(
                    carrier
                        .source_object
                        .as_ref()
                        .map(|source| &source.object_id),
                    Some(&records[0].id)
                );
                assert!(matches!(
                    &carrier.geometry,
                    SurfaceGeometry::Procedural { construction } if construction == &procedural.id
                ));
                assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
            }
        }
    }

    #[test]
    fn decode_resolves_surface_curve_to_its_basis_curve() {
        let stream = surface_curve_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");

        assert_eq!(result.ir().model.edges.len(), 1);
        let records = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidSurfaceCurveRecord>("parasolid_surface_curve_records")
            .expect("required invariant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].surface_xmt, 6);
        assert_eq!(records[0].pcurve_xmt, 9);
        assert_eq!(records[0].original_curve_xmt, 9);
        assert_eq!(records[0].tolerance_to_original, 0.000_01);
        assert_eq!(
            result.ir().model.edges[0].curve.as_ref(),
            Some(&result.ir().model.curves[0].id)
        );
        assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
    }

    #[test]
    fn decode_emits_rolling_ball_blend_surface() {
        let stream = blend_surface_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");

        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .first()
            .expect("blend surface");
        let ProceduralSurfaceDefinition::Blend {
            supports,
            radius,
            cross_section,
            spine,
            native,
        } = &procedural.definition
        else {
            panic!("blend definition");
        };
        assert_eq!(*cross_section, BlendCrossSection::Circular);
        assert_eq!(
            *radius,
            BlendRadiusLaw::Constant {
                signed_radius: -3.0
            }
        );
        assert_eq!(supports[0].as_ref().map(|side| side.reversed), Some(true));
        assert_eq!(supports[1].as_ref().map(|side| side.reversed), Some(false));
        assert!(spine.is_none());
        assert!(native.is_none());
        assert_eq!(result.ir().model.faces[0].surface, procedural.surface);
        let records = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidBlendSurfaceRecord>("parasolid_blend_surface_records")
            .expect("required invariant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].support_xmts, [6, 6]);
        assert_eq!(records[0].spine_xmt, 1);
        assert_eq!(records[0].offsets, [-3.0, 3.0]);
        assert_eq!(records[0].thumb_weights, [1.0, 1.0]);
        let carrier = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .expect("required invariant");
        assert_eq!(
            carrier
                .source_object
                .as_ref()
                .map(|association| association.object_id.as_str()),
            Some(records[0].id.as_str())
        );
        assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
    }

    #[test]
    fn decode_preserves_intersection_curve_as_connected_carrier() {
        let stream = intersection_curve_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");

        let edge_curve = result.ir().model.edges[0]
            .curve
            .as_ref()
            .expect("edge curve");
        let curve = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| &curve.id == edge_curve)
            .expect("intersection carrier");
        assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
        let records = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidIntersectionRecord>("parasolid_intersection_records")
            .expect("required invariant");
        assert_eq!(records.len(), 1);
        assert!(!records[0].delta_twin);
        assert_eq!(records[0].header_references[0], 1);
        assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
        assert_eq!(
            curve.source_object.as_ref().map(|source| &source.object_id),
            Some(&records[0].id)
        );
        assert_eq!(result.ir().model.procedural_curves.len(), 1);
        assert_eq!(result.ir().model.procedural_curves[0].curve, curve.id);
        assert!(result.report().losses.iter().any(|loss| {
            loss.code.category() == LossCategory::Geometry
                && loss.message.starts_with("1 surface-intersection record(s)")
        }));
        assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
    }

    #[test]
    fn decode_preserves_deltas_intersection_data_curve() {
        let mut partition = topology_partition_stream();
        for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
            let marker = [0, tag, 0, xmt];
            let record = partition
                .windows(marker.len())
                .position(|window| window == marker)
                .expect("topology record");
            put_ref(&mut partition, record + offset, 12);
        }
        let deltas = deltas_intersection_curve_stream();
        let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");

        assert_eq!(result.ir().model.procedural_curves.len(), 1);
        let records = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidIntersectionRecord>("parasolid_intersection_records")
            .expect("required invariant");
        assert_eq!(records.len(), 1);
        assert!(records[0].delta_twin);
        assert_eq!(records[0].header_references[0], 1);
        assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
        assert_eq!(
            result.ir().model.edges[0].curve.as_ref(),
            Some(&result.ir().model.procedural_curves[0].curve)
        );
        assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
    }

    #[test]
    fn decode_emits_charted_surface_intersection_construction() {
        let stream = charted_intersection_curve_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");

        let terms = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidTermUseRecord>("parasolid_term_use_records")
            .expect("required invariant");
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].count, 1);
        assert_eq!(terms[0].form, "L?");
        assert_eq!(terms[0].point, [0.0, 0.0, 0.0]);
        assert_eq!(terms[1].point, [10.0, 0.0, 0.0]);
        assert!(terms
            .iter()
            .all(|term| matches!(term.framing, crate::intersection::TermUseFraming::Direct)));
        let support_uv = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidSupportUvRecord>("parasolid_support_uv_records")
            .expect("required invariant");
        assert_eq!(support_uv.len(), 1);
        assert_eq!(support_uv[0].count, 4);
        assert_eq!(support_uv[0].marker, 2);
        assert_eq!(support_uv[0].values, [0.0, 0.0, 0.01, 0.0]);
        assert!(matches!(
            support_uv[0].framing,
            crate::intersection::SupportUvFraming::Direct
        ));
        let charts = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidChartRecord>("parasolid_chart_records")
            .expect("required invariant");
        assert_eq!(charts.len(), 1);
        assert_eq!(charts[0].count, 2);
        assert_eq!(charts[0].base_parameter, 0.0);
        assert_eq!(charts[0].base_scale, 1.0);
        assert_eq!(charts[0].chart_count, 2);
        assert_eq!(charts[0].chordal_error, 0.000_01);
        assert_eq!(charts[0].angular_error, 0.001);
        assert_eq!(charts[0].points, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
        assert!(matches!(
            charts[0].point_layout,
            crate::intersection::ChartPointLayout::Xyz3
        ));

        let procedural = result
            .ir()
            .model
            .procedural_curves
            .first()
            .expect("intersection construction");
        let curve = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == procedural.curve)
            .expect("solved chart cache");
        let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
            panic!("charted NURBS cache");
        };
        assert_eq!(nurbs.degree, 1);
        assert_eq!(nurbs.control_points[0].x, 0.0);
        assert_eq!(nurbs.control_points[1].x, 10.0);
        assert_eq!(procedural.cache_fit_tolerance, Some(0.01));
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
            &procedural.definition
        else {
            panic!("typed surface intersection");
        };
        assert!(context.sides[0].surface.is_some());
        assert!(context.sides[0].pcurve.is_some());
        assert!(context.sides[1].surface.is_none());
        assert_eq!(context.parameter_range, [0.0, 0.01]);
        assert!(result.ir().model.coedges[0].pcurves.is_empty());
        assert!(!result.report().losses.iter().any(|loss| {
            loss.code.category() == LossCategory::Geometry
                && loss.message.contains("surface-intersection record(s)")
        }));
        let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    }

    #[test]
    fn decode_resolves_intersection_second_support_through_blend_bound() {
        let stream = blend_bound_charted_intersection_curve_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");

        let records = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidBlendBoundRecord>("parasolid_blend_bound_records")
            .expect("required invariant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].header_references, [1; 5]);
        assert!(records[0].sense);
        assert_eq!(records[0].boundary_index, 0);
        assert_eq!(records[0].blend_surface_xmt, 13);
        assert_eq!(
            records[0].framing,
            crate::intersection::BlendBoundFraming::PartitionDirect
        );

        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
            &result.ir().model.procedural_curves[0].definition
        else {
            panic!("typed intersection");
        };
        let second = context.sides[1].surface.as_ref().expect("bridged support");
        assert_ne!(context.sides[0].surface.as_ref(), Some(second));
        assert!(context.sides[1].pcurve.is_some());
    }

    #[test]
    fn decode_resolves_trimmed_edge_to_its_basis_curve_and_range() {
        let mut cur = Cursor::new(prt_with_partition(&trimmed_topology_partition_stream()));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");
        let edge = result.ir().model.edges.first().expect("edge");
        assert_eq!(edge.curve.as_ref(), Some(&result.ir().model.curves[0].id));
        assert_eq!(edge.param_range, Some([0.25, 0.75]));
        let records = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidTrimmedCurveRecord>("parasolid_trimmed_curve_records")
            .expect("required invariant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].basis_xmt, 9);
        assert_eq!(records[0].points, [[0.0; 3]; 2]);
        assert_eq!(records[0].parameters, [0.000_25, 0.000_75]);
        assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
    }
}
