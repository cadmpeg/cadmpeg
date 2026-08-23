// SPDX-License-Identifier: Apache-2.0
//! Parasolid XT BODY, SHELL, REGION, and FACE nodes.
//!
//! The ordinary B-rep records in [`super::topology`] are compact `SolidWorks`
//! views of the same topology.  This module reads the ownership nodes that
//! carry the authoritative body kind and shell/region links.  It deliberately
//! keeps the parser separate from the compact topology scanner: a byte pair
//! equal to a node tag is not a node until its complete variable-width field
//! grammar and ownership invariants pass.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::BodyKind;

const BODY_TAG: [u8; 2] = [0x00, 0x0c];
const SHELL_TAG: [u8; 2] = [0x00, 0x0d];
const FACE_TAG: [u8; 2] = [0x00, 0x0e];
const REGION_TAG: [u8; 2] = [0x00, 0x13];

const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];
const RESOLUTION_SIZE_MIN: f64 = 1.0;
const RESOLUTION_SIZE_MAX: f64 = 1.0e6;
const RESOLUTION_LINEAR_MIN: f64 = 1.0e-15;
const RESOLUTION_LINEAR_MAX: f64 = 1.0e-2;

/// A typed BODY node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyNode {
    /// Stream-local transmit index.
    pub attr: u16,
    /// Persistent XT node id.
    pub node_id: u32,
    /// The seven trailing topology references, beginning with `shell`.
    /// Their region-chain slot is carried by `region_head_slot`; the remaining
    /// slots are the wire, vertex, and other topology heads for the BODY form.
    pub topology_refs: [u32; 7],
    /// Stored Parasolid body kind discriminator.
    pub body_type: u8,
    /// Slot containing the region-chain head in this BODY schema.
    pub region_head_slot: u8,
    /// Byte offset of the node payload.  The first BODY has no repeated tag.
    pub offset: usize,
    /// First byte after the complete node.
    pub end: usize,
}

impl BodyNode {
    /// Map the stored Parasolid discriminator to the neutral body kind.
    pub fn kind(&self) -> Option<BodyKind> {
        match self.body_type {
            1 => Some(BodyKind::Solid),
            2 => Some(BodyKind::Wire),
            3 => Some(BodyKind::Sheet),
            6 => Some(BodyKind::General),
            _ => None,
        }
    }

    /// First shell reference in the body topology fields.
    pub fn shell(&self) -> u32 {
        self.topology_refs[0]
    }

    /// Head of the body's region chain.
    pub fn region_head(&self) -> u32 {
        self.topology_refs[usize::from(self.region_head_slot)]
    }
}

/// A typed SHELL node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNode {
    /// Stream-local transmit index.
    pub attr: u16,
    /// Persistent XT node id.
    pub node_id: u32,
    /// `[attribute_chain, body, next, back_face, edge, vertex, region, front_face]`.
    pub refs: [u32; 8],
    /// Byte offset of the node tag.
    pub offset: usize,
    /// First byte after the complete node.
    pub end: usize,
}

/// A typed REGION node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionNode {
    /// Stream-local transmit index.
    pub attr: u16,
    /// Persistent XT node id.
    pub node_id: u32,
    /// `[attribute_chain, body, next, previous, shell_head]`.
    pub refs: [u32; 5],
    /// `S` for solid and `V` for void.
    pub kind: u8,
    /// Byte offset of the node tag.
    pub offset: usize,
    /// First byte after the complete node.
    pub end: usize,
}

/// A typed FACE node.  The compact bridge parser uses the same attribute and
/// node-id prefix, so `attr` is the bridge key used by the graph decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceNode {
    /// Stream-local transmit index.
    pub attr: u16,
    /// Persistent XT node id.
    pub node_id: u32,
    /// Attribute-chain head.
    pub attribute_chain: u32,
    /// `[next_face, previous_face, loop, shell, surface]`.
    pub refs: [u32; 5],
    /// Stored face sense marker.
    pub sense: u8,
    /// Byte offset of the node tag.
    pub offset: usize,
    /// First byte after the complete node.
    pub end: usize,
}

/// All typed ownership nodes recovered from one stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub bodies: Vec<BodyNode>,
    pub shells: Vec<ShellNode>,
    pub regions: Vec<RegionNode>,
    pub faces: Vec<FaceNode>,
}

impl Facts {
    /// Add only identities absent from the partition view.  A delta stream is
    /// subordinate to the partition for the same transmit index.
    pub fn merge_missing(&mut self, other: Self) {
        merge_nodes(&mut self.bodies, other.bodies, |node| node.attr);
        merge_nodes(&mut self.shells, other.shells, |node| node.attr);
        merge_nodes(&mut self.regions, other.regions, |node| node.attr);
        merge_nodes(&mut self.faces, other.faces, |node| node.attr);
    }

    /// Return body hierarchies only when the typed ownership graph is complete
    /// for the caller's compact face set.
    pub fn hierarchies(&self, bridge_attrs: &HashSet<u16>) -> Option<Vec<Hierarchy>> {
        let bodies = unique_map(&self.bodies, |node| node.attr)?;
        let regions = unique_map(&self.regions, |node| node.attr)?;
        let faces = if bridge_attrs.is_empty() {
            unique_map(&self.faces, |node| node.attr)?
        } else {
            unique_map_for_attrs(&self.faces, bridge_attrs, |node| node.attr)?
        };
        if bodies.is_empty() {
            return None;
        }

        let mut face_shells = HashMap::<u16, u16>::new();
        for attr in bridge_attrs {
            let face = faces.get(attr)?;
            let shell = u16_from_ref(face.refs[3])?;
            if face_shells.insert(*attr, shell).is_some() {
                return None;
            }
        }
        let shell_attrs = face_shells.values().copied().collect::<HashSet<_>>();
        let shells = if bridge_attrs.is_empty() {
            unique_map(&self.shells, |node| node.attr)?
        } else {
            unique_map_for_attrs(&self.shells, &shell_attrs, |node| node.attr)?
        };
        if face_shells
            .values()
            .any(|shell_attr| !shells.contains_key(shell_attr))
        {
            return None;
        }
        let mut relevant_shells = HashSet::new();
        let mut relevant_regions_by_body = HashMap::<u16, HashSet<u16>>::new();
        let mut relevant_bodies = HashSet::new();
        for shell_attr in face_shells.values().copied() {
            if !relevant_shells.insert(shell_attr) {
                continue;
            }
            let shell = shells.get(&shell_attr)?;
            let region = u16_from_ref(shell.refs[6])?;
            if region <= 1 || !regions.contains_key(&region) {
                return None;
            }
            let region_node = regions.get(&region)?;
            let region_body = u16_from_ref(region_node.refs[1])?;
            if region_body <= 1 || !bodies.contains_key(&region_body) {
                return None;
            }
            relevant_bodies.insert(region_body);
            relevant_regions_by_body
                .entry(region_body)
                .or_default()
                .insert(region);
            if shell.refs[1] > 1 {
                let shell_body = u16_from_ref(shell.refs[1])?;
                if shell_body != region_body {
                    return None;
                }
            }
        }
        if bridge_attrs.is_empty() {
            relevant_bodies.extend(bodies.keys().copied());
        }

        let mut out = Vec::new();
        let mut assigned_faces = HashSet::new();
        for body_attr in relevant_bodies {
            let body = bodies.get(&body_attr)?;
            let kind = body.kind()?;
            let body_attr = body.attr;
            if !null_like_or_existing(body.shell(), &shells) {
                return None;
            }

            let body_regions = region_chain(body, &regions)?;
            let body_region_attrs = body_regions
                .iter()
                .map(|region| region.attr)
                .collect::<HashSet<_>>();
            if !bridge_attrs.is_empty()
                && !relevant_regions_by_body
                    .get(&body_attr)
                    .is_some_and(|regions| regions.is_subset(&body_region_attrs))
            {
                return None;
            }
            let body_shells = shells
                .values()
                .filter(|shell| {
                    (bridge_attrs.is_empty() || relevant_shells.contains(&shell.attr))
                        && body_region_attrs
                            .contains(&u16_from_ref_or_none(shell.refs[6]).unwrap_or(0))
                        && (shell.refs[1] == u32::from(body_attr) || shell.refs[1] <= 1)
                })
                .cloned()
                .collect::<Vec<_>>();

            let mut hierarchy_faces = Vec::new();
            for (face_attr, shell_attr) in &face_shells {
                let Some(shell) = body_shells.iter().find(|shell| shell.attr == *shell_attr) else {
                    continue;
                };
                if !assigned_faces.insert(*face_attr) {
                    return None;
                }
                hierarchy_faces.push((*face_attr, shell.attr));
            }

            out.push(Hierarchy {
                body: body.clone(),
                kind,
                regions: body_regions,
                shells: body_shells,
                faces: hierarchy_faces,
            });
        }
        if assigned_faces != *bridge_attrs {
            return None;
        }
        Some(out)
    }
}

fn region_chain(body: &BodyNode, regions: &HashMap<u16, RegionNode>) -> Option<Vec<RegionNode>> {
    let head = body.region_head();
    if head <= 1 {
        return Some(Vec::new());
    }

    // A BODY may store either the first REGION attribute or the predecessor
    // sentinel used by the REGION doubly-linked list.  Both forms carry the
    // same closure: the first region's previous reference is the stored head
    // in the sentinel form, and each later region points back to its source.
    let (mut next, first_previous) =
        if let Some(attr) = u16_from_ref(head).filter(|attr| regions.contains_key(attr)) {
            (attr, None)
        } else {
            let mut candidates = regions
                .values()
                .filter(|region| region.refs[3] == head)
                .map(|region| region.attr);
            let attr = candidates.next()?;
            if candidates.next().is_some() {
                return None;
            }
            (attr, Some(head))
        };

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut expected_previous = first_previous;
    loop {
        if !seen.insert(next) {
            return None;
        }
        let region = regions.get(&next)?.clone();
        if region.refs[1] != u32::from(body.attr)
            || expected_previous.is_some_and(|previous| region.refs[3] != previous)
        {
            return None;
        }
        let following = region.refs[2];
        out.push(region.clone());
        if following <= 1 {
            break;
        }
        let following_attr = u16_from_ref(following)?;
        let following_region = regions.get(&following_attr)?;
        if following_region.refs[3] != u32::from(region.attr) {
            return None;
        }
        expected_previous = Some(u32::from(region.attr));
        next = following_attr;
    }
    Some(out)
}

/// One validated typed body hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hierarchy {
    pub body: BodyNode,
    pub kind: BodyKind,
    pub regions: Vec<RegionNode>,
    pub shells: Vec<ShellNode>,
    /// `(face bridge attr, owning shell attr)` pairs.
    pub faces: Vec<(u16, u16)>,
}

fn merge_nodes<T, F>(target: &mut Vec<T>, source: Vec<T>, key: F)
where
    F: Fn(&T) -> u16,
{
    let present = target.iter().map(&key).collect::<HashSet<_>>();
    target.extend(
        source
            .into_iter()
            .filter(|node| !present.contains(&key(node))),
    );
}

fn unique_map<T, F>(nodes: &[T], key: F) -> Option<HashMap<u16, T>>
where
    T: Clone,
    F: Fn(&T) -> u16,
{
    let mut out = HashMap::new();
    for node in nodes {
        let attr = key(node);
        if out.insert(attr, node.clone()).is_some() {
            return None;
        }
    }
    Some(out)
}

fn unique_map_for_attrs<T, F>(nodes: &[T], attrs: &HashSet<u16>, key: F) -> Option<HashMap<u16, T>>
where
    T: Clone,
    F: Fn(&T) -> u16,
{
    let mut out = HashMap::new();
    for node in nodes {
        let attr = key(node);
        if !attrs.contains(&attr) {
            continue;
        }
        if out.insert(attr, node.clone()).is_some() {
            return None;
        }
    }
    if attrs.iter().any(|attr| !out.contains_key(attr)) {
        None
    } else {
        Some(out)
    }
}

fn u16_from_ref(value: u32) -> Option<u16> {
    u16::try_from(value).ok()
}

fn u16_from_ref_or_none(value: u32) -> Option<u16> {
    u16_from_ref(value).filter(|value| *value > 1)
}

fn null_like_or_existing<T>(value: u32, nodes: &HashMap<u16, T>) -> bool {
    value <= 1 || u16_from_ref(value).is_some_and(|value| nodes.contains_key(&value))
}

fn read_ref(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let first = View::u16_be_at(bytes, *at)?;
    *at = at.checked_add(2)?;
    if first <= 0x7ffe {
        return Some(u32::from(first));
    }
    let second = View::u16_be_at(bytes, *at)?;
    *at = at.checked_add(2)?;
    Some(u32::from(first & 0x7fff) | (u32::from(second) << 15))
}

fn read_refs<const N: usize>(bytes: &[u8], at: &mut usize) -> Option<[u32; N]> {
    let mut refs = [0; N];
    for reference in &mut refs {
        *reference = read_ref(bytes, at)?;
    }
    Some(refs)
}

fn read_prefix(bytes: &[u8], at: usize, tag: [u8; 2]) -> Option<(usize, u16, u32)> {
    if bytes.get(at..at + 2) != Some(&tag) {
        return None;
    }
    let mut cursor = at + 2;
    if bytes.get(cursor) == Some(&0xff) {
        cursor += 1;
    }
    let attr = View::u16_be_at(bytes, cursor)?;
    let node_id = View::u32_be_at(bytes, cursor + 2)?;
    (attr > 1 && node_id != 0).then_some(())?;
    Some((cursor + 6, attr, node_id))
}

fn valid_resolution(size: f64, linear: f64) -> bool {
    size.is_finite()
        && linear.is_finite()
        && (RESOLUTION_SIZE_MIN..=RESOLUTION_SIZE_MAX).contains(&size)
        && (RESOLUTION_LINEAR_MIN..=RESOLUTION_LINEAR_MAX).contains(&linear.abs())
}

fn parse_body_fields<const N: usize>(
    bytes: &[u8],
    offset: usize,
    payload: usize,
    mut at: usize,
) -> Option<BodyNode> {
    let attr = View::u16_be_at(bytes, payload)?;
    let node_id = View::u32_be_at(bytes, payload + 2)?;
    (attr > 1 && node_id != 0).then_some(())?;
    let _header_refs = read_refs::<N>(bytes, &mut at)?;
    let size = View::f64_be_at(bytes, at)?;
    at += 8;
    let linear = View::f64_be_at(bytes, at)?;
    at += 8;
    let _body_links = read_refs::<3>(bytes, &mut at)?;
    if bytes.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    let _owner = read_ref(bytes, &mut at)?;
    let body_type = *bytes.get(at)?;
    at += 1;
    let _nominal_geometry_state = *bytes.get(at)?;
    at += 1;
    let topology_refs = read_refs::<7>(bytes, &mut at)?;
    valid_resolution(size, linear).then_some(())?;
    let region_head_slot = if N == 4 { 4 } else { 6 };
    let body = BodyNode {
        attr,
        node_id,
        topology_refs,
        body_type,
        region_head_slot,
        offset,
        end: at,
    };
    body.kind().map(|_| body)
}

fn parse_body_layout(bytes: &[u8], offset: usize, payload: usize) -> Option<BodyNode> {
    let frame = payload.checked_add(6)?;
    let length = View::u16_be_at(bytes, frame)?;
    let node_index = View::u16_be_at(bytes, frame + 2)?;
    if length == 0 || node_index == 0 {
        return None;
    }
    let fields = frame.checked_add(4)?;
    // BODY records use both four-reference and six-reference headers.  They
    // share the framing and the remainder of the field grammar; only one
    // interpretation may pass the complete body invariants.
    let four = parse_body_fields::<4>(bytes, offset, payload, fields);
    let six = parse_body_fields::<6>(bytes, offset, payload, fields);
    match (four, six) {
        (Some(_), Some(_)) | (None, None) => None,
        (Some(body), None) | (None, Some(body)) => Some(body),
    }
}

fn parse_body(bytes: &[u8], offset: usize, payload: usize) -> Option<BodyNode> {
    parse_body_layout(bytes, offset, payload)
}

fn parse_tagged_body(bytes: &[u8], offset: usize) -> Option<BodyNode> {
    let (payload, _, _) = read_prefix(bytes, offset, BODY_TAG)?;
    parse_body_layout(bytes, offset, payload.checked_sub(6)?)
}

fn parse_shell(bytes: &[u8], offset: usize) -> Option<ShellNode> {
    let (payload, attr, node_id) = read_prefix(bytes, offset, SHELL_TAG)?;
    let mut at = payload;
    let refs = read_refs::<8>(bytes, &mut at)?;
    (refs[6] > 1).then_some(())?;
    Some(ShellNode {
        attr,
        node_id,
        refs,
        offset,
        end: at,
    })
}

fn parse_region(bytes: &[u8], offset: usize) -> Option<RegionNode> {
    let (payload, attr, node_id) = read_prefix(bytes, offset, REGION_TAG)?;
    let mut at = payload;
    let refs = read_refs::<5>(bytes, &mut at)?;
    let kind = *bytes.get(at)?;
    if !matches!(kind, b'S' | b'V') || refs[1] <= 1 {
        return None;
    }
    Some(RegionNode {
        attr,
        node_id,
        refs,
        kind,
        offset,
        end: at + 1,
    })
}

fn parse_face(bytes: &[u8], offset: usize) -> Option<FaceNode> {
    let (payload, attr, node_id) = read_prefix(bytes, offset, FACE_TAG)?;
    let mut at = payload;
    let attribute_chain = read_ref(bytes, &mut at)?;
    let tolerance = bytes.get(at..at + 8)?;
    let tolerance_is_sentinel = tolerance == MAGIC;
    let tolerance_is_finite =
        View::f64_be_at(bytes, at).is_some_and(|value| value.is_finite() && value >= 0.0);
    if !tolerance_is_sentinel && !tolerance_is_finite {
        return None;
    }
    at += 8;
    let refs = read_refs::<5>(bytes, &mut at)?;
    let sense = *bytes.get(at)?;
    if !matches!(sense, 0x2b | 0x2d) || refs[3] <= 1 {
        return None;
    }
    Some(FaceNode {
        attr,
        node_id,
        attribute_chain,
        refs,
        sense,
        offset,
        end: at + 1,
    })
}

fn body_schema_bodies(bytes: &[u8], offset: usize) -> Vec<BodyNode> {
    let end = bytes.len();
    let mut bodies = Vec::new();
    let mut z = offset + 2;
    while z < end {
        if bytes[z] == b'Z' {
            let body = parse_body(bytes, z + 1, z + 1);
            if let Some(body) = body {
                let has_edit = bytes.get(offset + 2..z).is_some_and(|edit| {
                    edit.iter()
                        .any(|byte| matches!(byte, b'C' | b'D' | b'I' | b'A'))
                });
                if has_edit {
                    bodies.push(body);
                }
            }
        }
        z += 1;
    }
    bodies
}

/// Scan one partition-style stream for strictly framed typed ownership nodes.
pub fn scan(bytes: &[u8]) -> Facts {
    let mut facts = Facts::default();
    let mut offsets = HashSet::new();
    for body in body_schema_bodies(bytes, 0) {
        offsets.insert(body.offset);
        facts.bodies.push(body);
    }
    for offset in 0..bytes.len().saturating_sub(2) {
        if bytes.get(offset..offset + 2) == Some(&BODY_TAG) {
            if let Some(body) = parse_tagged_body(bytes, offset) {
                if offsets.insert(body.offset) {
                    facts.bodies.push(body);
                }
            }
        }
        if let Some(shell) = parse_shell(bytes, offset) {
            if offsets.insert(shell.offset) {
                facts.shells.push(shell);
            }
        }
        if let Some(region) = parse_region(bytes, offset) {
            if offsets.insert(region.offset) {
                facts.regions.push(region);
            }
        }
        if let Some(face) = parse_face(bytes, offset) {
            if offsets.insert(face.offset) {
                facts.faces.push(face);
            }
        }
    }
    facts.bodies.sort_by_key(|node| node.offset);
    facts.shells.sort_by_key(|node| node.offset);
    facts.regions.sort_by_key(|node| node.offset);
    facts.faces.sort_by_key(|node| node.offset);
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_ref(bytes: &mut Vec<u8>, value: u32) {
        if value <= 0x7ffe {
            bytes.extend_from_slice(&(value as u16).to_be_bytes());
        } else {
            bytes.extend_from_slice(&((0x8000 | (value as u16 & 0x7fff)).to_be_bytes()));
            bytes.extend_from_slice(&((value >> 15) as u16).to_be_bytes());
        }
    }

    fn body_fields<const N: usize>(
        length: u16,
        node_index: u16,
        header: [u32; N],
        body_type: u8,
        topology: [u32; 7],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&node_index.to_be_bytes());
        for value in header {
            push_ref(&mut bytes, value);
        }
        bytes.extend_from_slice(&1000.0f64.to_be_bytes());
        bytes.extend_from_slice(&1.0e-8f64.to_be_bytes());
        for value in [1, 1, 1] {
            push_ref(&mut bytes, value);
        }
        bytes.push(1);
        push_ref(&mut bytes, 2);
        bytes.push(body_type);
        bytes.push(1);
        for value in topology {
            push_ref(&mut bytes, value);
        }
        bytes
    }

    fn body_node_with_topology(
        attr: u16,
        node_id: u32,
        body_type: u8,
        topology: [u32; 7],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&node_id.to_be_bytes());
        bytes.extend(body_fields::<6>(
            6,
            7,
            [5, 6, 1, 1, 1, 1],
            body_type,
            topology,
        ));
        bytes
    }

    fn body_node(attr: u16, node_id: u32, body_type: u8) -> Vec<u8> {
        body_node_with_topology(attr, node_id, body_type, [7, 8, 9, 10, 1, 12, 11])
    }

    fn tagged_four_ref_body(attr: u16, node_id: u32) -> Vec<u8> {
        let mut bytes = typed_prefix(BODY_TAG, attr, node_id);
        bytes.extend(body_fields::<4>(
            38,
            39,
            [40, 41, 42, 43],
            1,
            [44, 45, 46, 47, 48, 49, 50],
        ));
        bytes
    }

    fn typed_prefix(tag: [u8; 2], attr: u16, node_id: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&tag);
        bytes.push(0xff);
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&node_id.to_be_bytes());
        bytes
    }

    fn typed_shell(attr: u16, node_id: u32, region: u32) -> Vec<u8> {
        let mut bytes = typed_prefix(SHELL_TAG, attr, node_id);
        for value in [1, 3, 1, 38, 1, 1, region, 1] {
            push_ref(&mut bytes, value);
        }
        bytes
    }

    fn typed_region(attr: u16, node_id: u32, refs: [u32; 5], kind: u8) -> Vec<u8> {
        let mut bytes = typed_prefix(REGION_TAG, attr, node_id);
        for value in refs {
            push_ref(&mut bytes, value);
        }
        bytes.push(kind);
        bytes
    }

    fn typed_face(attr: u16, node_id: u32, refs: [u32; 5]) -> Vec<u8> {
        let mut bytes = typed_prefix(FACE_TAG, attr, node_id);
        push_ref(&mut bytes, 1);
        bytes.extend_from_slice(&MAGIC);
        for value in refs {
            push_ref(&mut bytes, value);
        }
        bytes.push(0x2b);
        bytes
    }

    #[test]
    fn extended_reference_decodes_at_the_boundary() {
        for value in [0, 1, 0x7ffe, 0x7fff, 0x8000, 0x1_0000] {
            let mut bytes = Vec::new();
            push_ref(&mut bytes, value);
            let mut at = 0;
            assert_eq!(read_ref(&bytes, &mut at), Some(value));
            assert_eq!(at, if value <= 0x7ffe { 2 } else { 4 });
        }
    }

    #[test]
    fn first_body_follows_schema_terminator() {
        let mut bytes = vec![0, 0x0c, 0x1b];
        bytes.extend_from_slice(b"CCCCA");
        bytes.push(b'Z');
        bytes.extend(body_node(3, 0x18b9, 1));
        let facts = scan(&bytes);
        assert_eq!(facts.bodies.len(), 1);
        assert_eq!(facts.bodies[0].attr, 3);
        assert_eq!(facts.bodies[0].kind(), Some(BodyKind::Solid));
    }

    #[test]
    fn body_kind_is_stored_not_inferred() {
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node(3, 7, 3));
        let facts = scan(&bytes);
        assert_eq!(facts.bodies[0].kind(), Some(BodyKind::Sheet));
    }

    #[test]
    fn tagged_body_accepts_the_four_reference_header_form() {
        let facts = scan(&tagged_four_ref_body(7, 0x18b9));
        assert_eq!(facts.bodies.len(), 1);
        assert_eq!(facts.bodies[0].attr, 7);
        assert_eq!(facts.bodies[0].region_head_slot, 4);
        assert_eq!(facts.bodies[0].region_head(), 48);
        assert_eq!(facts.bodies[0].kind(), Some(BodyKind::Solid));
    }

    #[test]
    fn extended_body_references_are_decoded() {
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node_with_topology(
            3,
            7,
            1,
            [7, 8, 0x8000, 10, 11, 12, 13],
        ));
        let facts = scan(&bytes);
        assert_eq!(facts.bodies.len(), 1);
        assert_eq!(
            facts.bodies[0].topology_refs,
            [7, 8, 0x8000, 10, 11, 12, 13]
        );
    }

    #[test]
    fn typed_hierarchy_uses_previous_region_and_shell_owner() {
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node(3, 7, 1));
        bytes.extend(typed_shell(7, 814, 39));
        bytes.extend(typed_region(11, 244, [1, 3, 39, 1, 44], b'V'));
        bytes.extend(typed_region(39, 815, [1, 3, 1, 11, 7], b'S'));
        bytes.extend(typed_face(100, 900, [1, 1, 49, 7, 8]));

        let facts = scan(&bytes);
        let hierarchy = facts
            .hierarchies(&HashSet::from([100]))
            .expect("closed typed hierarchy");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].kind, BodyKind::Solid);
        assert_eq!(
            hierarchy[0]
                .regions
                .iter()
                .map(|region| region.attr)
                .collect::<Vec<_>>(),
            vec![11, 39]
        );
        assert_eq!(hierarchy[0].faces.as_slice(), &[(100, 7)]);
    }

    #[test]
    fn region_chain_accepts_body_predecessor_sentinel() {
        let body = BodyNode {
            attr: 3,
            node_id: 7,
            topology_refs: [7, 1, 1, 1, 10, 1, 1],
            body_type: 1,
            region_head_slot: 4,
            offset: 1,
            end: 2,
        };
        let regions = HashMap::from([(
            35,
            RegionNode {
                attr: 35,
                node_id: 52,
                refs: [1, 3, 1, 10, 7],
                kind: b'S',
                offset: 3,
                end: 4,
            },
        )]);
        let chain = region_chain(&body, &regions).expect("sentinel-linked region");
        assert_eq!(
            chain.iter().map(|region| region.attr).collect::<Vec<_>>(),
            vec![35]
        );
    }

    #[test]
    fn malformed_body_kind_is_withheld() {
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node(3, 7, 4));
        assert!(scan(&bytes).bodies.is_empty());
    }

    #[test]
    fn shell_links_are_not_limited_to_sentinel_payloads() {
        let mut bytes = typed_prefix(SHELL_TAG, 7, 814);
        for value in [9, 3, 8, 38, 42, 43, 39, 44] {
            push_ref(&mut bytes, value);
        }
        let facts = scan(&bytes);
        assert_eq!(facts.shells.len(), 1);
        assert_eq!(facts.shells[0].refs, [9, 3, 8, 38, 42, 43, 39, 44]);
    }

    #[test]
    fn typed_wire_body_is_valid_without_a_compact_face_bridge() {
        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [1, 1, 1, 1, 1, 1, 1],
                body_type: 2,
                region_head_slot: 6,
                offset: 1,
                end: 2,
            }],
            ..Default::default()
        };
        let hierarchy = facts
            .hierarchies(&HashSet::new())
            .expect("typed wire hierarchy");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].kind, BodyKind::Wire);
        assert!(hierarchy[0].regions.is_empty());
        assert!(hierarchy[0].faces.is_empty());
    }
}
