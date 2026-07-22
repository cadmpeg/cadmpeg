//! B2/B3/B4-family consolidated record vocabulary.
//!
//! Decodes analytic circle, cylinder, cone, and revolution charts, offset and
//! construction-use supports, class-`0x5e`/`0x61`/`0x62` owner and link records,
//! parameter-space packets, and consolidated UV pcurves.

use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::le::u16_at as u16_le;
use cadmpeg_ir::math::{Point3, Vector3};
#[cfg(test)]
use std::collections::BTreeMap;

use crate::families::a5a8::records::A8Surface;
#[cfg(test)]
use crate::families::consolidated::records::consolidated_records;
use crate::families::consolidated::records::{
    b_family_frames, parse_consolidated_pcurve, ConsolidatedFrame, ConsolidatedPcurve,
};
#[cfg(test)]
use crate::wire::bytes::persistent_ref;
use crate::wire::bytes::{
    allocation_ref, compact_int, f64_le, finite_f64_lane, read_f64_array, u32_le_24,
};

/// Offset-surface constructor stored in a `b2 03 31` support record or a
/// kind-`0x01` `b2 03 30` construction-use record.
#[derive(Debug, Clone)]
pub struct B2OffsetSupport {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced carrier-surface identifier.
    pub support_id: u32,
    /// Signed normal offset distance in millimetres.
    pub distance: f64,
    /// Carrier UV sub-domain `[u0, v0, u1, v1]`.
    pub domain: [f64; 4],
}

/// Parameter-space data stored in a `b2/b3/b4 03 18` record.
#[derive(Debug, Clone, PartialEq)]
#[cfg(test)]
pub enum B2ParameterPoint {
    /// Two-coordinate UV point (`L=0x12`).
    Uv {
        /// Record byte offset.
        pos: usize,
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Host-chain station followed by UV (`L=0x1a`).
    StationUv {
        /// Record byte offset.
        pos: usize,
        /// Host-chain axial boundary station.
        station: f64,
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Unsplit five-scalar layout (`L=0x2a`).
    FiveScalars {
        /// Record byte offset.
        pos: usize,
        /// Stored scalar payload.
        values: [f64; 5],
    },
}

/// Persistent-tag reference list stored in a `b2/b3/b4 03 37` record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2ReferenceList {
    /// Record byte offset.
    pub pos: usize,
    /// Compact persistent-tag references in serialization order.
    pub references: Vec<u32>,
}

/// Nine-reference owner packet stored in a `b2/b3/b4 03 62` record with a
/// 62-byte numeric tail.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2OwnerPacket {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Encoding selected by the first strong reference token.
    pub reference_encoding: B2OwnerReferenceEncoding,
    /// Nine compact persistent identities following the `0x89` count.
    pub references: [u32; 9],
    /// Fixed-width numeric tail retained byte-exactly.
    pub numeric_tail: [u8; 62],
}

/// Count-framed class-`0x62` owner record with a class-specific tail.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2CountedOwner {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Persistent identities selected by the leading `0x80+n` count.
    pub references: Vec<u32>,
    /// Nonempty class-specific bytes after the reference lane.
    pub tail: Vec<u8>,
}

/// Reference dialect used by a nine-reference class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub enum B2OwnerReferenceEncoding {
    /// Strong identities use `0x0a <u16le>` and weak identities use compact integers.
    TaggedU16Strong,
    /// Strong identities use width-coded compact integers and weak identities
    /// are raw one-byte values.
    WidthCodedStrong,
}

/// Count-prefixed class-`0x61` reference record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2Counted61 {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Compact values selected by the leading `0x80+n` count.
    pub references: Vec<u32>,
    /// Remaining class-specific bytes, including the terminal `0x03`.
    pub tail: Vec<u8>,
}

/// Long-form class-`0x61` record with a monotone u16 member lane.
#[derive(Debug, Clone, PartialEq)]
#[cfg(test)]
pub struct B2Long61 {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Eight opaque bytes preceding the `0x06` list marker.
    pub prefix: [u8; 8],
    /// Strictly increasing little-endian u16 values.
    pub members: Vec<u16>,
    /// Five `0x0a <u16le>` persistent identities after delimiter `0xfe`.
    pub references: [u16; 5],
    /// Finite scalar preceding the terminal byte.
    pub scalar: f64,
}

/// Fixed-shape class-`0x5f` link record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub struct B2Link5f {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Width-coded persistent target between `0x82` and the `03 05` tail.
    pub target: u32,
}

/// Adjacent class-`0x5f` link and class-`0x62` owner packet joined by their
/// allocation-successor identity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2LinkedOwner {
    /// Fixed link immediately preceding the owner packet.
    pub link: B2Link5f,
    /// Nine-reference owner packet.
    pub owner: B2OwnerPacket,
}

/// Adjacent class-`0x5f` link and count-framed class-`0x62` owner joined by
/// the owner's allocation-successor identity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2LinkedCountedOwner {
    /// Fixed link immediately preceding the owner packet.
    pub link: B2Link5f,
    /// Count-framed owner packet.
    pub owner: B2CountedOwner,
}

/// Cone-face chart descriptor stored in a `b2/b3/b4 03 3b` record.
#[derive(Debug, Clone, PartialEq)]
#[cfg(test)]
pub struct B2ConeFace {
    /// Record byte offset.
    pub pos: usize,
    /// Compact persistent-tag references.
    pub references: Vec<u32>,
    /// Stored angular chart scale.
    pub angular_scale: f64,
    /// Cone half-angle in radians.
    pub half_angle: f64,
}

/// Settled terminal sense code in a class-`0x06` consolidated use record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B2UseSense {
    /// Terminal byte `0x84`.
    Sense84,
    /// Terminal byte `0x88`.
    Sense88,
}

/// Byte-level metadata from a class-`0x06` consolidated use record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2UseMetadata {
    /// Record byte offset.
    pub pos: usize,
    /// Complete payload bytes.
    pub payload: Vec<u8>,
    /// Compact persistent references following the `0x80+n` count and
    /// preceding a settled terminal sense. `None` when the payload does not
    /// close under that grammar.
    pub references: Option<Vec<u32>>,
    /// Decoded terminal sense when the payload ends in `0x84` or `0x88`.
    pub sense: Option<B2UseSense>,
}

/// Byte-level metadata from a class-`0x5e` consolidated record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2EdgeMetadata {
    /// Record byte offset.
    pub pos: usize,
    /// Complete payload bytes.
    pub payload: Vec<u8>,
    /// Values carried by each `0x0a <u16le>` reference token.
    pub references: Vec<u16>,
}

/// Structurally decoded width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B2EdgeNode {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token following the payload length.
    pub header_token: u32,
    /// Allocation-local curve-support reference terminating the use chain.
    pub curve_ref: u32,
    /// Native start-vertex identity.
    pub start_vertex_ref: u32,
    /// Native end-vertex identity.
    pub end_vertex_ref: u32,
    /// Allocation-local start-parameter selector.
    pub start_parameter_ref: u32,
    /// Allocation-local end-parameter selector.
    pub end_parameter_ref: u32,
    /// Terminal layout byte following the five references.
    pub tail: u8,
}

/// Decode class-`0x06` payloads and their settled terminal sense codes.
#[must_use]
pub fn b2_use_metadata(data: &[u8]) -> Vec<B2UseMetadata> {
    b_family_frames(data, 0x06)
        .into_iter()
        .map(|frame| {
            let payload = data[frame.payload..frame.end].to_vec();
            let sense = match payload.last() {
                Some(0x84) => Some(B2UseSense::Sense84),
                Some(0x88) => Some(B2UseSense::Sense88),
                _ => None,
            };
            let references = sense.and_then(|_| {
                let end = frame.end.checked_sub(1)?;
                let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
                let mut at = frame.payload + 1;
                let mut references = Vec::new();
                for _ in 0..count {
                    references.push(compact_int(data, &mut at)?);
                }
                (at == end).then_some(references)
            });
            B2UseMetadata {
                pos: frame.pos,
                payload,
                references,
                sense,
            }
        })
        .collect()
}

/// Decode class-`0x5e` payloads and their `0x0a <u16le>` reference tokens.
#[must_use]
#[cfg(test)]
pub fn b2_edge_metadata(data: &[u8]) -> Vec<B2EdgeMetadata> {
    b_family_frames(data, 0x5e)
        .into_iter()
        .map(|frame| {
            let payload = data[frame.payload..frame.end].to_vec();
            let mut references = Vec::new();
            let mut at = 0;
            while at < payload.len() {
                if payload[at] == 0x0a && at + 3 <= payload.len() {
                    references.push(u16::from_le_bytes([payload[at + 1], payload[at + 2]]));
                    at += 3;
                } else {
                    at += 1;
                }
            }
            B2EdgeMetadata {
                pos: frame.pos,
                payload,
                references,
            }
        })
        .collect()
}

/// Decode length-closed `b2/b3/b4 03 5e` records containing one compact curve
/// reference, two persistent vertex references, two compact parameter
/// references, and one terminal byte.
#[must_use]
pub fn b2_edge_nodes(data: &[u8]) -> Vec<B2EdgeNode> {
    b_family_frames(data, 0x5e)
        .into_iter()
        .filter_map(|frame| {
            let mut at = frame.payload;
            let curve_ref = compact_int(data, &mut at)?;
            let start_vertex_ref = allocation_ref(data, &mut at)?;
            let end_vertex_ref = allocation_ref(data, &mut at)?;
            let start_parameter_ref = compact_int(data, &mut at)?;
            let end_parameter_ref = compact_int(data, &mut at)?;
            let tail = *data.get(at)?;
            (at + 1 == frame.end).then_some(B2EdgeNode {
                pos: frame.pos,
                header_token: frame.header_token,
                curve_ref,
                start_vertex_ref,
                end_vertex_ref,
                start_parameter_ref,
                end_parameter_ref,
                tail,
            })
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 3b` cone-face descriptors.
#[must_use]
#[cfg(test)]
pub fn b2_cone_faces(data: &[u8]) -> Vec<B2ConeFace> {
    b_family_frames(data, 0x3b)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 || frame.end - frame.payload != 0x20 {
                return None;
            }
            let scalar_at = frame.end - 16;
            let angular_scale = f64_le(data, scalar_at)?;
            let half_angle = f64_le(data, scalar_at + 8)?;
            if !angular_scale.is_finite()
                || !(0.0..std::f64::consts::FRAC_PI_2).contains(&half_angle)
            {
                return None;
            }
            let mut at = frame.payload;
            let mut references = Vec::new();
            while at < scalar_at {
                references.push(compact_int(data, &mut at)?);
            }
            (at == scalar_at).then_some(B2ConeFace {
                pos: frame.pos,
                references,
                angular_scale,
                half_angle,
            })
        })
        .collect()
}

/// Decode `b2/b3/b4 03 37` compact reference lists with their unit tail.
#[must_use]
#[cfg(test)]
pub fn b2_reference_lists(data: &[u8]) -> Vec<B2ReferenceList> {
    b_family_frames(data, 0x37)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5
                || !matches!(frame.end - frame.payload, 0x22 | 0x24 | 0x26)
                || f64_le(data, frame.end.checked_sub(8)?)? != 1.0
            {
                return None;
            }
            let refs_end = frame.end - 8;
            let mut at = frame.payload;
            let mut references = Vec::new();
            while at < refs_end {
                references.push(compact_int(data, &mut at)?);
            }
            (at == refs_end).then_some(B2ReferenceList {
                pos: frame.pos,
                references,
            })
        })
        .collect()
}

/// Decode class-`0x62` owner packets whose leading count fixes the persistent
/// reference lane and leaves a nonempty class-specific tail.
#[must_use]
#[cfg(test)]
pub fn b2_counted_owners(data: &[u8]) -> Vec<B2CountedOwner> {
    b_family_frames(data, 0x62)
        .into_iter()
        .filter_map(|frame| {
            let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
            if count == 0 {
                return None;
            }
            let mut at = frame.payload + 1;
            let references = (0..count)
                .map(|_| persistent_ref(data, &mut at))
                .collect::<Option<Vec<_>>>()?;
            (at < frame.end).then(|| B2CountedOwner {
                pos: frame.pos,
                header_token: frame.header_token,
                references,
                tail: data[at..frame.end].to_vec(),
            })
        })
        .collect()
}

/// Decode width-coded class-`0x62` owner packets whose counted references and
/// fixed numeric tail consume the complete frame.
#[must_use]
#[cfg(test)]
pub fn b2_owner_packets(data: &[u8]) -> Vec<B2OwnerPacket> {
    b_family_frames(data, 0x62)
        .into_iter()
        .filter_map(|frame| {
            if data.get(frame.payload) != Some(&0x89) {
                return None;
            }
            let mut at = frame.payload + 1;
            let reference_encoding = if data.get(at) == Some(&0x0a) {
                B2OwnerReferenceEncoding::TaggedU16Strong
            } else {
                B2OwnerReferenceEncoding::WidthCodedStrong
            };
            let mut references = [0u32; 9];
            for (index, reference) in references.iter_mut().enumerate() {
                *reference = match (reference_encoding, index % 2) {
                    (B2OwnerReferenceEncoding::TaggedU16Strong, 0) => {
                        persistent_ref(data, &mut at)?
                    }
                    (B2OwnerReferenceEncoding::TaggedU16Strong, 1)
                    | (B2OwnerReferenceEncoding::WidthCodedStrong, 0) => {
                        compact_int(data, &mut at)?
                    }
                    (B2OwnerReferenceEncoding::WidthCodedStrong, 1) => {
                        let value = u32::from(*data.get(at)?);
                        at += 1;
                        value
                    }
                    _ => unreachable!(),
                };
            }
            let numeric_tail = data.get(at..frame.end)?.try_into().ok()?;
            Some(B2OwnerPacket {
                pos: frame.pos,
                header_token: frame.header_token,
                reference_encoding,
                references,
                numeric_tail,
            })
        })
        .collect()
}

/// Decode the count-prefixed class-`0x61` payload family. Long class-`0x61`
/// records without a leading count belong to a separate grammar and are not
/// returned.
#[must_use]
#[cfg(test)]
pub fn b2_counted_61(data: &[u8]) -> Vec<B2Counted61> {
    b_family_frames(data, 0x61)
        .into_iter()
        .filter_map(|frame| {
            let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
            if count == 0 {
                return None;
            }
            let mut at = frame.payload + 1;
            let references = (0..count)
                .map(|_| compact_int(data, &mut at))
                .collect::<Option<Vec<_>>>()?;
            let tail = data.get(at..frame.end)?;
            if tail.is_empty() || tail.last() != Some(&0x03) {
                return None;
            }
            Some(B2Counted61 {
                pos: frame.pos,
                header_token: frame.header_token,
                references,
                tail: tail.to_vec(),
            })
        })
        .collect()
}

/// Decode the long class-`0x61` form. Its fixed 25-byte suffix determines the
/// monotone member-list boundary without searching for delimiter bytes.
#[must_use]
#[cfg(test)]
pub fn b2_long_61(data: &[u8]) -> Vec<B2Long61> {
    b_family_frames(data, 0x61)
        .into_iter()
        .filter_map(|frame| {
            let payload_len = frame.end.checked_sub(frame.payload)?;
            let delimiter = frame.end.checked_sub(25)?;
            if payload_len < 36
                || data.get(frame.payload + 8) != Some(&0x06)
                || data.get(delimiter) != Some(&0xfe)
                || (delimiter - (frame.payload + 9)) % 2 != 0
                || data.get(frame.end - 1) != Some(&0x03)
            {
                return None;
            }
            let prefix = data
                .get(frame.payload..frame.payload + 8)?
                .try_into()
                .ok()?;
            let members = data[frame.payload + 9..delimiter]
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>();
            if members.is_empty() || members.windows(2).any(|pair| pair[0] >= pair[1]) {
                return None;
            }
            let mut at = delimiter + 1;
            let mut references = [0u16; 5];
            for reference in &mut references {
                if data.get(at) != Some(&0x0a) {
                    return None;
                }
                *reference = u16_le(data, at + 1)?;
                at += 3;
            }
            let scalar = f64_le(data, at)?;
            if !scalar.is_finite() || at + 9 != frame.end {
                return None;
            }
            Some(B2Long61 {
                pos: frame.pos,
                header_token: frame.header_token,
                prefix,
                members,
                references,
                scalar,
            })
        })
        .collect()
}

/// Decode `82 <width-coded target> 03 05` class-`0x5f` links.
#[must_use]
#[cfg(test)]
pub fn b2_links_5f(data: &[u8]) -> Vec<B2Link5f> {
    b_family_frames(data, 0x5f)
        .into_iter()
        .filter_map(|frame| {
            if data.get(frame.payload) != Some(&0x82) {
                return None;
            }
            let mut at = frame.payload + 1;
            let target = compact_int(data, &mut at)?;
            (at + 2 == frame.end && data.get(at..frame.end) == Some(&[0x03, 0x05])).then_some(
                B2Link5f {
                    pos: frame.pos,
                    header_token: frame.header_token,
                    target,
                },
            )
        })
        .collect()
}

/// Bind immediately adjacent `5f,62` records when the owner's ninth identity
/// is the checked successor of the link target.
#[must_use]
#[cfg(test)]
pub fn b2_linked_owners(data: &[u8]) -> Vec<B2LinkedOwner> {
    let links = b2_links_5f(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let owners = b2_owner_packets(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(2)
        .filter_map(|window| {
            let [link_record, owner_record] = window else {
                return None;
            };
            let link = links.get(&link_record.range.start)?;
            let owner = owners.get(&owner_record.range.start)?;
            (link.target.checked_add(1) == Some(owner.references[8])).then(|| B2LinkedOwner {
                link: *link,
                owner: owner.clone(),
            })
        })
        .collect()
}

/// Bind immediately adjacent `5f,62` records when the count-framed owner's
/// final identity is the checked successor of the link target.
#[must_use]
#[cfg(test)]
pub fn b2_linked_counted_owners(data: &[u8]) -> Vec<B2LinkedCountedOwner> {
    let links = b2_links_5f(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let owners = b2_counted_owners(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(2)
        .filter_map(|window| {
            let [link_record, owner_record] = window else {
                return None;
            };
            let link = links.get(&link_record.range.start)?;
            let owner = owners.get(&owner_record.range.start)?;
            (link.target.checked_add(1) == owner.references.last().copied()).then(|| {
                B2LinkedCountedOwner {
                    link: *link,
                    owner: owner.clone(),
                }
            })
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 18` parameter-space records.
#[must_use]
#[cfg(test)]
pub fn b2_parameter_points(data: &[u8]) -> Vec<B2ParameterPoint> {
    b_family_frames(data, 0x18)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 || data.get(frame.payload) != Some(&0x05) {
                return None;
            }
            let at = frame.payload + 2;
            match frame.end - frame.payload {
                0x12 => Some(B2ParameterPoint::Uv {
                    pos: frame.pos,
                    uv: read_f64_array::<2>(data, at)?,
                }),
                0x1a => {
                    let values = read_f64_array::<3>(data, at)?;
                    Some(B2ParameterPoint::StationUv {
                        pos: frame.pos,
                        station: values[0],
                        uv: [values[1], values[2]],
                    })
                }
                0x2a => Some(B2ParameterPoint::FiveScalars {
                    pos: frame.pos,
                    values: read_f64_array::<5>(data, at)?,
                }),
                _ => None,
            }
            .filter(|value| match value {
                B2ParameterPoint::Uv { uv, .. } => uv.iter().all(|v| v.is_finite()),
                B2ParameterPoint::StationUv { station, uv, .. } => {
                    station.is_finite() && uv.iter().all(|v| v.is_finite())
                }
                B2ParameterPoint::FiveScalars { values, .. } => {
                    values.iter().all(|v| v.is_finite())
                }
            })
        })
        .collect()
}

/// Decode class-`0x18` descriptors that prefix class-`0x25` edge definitions.
#[must_use]
pub fn b2_class25_descriptors(data: &[u8]) -> Vec<B2Class25Descriptor> {
    b_family_frames(data, 0x18)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 {
                return None;
            }
            let mut at = frame.payload;
            let record_id = compact_int(data, &mut at)?;
            let control = *data.get(at)?;
            at += 1;
            if !matches!(control, 0x02 | 0x0a) {
                return None;
            }
            let values = finite_f64_lane(data.get(at..frame.end)?)?;
            matches!(values.len(), 2 | 3).then_some(B2Class25Descriptor {
                pos: frame.pos,
                record_id,
                control,
                values,
            })
        })
        .collect()
}

/// Shared-edge parameter range stored in a `b2 03 23` packet.
#[derive(Debug, Clone)]
pub struct B2EdgeParameters {
    /// Record byte offset.
    pub pos: usize,
    /// Native shared-edge parameter range.
    pub range: [f64; 2],
    /// Shared-edge geometric tolerance.
    pub tolerance: f64,
}

/// Typed class-`0x18` descriptor immediately preceding a class-`0x25` edge.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Class25Descriptor {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded allocation identity.
    pub record_id: u32,
    /// Descriptor control byte (`0x02` or `0x0a`).
    pub control: u8,
    /// Complete finite scalar lane containing two or three values.
    pub values: Vec<f64>,
}

pub(crate) fn b2_cone_point(cone: &B2Cone, uv: [f64; 2]) -> Option<Point3> {
    if !(cone.slant_range[0] - 1e-6..=cone.slant_range[1] + 1e-6).contains(&uv[1]) {
        return None;
    }
    let phi = uv[0] / cone.angular_scale;
    let radial = [
        phi.cos() * cone.t1[0] + phi.sin() * cone.t2[0],
        phi.cos() * cone.t1[1] + phi.sin() * cone.t2[1],
        phi.cos() * cone.t1[2] + phi.sin() * cone.t2[2],
    ];
    let axial = cone.half_angle.cos();
    let transverse = cone.half_angle.sin();
    Some(Point3::new(
        cone.apex[0] + uv[1] * (axial * cone.axis[0] + transverse * radial[0]),
        cone.apex[1] + uv[1] * (axial * cone.axis[1] + transverse * radial[1]),
        cone.apex[2] + uv[1] * (axial * cone.axis[2] + transverse * radial[2]),
    ))
}

pub(crate) fn b2_cylinder_point(cylinder: &B2Cylinder, uv: [f64; 2]) -> Option<Point3> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = cylinder.geometry.as_ref()?
    else {
        return None;
    };
    if !(cylinder.u_range[0] - 1e-6..=cylinder.u_range[1] + 1e-6).contains(&uv[0])
        || !(cylinder.v_range[0] - 1e-6..=cylinder.v_range[1] + 1e-6).contains(&uv[1])
    {
        return None;
    }
    let angle = uv[0] / radius;
    let perpendicular = (*axis).cross(*ref_direction);
    Some(Point3::new(
        origin.x
            + uv[1] * axis.x
            + radius * (angle.cos() * ref_direction.x + angle.sin() * perpendicular.x),
        origin.y
            + uv[1] * axis.y
            + radius * (angle.cos() * ref_direction.y + angle.sin() * perpendicular.y),
        origin.z
            + uv[1] * axis.z
            + radius * (angle.cos() * ref_direction.z + angle.sin() * perpendicular.z),
    ))
}

pub(crate) fn point_distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Arc-length circle support stored in a `b2 03 19` record.
#[derive(Debug, Clone)]
pub struct B2Circle {
    /// Record byte offset.
    pub pos: usize,
    /// Compact persistent record identifier.
    pub record_id: u32,
    /// Frame token following the record length.
    pub frame_token: u8,
    /// Two center coordinates in the host-implied carrier plane.
    pub center_pair: [f64; 2],
    /// Circle radius in millimetres.
    pub radius: f64,
    /// Arc-length parameter interval.
    pub range: [f64; 2],
    /// Whether the interval spans one complete circumference.
    pub full_circle: bool,
}

/// Analytic cylinder support stored in a `b2 03 28` record.
#[derive(Debug, Clone)]
pub struct B2Cylinder {
    /// Record byte offset.
    pub pos: usize,
    /// Payload-layout discriminator (`0x52`, `0x5a`, or `0x62`).
    #[cfg(test)]
    pub layout: u8,
    /// Decoded carrier; absent for the unresolved phase-tailed `0x62` frame.
    pub geometry: Option<SurfaceGeometry>,
    /// Arc-length circumferential range.
    pub u_range: [f64; 2],
    /// Axial range.
    pub v_range: [f64; 2],
    /// Stored planar vector for a phase-tailed `0x62` frame.
    #[cfg(test)]
    pub stored_vector: Option<[f64; 2]>,
    /// Phase scalar for a phase-tailed `0x62` frame.
    #[cfg(test)]
    pub phase: Option<f64>,
}

/// Slant-coordinate cone chart stored in a `b2 03 29` record.
#[derive(Debug, Clone)]
pub struct B2Cone {
    /// Record byte offset.
    pub pos: usize,
    /// Cone apex.
    pub apex: [f64; 3],
    /// First transverse unit direction.
    pub t1: [f64; 3],
    /// Second transverse unit direction.
    pub t2: [f64; 3],
    /// Cone-axis unit direction.
    pub axis: [f64; 3],
    /// Cone half-angle in radians.
    pub half_angle: f64,
    /// Native slant-coordinate range.
    pub slant_range: [f64; 2],
    /// Divisor mapping the stored U coordinate to azimuth.
    pub angular_scale: f64,
}

/// Axis-and-profile surface of revolution stored in a `b2 03 2d` record.
#[derive(Debug, Clone)]
#[cfg(test)]
pub struct B2Revolution {
    /// Referenced profile-curve identifier.
    pub profile_curve_id: u16,
    /// Axis-frame origin.
    pub origin: [f64; 3],
    /// Revolution-axis direction.
    pub axis: [f64; 3],
    /// Stored profile parameter interval.
    pub profile_range: [f64; 2],
}

/// Constant `b2 03 65` separator preceding a typed group opener.
#[derive(Debug, Clone)]
#[cfg(test)]
pub struct B2GroupSeparator {
    /// Consolidated-frame header token.
    pub token: u32,
}

/// Typed group opener stored in a `b2 03 60` record.
#[derive(Debug, Clone)]
pub struct B2Group {
    /// Record byte offset.
    pub pos: usize,
    /// Compact group identifier.
    #[cfg(test)]
    pub group_id: u32,
    /// Compact group-type code; type `3` opens a cylinder chain.
    pub group_type: u32,
}

/// Construction-use wrapper stored in a `b2 03 30` record.
#[derive(Debug, Clone)]
pub struct B2ConstructionUse {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced support identifier.
    pub support_id: u32,
    /// Signed wall or offset scalar.
    pub distance: f64,
    /// Construction-type discriminant.
    pub kind: u8,
    /// Carrier domain `[u0, v0, u1, v1]` for kind `0x01`.
    pub domain: Option<[f64; 4]>,
}

/// Cylinder frame following a type-3 `b2 03 60` group opener.
#[derive(Debug, Clone)]
pub struct B2EmbeddedCylinder {
    /// Group-opener byte offset.
    pub wrapper_pos: usize,
    /// Embedded frame byte offset, including its varying pre-byte.
    pub pos: usize,
    /// Compact embedded object identifier.
    pub object_id: u32,
    /// Decoded `0x5a` cylinder frame.
    pub cylinder: B2Cylinder,
}

/// Decode `0x5a` cylinder frames following type-3 `b2 03 60` group openers.
#[must_use]
pub fn b2_embedded_cylinders(data: &[u8]) -> Vec<B2EmbeddedCylinder> {
    let groups = b2_groups(data);
    let mut out = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        if group.group_type != 3 {
            continue;
        }
        let wrapper_pos = group.pos;
        let end = groups
            .get(index + 1)
            .map_or(data.len(), |next| next.pos)
            .min(wrapper_pos.saturating_add(2500));
        let mut search = wrapper_pos + 3;
        while search + 3 <= end {
            let Some(relative) = data[search..end]
                .windows(3)
                .position(|bytes| bytes == [0x03, 0x28, 0x5a])
            else {
                break;
            };
            let marker = search + relative;
            search = marker + 3;
            let mut payload = marker + 3;
            let Some(object_id) = compact_int(data, &mut payload) else {
                continue;
            };
            let Some(payload_end) = payload.checked_add(90) else {
                continue;
            };
            if payload_end > end {
                continue;
            }
            let mut standalone = vec![0xb2, 0x03, 0x28, 0x5a, 0];
            standalone.extend_from_slice(&data[payload..payload_end]);
            let Some(mut cylinder) = parse_b2_cylinder(
                &standalone,
                ConsolidatedFrame {
                    pos: 0,
                    payload: 5,
                    end: 95,
                    header_token: 0,
                },
            ) else {
                continue;
            };
            cylinder.pos = marker - 1;
            out.push(B2EmbeddedCylinder {
                wrapper_pos,
                pos: marker - 1,
                object_id,
                cylinder,
            });
        }
    }
    out
}

/// Decode `b2 03 30` construction-use wrappers.
#[must_use]
pub fn b2_construction_uses(data: &[u8]) -> Vec<B2ConstructionUse> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x30) {
        let pos = frame.pos;
        let payload = frame.payload;
        if frame.header_token != 5 || data.get(payload) != Some(&0x05) {
            continue;
        }
        let (support_id, at) = match data.get(payload + 1) {
            Some(0x08) => {
                let Some(value) = u16_le(data, payload + 2) else {
                    continue;
                };
                (u32::from(value), payload + 4)
            }
            Some(0x0c) => {
                let Some(value) = u32_le_24(data, payload + 2) else {
                    continue;
                };
                (value, payload + 5)
            }
            _ => continue,
        };
        let Some(distance) = f64_le(data, at) else {
            continue;
        };
        let Some(&kind) = data.get(at + 8) else {
            continue;
        };
        let Some(fields) = read_f64_array::<4>(data, at + 9) else {
            continue;
        };
        if at + 41 != frame.end || !distance.is_finite() || fields.iter().any(|v| !v.is_finite()) {
            continue;
        }
        out.push(B2ConstructionUse {
            pos,
            support_id,
            distance,
            kind,
            domain: (kind == 0x01).then_some([fields[0], fields[2], fields[1], fields[3]]),
        });
    }
    out
}

/// Decode `b2 03 29` analytic cone charts.
#[must_use]
pub fn b2_cones(data: &[u8]) -> Vec<B2Cone> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x29) {
        let pos = frame.pos;
        let p = frame.payload;
        if frame.end - p != 0xb8 || p + 153 > frame.end {
            continue;
        }
        let Some(apex) = read_f64_array::<3>(data, p) else {
            continue;
        };
        let Some(t1) = read_f64_array::<3>(data, p + 24) else {
            continue;
        };
        let Some(t2) = read_f64_array::<3>(data, p + 48) else {
            continue;
        };
        let Some(axis) = read_f64_array::<3>(data, p + 72) else {
            continue;
        };
        let Some(half_angle) = f64_le(data, p + 96) else {
            continue;
        };
        let Some(angular_offset) = f64_le(data, p + 120) else {
            continue;
        };
        let Some(slant_range) = read_f64_array::<2>(data, p + 128) else {
            continue;
        };
        let Some(angular_scale) = f64_le(data, p + 144) else {
            continue;
        };
        let unit = |v: [f64; 3]| ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) - 1.0).abs() < 1e-9;
        if unit(t1)
            && unit(t2)
            && unit(axis)
            && (0.0..std::f64::consts::FRAC_PI_2).contains(&half_angle)
            && (0.0..1e6).contains(&angular_scale)
            && 0.0 < slant_range[0]
            && slant_range[0] < slant_range[1]
            && slant_range[1] < 1e6
            && apex
                .iter()
                .chain(&[angular_offset])
                .all(|value| value.is_finite())
        {
            out.push(B2Cone {
                pos,
                apex,
                t1,
                t2,
                axis,
                half_angle,
                slant_range,
                angular_scale,
            });
        }
    }
    out
}

/// Decode `b2 03 2d` axis-and-profile surfaces of revolution.
#[must_use]
#[cfg(test)]
pub fn b2_revolutions(data: &[u8]) -> Vec<B2Revolution> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x2d) {
        let p = frame.payload;
        if frame.end - p != 0xae
            || !matches!(data.get(p), Some(0x08 | 0x0a))
            || data.get(p + 131..p + 133) != Some(&[0x05, 0x05])
            || f64_le(data, p + 141) != Some(1.0)
            || f64_le(data, p + 149) != Some(1.0)
            || f64_le(data, p + 157) != Some(0.0)
            || data.get(p + 165) != Some(&0x01)
        {
            continue;
        }
        let Some(profile_curve_id) = u16_le(data, p + 1) else {
            continue;
        };
        let Some(axis_frame) = read_f64_array::<12>(data, p + 3) else {
            continue;
        };
        let Some(bounds) = read_f64_array::<4>(data, p + 99) else {
            continue;
        };
        let Some(angular_scale) = f64_le(data, p + 133) else {
            continue;
        };
        let Some(mean_angle_parameter) = f64_le(data, p + 166) else {
            continue;
        };
        if axis_frame
            .iter()
            .chain(&bounds)
            .chain(&[angular_scale, mean_angle_parameter])
            .any(|value| !value.is_finite())
            || angular_scale <= 0.0
            || bounds[0] / angular_scale != 0.5
            || (bounds[1] - bounds[0]) / angular_scale != std::f64::consts::TAU
            || mean_angle_parameter / angular_scale != std::f64::consts::PI + 0.5
        {
            continue;
        }
        out.push(B2Revolution {
            profile_curve_id,
            origin: axis_frame[0..3].try_into().expect("three origin values"),
            axis: axis_frame[9..12].try_into().expect("three axis values"),
            profile_range: [bounds[2], bounds[3]],
        });
    }
    out
}

/// Decode constant `b2 03 65` group separators.
#[must_use]
#[cfg(test)]
pub fn b2_group_separators(data: &[u8]) -> Vec<B2GroupSeparator> {
    b_family_frames(data, 0x65)
        .into_iter()
        .filter(|frame| data.get(frame.payload..frame.end) == Some(&[0x81, 0x03, 0x05, 0x0d]))
        .map(|frame| B2GroupSeparator {
            token: frame.header_token,
        })
        .collect()
}

/// Decode `b2 03 60` typed group openers.
#[must_use]
pub fn b2_groups(data: &[u8]) -> Vec<B2Group> {
    b_family_frames(data, 0x60)
        .into_iter()
        .filter_map(|frame| {
            let mut at = frame.payload;
            // Advances `at` past the compact group id; the value is retained
            // only for test inspection.
            let group_id = compact_int(data, &mut at)?;
            #[cfg(not(test))]
            let _ = group_id;
            let group_type = compact_int(data, &mut at)?;
            (at == frame.end).then_some(B2Group {
                pos: frame.pos,
                #[cfg(test)]
                group_id,
                group_type,
            })
        })
        .collect()
}

/// Convert a decoded B2 slant-coordinate cone chart to its equivalent IR carrier.
#[must_use]
pub fn b2_cone_geometry(cone: &B2Cone) -> SurfaceGeometry {
    let slant = cone.slant_range[0];
    let axial = slant * cone.half_angle.cos();
    SurfaceGeometry::Cone {
        origin: Point3::new(
            cone.apex[0] + axial * cone.axis[0],
            cone.apex[1] + axial * cone.axis[1],
            cone.apex[2] + axial * cone.axis[2],
        ),
        axis: Vector3::new(cone.axis[0], cone.axis[1], cone.axis[2]),
        ref_direction: Vector3::new(cone.t1[0], cone.t1[1], cone.t1[2]),
        radius: slant * cone.half_angle.sin(),
        ratio: 1.0,
        half_angle: cone.half_angle,
    }
}

/// Decode standalone `b2 03 28` analytic cylinder supports.
#[must_use]
pub fn b2_cylinders(data: &[u8]) -> Vec<B2Cylinder> {
    b_family_frames(data, 0x28)
        .into_iter()
        .filter_map(|frame| parse_b2_cylinder(data, frame))
        .collect()
}

fn parse_b2_cylinder(data: &[u8], frame: ConsolidatedFrame) -> Option<B2Cylinder> {
    let pos = frame.pos;
    let layout = u8::try_from(frame.end.checked_sub(frame.payload)?).ok()?;
    let p = frame.payload;
    let origin_values = read_f64_array::<3>(data, p)?;
    let origin = Point3::new(origin_values[0], origin_values[1], origin_values[2]);
    let frame_token = *data.get(p + 24)?;
    match layout {
        0x5a => {
            if data.get(p + 89) != Some(&0x07) {
                return None;
            }
            let vector = read_f64_array::<2>(data, p + 25)?;
            let one = f64_le(data, p + 41)?;
            let radius = f64_le(data, p + 49)?;
            let u_range = read_f64_array::<2>(data, p + 57)?;
            let v_range = read_f64_array::<2>(data, p + 73)?;
            if one != 1.0
                || !(0.0..1e6).contains(&radius)
                || (vector[0].hypot(vector[1]) - 1.0).abs() > 1e-9
                || ((u_range[1] - u_range[0]) - 2.0 * std::f64::consts::PI * radius).abs() > 1e-6
            {
                return None;
            }
            let axis = match frame_token {
                0x19 => Vector3::new(vector[0], vector[1], 0.0),
                0x1c => Vector3::new(vector[1], -vector[0], 0.0),
                _ => return None,
            };
            let ref_direction = Vector3::new(-axis.y, axis.x, 0.0);
            Some(B2Cylinder {
                pos,
                #[cfg(test)]
                layout,
                geometry: Some(SurfaceGeometry::Cylinder {
                    origin,
                    axis,
                    ref_direction,
                    radius,
                }),
                u_range,
                v_range,
                #[cfg(test)]
                stored_vector: None,
                #[cfg(test)]
                phase: None,
            })
        }
        0x52 => {
            if frame_token != 0x1d
                || f64_le(data, p + 25)? != 1.0
                || f64_le(data, p + 33)? != 1.0
                || data.get(p + 81) != Some(&0x07)
            {
                return None;
            }
            let radius = f64_le(data, p + 41)?;
            let u_range = read_f64_array::<2>(data, p + 49)?;
            let v_range = read_f64_array::<2>(data, p + 65)?;
            if !(0.0..1e6).contains(&radius)
                || ((u_range[1] - u_range[0]) - 2.0 * std::f64::consts::PI * radius).abs() > 1e-6
            {
                return None;
            }
            Some(B2Cylinder {
                pos,
                #[cfg(test)]
                layout,
                geometry: Some(SurfaceGeometry::Cylinder {
                    origin,
                    axis: Vector3::new(1.0, 0.0, 0.0),
                    ref_direction: Vector3::new(0.0, 1.0, 0.0),
                    radius,
                }),
                u_range,
                v_range,
                #[cfg(test)]
                stored_vector: None,
                #[cfg(test)]
                phase: None,
            })
        }
        0x62 if frame_token == 0x0e && data.get(p + 89) == Some(&0x03) => {
            let vector = read_f64_array::<2>(data, p + 25)?;
            let one = f64_le(data, p + 41)?;
            let radius = f64_le(data, p + 49)?;
            let u_range = read_f64_array::<2>(data, p + 57)?;
            let v_range = read_f64_array::<2>(data, p + 73)?;
            let phase = f64_le(data, p + 90)?;
            if one != 1.0
                || !(0.0..1e6).contains(&radius)
                || origin_values.iter().any(|value| !value.is_finite())
                || vector.iter().any(|value| !value.is_finite())
                || u_range.iter().any(|value| !value.is_finite())
                || v_range.iter().any(|value| !value.is_finite())
                || (vector[0].hypot(vector[1]) - 1.0).abs() > 1e-9
                || !phase.is_finite()
                || u_range[0] >= u_range[1]
                || v_range[0] >= v_range[1]
                || u_range[1] - u_range[0] > 2.0 * std::f64::consts::PI * radius + 1e-6
            {
                return None;
            }
            Some(B2Cylinder {
                pos,
                #[cfg(test)]
                layout,
                geometry: None,
                u_range,
                v_range,
                #[cfg(test)]
                stored_vector: Some(vector),
                #[cfg(test)]
                phase: Some(phase),
            })
        }
        _ => None,
    }
}

/// Decode `b2 03 19` arc-length circle supports.
#[must_use]
pub fn b2_circles(data: &[u8]) -> Vec<B2Circle> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x19) {
        let pos = frame.pos;
        if !(0x32..=0x34).contains(&(frame.end - frame.payload)) {
            continue;
        }
        let Ok(frame_token) = u8::try_from(frame.header_token) else {
            continue;
        };
        let mut at = frame.payload;
        let Some(record_id) = compact_int(data, &mut at) else {
            continue;
        };
        let Some(values) = read_f64_array::<5>(data, at) else {
            continue;
        };
        let [c1, c2, radius, lo, hi] = values;
        if values.iter().all(|v| v.is_finite())
            && (0.0..1e6).contains(&radius)
            && c1.abs() <= 1e6
            && c2.abs() <= 1e6
            && hi > lo
        {
            out.push(B2Circle {
                pos,
                record_id,
                frame_token,
                center_pair: [c1, c2],
                radius,
                range: [lo, hi],
                full_circle: ((hi - lo) - 2.0 * std::f64::consts::PI * radius).abs() < 1e-9,
            });
        }
    }
    out
}

/// Decode structurally repeated `b2 03 23` edge-range packets.
#[must_use]
pub fn b2_edge_parameters(data: &[u8]) -> Vec<B2EdgeParameters> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x23) {
        let pos = frame.pos;
        if frame.end - frame.payload != 0x4e {
            continue;
        }
        let Some(values) = read_f64_array::<9>(data, frame.payload + 6) else {
            continue;
        };
        if values.iter().all(|v| v.is_finite())
            && values[0] == values[3]
            && values[0] == values[6]
            && values[1] == values[4]
            && values[1] == values[7]
            && values[5] == 1.0
            && values[2] == values[8]
        {
            out.push(B2EdgeParameters {
                pos,
                range: [values[0], values[1]],
                tolerance: values[2],
            });
        }
    }
    out
}

/// Decode `b2 03 31` offset-surface constructors.
#[must_use]
pub fn b2_offset_supports(data: &[u8]) -> Vec<B2OffsetSupport> {
    let mut offsets = b_family_frames(data, 0x31)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 {
                return None;
            }
            let length = frame.end - frame.payload;
            let (support_id, at) = match data.get(frame.payload) {
                Some(0x08) if length == 0x2b => (
                    u32::from(u16_le(data, frame.payload + 1)?),
                    frame.payload + 3,
                ),
                Some(0x0c) if length == 0x2c => {
                    (u32_le_24(data, frame.payload + 1)?, frame.payload + 4)
                }
                _ => return None,
            };
            let values = read_f64_array::<5>(data, at)?;
            values
                .iter()
                .all(|v| v.is_finite())
                .then_some(B2OffsetSupport {
                    pos: frame.pos,
                    support_id,
                    distance: values[0],
                    domain: [values[1], values[2], values[3], values[4]],
                })
        })
        .collect::<Vec<_>>();
    offsets.extend(
        b2_construction_uses(data)
            .into_iter()
            .filter_map(|construction| {
                if construction.kind != 0x01 {
                    return None;
                }
                Some(B2OffsetSupport {
                    pos: construction.pos,
                    support_id: construction.support_id,
                    distance: construction.distance,
                    domain: construction.domain?,
                })
            }),
    );
    offsets.sort_unstable_by_key(|offset| offset.pos);
    offsets
}

/// Bind each offset constructor to the unique consolidated NURBS carrier whose
/// parameter domain contains the offset box and whose V-knot lane contains both
/// serialized V limits.
#[must_use]
pub fn offset_support_carriers(
    offsets: &[B2OffsetSupport],
    carriers: &[A8Surface],
) -> Vec<Option<usize>> {
    const PARAMETER_TOLERANCE: f64 = 1e-3;
    offsets
        .iter()
        .map(|offset| {
            let [u0, v0, u1, v1] = offset.domain;
            let candidates = carriers
                .iter()
                .enumerate()
                .filter_map(|(index, carrier)| {
                    let SurfaceGeometry::Nurbs(surface) = &carrier.geometry else {
                        return None;
                    };
                    let u_min = *surface.u_knots.first()?;
                    let u_max = *surface.u_knots.last()?;
                    let v_min = *surface.v_knots.first()?;
                    let v_max = *surface.v_knots.last()?;
                    let contains = u0 >= u_min - PARAMETER_TOLERANCE
                        && u1 <= u_max + PARAMETER_TOLERANCE
                        && v0 >= v_min - PARAMETER_TOLERANCE
                        && v1 <= v_max + PARAMETER_TOLERANCE;
                    let has_v_limit = |limit: f64| {
                        surface
                            .v_knots
                            .iter()
                            .any(|knot| (*knot - limit).abs() <= PARAMETER_TOLERANCE)
                    };
                    (contains && has_v_limit(v0) && has_v_limit(v1)).then_some(index)
                })
                .collect::<Vec<_>>();
            <[usize; 1]>::try_from(candidates).ok().map(|[index]| index)
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 20` consolidated UV jets.
#[must_use]
pub fn b2_pcurves(data: &[u8]) -> Vec<ConsolidatedPcurve> {
    b_family_frames(data, 0x20)
        .into_iter()
        .filter_map(|frame| parse_consolidated_pcurve(data, frame.pos, frame.payload, frame.end))
        .collect()
}
