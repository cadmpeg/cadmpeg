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
const BODY_HEADER_REF_MAX: usize = 32;
const BODY_POST_TOPOLOGY_REF_MAX: usize = 4;

/// A typed BODY node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyNode {
    /// Stream-local transmit index.
    pub attr: u16,
    /// Persistent XT node id.
    pub node_id: u32,
    /// The first seven pointer cells in the BODY ownership field sequence.
    pub topology_refs: [u32; 7],
    /// Additional pointer cells following the topology fields.  Edited BODY
    /// schemas may retain the region head in this lane; ownership closure
    /// selects it only when REGION links validate.
    pub ownership_refs: Vec<u32>,
    /// Stored Parasolid body kind discriminator.
    pub body_type: u8,
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

    fn region_head_candidates(&self) -> Vec<u32> {
        let mut refs = self.ownership_refs.clone();
        if refs.is_empty() {
            refs.extend(self.topology_refs);
        }
        refs.sort_unstable();
        refs.dedup();
        refs
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

type OwnershipMaps = (
    HashMap<u16, BodyNode>,
    HashMap<u16, RegionNode>,
    HashMap<u16, ShellNode>,
    HashMap<u16, FaceNode>,
);

impl Facts {
    /// Add only identities absent from the partition view.  A delta stream is
    /// subordinate to the partition for the same transmit index.
    pub fn merge_missing(&mut self, other: Self) {
        merge_nodes(&mut self.bodies, other.bodies, |node| node.attr);
        merge_nodes(&mut self.shells, other.shells, |node| node.attr);
        merge_nodes(&mut self.regions, other.regions, |node| node.attr);
        merge_nodes(&mut self.faces, other.faces, |node| node.attr);
    }

    /// Return whether the stream contains a closed typed BODY ownership set.
    /// FACE-to-SHELL closure is checked separately against compact bridge
    /// records because a stream may carry subordinate faces in another site.
    pub fn has_valid_ownership(&self) -> bool {
        let Some((bodies, regions, shells, _faces)) = self.ownership_maps() else {
            return false;
        };
        !bodies.is_empty()
            && bodies.values().all(|body| {
                body.kind().is_some()
                    && null_like_or_existing(body.shell(), &shells)
                    && region_chain(body, &regions).is_some()
            })
    }

    /// Return FACE attributes whose shell pointers close through the validated
    /// typed ownership maps.  Raw FACE candidates can be byte-window matches
    /// with a pointer outside the u16 attribute identity space.
    pub fn valid_face_attrs(&self) -> Option<HashSet<u16>> {
        let (_, _, _, faces) = self.ownership_maps()?;
        Some(faces.keys().copied().collect())
    }

    /// Return body hierarchies only when the typed ownership graph is complete
    /// for the caller's compact face set.
    pub fn hierarchies(&self, bridge_attrs: &HashSet<u16>) -> Option<Vec<Hierarchy>> {
        let (bodies, regions, shells, all_faces) = self.ownership_maps()?;
        let faces = if bridge_attrs.is_empty() {
            all_faces
        } else {
            unique_map_for_attrs(
                &all_faces.values().cloned().collect::<Vec<_>>(),
                bridge_attrs,
                |node| node.attr,
            )?
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

    fn ownership_maps(&self) -> Option<OwnershipMaps> {
        let body_attrs = self
            .bodies
            .iter()
            .filter(|body| body.kind().is_some())
            .map(|body| body.attr)
            .collect::<HashSet<_>>();
        let mut regions = HashMap::new();
        for region in &self.regions {
            let Some(body) = u16_from_ref_or_none(region.refs[1]) else {
                continue;
            };
            if !body_attrs.contains(&body) || !matches!(region.kind, b'S' | b'V') {
                continue;
            }
            if regions.insert(region.attr, region.clone()).is_some() {
                return None;
            }
        }

        let mut shell_candidates = Vec::new();
        for shell in &self.shells {
            let Some(region) = u16_from_ref_or_none(shell.refs[6]) else {
                continue;
            };
            let Some(region_node) = regions.get(&region) else {
                continue;
            };
            let Some(body) = u16_from_ref(region_node.refs[1]) else {
                continue;
            };
            let shell_body = if shell.refs[1] <= 1 {
                None
            } else {
                let Some(body) = u16_from_ref(shell.refs[1]) else {
                    continue;
                };
                Some(body)
            };
            if shell_body.is_some_and(|shell_body| shell_body != body) {
                continue;
            }
            shell_candidates.push(shell.clone());
        }

        let mut shells = HashMap::new();
        for shell in &shell_candidates {
            let region = u16_from_ref(shell.refs[6])?;
            let region_node = regions.get(&region)?;
            if !shell_is_reachable_from_region(region_node, shell.attr, &shell_candidates) {
                continue;
            }
            if shells.insert(shell.attr, shell.clone()).is_some() {
                return None;
            }
        }

        let mut faces = HashMap::new();
        for face in &self.faces {
            let shell = u16_from_ref_or_none(face.refs[3]);
            if !shell.is_some_and(|shell| shells.contains_key(&shell)) {
                continue;
            }
            if faces.insert(face.attr, face.clone()).is_some() {
                return None;
            }
        }
        let bodies = self
            .bodies
            .iter()
            .filter(|body| {
                body.kind().is_some()
                    && null_like_or_existing(body.shell(), &shells)
                    && region_chain(body, &regions).is_some()
            })
            .cloned()
            .collect::<Vec<_>>();
        let bodies = unique_map(&bodies, |node| node.attr)?;
        Some((bodies, regions, shells, faces))
    }
}

fn shell_is_reachable_from_region(
    region: &RegionNode,
    target: u16,
    candidates: &[ShellNode],
) -> bool {
    let Some(mut next) = u16_from_ref_or_none(region.refs[4]) else {
        return false;
    };
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(next) {
            return false;
        }
        let mut matches = candidates.iter().filter(|shell| shell.attr == next);
        let Some(shell) = matches.next() else {
            return false;
        };
        if matches.next().is_some() {
            return false;
        }
        if next == target {
            return true;
        }
        let Some(following) = u16_from_ref_or_none(shell.refs[2]) else {
            return false;
        };
        next = following;
    }
}

fn region_chain(body: &BodyNode, regions: &HashMap<u16, RegionNode>) -> Option<Vec<RegionNode>> {
    let mut chains = Vec::new();
    for head in body.region_head_candidates() {
        if let Some(chain) = region_chain_from_head(body, regions, head) {
            if !chains.iter().any(|previous: &Vec<RegionNode>| {
                previous
                    .iter()
                    .map(|region| region.attr)
                    .eq(chain.iter().map(|region| region.attr))
            }) {
                chains.push(chain);
            }
        }
    }
    let nonempty = chains
        .iter()
        .filter(|chain| !chain.is_empty())
        .collect::<Vec<_>>();
    match nonempty.as_slice() {
        [chain] => Some((*chain).clone()),
        [] if chains.iter().any(Vec::is_empty) => Some(Vec::new()),
        _ => None,
    }
}

fn region_chain_from_head(
    body: &BodyNode,
    regions: &HashMap<u16, RegionNode>,
    head: u32,
) -> Option<Vec<RegionNode>> {
    if head <= 1 {
        return Some(Vec::new());
    }

    // A BODY may store either the first REGION attribute or the predecessor
    // sentinel used by the REGION doubly-linked list.  Both forms carry the
    // same closure: the first region's previous reference is the stored head
    // in the sentinel form, and each later region points back to its source.
    let (mut next, first_previous) =
        if let Some(attr) = u16_from_ref(head).filter(|attr| regions.contains_key(attr)) {
            let first = regions.get(&attr)?;
            // A direct region head names the first node.  A later region in
            // the chain is not another valid head: its previous pointer must
            // point back to the preceding region.  The sentinel form below
            // is the only form that admits a non-null first previous link.
            (first.refs[3] <= 1).then_some((attr, None))?
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

fn parse_body_fields(
    bytes: &[u8],
    offset: usize,
    payload: usize,
    mut at: usize,
    header_ref_count: usize,
) -> Option<BodyNode> {
    let attr = View::u16_be_at(bytes, payload)?;
    let node_id = View::u32_be_at(bytes, payload + 2)?;
    (attr > 1 && node_id != 0).then_some(())?;
    for _ in 0..header_ref_count {
        read_ref(bytes, &mut at)?;
    }
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
    let mut ownership_refs = topology_refs.to_vec();
    let mut tail_at = at;
    for _ in 0..BODY_POST_TOPOLOGY_REF_MAX {
        let Some(reference) = read_ref(bytes, &mut tail_at) else {
            break;
        };
        ownership_refs.push(reference);
    }
    valid_resolution(size, linear).then_some(())?;
    let body = BodyNode {
        attr,
        node_id,
        topology_refs,
        ownership_refs,
        body_type,
        offset,
        end: at,
    };
    body.kind().map(|_| body)
}

fn parse_body_layout(bytes: &[u8], offset: usize, payload: usize) -> Option<BodyNode> {
    // BODY is a fixed XT node.  Its fields begin immediately after the
    // attribute/node-id prefix; there is no additional length/index frame.
    // The embedded schema can add or remove leading reference fields, so the
    // field count is selected only when exactly one complete interpretation
    // passes the resolution, state, kind, and topology guards.
    let fields = payload.checked_add(6)?;
    let candidates = (0..=BODY_HEADER_REF_MAX)
        .filter_map(|header_ref_count| {
            parse_body_fields(bytes, offset, payload, fields, header_ref_count)
        })
        .collect::<Vec<_>>();
    let mut candidates = candidates.into_iter();
    let body = candidates.next()?;
    candidates.next().is_none().then_some(body)
}

fn parse_body(bytes: &[u8], offset: usize, payload: usize) -> Option<BodyNode> {
    parse_body_layout(bytes, offset, payload)
}

fn parse_tagged_body(bytes: &[u8], offset: usize) -> Option<BodyNode> {
    let (payload, _, _) = read_prefix(bytes, offset, BODY_TAG)?;
    parse_body_layout(bytes, offset, payload.checked_sub(6)?)
}

fn parse_shell_fields(bytes: &[u8], offset: usize, payload: usize) -> Option<ShellNode> {
    let attr = View::u16_be_at(bytes, payload)?;
    let node_id = View::u32_be_at(bytes, payload + 2)?;
    (attr > 1 && node_id != 0).then_some(())?;
    let mut at = payload + 6;
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

fn parse_shell(bytes: &[u8], offset: usize) -> Option<ShellNode> {
    let (payload, _, _) = read_prefix(bytes, offset, SHELL_TAG)?;
    parse_shell_fields(bytes, offset, payload.checked_sub(6)?)
}

fn parse_region_fields(bytes: &[u8], offset: usize, payload: usize) -> Option<RegionNode> {
    let attr = View::u16_be_at(bytes, payload)?;
    let node_id = View::u32_be_at(bytes, payload + 2)?;
    (attr > 1 && node_id != 0).then_some(())?;
    let mut at = payload + 6;
    let refs = read_refs::<5>(bytes, &mut at)?;
    // A schema edit may retain one additional reference before the semantic
    // kind byte.  It is not part of the ownership tuple, but it must be
    // consumed so the node boundary remains correct.
    let kind = if bytes
        .get(at)
        .is_some_and(|byte| matches!(byte, b'S' | b'V'))
    {
        *bytes.get(at)?
    } else {
        read_ref(bytes, &mut at)?;
        *bytes.get(at)?
    };
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

fn parse_region(bytes: &[u8], offset: usize) -> Option<RegionNode> {
    let (payload, _, _) = read_prefix(bytes, offset, REGION_TAG)?;
    parse_region_fields(bytes, offset, payload.checked_sub(6)?)
}

fn parse_face_fields(bytes: &[u8], offset: usize, payload: usize) -> Option<FaceNode> {
    let attr = View::u16_be_at(bytes, payload)?;
    let node_id = View::u32_be_at(bytes, payload + 2)?;
    (attr > 1 && node_id != 0).then_some(())?;
    let mut at = payload + 6;
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

fn parse_face(bytes: &[u8], offset: usize) -> Option<FaceNode> {
    let (payload, _, _) = read_prefix(bytes, offset, FACE_TAG)?;
    parse_face_fields(bytes, offset, payload.checked_sub(6)?)
}

fn body_schema_bodies(bytes: &[u8], offset: usize) -> Vec<BodyNode> {
    let end = bytes.len();
    let mut bodies = Vec::new();
    let mut z = offset + 2;
    while z < end {
        if bytes[z] == b'Z' {
            let body = parse_body(bytes, z + 1, z + 1);
            if let Some(body) = body {
                let has_edit = has_schema_edit(bytes, offset, z);
                if has_edit {
                    bodies.push(body);
                }
            }
        }
        z += 1;
    }
    bodies
}

fn has_schema_edit(bytes: &[u8], offset: usize, terminator: usize) -> bool {
    bytes.get(offset + 2..terminator).is_some_and(|edit| {
        edit.iter()
            .any(|byte| matches!(byte, b'C' | b'D' | b'I' | b'A'))
    })
}

fn schema_regions(bytes: &[u8], offset: usize) -> Vec<RegionNode> {
    let mut regions = Vec::new();
    for z in offset + 2..bytes.len() {
        if bytes[z] == b'Z' && has_schema_edit(bytes, offset, z) {
            if let Some(region) = parse_region_fields(bytes, z + 1, z + 1) {
                regions.push(region);
            }
        }
    }
    regions
}

fn schema_shells(bytes: &[u8], offset: usize) -> Vec<ShellNode> {
    let mut shells = Vec::new();
    for z in offset + 2..bytes.len() {
        if bytes[z] == b'Z' && has_schema_edit(bytes, offset, z) {
            if let Some(shell) = parse_shell_fields(bytes, z + 1, z + 1) {
                shells.push(shell);
            }
        }
    }
    shells
}

fn schema_faces(bytes: &[u8], offset: usize) -> Vec<FaceNode> {
    let mut faces = Vec::new();
    for z in offset + 2..bytes.len() {
        if bytes[z] == b'Z' && has_schema_edit(bytes, offset, z) {
            if let Some(face) = parse_face_fields(bytes, z + 1, z + 1) {
                faces.push(face);
            }
        }
    }
    faces
}

/// Scan one partition-style stream for strictly framed typed ownership nodes.
pub fn scan(bytes: &[u8]) -> Facts {
    let mut facts = Facts::default();
    let mut body_offsets = HashSet::new();
    let mut shell_offsets = HashSet::new();
    let mut region_offsets = HashSet::new();
    let mut face_offsets = HashSet::new();
    for body in body_schema_bodies(bytes, 0) {
        body_offsets.insert(body.offset);
        facts.bodies.push(body);
    }
    for shell in schema_shells(bytes, 0) {
        if shell_offsets.insert(shell.offset) {
            facts.shells.push(shell);
        }
    }
    for region in schema_regions(bytes, 0) {
        if region_offsets.insert(region.offset) {
            facts.regions.push(region);
        }
    }
    for face in schema_faces(bytes, 0) {
        if face_offsets.insert(face.offset) {
            facts.faces.push(face);
        }
    }
    for offset in 0..bytes.len().saturating_sub(2) {
        if bytes.get(offset..offset + 2) == Some(&BODY_TAG) {
            if let Some(body) = parse_tagged_body(bytes, offset) {
                if body_offsets.insert(body.offset) {
                    facts.bodies.push(body);
                }
            }
        }
        if let Some(shell) = parse_shell(bytes, offset) {
            if shell_offsets.insert(shell.offset) {
                facts.shells.push(shell);
            }
        }
        if let Some(region) = parse_region(bytes, offset) {
            if region_offsets.insert(region.offset) {
                facts.regions.push(region);
            }
        }
        if let Some(face) = parse_face(bytes, offset) {
            if let Some(existing) = facts
                .faces
                .iter_mut()
                .find(|existing| existing.offset == face.offset)
            {
                // A schema prepass can start at the same byte as a later
                // tagged node.  The tag carries the complete framing and is
                // the stronger interpretation.
                *existing = face;
            } else if face_offsets.insert(face.offset) {
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

    fn body_fields<const N: usize>(header: [u32; N], body_type: u8, topology: [u32; 7]) -> Vec<u8> {
        let mut bytes = Vec::new();
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

    fn body_node_with_header<const N: usize>(
        attr: u16,
        node_id: u32,
        body_type: u8,
        header: [u32; N],
        topology: [u32; 7],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&node_id.to_be_bytes());
        bytes.extend(body_fields::<N>(header, body_type, topology));
        bytes
    }

    fn body_node_with_topology(
        attr: u16,
        node_id: u32,
        body_type: u8,
        topology: [u32; 7],
    ) -> Vec<u8> {
        body_node_with_header(attr, node_id, body_type, [5, 6, 1, 1, 1, 1], topology)
    }

    fn body_node(attr: u16, node_id: u32, body_type: u8) -> Vec<u8> {
        body_node_with_topology(attr, node_id, body_type, [7, 8, 9, 10, 1, 12, 11])
    }

    fn tagged_four_ref_body(attr: u16, node_id: u32) -> Vec<u8> {
        let mut bytes = typed_prefix(BODY_TAG, attr, node_id);
        bytes.extend(body_fields::<4>(
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

    fn typed_shell_with_body(attr: u16, node_id: u32, body: u32, region: u32) -> Vec<u8> {
        let mut bytes = typed_prefix(SHELL_TAG, attr, node_id);
        for value in [1, body, 1, 38, 1, 1, region, 1] {
            push_ref(&mut bytes, value);
        }
        bytes
    }

    fn typed_shell(attr: u16, node_id: u32, region: u32) -> Vec<u8> {
        typed_shell_with_body(attr, node_id, 3, region)
    }

    fn typed_region(attr: u16, node_id: u32, refs: [u32; 5], kind: u8) -> Vec<u8> {
        let mut bytes = typed_prefix(REGION_TAG, attr, node_id);
        for value in refs {
            push_ref(&mut bytes, value);
        }
        bytes.push(kind);
        bytes
    }

    fn typed_region_with_extra_ref(
        attr: u16,
        node_id: u32,
        refs: [u32; 5],
        extra_ref: u32,
        kind: u8,
    ) -> Vec<u8> {
        let mut bytes = typed_prefix(REGION_TAG, attr, node_id);
        for value in refs {
            push_ref(&mut bytes, value);
        }
        push_ref(&mut bytes, extra_ref);
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
        assert!(facts.bodies[0].ownership_refs.contains(&48));
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
    fn extended_references_close_every_typed_ownership_edge() {
        const BODY: u32 = 40_000;
        const SHELL: u32 = 40_001;
        const REGION: u32 = 40_002;
        const FACE: u16 = 40_003;

        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node_with_topology(
            BODY as u16,
            7,
            1,
            [SHELL, 1, 1, 1, REGION, 1, 1],
        ));
        bytes.extend(typed_shell_with_body(SHELL as u16, 8, BODY, REGION));
        bytes.extend(typed_region(REGION as u16, 9, [1, BODY, 1, 1, SHELL], b'S'));
        bytes.extend(typed_face(FACE, 10, [1, 1, 1, SHELL, 12]));

        let facts = scan(&bytes);
        let hierarchy = facts
            .hierarchies(&HashSet::from([FACE]))
            .expect("extended typed references close the ownership graph");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].body.attr, BODY as u16);
        assert_eq!(hierarchy[0].shells[0].attr, SHELL as u16);
        assert_eq!(
            hierarchy[0]
                .regions
                .iter()
                .map(|region| region.attr)
                .collect::<Vec<_>>(),
            vec![REGION as u16]
        );
        assert_eq!(hierarchy[0].faces.as_slice(), &[(FACE, SHELL as u16)]);
    }

    #[test]
    fn seven_reference_body_and_extended_region_fields_are_decoded() {
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node_with_header::<7>(
            3,
            7,
            1,
            [5, 6, 1, 1, 1, 1, 1],
            [7, 1, 8, 9, 10, 1, 1],
        ));
        bytes.extend(typed_region_with_extra_ref(
            11,
            244,
            [1, 3, 1, 1, 7],
            1,
            b'V',
        ));

        let facts = scan(&bytes);
        assert_eq!(facts.bodies.len(), 1);
        assert_eq!(facts.bodies[0].topology_refs, [7, 1, 8, 9, 10, 1, 1]);
        assert_eq!(facts.regions.len(), 1);
        assert_eq!(facts.regions[0].refs, [1, 3, 1, 1, 7]);
        assert_eq!(facts.regions[0].kind, b'V');
    }

    #[test]
    fn first_region_follows_schema_terminator() {
        let mut bytes = vec![0, 0x13, 0x1b, b'C', b'Z'];
        bytes.extend_from_slice(&11u16.to_be_bytes());
        bytes.extend_from_slice(&9u32.to_be_bytes());
        for value in [1, 3, 45, 1, 51, 1] {
            push_ref(&mut bytes, value);
        }
        bytes.push(b'V');

        let facts = scan(&bytes);
        assert_eq!(facts.regions.len(), 1);
        assert_eq!(facts.regions[0].attr, 11);
        assert_eq!(facts.regions[0].refs, [1, 3, 45, 1, 51]);
        assert_eq!(facts.regions[0].kind, b'V');
    }

    #[test]
    fn tagged_face_replaces_a_schema_prepass_collision() {
        let mut bytes = vec![0, 0x0e, b'C', b'Z'];
        bytes.extend(typed_face(100, 900, [1, 1, 1, 8, 12]));

        let facts = scan(&bytes);
        assert_eq!(facts.faces.len(), 1);
        assert_eq!(facts.faces[0].offset, 4);
        assert_eq!(facts.faces[0].attr, 100);
        assert_eq!(facts.faces[0].node_id, 900);
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
    fn invalid_duplicate_shell_is_filtered_before_identity_checks() {
        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [8, 1, 1, 1, 1, 1, 1],
                ownership_refs: vec![41],
                body_type: 1,
                offset: 1,
                end: 2,
            }],
            shells: vec![
                ShellNode {
                    attr: 8,
                    node_id: 53,
                    refs: [1, 3, 1, 1, 1, 1, 41, 1],
                    offset: 3,
                    end: 4,
                },
                ShellNode {
                    attr: 8,
                    node_id: 54,
                    refs: [28160, 65_536, 922_777_600, 12032, 20736, 0, 41, 21248],
                    offset: 5,
                    end: 6,
                },
            ],
            regions: vec![RegionNode {
                attr: 41,
                node_id: 52,
                refs: [1, 3, 1, 1, 8],
                kind: b'S',
                offset: 7,
                end: 8,
            }],
            faces: vec![FaceNode {
                attr: 100,
                node_id: 55,
                attribute_chain: 1,
                refs: [1, 1, 1, 8, 12],
                sense: 0x2b,
                offset: 9,
                end: 10,
            }],
        };

        let hierarchy = facts
            .hierarchies(&HashSet::from([100]))
            .expect("the invalid byte-window candidate is not an ownership node");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].shells.len(), 1);
        assert_eq!(hierarchy[0].shells[0].node_id, 53);
        assert_eq!(hierarchy[0].faces.as_slice(), &[(100, 8)]);
    }

    #[test]
    fn invalid_body_candidate_is_filtered_before_identity_checks() {
        let facts = Facts {
            bodies: vec![
                BodyNode {
                    attr: 3,
                    node_id: 7,
                    topology_refs: [8, 1, 1, 1, 1, 1, 1],
                    ownership_refs: vec![41],
                    body_type: 1,
                    offset: 1,
                    end: 2,
                },
                BodyNode {
                    attr: 900,
                    node_id: 70,
                    topology_refs: [999, 1, 1, 1, 1, 1, 1],
                    ownership_refs: vec![900],
                    body_type: 1,
                    offset: 3,
                    end: 4,
                },
            ],
            shells: vec![ShellNode {
                attr: 8,
                node_id: 53,
                refs: [1, 3, 1, 1, 1, 1, 41, 1],
                offset: 5,
                end: 6,
            }],
            regions: vec![RegionNode {
                attr: 41,
                node_id: 52,
                refs: [1, 3, 1, 1, 8],
                kind: b'S',
                offset: 7,
                end: 8,
            }],
            ..Default::default()
        };

        assert!(facts.has_valid_ownership());
        let hierarchy = facts
            .hierarchies(&HashSet::new())
            .expect("the body with no closed shell or region is not an ownership node");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].body.attr, 3);
    }

    #[test]
    fn invalid_face_shell_pointer_is_not_a_valid_face_identity() {
        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [8, 1, 1, 1, 1, 1, 1],
                ownership_refs: vec![41],
                body_type: 1,
                offset: 1,
                end: 2,
            }],
            shells: vec![ShellNode {
                attr: 8,
                node_id: 53,
                refs: [1, 3, 1, 1, 1, 1, 41, 1],
                offset: 3,
                end: 4,
            }],
            regions: vec![RegionNode {
                attr: 41,
                node_id: 52,
                refs: [1, 3, 1, 1, 8],
                kind: b'S',
                offset: 5,
                end: 6,
            }],
            faces: vec![
                FaceNode {
                    attr: 100,
                    node_id: 55,
                    attribute_chain: 1,
                    refs: [1, 1, 1, 65_536, 12],
                    sense: 0x2b,
                    offset: 7,
                    end: 8,
                },
                FaceNode {
                    attr: 101,
                    node_id: 56,
                    attribute_chain: 1,
                    refs: [1, 1, 1, 8, 12],
                    sense: 0x2b,
                    offset: 9,
                    end: 10,
                },
            ],
        };

        assert_eq!(facts.valid_face_attrs(), Some(HashSet::from([101])));
        assert!(facts.hierarchies(&HashSet::from([100])).is_none());
        assert!(facts.hierarchies(&HashSet::from([101])).is_some());
    }

    #[test]
    fn region_shell_chain_keeps_all_reachable_shells() {
        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [8, 1, 1, 1, 1, 1, 1],
                ownership_refs: vec![41],
                body_type: 1,
                offset: 1,
                end: 2,
            }],
            shells: vec![
                ShellNode {
                    attr: 8,
                    node_id: 53,
                    refs: [1, 3, 9, 1, 1, 1, 41, 1],
                    offset: 3,
                    end: 4,
                },
                ShellNode {
                    attr: 9,
                    node_id: 54,
                    refs: [1, 3, 1, 1, 1, 1, 41, 1],
                    offset: 5,
                    end: 6,
                },
            ],
            regions: vec![RegionNode {
                attr: 41,
                node_id: 52,
                refs: [1, 3, 1, 1, 8],
                kind: b'S',
                offset: 7,
                end: 8,
            }],
            ..Default::default()
        };

        let hierarchy = facts
            .hierarchies(&HashSet::new())
            .expect("all shells reachable from the region head are retained");
        assert_eq!(hierarchy.len(), 1);
        let mut shell_attrs = hierarchy[0]
            .shells
            .iter()
            .map(|shell| shell.attr)
            .collect::<Vec<_>>();
        shell_attrs.sort_unstable();
        assert_eq!(shell_attrs, vec![8, 9]);
    }

    #[test]
    fn duplicate_closed_shell_identity_withholds_typed_graph() {
        let shell = ShellNode {
            attr: 8,
            node_id: 53,
            refs: [1, 3, 1, 1, 1, 1, 41, 1],
            offset: 3,
            end: 4,
        };
        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [8, 1, 1, 1, 1, 1, 1],
                ownership_refs: vec![41],
                body_type: 1,
                offset: 1,
                end: 2,
            }],
            shells: vec![shell.clone(), shell],
            regions: vec![RegionNode {
                attr: 41,
                node_id: 52,
                refs: [1, 3, 1, 1, 8],
                kind: b'S',
                offset: 5,
                end: 6,
            }],
            ..Default::default()
        };

        assert!(!facts.has_valid_ownership());
        assert!(facts.hierarchies(&HashSet::new()).is_none());
    }

    #[test]
    fn region_chain_accepts_body_predecessor_sentinel() {
        let body = BodyNode {
            attr: 3,
            node_id: 7,
            topology_refs: [7, 1, 1, 1, 10, 1, 1],
            ownership_refs: Vec::new(),
            body_type: 1,
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
                ownership_refs: Vec::new(),
                body_type: 2,
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
