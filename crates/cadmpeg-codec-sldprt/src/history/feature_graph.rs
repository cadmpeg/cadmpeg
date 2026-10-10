// SPDX-License-Identifier: Apache-2.0
//! Native Keywords feature ancestry validation.

use std::collections::HashMap;

use cadmpeg_core::CodecError;

use crate::history::write::xml::valid_xml_name;

pub(crate) fn validate_feature_graph(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[crate::records::Feature],
) -> Result<(), CodecError> {
    let ((), storage) = ctx.with_scoped_storage("SLDPRT feature graph workspace", || {
        if ctx.any_by(
            features,
            |feature| Ok(!valid_xml_name(ctx, &feature.xml_tag)?),
            "scan SLDPRT validate_feature_graph values",
        )? {
            return Err(CodecError::Malformed(
                "invalid feature XML element name".into(),
            ));
        }
        let mut source_edges = false;
        let mut source_count = 0;
        let mut by_id = HashMap::new();
        for (position, feature) in ctx
            .admit_iter(features, "validate SLDPRT feature graph")?
            .enumerate()
        {
            source_edges |= feature.parent_source_id().is_some();
            if let Some(id) = feature.source_id {
                source_count += 1;
                ctx.insert_hash_map(&mut by_id, id, position, "index SLDPRT feature graph")?;
            }
        }
        if by_id.len() != source_count {
            return Err(CodecError::Malformed("duplicate feature source id".into()));
        }
        let mut tree_edges = false;
        let mut by_record = HashMap::new();
        for (position, feature) in ctx
            .admit_iter(features, "scan SLDPRT validate_feature_graph values")?
            .enumerate()
        {
            tree_edges |= feature.tree_parent_record_id().is_some();
            ctx.insert_hash_map(
                &mut by_record,
                feature.id.as_str(),
                position,
                "index SLDPRT feature graph",
            )?;
        }
        if by_record.len() != features.len() {
            return Err(CodecError::Malformed("duplicate feature record id".into()));
        }
        if !source_edges && !tree_edges {
            return Ok(());
        }
        let mut source_walk = source_edges
            .then(|| AncestryWalk::new(ctx, features.len()))
            .transpose()?;
        let mut tree_walk = tree_edges
            .then(|| AncestryWalk::new(ctx, features.len()))
            .transpose()?;
        for position in ctx.admit_iter(
            0..features.len(),
            "scan SLDPRT validate_feature_graph values",
        )? {
            if let Some(source_walk) = &mut source_walk {
                source_walk.walk(
                    ctx,
                    features,
                    position,
                    &by_id,
                    crate::records::Feature::parent_source_id,
                    ("feature references missing parent", "feature parent cycle"),
                )?;
            }
            if let Some(tree_walk) = &mut tree_walk {
                tree_walk.walk(
                    ctx,
                    features,
                    position,
                    &by_record,
                    crate::records::Feature::tree_parent_record_id,
                    (
                        "feature references missing tree parent",
                        "feature tree cycle",
                    ),
                )?;
            }
        }
        Ok(())
    })?;
    drop(storage);
    Ok(())
}

/// Ancestry walks over one parent relation in which every feature is walked
/// once: a walk ends at a root or at a feature an earlier walk proved reaches
/// one.
struct AncestryWalk {
    /// `0` for an unwalked feature, `w + 1` while walk `w` is on it, and
    /// `usize::MAX` once it is proven to reach a root.
    marks: Vec<usize>,
    path: Vec<usize>,
}

impl AncestryWalk {
    const OPERATION: &'static str = "walk SLDPRT feature graph";
    const ROOTED: usize = usize::MAX;

    fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        count: usize,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            marks: ctx.collect_indexed_vec(count, Self::OPERATION, |_| Ok(0))?,
            path: Vec::new(),
        })
    }

    /// Walk the ancestry of the feature at `start`, refusing a missing parent
    /// or a cycle with the given messages.
    fn walk<'f, K: Copy + Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost>(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        features: &'f [crate::records::Feature],
        start: usize,
        index: &HashMap<K, usize>,
        parent: impl Fn(&'f crate::records::Feature) -> Option<K>,
        (missing, cycle): (&'static str, &'static str),
    ) -> Result<(), CodecError> {
        let walk_mark = start
            .checked_add(1)
            .filter(|mark| *mark != Self::ROOTED)
            .ok_or_else(|| ctx.refuse_codec_limit(Self::OPERATION, u64::MAX, u64::MAX))?;
        self.path.clear();
        let mut next = features.get(start).and_then(&parent);
        while let Some(key) = next {
            ctx.charge_work(1, Self::OPERATION)?;
            let position = *ctx
                .get_hash_map(index, &key, Self::OPERATION)?
                .ok_or_else(|| CodecError::Malformed(missing.into()))?;
            let mark = self
                .marks
                .get_mut(position)
                .ok_or_else(|| CodecError::Malformed(missing.into()))?;
            if *mark == Self::ROOTED {
                break;
            }
            if *mark == walk_mark {
                return Err(CodecError::Malformed(cycle.into()));
            }
            *mark = walk_mark;
            ctx.push_vec(&mut self.path, position, Self::OPERATION)?;
            next = features.get(position).and_then(&parent);
        }
        for position in ctx
            .admit_iter(&self.path, Self::OPERATION)?
            .copied()
            .chain([start])
        {
            if let Some(mark) = self.marks.get_mut(position) {
                *mark = Self::ROOTED;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
