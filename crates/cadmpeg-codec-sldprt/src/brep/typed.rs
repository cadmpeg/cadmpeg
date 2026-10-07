// SPDX-License-Identifier: Apache-2.0
//! Parasolid XT BODY, SHELL, REGION, and FACE nodes.
//!
//! The ordinary B-rep records in [`super::topology`] are compact `SolidWorks`
//! views of the same topology.  This module reads the ownership nodes that
//! carry the authoritative body kind and shell/region links.  It deliberately
//! keeps the parser separate from the compact topology scanner: a byte pair
//! equal to a node tag is not a node until its complete variable-width field
//! grammar and ownership invariants pass.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::topology::{BodyKind, Sense};

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
pub(super) struct BodyNode {
    /// Stream-local transmit index.
    pub(super) attr: u16,
    /// Persistent XT node id.
    pub(super) node_id: u32,
    /// The first seven pointer cells in the BODY ownership field sequence.
    pub(super) topology_refs: [u32; 7],
    /// Additional pointer cells following the topology fields.  Edited BODY
    /// schemas may retain the region head in this lane; ownership closure
    /// selects it only when REGION links validate.
    pub(super) ownership_refs: Vec<u32>,
    /// Stored Parasolid body kind discriminator.
    pub(super) kind: BodyKind,
    /// Byte offset of the node payload.  The first BODY has no repeated tag.
    pub(super) offset: usize,
    /// First byte after the complete node.
    pub(super) end: usize,
}

struct BodyCandidate {
    attr: u16,
    node_id: u32,
    topology_refs: [u32; 7],
    ownership_refs: [u32; 7 + BODY_POST_TOPOLOGY_REF_MAX],
    ownership_len: usize,
    kind: BodyKind,
    offset: usize,
    end: usize,
}

impl BodyCandidate {
    fn into_node(self, ctx: &DecodeContext<'_>) -> Result<BodyNode, CodecError> {
        Ok(BodyNode {
            attr: self.attr,
            node_id: self.node_id,
            topology_refs: self.topology_refs,
            ownership_refs: ctx.copy_slice(
                self.ownership_refs
                    .get(..self.ownership_len)
                    .unwrap_or_default(),
                "copy Parasolid body ownership references",
            )?,
            kind: self.kind,
            offset: self.offset,
            end: self.end,
        })
    }
}

impl BodyNode {
    fn try_clone(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            attr: self.attr,
            node_id: self.node_id,
            topology_refs: self.topology_refs,
            ownership_refs: ctx.copy_slice(
                &self.ownership_refs,
                "copy typed Parasolid body ownership references",
            )?,
            kind: self.kind,
            offset: self.offset,
            end: self.end,
        })
    }

    /// First shell reference in the body topology fields.
    fn shell(&self) -> u32 {
        self.topology_refs[0]
    }

    /// Candidate region-chain heads: the ownership lane when the schema
    /// stores one, otherwise the topology fields. At most eleven cells.
    fn region_head_candidates(&self) -> &[u32] {
        if self.ownership_refs.is_empty() {
            &self.topology_refs
        } else {
            &self.ownership_refs
        }
    }
}

/// A typed SHELL node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShellNode {
    /// Stream-local transmit index.
    pub(super) attr: u16,
    /// Persistent XT node id.
    pub(super) node_id: u32,
    /// `[attribute_chain, body, next, back_face, edge, vertex, region, front_face]`.
    pub(super) refs: [u32; 8],
    /// Byte offset of the node tag.
    pub(super) offset: usize,
    /// First byte after the complete node.
    pub(super) end: usize,
}

/// A typed REGION node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RegionNode {
    /// Stream-local transmit index.
    pub(super) attr: u16,
    /// Persistent XT node id.
    pub(super) node_id: u32,
    /// `[attribute_chain, body, next, previous, shell_head]`.
    pub(super) refs: [u32; 5],
    /// Byte offset of the node tag.
    pub(super) offset: usize,
    /// First byte after the complete node.
    pub(super) end: usize,
}

/// A typed FACE node.  The compact bridge parser uses the same attribute and
/// node-id prefix, so `attr` is the bridge key used by the graph decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FaceNode {
    /// Stream-local transmit index.
    pub(super) attr: u16,
    /// Persistent XT node id.
    pub(super) node_id: u32,
    /// `[next_face, previous_face, loop, shell, surface]`.
    pub(super) refs: [u32; 5],
    /// Stored face sense marker.
    pub(super) sense: Sense,
    /// Byte offset of the node tag.
    pub(super) offset: usize,
    /// First byte after the complete node.
    pub(super) end: usize,
}

/// All typed ownership nodes recovered from one stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Facts {
    pub(super) bodies: Vec<BodyNode>,
    pub(super) shells: Vec<ShellNode>,
    pub(super) regions: Vec<RegionNode>,
    pub(super) faces: Vec<FaceNode>,
}

/// Regions owned by a known body, by attribute and by previous reference.
struct Regions<'a> {
    by_attr: BTreeMap<u16, &'a RegionNode>,
    /// The region whose previous reference is the key; `None` when several
    /// regions share that previous reference.
    by_previous: BTreeMap<u32, Option<u16>>,
}

impl<'a> Regions<'a> {
    /// Index the regions of `by_attr` by their previous reference.
    fn new(
        ctx: &DecodeContext<'_>,
        by_attr: BTreeMap<u16, &'a RegionNode>,
        storage: &mut ScopedReservation<'_>,
    ) -> Result<Self, CodecError> {
        let mut by_previous = BTreeMap::new();
        for region in ctx.admit_iter(&by_attr, "index typed Parasolid region predecessors")? {
            let (&attr, region) = region;
            storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut by_previous,
                        region.refs[3],
                        "index typed Parasolid region predecessors",
                    )
                })?
                .and_modify(|unique| *unique = None)
                .or_insert(Some(attr));
        }
        Ok(Self {
            by_attr,
            by_previous,
        })
    }
}

/// The closed typed ownership graph of one stream, borrowed from its facts.
/// The indexes are decode scratch held under their own reservation.
struct Ownership<'a, 'ctx> {
    bodies: BTreeMap<u16, &'a BodyNode>,
    regions: Regions<'a>,
    shells: BTreeMap<u16, &'a ShellNode>,
    faces: BTreeMap<u16, &'a FaceNode>,
    _storage: ScopedReservation<'ctx>,
}

/// A body hierarchy under assembly.
struct PendingHierarchy<'a, 'ctx> {
    body: &'a BodyNode,
    regions: Vec<&'a RegionNode>,
    _regions_storage: ScopedReservation<'ctx>,
    shells: Vec<ShellNode>,
    faces: Vec<(u16, u16)>,
}

impl Facts {
    pub(super) fn try_clone(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            bodies: ctx.try_collect_vec(
                self.bodies.iter().map(|body| body.try_clone(ctx)),
                "copy typed Parasolid bodies",
            )?,
            shells: ctx.copy_slice(&self.shells, "copy typed Parasolid shells")?,
            regions: ctx.copy_slice(&self.regions, "copy typed Parasolid regions")?,
            faces: ctx.copy_slice(&self.faces, "copy typed Parasolid faces")?,
        })
    }

    /// Add only identities absent from the partition view.  A delta stream is
    /// subordinate to the partition for the same transmit index.
    pub(super) fn merge_missing(
        &mut self,
        ctx: &DecodeContext<'_>,
        other: Self,
    ) -> Result<(), CodecError> {
        merge_nodes(ctx, &mut self.bodies, other.bodies, |node| node.attr)?;
        merge_nodes(ctx, &mut self.shells, other.shells, |node| node.attr)?;
        merge_nodes(ctx, &mut self.regions, other.regions, |node| node.attr)?;
        merge_nodes(ctx, &mut self.faces, other.faces, |node| node.attr)?;
        Ok(())
    }

    /// Return whether the stream contains a closed typed BODY ownership set.
    /// FACE-to-SHELL closure is checked separately against compact bridge
    /// records because a stream may carry subordinate faces in another site.
    ///
    /// The decode routes read the face attributes through
    /// [`Self::valid_ownership_face_attrs`]; this predicate exists for the
    /// tests that assert closure without naming the attributes.
    #[cfg(test)]
    fn has_valid_ownership(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        Ok(self
            .ownership(ctx)?
            .is_some_and(|ownership| !ownership.bodies.is_empty()))
    }

    /// Return FACE attributes from a closed typed BODY ownership set. Raw FACE
    /// candidates can be byte-window matches with a pointer outside the u16
    /// attribute identity space.
    pub(super) fn valid_ownership_face_attrs(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<BTreeSet<u16>>, CodecError> {
        let Some(ownership) = self.ownership(ctx)? else {
            return Ok(None);
        };
        if ownership.bodies.is_empty() {
            return Ok(None);
        }
        let mut attrs = BTreeSet::new();
        for &attr in ctx
            .admit_iter(&ownership.faces, "select typed Parasolid face attributes")?
            .map(|(attr, _)| attr)
        {
            ctx.insert_btree_set(&mut attrs, attr, "collect typed Parasolid face attributes")?;
        }
        Ok(Some(attrs))
    }

    /// Return body hierarchies only when the typed ownership graph is complete
    /// for the caller's compact face set.
    pub(super) fn hierarchies(
        &self,
        ctx: &DecodeContext<'_>,
        bridge_attrs: &BTreeSet<u16>,
    ) -> Result<Option<Vec<Hierarchy>>, CodecError> {
        let Some(Ownership {
            bodies,
            regions,
            shells,
            faces,
            _storage,
        }) = self.ownership(ctx)?
        else {
            return Ok(None);
        };
        if bodies.is_empty() {
            return Ok(None);
        }
        let mut storage = ctx.reserve_scoped(0, "hold typed Parasolid hierarchy indexes")?;

        // Every compact face bridge names a typed face whose shell is closed.
        let mut face_shells = BTreeMap::<u16, u16>::new();
        for &attr in ctx.admit_iter(bridge_attrs, "match typed Parasolid face bridges")? {
            let Some(face) =
                ctx.get_btree_map(&faces, &attr, "match typed Parasolid face bridges")?
            else {
                return Ok(None);
            };
            let Some(shell) = u16_from_ref(face.refs[3]) else {
                return Ok(None);
            };
            if !ctx.contains_key_btree_map(&shells, &shell, "match typed Parasolid face bridges")? {
                return Ok(None);
            }
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut face_shells,
                    attr,
                    shell,
                    "index typed Parasolid face shells",
                )
            })?;
        }

        let mut relevant_shells = BTreeSet::new();
        let mut relevant_regions_by_body = BTreeMap::<u16, BTreeSet<u16>>::new();
        let mut relevant_bodies = BTreeSet::new();
        for (_, &shell_attr) in
            ctx.admit_iter(&face_shells, "track relevant typed Parasolid shells")?
        {
            if !storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut relevant_shells,
                    shell_attr,
                    "track relevant typed Parasolid shells",
                )
            })? {
                continue;
            }
            let Some(shell) = ctx.get_btree_map(
                &shells,
                &shell_attr,
                "track relevant typed Parasolid shells",
            )?
            else {
                return Ok(None);
            };
            let Some(region) = u16_from_ref(shell.refs[6]).filter(|region| *region > 1) else {
                return Ok(None);
            };
            let Some(region_node) = ctx.get_btree_map(
                &regions.by_attr,
                &region,
                "track relevant typed Parasolid regions",
            )?
            else {
                return Ok(None);
            };
            let Some(region_body) = u16_from_ref(region_node.refs[1]).filter(|body| *body > 1)
            else {
                return Ok(None);
            };
            if !ctx.contains_key_btree_map(
                &bodies,
                &region_body,
                "track relevant typed Parasolid bodies",
            )? {
                return Ok(None);
            }
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut relevant_bodies,
                    region_body,
                    "track relevant typed Parasolid bodies",
                )?;
                let body_regions = ctx
                    .entry_btree_map(
                        &mut relevant_regions_by_body,
                        region_body,
                        "index relevant typed Parasolid regions",
                    )?
                    .or_default();
                ctx.insert_btree_set(
                    body_regions,
                    region,
                    "track relevant typed Parasolid regions",
                )
            })?;
            if shell.refs[1] > 1 && u16_from_ref(shell.refs[1]) != Some(region_body) {
                return Ok(None);
            }
        }
        if bridge_attrs.is_empty() {
            for (&body_attr, _) in
                ctx.admit_iter(&bodies, "track relevant typed Parasolid bodies")?
            {
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut relevant_bodies,
                        body_attr,
                        "track relevant typed Parasolid bodies",
                    )
                })?;
            }
        }

        // Each region chain belongs to one body, so the chains index every
        // region to the hierarchy that owns it.
        let mut pending = Vec::<PendingHierarchy<'_, '_>>::new();
        let mut region_owner = BTreeMap::<u16, usize>::new();
        for &body_attr in ctx.admit_iter(&relevant_bodies, "assemble typed Parasolid hierarchy")? {
            let Some(&body) =
                ctx.get_btree_map(&bodies, &body_attr, "assemble typed Parasolid hierarchy")?
            else {
                return Ok(None);
            };
            if !null_like_or_existing(ctx, body.shell(), &shells)? {
                return Ok(None);
            }
            let Some((body_regions, regions_storage)) = region_chain(ctx, body, &regions)? else {
                return Ok(None);
            };
            let owner = pending.len();
            for region in ctx.admit_iter(&body_regions, "index typed Parasolid body regions")? {
                if storage
                    .with_storage(|| {
                        ctx.insert_btree_map(
                            &mut region_owner,
                            region.attr,
                            owner,
                            "index typed Parasolid body regions",
                        )
                    })?
                    .is_some()
                {
                    return Ok(None);
                }
            }
            if !bridge_attrs.is_empty() {
                let Some(needed) = ctx.get_btree_map(
                    &relevant_regions_by_body,
                    &body_attr,
                    "check typed Parasolid body regions",
                )?
                else {
                    return Ok(None);
                };
                if !ctx.all_by(
                    needed,
                    |region| {
                        Ok(ctx
                            .get_btree_map(
                                &region_owner,
                                region,
                                "check typed Parasolid body regions",
                            )?
                            .is_some_and(|region_owner| *region_owner == owner))
                    },
                    "check typed Parasolid body regions",
                )? {
                    return Ok(None);
                }
            }
            ctx.push_scoped_vec(
                &mut storage,
                &mut pending,
                PendingHierarchy {
                    body,
                    regions: body_regions,
                    _regions_storage: regions_storage,
                    shells: Vec::new(),
                    faces: Vec::new(),
                },
                "collect typed Parasolid hierarchies",
            )?;
        }

        // A shell joins the hierarchy whose region chain holds its region.
        let mut shell_owner = BTreeMap::<u16, usize>::new();
        for (&shell_attr, &shell) in
            ctx.admit_iter(&shells, "collect typed Parasolid body shells")?
        {
            if !bridge_attrs.is_empty()
                && !ctx.contains_btree_set(
                    &relevant_shells,
                    &shell_attr,
                    "collect typed Parasolid body shells",
                )?
            {
                continue;
            }
            let Some(region) = u16_from_ref_or_none(shell.refs[6]) else {
                continue;
            };
            let Some(&owner) = ctx.get_btree_map(
                &region_owner,
                &region,
                "collect typed Parasolid body shells",
            )?
            else {
                continue;
            };
            let Some(hierarchy) = pending.get_mut(owner) else {
                continue;
            };
            if shell.refs[1] > 1 && shell.refs[1] != u32::from(hierarchy.body.attr) {
                continue;
            }
            ctx.push_vec(
                &mut hierarchy.shells,
                *shell,
                "collect typed Parasolid body shells",
            )?;
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut shell_owner,
                    shell_attr,
                    owner,
                    "index typed Parasolid body shells",
                )
            })?;
        }

        // A face joins the hierarchy that holds its shell; every face must.
        for (&face_attr, &shell_attr) in
            ctx.admit_iter(&face_shells, "collect typed Parasolid hierarchy faces")?
        {
            let Some(&owner) = ctx.get_btree_map(
                &shell_owner,
                &shell_attr,
                "collect typed Parasolid hierarchy faces",
            )?
            else {
                return Ok(None);
            };
            let Some(hierarchy) = pending.get_mut(owner) else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut hierarchy.faces,
                (face_attr, shell_attr),
                "collect typed Parasolid hierarchy faces",
            )?;
        }

        let mut out = Vec::new();
        for hierarchy in ctx.admit_iter(pending, "collect typed Parasolid hierarchies")? {
            let regions = ctx.collect_vec(
                hierarchy.regions.iter().map(|region| **region),
                "copy typed Parasolid hierarchy regions",
            )?;
            ctx.push_vec(
                &mut out,
                Hierarchy {
                    body: hierarchy.body.try_clone(ctx)?,
                    regions,
                    shells: hierarchy.shells,
                    faces: hierarchy.faces,
                },
                "collect typed Parasolid hierarchies",
            )?;
        }
        Ok(Some(out))
    }

    /// Build the closed ownership graph: regions of known bodies, shells
    /// reachable from their region's shell chain, faces of those shells, and
    /// bodies whose shell and region chain close. `None` when an identity in
    /// the graph is repeated.
    fn ownership<'ctx>(
        &self,
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<Option<Ownership<'_, 'ctx>>, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "hold typed Parasolid ownership indexes")?;
        let mut scratch = ctx.reserve_scoped(0, "hold typed Parasolid ownership candidates")?;
        let mut body_attrs = BTreeSet::new();
        for body in ctx.admit_iter(&self.bodies, "index typed Parasolid ownership bodies")? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut body_attrs,
                    body.attr,
                    "index typed Parasolid ownership bodies",
                )
            })?;
        }
        let mut regions = BTreeMap::new();
        for region in ctx.admit_iter(&self.regions, "index typed Parasolid ownership regions")? {
            let Some(body) = u16_from_ref_or_none(region.refs[1]) else {
                continue;
            };
            if !ctx.contains_btree_set(
                &body_attrs,
                &body,
                "index typed Parasolid ownership regions",
            )? {
                continue;
            }
            if storage
                .with_storage(|| {
                    ctx.insert_btree_map(
                        &mut regions,
                        region.attr,
                        region,
                        "index typed Parasolid ownership regions",
                    )
                })?
                .is_some()
            {
                return Ok(None);
            }
        }
        let regions = Regions::new(ctx, regions, &mut storage)?;

        let mut candidates = Vec::new();
        for shell in ctx.admit_iter(&self.shells, "select typed Parasolid ownership shells")? {
            let Some(region) = u16_from_ref_or_none(shell.refs[6]) else {
                continue;
            };
            let Some(region_node) = ctx.get_btree_map(
                &regions.by_attr,
                &region,
                "select typed Parasolid ownership shells",
            )?
            else {
                continue;
            };
            let Some(body) = u16_from_ref(region_node.refs[1]) else {
                continue;
            };
            if shell.refs[1] > 1 && u16_from_ref(shell.refs[1]) != Some(body) {
                continue;
            }
            ctx.push_scoped_vec(
                &mut scratch,
                &mut candidates,
                (region, shell),
                "collect typed Parasolid shell candidates",
            )?;
        }

        let (reachable, _reachable_storage) = reachable_shells(ctx, &regions, &candidates)?;
        let mut shells = BTreeMap::new();
        for &(region, shell) in
            ctx.admit_iter(&candidates, "index typed Parasolid ownership shells")?
        {
            if !ctx.contains_btree_set(
                &reachable,
                &(region, shell.attr),
                "index typed Parasolid ownership shells",
            )? {
                continue;
            }
            if storage
                .with_storage(|| {
                    ctx.insert_btree_map(
                        &mut shells,
                        shell.attr,
                        shell,
                        "index typed Parasolid ownership shells",
                    )
                })?
                .is_some()
            {
                return Ok(None);
            }
        }

        let mut faces = BTreeMap::new();
        for face in ctx.admit_iter(&self.faces, "index typed Parasolid ownership faces")? {
            let Some(shell) = u16_from_ref_or_none(face.refs[3]) else {
                continue;
            };
            if !ctx.contains_key_btree_map(
                &shells,
                &shell,
                "index typed Parasolid ownership faces",
            )? {
                continue;
            }
            if storage
                .with_storage(|| {
                    ctx.insert_btree_map(
                        &mut faces,
                        face.attr,
                        face,
                        "index typed Parasolid ownership faces",
                    )
                })?
                .is_some()
            {
                return Ok(None);
            }
        }
        let mut bodies = BTreeMap::new();
        for body in ctx.admit_iter(&self.bodies, "select typed Parasolid ownership bodies")? {
            if !null_like_or_existing(ctx, body.shell(), &shells)?
                || region_chain(ctx, body, &regions)?.is_none()
            {
                continue;
            }
            if storage
                .with_storage(|| {
                    ctx.insert_btree_map(
                        &mut bodies,
                        body.attr,
                        body,
                        "index valid typed Parasolid bodies",
                    )
                })?
                .is_some()
            {
                return Ok(None);
            }
        }
        Ok(Some(Ownership {
            bodies,
            regions,
            shells,
            faces,
            _storage: storage,
        }))
    }
}

/// The `(region, shell)` pairs where the shell lies on the region's shell
/// chain. Shell links are resolved once per shell. Each region emits its own
/// membership pairs until a link is absent, ambiguous, or already visited.
fn reachable_shells<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    regions: &Regions<'_>,
    candidates: &[(u16, &ShellNode)],
) -> Result<(BTreeSet<(u16, u16)>, ScopedReservation<'ctx>), CodecError> {
    let (by_attr, _index_storage) = ctx.unique_index(
        candidates.iter().map(|&(_, shell)| (shell.attr, shell)),
        "index typed Parasolid shell candidates",
    )?;
    let mut storage = ctx.reserve_scoped(0, "hold typed Parasolid shell chains")?;
    let mut walked = BTreeSet::new();
    let mut reachable = BTreeSet::new();
    let mut reachable_storage = ctx.reserve_scoped(0, "hold typed Parasolid reachable shells")?;
    let mut links = BTreeMap::<u16, Option<Option<u16>>>::new();
    for &(region, _) in ctx.admit_iter(candidates, "walk typed Parasolid shell chains")? {
        if !storage.with_storage(|| {
            ctx.insert_btree_set(&mut walked, region, "walk typed Parasolid shell chains")
        })? {
            continue;
        }
        let Some(region_node) = ctx.get_btree_map(
            &regions.by_attr,
            &region,
            "walk typed Parasolid shell chains",
        )?
        else {
            continue;
        };
        let mut next = u16_from_ref_or_none(region_node.refs[4]);
        while let Some(attr) = next {
            ctx.charge_work(1, "walk typed Parasolid shell chain")?;
            const RESOLVE: &str = "resolve typed Parasolid shell link";
            let link = match ctx.get_btree_map(&links, &attr, RESOLVE)? {
                Some(&link) => link,
                None => {
                    let link = ctx.get_hash_map(&by_attr, &attr, RESOLVE)?
                        .and_then(Option::as_ref)
                        .map(|shell| u16_from_ref_or_none(shell.refs[2]));
                    storage.with_storage(|| ctx.insert_btree_map(&mut links, attr, link, RESOLVE))?;
                    link
                }
            };
            let Some(link) = link else { break; };
            if !reachable_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut reachable,
                    (region, attr),
                    "track typed Parasolid shell chain",
                )
            })? {
                break;
            }
            next = link;
        }
    }
    Ok((reachable, reachable_storage))
}

/// The region chain shared by every body head candidate that names one, or
/// an empty chain when the candidates name none. The chain is decode scratch
/// held under the returned reservation.
fn region_chain<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    body: &BodyNode,
    regions: &Regions<'a>,
) -> Result<Option<(Vec<&'a RegionNode>, ScopedReservation<'ctx>)>, CodecError> {
    let mut nonempty: Option<(Vec<&'a RegionNode>, ScopedReservation<'ctx>)> = None;
    let mut saw_empty = false;
    for &head in ctx.admit_iter(
        body.region_head_candidates(),
        "select typed Parasolid region chain",
    )? {
        let Some((chain, storage)) = region_chain_from_head(ctx, body, regions, head)? else {
            continue;
        };
        if chain.is_empty() {
            saw_empty = true;
            continue;
        }
        if let Some((previous, _)) = &nonempty {
            if previous.len() != chain.len()
                || !ctx.all_by(
                    previous.iter().zip(&chain),
                    |(left, right)| Ok(left.attr == right.attr),
                    "compare typed Parasolid region chains",
                )?
            {
                return Ok(None);
            }
        } else {
            nonempty = Some((chain, storage));
        }
    }
    match nonempty {
        Some(chain) => Ok(Some(chain)),
        None if saw_empty => Ok(Some((
            Vec::new(),
            ctx.reserve_scoped(0, "hold typed Parasolid region chain")?,
        ))),
        None => Ok(None),
    }
}

/// Walk the region chain a body head names. Each region after the first
/// names its predecessor and the attribute index is unique, so the walk
/// visits each region at most once.
fn region_chain_from_head<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    body: &BodyNode,
    regions: &Regions<'a>,
    head: u32,
) -> Result<Option<(Vec<&'a RegionNode>, ScopedReservation<'ctx>)>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold typed Parasolid region chain")?;
    if head <= 1 {
        return Ok(Some((Vec::new(), storage)));
    }

    // A BODY may store either the first REGION attribute or the predecessor
    // sentinel used by the REGION doubly-linked list.  Both forms carry the
    // same closure: the first region's previous reference is the stored head
    // in the sentinel form, and each later region points back to its source.
    let direct = match u16_from_ref(head) {
        Some(attr) => ctx
            .get_btree_map(&regions.by_attr, &attr, "find typed Parasolid region head")?
            .copied(),
        None => None,
    };
    let (mut region, mut expected_previous) = if let Some(first) = direct {
        // A direct region head names the first node.  A later region in
        // the chain is not another valid head: its previous pointer must
        // point back to the preceding region.  The sentinel form below
        // is the only form that admits a non-null first previous link.
        if first.refs[3] > 1 {
            return Ok(None);
        }
        (first, None)
    } else {
        let Some(&Some(attr)) = ctx.get_btree_map(
            &regions.by_previous,
            &head,
            "find typed Parasolid region head",
        )?
        else {
            return Ok(None);
        };
        let Some(&first) =
            ctx.get_btree_map(&regions.by_attr, &attr, "find typed Parasolid region head")?
        else {
            return Ok(None);
        };
        (first, Some(head))
    };

    let mut out = Vec::new();
    loop {
        ctx.charge_work(1, "walk typed Parasolid region chain")?;
        if region.refs[1] != u32::from(body.attr)
            || expected_previous.is_some_and(|previous| region.refs[3] != previous)
        {
            return Ok(None);
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut out,
            region,
            "collect typed Parasolid region chain",
        )?;
        let following = region.refs[2];
        if following <= 1 {
            break;
        }
        let Some(following_attr) = u16_from_ref(following) else {
            return Ok(None);
        };
        let Some(&following_region) = ctx.get_btree_map(
            &regions.by_attr,
            &following_attr,
            "walk typed Parasolid region chain",
        )?
        else {
            return Ok(None);
        };
        if following_region.refs[3] != u32::from(region.attr) {
            return Ok(None);
        }
        expected_previous = Some(u32::from(region.attr));
        region = following_region;
    }
    Ok(Some((out, storage)))
}

/// One validated typed body hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::brep) struct Hierarchy {
    pub(super) body: BodyNode,
    pub(super) regions: Vec<RegionNode>,
    pub(super) shells: Vec<ShellNode>,
    /// `(face bridge attr, owning shell attr)` pairs.
    pub(super) faces: Vec<(u16, u16)>,
}

fn merge_nodes<T, F>(
    ctx: &DecodeContext<'_>,
    target: &mut Vec<T>,
    source: Vec<T>,
    key: F,
) -> Result<(), CodecError>
where
    F: Fn(&T) -> u16,
{
    let mut storage = ctx.reserve_scoped(0, "index typed Parasolid merge identities")?;
    let mut present = BTreeSet::new();
    for node in ctx.admit_iter(&target[..], "index typed Parasolid merge identities")? {
        let attr = key(node);
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut present, attr, "index typed Parasolid merge identities")
        })?;
    }
    for node in ctx.admit_iter(source, "merge typed Parasolid records")? {
        if !ctx.contains_btree_set(&present, &key(&node), "merge typed Parasolid records")? {
            ctx.push_vec(target, node, "merge typed Parasolid records")?;
        }
    }
    Ok(())
}

fn u16_from_ref(value: u32) -> Option<u16> {
    u16::try_from(value).ok()
}

fn u16_from_ref_or_none(value: u32) -> Option<u16> {
    u16_from_ref(value).filter(|value| *value > 1)
}

fn null_like_or_existing<T>(
    ctx: &DecodeContext<'_>,
    value: u32,
    nodes: &BTreeMap<u16, T>,
) -> Result<bool, CodecError> {
    if value <= 1 {
        return Ok(true);
    }
    match u16_from_ref(value) {
        Some(value) => ctx.contains_key_btree_map(nodes, &value, "find typed Parasolid node"),
        None => Ok(false),
    }
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
) -> Option<BodyCandidate> {
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
    let kind = match *bytes.get(at)? {
        1 => BodyKind::Solid,
        2 => BodyKind::Wire,
        3 => BodyKind::Sheet,
        6 => BodyKind::General,
        _ => return None,
    };
    at += 1;
    let _nominal_geometry_state = *bytes.get(at)?;
    at += 1;
    let topology_refs = read_refs::<7>(bytes, &mut at)?;
    let mut ownership_refs = [0; 7 + BODY_POST_TOPOLOGY_REF_MAX];
    ownership_refs[..7].copy_from_slice(&topology_refs);
    let mut ownership_len = 7;
    let mut tail_at = at;
    for _ in 0..BODY_POST_TOPOLOGY_REF_MAX {
        let Some(reference) = read_ref(bytes, &mut tail_at) else {
            break;
        };
        ownership_refs[ownership_len] = reference;
        ownership_len += 1;
    }
    valid_resolution(size, linear).then_some(())?;
    Some(BodyCandidate {
        attr,
        node_id,
        topology_refs,
        ownership_refs,
        ownership_len,
        kind,
        offset,
        end: at,
    })
}

fn parse_body_layout(bytes: &[u8], offset: usize, payload: usize) -> Option<BodyCandidate> {
    // BODY is a fixed XT node.  Its fields begin immediately after the
    // attribute/node-id prefix; there is no additional length/index frame.
    // The embedded schema can add or remove leading reference fields, so the
    // field count is selected only when exactly one complete interpretation
    // passes the resolution, state, kind, and topology guards.
    let fields = payload.checked_add(6)?;
    let mut candidate = None;
    for header_ref_count in 0..=BODY_HEADER_REF_MAX {
        if let Some(body) = parse_body_fields(bytes, offset, payload, fields, header_ref_count) {
            if candidate.is_some() {
                return None;
            }
            candidate = Some(body);
        }
    }
    candidate
}

fn parse_tagged_body(bytes: &[u8], offset: usize) -> Option<BodyCandidate> {
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
    if !matches!(bytes.get(at), Some(b'S' | b'V')) {
        read_ref(bytes, &mut at)?;
        if !matches!(bytes.get(at), Some(b'S' | b'V')) {
            return None;
        }
    }
    if refs[1] <= 1 {
        return None;
    }
    Some(RegionNode {
        attr,
        node_id,
        refs,
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
    read_ref(bytes, &mut at)?;
    let tolerance = bytes.get(at..at + 8)?;
    let tolerance_is_sentinel = tolerance == MAGIC;
    let tolerance_is_finite =
        View::f64_be_at(bytes, at).is_some_and(|value| value.is_finite() && value >= 0.0);
    if !tolerance_is_sentinel && !tolerance_is_finite {
        return None;
    }
    at += 8;
    let refs = read_refs::<5>(bytes, &mut at)?;
    let sense = match *bytes.get(at)? {
        0x2b => Sense::Forward,
        0x2d => Sense::Reversed,
        _ => return None,
    };
    if refs[3] <= 1 {
        return None;
    }
    Some(FaceNode {
        attr,
        node_id,
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

/// Add one framed typed node to the scan result.
fn push_record<T>(
    ctx: &DecodeContext<'_>,
    records: &mut Vec<T>,
    record: impl FnOnce() -> Result<T, CodecError>,
) -> Result<(), CodecError> {
    ctx.reserve_vec(records, 1, "admit typed Parasolid record")?;
    records.push(record()?);
    Ok(())
}

/// Position of the first-pass record that starts at `offset`. The first pass
/// adds records at strictly increasing offsets, so its records are sorted.
fn first_pass_record<T>(
    ctx: &DecodeContext<'_>,
    records: &[T],
    first_pass: usize,
    offset: usize,
    record_offset: impl Fn(&T) -> usize,
) -> Result<Option<usize>, CodecError> {
    Ok(ctx
        .binary_search_by_key(
            records.get(..first_pass).unwrap_or_default(),
            &offset,
            |record| Ok(record_offset(record)),
            "find first-pass typed Parasolid records",
        )?
        .ok())
}

/// Scan typed nodes in two passes. The first pass reads the untagged nodes
/// that follow a `Z` schema terminator once an edit marker has appeared; the
/// second reads tagged nodes. A second-pass node at an offset the first pass
/// already read is the same node: a tagged FACE replaces the untagged reading,
/// and other kinds keep the first reading. Second-pass offsets are distinct,
/// so only first-pass records can collide.
pub(super) fn scan(bytes: &[u8], ctx: &DecodeContext<'_>) -> Result<Facts, CodecError> {
    let mut facts = Facts::default();
    let mut has_edit = false;
    for (z, &byte) in ctx
        .admit_iter(bytes, "scan typed Parasolid records")?
        .enumerate()
        .skip(2)
    {
        has_edit |= matches!(byte, b'C' | b'D' | b'I' | b'A');
        if byte != b'Z' || !has_edit {
            continue;
        }
        if let Some(body) = parse_body_layout(bytes, z + 1, z + 1) {
            push_record(ctx, &mut facts.bodies, || body.into_node(ctx))?;
        }
        if let Some(shell) = parse_shell_fields(bytes, z + 1, z + 1) {
            push_record(ctx, &mut facts.shells, || Ok(shell))?;
        }
        if let Some(region) = parse_region_fields(bytes, z + 1, z + 1) {
            push_record(ctx, &mut facts.regions, || Ok(region))?;
        }
        if let Some(face) = parse_face_fields(bytes, z + 1, z + 1) {
            push_record(ctx, &mut facts.faces, || Ok(face))?;
        }
    }
    let first_bodies = facts.bodies.len();
    let first_shells = facts.shells.len();
    let first_regions = facts.regions.len();
    let first_faces = facts.faces.len();
    let offsets = 0..bytes.len().checked_sub(2).map_or(0, |end| end);
    for offset in ctx.admit_iter(offsets, "scan typed Parasolid records")? {
        if bytes.get(offset..offset + 2) == Some(&BODY_TAG) {
            if let Some(body) = parse_tagged_body(bytes, offset) {
                if first_pass_record(ctx, &facts.bodies, first_bodies, offset, |node| node.offset)?
                    .is_none()
                {
                    push_record(ctx, &mut facts.bodies, || body.into_node(ctx))?;
                }
            }
        }
        if let Some(shell) = parse_shell(bytes, offset) {
            if first_pass_record(ctx, &facts.shells, first_shells, offset, |node| node.offset)?
                .is_none()
            {
                push_record(ctx, &mut facts.shells, || Ok(shell))?;
            }
        }
        if let Some(region) = parse_region(bytes, offset) {
            if first_pass_record(ctx, &facts.regions, first_regions, offset, |node| {
                node.offset
            })?
            .is_none()
            {
                push_record(ctx, &mut facts.regions, || Ok(region))?;
            }
        }
        if let Some(face) = parse_face(bytes, offset) {
            match first_pass_record(ctx, &facts.faces, first_faces, offset, |node| node.offset)?
                .and_then(|index| facts.faces.get_mut(index))
            {
                // A schema prepass can start at the same byte as a later
                // tagged node.  The tag carries the complete framing and is
                // the stronger interpretation.
                Some(existing) => *existing = face,
                None => push_record(ctx, &mut facts.faces, || Ok(face))?,
            }
        }
    }
    ctx.stable_sort_by(
        &mut facts.bodies,
        |value| &value.offset,
        Ord::cmp,
        "sort typed Parasolid records",
    )?;
    ctx.stable_sort_by(
        &mut facts.shells,
        |value| &value.offset,
        Ord::cmp,
        "sort typed Parasolid records",
    )?;
    ctx.stable_sort_by(
        &mut facts.regions,
        |value| &value.offset,
        Ord::cmp,
        "sort typed Parasolid records",
    )?;
    ctx.stable_sort_by(
        &mut facts.faces,
        |value| &value.offset,
        Ord::cmp,
        "sort typed Parasolid records",
    )?;
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::{
        read_ref, region_chain, scan, BodyCandidate, BodyNode, FaceNode, Facts, RegionNode,
        Regions, ShellNode, BODY_POST_TOPOLOGY_REF_MAX, BODY_TAG, FACE_TAG, MAGIC, REGION_TAG,
        SHELL_TAG,
    };
    use crate::test_support::container::sldprt_with_body;
    use crate::test_support::parasolid::triangle_body;
    use crate::SldprtCodec;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::topology::BodyKind;
    use cadmpeg_ir::topology::Sense;
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::io::Cursor;

    fn with_test_context<T>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test context");
        f(&ctx)
    }

    fn test_hierarchies(facts: &Facts, attrs: &BTreeSet<u16>) -> Option<Vec<super::Hierarchy>> {
        with_test_context(|ctx| facts.hierarchies(ctx, attrs).expect("hierarchy allocation"))
    }

    fn test_has_valid_ownership(facts: &Facts) -> bool {
        with_test_context(|ctx| {
            facts
                .has_valid_ownership(ctx)
                .expect("ownership allocation")
        })
    }

    fn test_region_chain(
        body: &BodyNode,
        regions: &BTreeMap<u16, RegionNode>,
    ) -> Option<Vec<RegionNode>> {
        with_test_context(|ctx| {
            let mut storage = ctx.reserve_scoped(0, "test regions").expect("test storage");
            let by_attr = regions
                .iter()
                .map(|(attr, region)| (*attr, region))
                .collect();
            let regions = Regions::new(ctx, by_attr, &mut storage).expect("region index");
            region_chain(ctx, body, &regions)
                .expect("region chain allocation")
                .map(|(chain, _)| chain.into_iter().copied().collect())
        })
    }

    #[test]
    fn shared_shell_chain_emits_each_region_membership_and_stops_cycles() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let shells = [8, 9].map(|attr| ShellNode { attr, node_id: u32::from(attr),
            refs: [0, 3, if attr == 8 { 9 } else { 8 }, 0, 0, 0, 20, 0], offset: 0, end: 0 });
        let regions = [20, 21].map(|attr| RegionNode { attr, node_id: u32::from(attr),
            refs: [0, 3, 0, 0, 8], offset: 0, end: 0 });
        let regions = Regions { by_attr: regions.iter().map(|r| (r.attr, r)).collect(), by_previous: Default::default() };
        let candidates = [(20, &shells[0]), (21, &shells[1])];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        assert_eq!(super::reachable_shells(&ctx, &regions, &candidates).unwrap().0,
            std::collections::BTreeSet::from([(20, 8), (20, 9), (21, 8), (21, 9)]));
    }

    #[test]
    fn typed_ownership_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let body = triangle_body();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&body, &arena, &DecodePolicy::service()).expect("root");
        let facts = scan(&body, &ctx).expect("typed facts");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (limited, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).expect("root");
        assert!(matches!(
            facts.valid_ownership_face_attrs(&limited),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index typed Parasolid ownership bodies"
        ));
    }

    #[test]
    fn typed_ownership_refuses_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let body = triangle_body();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&body, &arena, &DecodePolicy::service()).expect("root");
        let facts = scan(&body, &ctx).expect("typed facts");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (limited, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).expect("root");
        assert!(matches!(
            facts.valid_ownership_face_attrs(&limited),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "index typed Parasolid ownership bodies"
        ));
    }

    #[test]
    fn semantic_writer_emits_typed_body_ownership_nodes() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(sldprt_with_body(&triangle_body())),
                &DecodeOptions::default(),
            )
            .unwrap();
        let body = crate::writer::brep_body(decoded.ir(), 0.001, false).unwrap();
        let facts = scan(&body, &ctx).expect("typed scan");

        assert!(test_has_valid_ownership(&facts));
        assert_eq!(facts.bodies.len(), 1);
        assert!(!facts.shells.is_empty());
        assert!(!facts.regions.is_empty());
        assert!(!facts.faces.is_empty());
    }

    #[test]
    fn typed_brep_record_refuses_collection_limit_before_fact_insertion() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let body = triangle_body();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).expect("root");
        let error = scan(&body, &ctx).expect_err("typed record must be admitted");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit typed Parasolid record"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&body, &arena, &DecodePolicy::service()).expect("root");
        assert!(!scan(&body, &ctx)
            .expect("service profile admits typed records")
            .bodies
            .is_empty());
    }

    #[test]
    fn typed_brep_scan_refuses_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let body = triangle_body();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(body.len()).expect("body length") - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).expect("root");
        assert!(matches!(
            scan(&body, &ctx),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "scan typed Parasolid records"
        ));
    }

    #[test]
    fn parasolid_body_ownership_references_refuse_before_copy() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let bytes = [0_u8; 1];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let candidate = BodyCandidate {
            attr: 2,
            node_id: 1,
            topology_refs: [0; 7],
            ownership_refs: [0; 7 + BODY_POST_TOPOLOGY_REF_MAX],
            ownership_len: 7,
            kind: BodyKind::Solid,
            offset: 0,
            end: 0,
        };
        let error = candidate
            .into_node(&ctx)
            .expect_err("seven references exceed six items");
        assert!(matches!(error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "copy Parasolid body ownership references"));
    }

    fn push_ref(bytes: &mut Vec<u8>, value: u32) {
        if value <= 0x7ffe {
            bytes.extend_from_slice(
                &u16::try_from(value)
                    .expect("reference fits u16")
                    .to_be_bytes(),
            );
        } else {
            bytes.extend_from_slice(
                &((0x8000 | u16::try_from(value & 0x7fff).expect("masked reference fits u16"))
                    .to_be_bytes()),
            );
            bytes.extend_from_slice(
                &u16::try_from(value >> 15)
                    .expect("high reference bits fit u16")
                    .to_be_bytes(),
            );
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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = vec![0, 0x0c, 0x1b];
        bytes.extend_from_slice(b"CCCCA");
        bytes.push(b'Z');
        bytes.extend(body_node(3, 0x18b9, 1));
        let facts = scan(&bytes, &ctx).expect("typed scan");
        assert_eq!(facts.bodies.len(), 1);
        assert_eq!(facts.bodies[0].attr, 3);
        assert_eq!(facts.bodies[0].kind, BodyKind::Solid);
    }

    #[test]
    fn body_kind_is_stored_not_inferred() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node(3, 7, 3));
        let facts = scan(&bytes, &ctx).expect("typed scan");
        assert_eq!(facts.bodies[0].kind, BodyKind::Sheet);
    }

    #[test]
    fn tagged_body_accepts_the_four_reference_header_form() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let facts = scan(&tagged_four_ref_body(7, 0x18b9), &ctx).expect("typed scan");
        assert_eq!(facts.bodies.len(), 1);
        assert_eq!(facts.bodies[0].attr, 7);
        assert!(facts.bodies[0].ownership_refs.contains(&48));
        assert_eq!(facts.bodies[0].kind, BodyKind::Solid);
    }

    #[test]
    fn extended_body_references_are_decoded() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node_with_topology(
            3,
            7,
            1,
            [7, 8, 0x8000, 10, 11, 12, 13],
        ));
        let facts = scan(&bytes, &ctx).expect("typed scan");
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
        const BODY_ATTR: u16 = 40_000;
        const SHELL_ATTR: u16 = 40_001;
        const REGION_ATTR: u16 = 40_002;
        const FACE: u16 = 40_003;

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();

        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node_with_topology(
            BODY_ATTR,
            7,
            1,
            [SHELL, 1, 1, 1, REGION, 1, 1],
        ));
        bytes.extend(typed_shell_with_body(SHELL_ATTR, 8, BODY, REGION));
        bytes.extend(typed_region(REGION_ATTR, 9, [1, BODY, 1, 1, SHELL], b'S'));
        bytes.extend(typed_face(FACE, 10, [1, 1, 1, SHELL, 12]));

        let facts = scan(&bytes, &ctx).expect("typed scan");
        let hierarchy = test_hierarchies(&facts, &BTreeSet::from([FACE]))
            .expect("extended typed references close the ownership graph");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].body.attr, BODY_ATTR);
        assert_eq!(hierarchy[0].shells[0].attr, SHELL_ATTR);
        assert_eq!(
            hierarchy[0]
                .regions
                .iter()
                .map(|region| region.attr)
                .collect::<Vec<_>>(),
            vec![REGION_ATTR]
        );
        assert_eq!(hierarchy[0].faces.as_slice(), &[(FACE, SHELL_ATTR)]);
    }

    #[test]
    fn seven_reference_body_and_extended_region_fields_are_decoded() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
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

        let facts = scan(&bytes, &ctx).expect("typed scan");
        assert_eq!(facts.bodies.len(), 1);
        assert_eq!(facts.bodies[0].topology_refs, [7, 1, 8, 9, 10, 1, 1]);
        assert_eq!(facts.regions.len(), 1);
        assert_eq!(facts.regions[0].refs, [1, 3, 1, 1, 7]);
    }

    #[test]
    fn first_region_follows_schema_terminator() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = vec![0, 0x13, 0x1b, b'C', b'Z'];
        bytes.extend_from_slice(&11u16.to_be_bytes());
        bytes.extend_from_slice(&9u32.to_be_bytes());
        for value in [1, 3, 45, 1, 51, 1] {
            push_ref(&mut bytes, value);
        }
        bytes.push(b'V');

        let facts = scan(&bytes, &ctx).expect("typed scan");
        assert_eq!(facts.regions.len(), 1);
        assert_eq!(facts.regions[0].attr, 11);
        assert_eq!(facts.regions[0].refs, [1, 3, 45, 1, 51]);
    }

    #[test]
    fn tagged_face_replaces_a_schema_prepass_collision() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = vec![0, 0x0e, b'C', b'Z'];
        bytes.extend(typed_face(100, 900, [1, 1, 1, 8, 12]));

        let facts = scan(&bytes, &ctx).expect("typed scan");
        assert_eq!(facts.faces.len(), 1);
        assert_eq!(facts.faces[0].offset, 4);
        assert_eq!(facts.faces[0].attr, 100);
        assert_eq!(facts.faces[0].node_id, 900);
    }

    #[test]
    fn typed_hierarchy_uses_previous_region_and_shell_owner() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node(3, 7, 1));
        bytes.extend(typed_shell(7, 814, 39));
        bytes.extend(typed_region(11, 244, [1, 3, 39, 1, 44], b'V'));
        bytes.extend(typed_region(39, 815, [1, 3, 1, 11, 7], b'S'));
        bytes.extend(typed_face(100, 900, [1, 1, 49, 7, 8]));

        let facts = scan(&bytes, &ctx).expect("typed scan");
        let hierarchy =
            test_hierarchies(&facts, &BTreeSet::from([100])).expect("closed typed hierarchy");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].body.kind, BodyKind::Solid);
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
                kind: BodyKind::Solid,
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
                offset: 7,
                end: 8,
            }],
            faces: vec![FaceNode {
                attr: 100,
                node_id: 55,
                refs: [1, 1, 1, 8, 12],
                sense: Sense::Forward,
                offset: 9,
                end: 10,
            }],
        };

        let hierarchy = test_hierarchies(&facts, &BTreeSet::from([100]))
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
                    kind: BodyKind::Solid,
                    offset: 1,
                    end: 2,
                },
                BodyNode {
                    attr: 900,
                    node_id: 70,
                    topology_refs: [999, 1, 1, 1, 1, 1, 1],
                    ownership_refs: vec![900],
                    kind: BodyKind::Solid,
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
                offset: 7,
                end: 8,
            }],
            ..Default::default()
        };

        assert!(test_has_valid_ownership(&facts));
        let hierarchy = test_hierarchies(&facts, &BTreeSet::new())
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
                kind: BodyKind::Solid,
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
                offset: 5,
                end: 6,
            }],
            faces: vec![
                FaceNode {
                    attr: 100,
                    node_id: 55,
                    refs: [1, 1, 1, 65_536, 12],
                    sense: Sense::Forward,
                    offset: 7,
                    end: 8,
                },
                FaceNode {
                    attr: 101,
                    node_id: 56,
                    refs: [1, 1, 1, 8, 12],
                    sense: Sense::Forward,
                    offset: 9,
                    end: 10,
                },
            ],
        };

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test context");
        assert_eq!(
            facts
                .valid_ownership_face_attrs(&ctx)
                .expect("face attributes"),
            Some(BTreeSet::from([101]))
        );
        assert!(test_hierarchies(&facts, &BTreeSet::from([100])).is_none());
        assert!(test_hierarchies(&facts, &BTreeSet::from([101])).is_some());
    }

    #[test]
    fn region_shell_chain_keeps_all_reachable_shells() {
        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [8, 1, 1, 1, 1, 1, 1],
                ownership_refs: vec![41],
                kind: BodyKind::Solid,
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
                offset: 7,
                end: 8,
            }],
            ..Default::default()
        };

        let hierarchy = test_hierarchies(&facts, &BTreeSet::new())
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
                kind: BodyKind::Solid,
                offset: 1,
                end: 2,
            }],
            shells: vec![shell, shell],
            regions: vec![RegionNode {
                attr: 41,
                node_id: 52,
                refs: [1, 3, 1, 1, 8],
                offset: 5,
                end: 6,
            }],
            ..Default::default()
        };

        assert!(!test_has_valid_ownership(&facts));
        assert!(test_hierarchies(&facts, &BTreeSet::new()).is_none());
    }

    #[test]
    fn region_chain_accepts_body_predecessor_sentinel() {
        let body = BodyNode {
            attr: 3,
            node_id: 7,
            topology_refs: [7, 1, 1, 1, 10, 1, 1],
            ownership_refs: Vec::new(),
            kind: BodyKind::Solid,
            offset: 1,
            end: 2,
        };
        let regions = BTreeMap::from([(
            35,
            RegionNode {
                attr: 35,
                node_id: 52,
                refs: [1, 3, 1, 10, 7],
                offset: 3,
                end: 4,
            },
        )]);
        let chain = test_region_chain(&body, &regions).expect("sentinel-linked region");
        assert_eq!(
            chain.iter().map(|region| region.attr).collect::<Vec<_>>(),
            vec![35]
        );
    }

    #[test]
    fn malformed_body_kind_is_withheld() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = vec![0, 0x0c, 0x1b, b'C', b'Z'];
        bytes.extend(body_node(3, 7, 4));
        assert!(scan(&bytes, &ctx).expect("typed scan").bodies.is_empty());
    }

    #[test]
    fn shell_links_are_not_limited_to_sentinel_payloads() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = typed_prefix(SHELL_TAG, 7, 814);
        for value in [9, 3, 8, 38, 42, 43, 39, 44] {
            push_ref(&mut bytes, value);
        }
        let facts = scan(&bytes, &ctx).expect("typed scan");
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
                kind: BodyKind::Wire,
                offset: 1,
                end: 2,
            }],
            ..Default::default()
        };
        let hierarchy = test_hierarchies(&facts, &BTreeSet::new()).expect("typed wire hierarchy");
        assert_eq!(hierarchy.len(), 1);
        assert_eq!(hierarchy[0].body.kind, BodyKind::Wire);
        assert!(hierarchy[0].regions.is_empty());
        assert!(hierarchy[0].faces.is_empty());
    }
}
