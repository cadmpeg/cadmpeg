// SPDX-License-Identifier: Apache-2.0
//! Native Keywords feature ancestry validation.

use crate::history::write::xml::valid_xml_name;
use crate::records::Feature;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

pub(crate) fn validate_feature_graph(features: &[Feature]) -> Result<(), CodecError> {
    if features
        .iter()
        .any(|feature| !valid_xml_name(&feature.xml_tag))
    {
        return Err(CodecError::malformed("invalid feature XML element name"));
    }
    let mut by_source = HashMap::new();
    let mut source_count = 0;
    for (slot, feature) in features.iter().enumerate() {
        if let Some(id) = feature.source_id {
            source_count += 1;
            by_source.insert(id, slot);
        }
    }
    if by_source.len() != source_count {
        return Err(CodecError::malformed("duplicate feature source id"));
    }
    let mut by_record = HashMap::new();
    for (slot, feature) in features.iter().enumerate() {
        by_record.insert(feature.id.as_str(), slot);
    }
    if by_record.len() != features.len() {
        return Err(CodecError::malformed("duplicate feature record id"));
    }
    let mut sources = features
        .iter()
        .any(|feature| feature.parent_source_id().is_some())
        .then(|| AncestryWalk::new(features.len()));
    let mut records = features
        .iter()
        .any(|feature| feature.tree_parent_record_id().is_some())
        .then(|| AncestryWalk::new(features.len()));
    for slot in 0..features.len() {
        if let Some(walk) = &mut sources {
            walk.walk(
                slot,
                |slot| {
                    features[slot]
                        .parent_source_id()
                        .map(|id| {
                            by_source.get(&id).copied().ok_or_else(|| {
                                CodecError::malformed("feature references missing parent")
                            })
                        })
                        .transpose()
                },
                "feature parent cycle",
            )?;
        }
        if let Some(walk) = &mut records {
            walk.walk(
                slot,
                |slot| {
                    features[slot]
                        .tree_parent_record_id()
                        .map(|id| {
                            by_record.get(id).copied().ok_or_else(|| {
                                CodecError::malformed("feature references missing tree parent")
                            })
                        })
                        .transpose()
                },
                "feature tree cycle",
            )?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Mark {
    Unseen,
    Active,
    Rooted,
}

/// A completed path is reused by every descendant that reaches it.
struct AncestryWalk {
    marks: Vec<Mark>,
    path: Vec<usize>,
}

impl AncestryWalk {
    fn new(count: usize) -> Self {
        Self {
            marks: (0..count).map(|_| Mark::Unseen).collect(),
            path: Vec::new(),
        }
    }

    fn walk(
        &mut self,
        start: usize,
        mut parent: impl FnMut(usize) -> Result<Option<usize>, CodecError>,
        cycle: &'static str,
    ) -> Result<(), CodecError> {
        self.path.clear();
        let mut next = Some(start);
        while let Some(slot) = next {
            match self.marks[slot] {
                Mark::Rooted => break,
                Mark::Active => return Err(CodecError::malformed(cycle)),
                Mark::Unseen => {}
            }
            self.marks[slot] = Mark::Active;
            self.path.push(slot);
            next = parent(slot)?;
        }
        for &slot in &self.path {
            self.marks[slot] = Mark::Rooted;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
