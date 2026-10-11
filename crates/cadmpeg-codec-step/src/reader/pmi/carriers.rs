// SPDX-License-Identifier: Apache-2.0
//! Shared carrier sets and strongly connected record summaries.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum SetId {
    #[default]
    Empty,
    Leaf(u64),
    Branch(usize),
}

impl cadmpeg_core::decode::cost::DecodeCost for SetId {
    const FIXED_BYTES: Option<u64> = Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Self>()));
    fn decode_cost(&self,_ctx: &DecodeContext<'_>,_operation: &'static str) -> Result<u64,CodecError> { Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Self>())) }
}

#[derive(Clone, Copy)]
struct SetNode {
    first: u64,
    last: u64,
    count: usize,
    children: [SetId; 2],
}

/// Immutable Patricia sets share branches and memoize pairwise unions.
/// Counts and singleton selection do not enumerate reachable carriers.
pub(super) struct CarrierSets<'ctx, 'arena> {
    nodes: Vec<SetNode>,
    unions: HashMap<(SetId, SetId), SetId>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'arena> CarrierSets<'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            nodes: Vec::new(),
            unions: HashMap::new(),
            ctx,
            storage: ctx.reserve_scoped(0, "STEP carrier sets scratch")?,
        })
    }

    fn node(&self, id: SetId) -> SetNode {
        match id {
            SetId::Leaf(key) => SetNode { first: key, last: key, count: 1, children: [SetId::Empty;2] },
            SetId::Branch(index) => self.nodes[index],
            SetId::Empty => unreachable!("empty set has no node"),
        }
    }

    pub(super) fn count(&self, id: SetId) -> usize {
        if id == SetId::Empty { 0 } else { self.node(id).count }
    }

    pub(super) fn first(&self, id: SetId) -> Option<u64> {
        (id != SetId::Empty).then(|| self.node(id).first)
    }

    pub(super) fn leaf(key: u64) -> SetId { SetId::Leaf(key) }

    fn branch(&mut self, left: SetId, right: SetId) -> Result<SetId, CodecError> {
        if left == SetId::Empty { return Ok(right); }
        if right == SetId::Empty { return Ok(left); }
        let a = self.node(left);
        let b = self.node(right);
        let id = SetId::Branch(self.nodes.len());
        self.ctx.push_scoped_vec(&mut self.storage, &mut self.nodes, SetNode {
            first: a.first, last: b.last, count: a.count + b.count, children: [left, right],
        }, "STEP carrier set nodes")?;
        Ok(id)
    }

    pub(super) fn union(&mut self, a: SetId, b: SetId) -> Result<SetId, CodecError> {
        if a == SetId::Empty || a == b { return Ok(b); }
        if b == SetId::Empty { return Ok(a); }
        let ctx = self.ctx;
        let an = self.node(a);
        let bn = self.node(b);
        let ad = (an.first ^ an.last).leading_zeros();
        let bd = (bn.first ^ bn.last).leading_zeros();
        let common = (an.first ^ bn.first).leading_zeros();
        // Disjoint prefixes join in constant time; memoizing them adds no
        // shared traversal and can allocate more than the carrier tree itself.
        if common < ad.min(bd) {
            return if an.first < bn.first { self.branch(a,b) } else { self.branch(b,a) };
        }
        let key = if a < b { (a,b) } else { (b,a) };
        if let Some(&id) = ctx.get_hash_map(&self.unions, &key, "STEP carrier union lookup")? {
            return Ok(id);
        }
        // A Patricia descent has at most 64 branches, independent of graph depth.
        ctx.charge_work(1, "STEP carrier union branch")?;
        let result = if ad < bd {
            let side = usize::from((bn.first >> (63-ad)) & 1 != 0);
            let mut children = an.children;
            children[side] = self.union(children[side], b)?;
            if children == an.children { a } else { self.branch(children[0],children[1])? }
        } else if bd < ad {
            let side = usize::from((an.first >> (63-bd)) & 1 != 0);
            let mut children = bn.children;
            children[side] = self.union(a, children[side])?;
            if children == bn.children { b } else { self.branch(children[0],children[1])? }
        } else {
            let left = self.union(an.children[0],bn.children[0])?;
            let right = self.union(an.children[1],bn.children[1])?;
            let children = [left,right];
            if children == an.children { a } else if children == bn.children { b } else { self.branch(left,right)? }
        };
        self.storage.with_storage(|| ctx.insert_hash_map(&mut self.unions, key, result, "STEP carrier unions"))?;
        Ok(result)
    }

    pub(super) fn visit(&self, set: SetId, visitor: &mut impl FnMut(u64) -> Result<(), CodecError>) -> Result<(), CodecError> {
        if set == SetId::Empty { return Ok(()); }
        self.ctx.charge_work(1, "STEP carrier set traversal")?;
        let node = self.node(set);
        if node.count == 1 { visitor(node.first)?; } else {
            self.visit(node.children[0], visitor)?;
            self.visit(node.children[1], visitor)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct SetAccumulator { parts: [SetId;64] }

impl Default for SetAccumulator {
    fn default() -> Self { Self { parts: [SetId::Empty;64] } }
}

impl SetAccumulator {
    fn add(&mut self, mut set: SetId, sets: &mut CarrierSets<'_, '_>) -> Result<(),CodecError> {
        if set == SetId::Empty { return Ok(()); }
        for slot in &mut self.parts {
            if *slot == SetId::Empty { *slot = set; return Ok(()); }
            set = sets.union(std::mem::take(slot),set)?;
        }
        unreachable!("a collection cannot have more than u64::MAX members")
    }

    fn finish(self, sets: &mut CarrierSets<'_, '_>) -> Result<SetId,CodecError> {
        let mut result = SetId::Empty;
        for set in self.parts { result = sets.union(result,set)?; }
        Ok(result)
    }
}

pub(super) type Summary = [SetId; 3];

pub(super) struct RecordCarriers {
    pub(super) local: Summary,
    pub(super) children: Vec<(u64, usize)>,
}

struct Node {
    data: RecordCarriers,
    index: usize,
    low: usize,
    active: bool,
    component: u64,
    summary: Summary,
    height: usize,
    parent: Option<u64>,
    previous: Option<u64>,
    next: usize,
}

/// Tarjan completion installs one summary for every member of a component.
/// Record discovery is stage-wide. Depth-limited queries share (record, depth)
/// summaries and never reset graph discovery at a new representation root.
pub(super) struct CarrierGraph<'ctx, 'arena> {
    nodes: HashMap<u64, Node>,
    bounded: HashMap<(u64, usize), Summary>,
    pub(super) sets: CarrierSets<'ctx, 'arena>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'arena> CarrierGraph<'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            nodes: HashMap::new(), bounded: HashMap::new(), sets: CarrierSets::new(ctx)?, ctx,
            storage: ctx.reserve_scoped(0, "STEP carrier graph scratch")?,
        })
    }

    pub(super) fn ensure(&mut self, root: u64, mut inspect: impl FnMut(u64, &mut CarrierSets<'ctx, 'arena>, &mut ScopedReservation<'ctx>) -> Result<RecordCarriers, CodecError>) -> Result<(), CodecError> {
        let ctx = self.ctx;
        if ctx.contains_key_hash_map(&self.nodes, &root, "STEP carrier graph lookup")? { return Ok(()); }
        let mut current = None;
        let mut active_head = None;
        let mut next = Some((root,None));
        loop {
            if let Some((id,parent)) = next.take() {
                let index = self.nodes.len();
                self.storage.with_storage(|| ctx.insert_hash_map(&mut self.nodes, id, Node {
                    summary: Summary::default(), data: RecordCarriers { local: Summary::default(), children: Vec::new() }, index, low: index, active: true, component: id, height: 0,
                    parent, previous: active_head, next: 0,
                }, "STEP carrier graph nodes"))?;
                let mut data_storage = ctx.reserve_scoped(0, "STEP carrier record scratch")?;
                let data = inspect(id, &mut self.sets, &mut data_storage)?;
                self.storage.absorb(&mut data_storage)?;
                ctx.get_mut_hash_map(&mut self.nodes,&id,"STEP carrier graph update")?.expect("admitted record").data = data;
                active_head = Some(id);
                current = Some(id);
            }
            let Some(id) = current else { break; };
            ctx.charge_work(1, "STEP carrier graph frame traversal")?;
            let node = ctx.get_hash_map(&self.nodes, &id, "STEP carrier graph lookup")?.expect("active record");
            if let Some(&(child, _)) = node.data.children.get(node.next) {
                let child_index = ctx.get_hash_map(&self.nodes, &child, "STEP carrier graph child lookup")?.map(|child| (child.active,child.index));
                let node = ctx.get_mut_hash_map(&mut self.nodes, &id, "STEP carrier graph update")?.expect("active record");
                node.next += 1;
                match child_index {
                    Some((true,index)) => node.low = node.low.min(index),
                    Some((false,_)) => {},
                    None => next = Some((child,Some(id))),
                }
                continue;
            }
            let low = node.low;
            let index = node.index;
            let parent = node.parent;
            if low == index {
                let head = active_head;
                let mut member = head;
                while let Some(key) = member {
                    ctx.charge_work(1, "STEP carrier component completion")?;
                    let node = ctx.get_mut_hash_map(&mut self.nodes, &key, "STEP carrier graph update")?.expect("component record");
                    node.active = false;
                    node.component = id;
                    member = node.previous;
                    if key == id { active_head = member; break; }
                }
                let mut accumulated = [SetAccumulator::default();3];
                let mut span = 0usize;
                let mut suffix = 0usize;
                let mut member = head;
                while let Some(key) = member {
                    ctx.charge_work(1, "STEP carrier component summary traversal")?;
                    let node = ctx.get_hash_map(&self.nodes, &key, "STEP carrier graph lookup")?.expect("component record");
                    for (target, local) in accumulated.iter_mut().zip(node.data.local) { target.add(local,&mut self.sets)?; }
                    let mut step = 0;
                    let mut edges = node.data.children.iter();
                    for _ in 0..edges.len() {
                        let &(child, weight) = ctx.next_charged(&mut edges, "STEP carrier component edge traversal")?.ok_or_else(|| CodecError::malformed("STEP carrier edge source ended early"))?;
                        step = step.max(weight);
                        let child = ctx.get_hash_map(&self.nodes, &child, "STEP carrier graph child lookup")?.expect("complete child");
                        if child.component != id {
                            suffix = suffix.max(child.height);
                            for (target, local) in accumulated.iter_mut().zip(child.summary) { target.add(local,&mut self.sets)?; }
                        }
                    }
                    span = span.saturating_add(step);
                    member = node.previous;
                    if key == id { break; }
                }
                let mut summary = Summary::default();
                for (target, accumulated) in summary.iter_mut().zip(accumulated) { *target = accumulated.finish(&mut self.sets)?; }
                let height = span.saturating_add(suffix);
                let mut member = head;
                while let Some(key) = member {
                    ctx.charge_work(1, "STEP carrier component summary installation")?;
                    let node = ctx.get_mut_hash_map(&mut self.nodes, &key, "STEP carrier graph update")?.expect("component record");
                    node.summary = summary;
                    node.height = height;
                    member = node.previous;
                    if key == id { break; }
                }
            }
            if let Some(parent) = parent {
                let parent = ctx.get_mut_hash_map(&mut self.nodes, &parent, "STEP carrier graph update")?.expect("parent record");
                parent.low = parent.low.min(low);
            }
            current = parent;
        }
        Ok(())
    }

    fn resolved(&self, id: u64, remaining: usize) -> Result<Option<Summary>, CodecError> {
        if remaining == 0 { return Ok(Some(Summary::default())); }
        let node = self.ctx.get_hash_map(&self.nodes, &id, "STEP carrier summary lookup")?.expect("discovered record");
        if node.height < remaining { return Ok(Some(node.summary)); }
        Ok(self.ctx.get_hash_map(&self.bounded, &(id,remaining), "STEP bounded carrier summary lookup")?.copied())
    }

    pub(super) fn summary(&mut self, id: u64, remaining: usize) -> Result<Summary, CodecError> {
        if let Some(summary) = self.resolved(id,remaining)? { return Ok(summary); }
        let ctx = self.ctx;
        let _nested = ctx.enter_nested("STEP bounded carrier summary walk")?;
        let mut scratch = ctx.reserve_scoped(0,"STEP bounded carrier frames scratch")?;
        let mut frames = Vec::new();
        ctx.push_scoped_vec(&mut scratch,&mut frames,(id,remaining,0usize,self.local(id)?),"STEP bounded carrier frames")?;
        loop {
            ctx.charge_work(1,"STEP bounded carrier frame traversal")?;
            let (id,remaining,next,summary) = frames.last_mut().expect("bounded carrier frame");
            if let Some(&(child,weight)) = self.children(*id)?.get(*next) {
                ctx.charge_work(1,"STEP bounded carrier edge traversal")?;
                *next += 1;
                let child_remaining = remaining.saturating_sub(weight);
                if let Some(child) = self.resolved(child,child_remaining)? {
                    for (target,local) in summary.iter_mut().zip(child) { *target = self.sets.union(*target,local)?; }
                } else {
                    ctx.push_scoped_vec(&mut scratch,&mut frames,(child,child_remaining,0,self.local(child)?),"STEP bounded carrier frames")?;
                }
            } else {
                let (id,remaining,_,summary) = frames.pop().expect("completed bounded frame");
                self.storage.with_storage(||ctx.insert_hash_map(&mut self.bounded,(id,remaining),summary,"STEP bounded carrier summaries"))?;
                if let Some((_,_,_,parent)) = frames.last_mut() {
                    for (target,local) in parent.iter_mut().zip(summary) { *target = self.sets.union(*target,local)?; }
                } else { return Ok(summary); }
            }
        }
    }

    pub(super) fn height(&self, id: u64) -> Result<usize, CodecError> {
        Ok(self.ctx.get_hash_map(&self.nodes, &id, "STEP carrier graph lookup")?.expect("discovered record").height)
    }

    pub(super) fn local(&self, id: u64) -> Result<Summary, CodecError> {
        Ok(self.ctx.get_hash_map(&self.nodes, &id, "STEP carrier graph lookup")?.expect("discovered record").data.local)
    }

    pub(super) fn children(&self, id: u64) -> Result<&[(u64,usize)], CodecError> {
        Ok(&self.ctx.get_hash_map(&self.nodes, &id, "STEP carrier graph lookup")?.expect("discovered record").data.children)
    }
}

#[cfg(test)]
mod tests;
