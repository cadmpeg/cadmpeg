// SPDX-License-Identifier: Apache-2.0
//! Appendix C part identity, source framing and transactional decode output.
//!
//! Part staging records source facts. The per-byte B1 finalizer, not this
//! helper, derives unrepresented arena classes from the completed partition.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{PartCapture, View};
use cadmpeg_core::CodecError;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::UnknownId;

mod helper;
mod levels;
mod rescope;
mod sequential;

pub use helper::{
    CapturedRecordLayout, DecodeParts, FinalizedPartOutput, ParserPartOutput,
    PartDependencyPruning, PartOutput, PrunedPartCaptures,
};
pub use rescope::PartIdentityMap;
pub use sequential::SequentialParts;

/// The way Appendix C locates a part's next independently readable sibling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartFraming {
    /// An extent comes from independent framing bytes.
    Located,
    /// An independent terminator ends the part.
    Terminated,
    /// Failure consumes this part and the remaining parent content.
    Sequential,
}

/// A qualified level present in the immutable Appendix C table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartLevel(&'static str, PartFraming);

impl PartLevel {
    /// Admit an exact qualified Appendix C level name.
    pub fn parse(name: &str) -> Option<Self> {
        levels::LEVELS
            .iter()
            .find(|(level, _)| *level == name)
            .map(|(level, framing)| Self(level, *framing))
    }
    /// Qualified `codec.level` spelling from Appendix C.
    pub fn as_str(self) -> &'static str {
        self.0
    }
    /// The table's framing classification, without decoder inference.
    pub fn framing(self) -> PartFraming {
        self.1
    }
}

impl Serialize for PartLevel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}
impl<'de> Deserialize<'de> for PartLevel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| serde::de::Error::custom("part level is absent from Appendix C"))
    }
}
#[cfg(feature = "schema")]
impl JsonSchema for PartLevel {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PartLevel".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let values = levels::LEVELS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        schemars::json_schema!({"type":"string", "enum": values})
    }
}

/// Framing supplied by the Appendix C framer for one bounded invocation.
#[derive(Debug)]
pub struct PartSpec {
    level: PartLevel,
    framing_bytes: Vec<Range<u64>>,
}
impl PartSpec {
    /// Bind the table level and exact framing intervals.
    /// Framing intervals are checked against the actual view before decoding.
    pub fn new(level: PartLevel, framing_bytes: Vec<Range<u64>>) -> Self {
        Self {
            level,
            framing_bytes,
        }
    }
    pub(super) fn validate(&self, extent: View<'_>) -> Result<(), CodecError> {
        let start = cadmpeg_core::decode::u64_from_index(extent.start());
        let end = cadmpeg_core::decode::u64_from_index(extent.end());
        validate_framing(start, end, &self.framing_bytes).map_err(CodecError::malformed)
    }
}

/// Whether this reached part delivered records or is owned by one give-up home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PartState {
    /// The part's callback completed successfully.
    Decoded {},
    /// This part owns the sole unknown stub, retained bytes and unreadable loss.
    GivenUp {},
    /// A given-up ancestor owns this reached descendant's source bytes.
    Covered {
        /// Identity of the actual given-up ancestor.
        by: UnknownId,
    },
}

/// One reached Appendix C part in source fidelity, with no failure text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PartRecordWire")]
pub struct PartRecord {
    /// Exact Appendix C level.
    level: PartLevel,
    /// Stable stream containing this part.
    stream: String,
    /// Start in that stream.
    offset: u64,
    /// Exact bounded extent length.
    byte_len: u64,
    /// Parent invocation, when this is not a tree root.
    parent: Option<UnknownId>,
    /// Decoded, independently given up, or covered by an actual ancestor.
    state: PartState,
    /// Independent framing intervals, which are never B1.
    framing_bytes: Vec<Range<u64>>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PartRecordWire {
    /// Exact Appendix C level.
    level: PartLevel,
    /// Stable source stream name.
    stream: String,
    /// Start within the stream.
    offset: u64,
    /// Exact bounded length.
    byte_len: u64,
    /// Parent invocation when present.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    parent: Option<UnknownId>,
    /// Delivery or covering-ancestor state.
    state: PartState,
    /// Framing intervals excluded from B1.
    framing_bytes: Vec<Range<u64>>,
}
impl TryFrom<PartRecordWire> for PartRecord {
    type Error = String;
    fn try_from(wire: PartRecordWire) -> Result<Self, Self::Error> {
        if wire.stream.is_empty() {
            return Err("part stream is empty".into());
        }
        let end = wire
            .offset
            .checked_add(wire.byte_len)
            .ok_or("part source extent overflows")?;
        validate_framing(wire.offset, end, &wire.framing_bytes)?;
        Ok(Self {
            level: wire.level,
            stream: wire.stream,
            offset: wire.offset,
            byte_len: wire.byte_len,
            parent: wire.parent,
            state: wire.state,
            framing_bytes: wire.framing_bytes,
        })
    }
}
impl PartRecord {
    /// Exact table level.
    pub fn level(&self) -> PartLevel {
        self.level
    }
    /// Stable source stream name.
    pub fn stream(&self) -> &str {
        &self.stream
    }
    /// First source byte.
    pub fn offset(&self) -> u64 {
        self.offset
    }
    /// Exact part length.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }
    /// Parent part identity.
    pub fn parent(&self) -> Option<&UnknownId> {
        self.parent.as_ref()
    }
    /// Current delivery or covering-ancestor state.
    pub fn state(&self) -> &PartState {
        &self.state
    }
    /// Framing bytes, never untyped content.
    pub fn framing_bytes(&self) -> &[Range<u64>] {
        &self.framing_bytes
    }
    /// Exact content intervals after subtracting independent framing bytes.
    pub fn content_ranges(&self) -> Vec<Range<u64>> {
        let mut ranges = Vec::new();
        let mut start = self.offset;
        for framing in &self.framing_bytes {
            if start < framing.start {
                ranges.push(start..framing.start);
            }
            start = framing.end;
        }
        if let Some(end) = self.offset.checked_add(self.byte_len) {
            if start < end {
                ranges.push(start..end);
            }
        }
        ranges
    }
    pub(super) fn captured(capture: &PartCapture, spec: PartSpec, state: PartState) -> Self {
        Self {
            level: spec.level,
            stream: capture.stream().to_owned(),
            offset: capture.offset(),
            byte_len: capture.byte_len(),
            parent: None,
            state,
            framing_bytes: spec.framing_bytes,
        }
    }
}

fn validate_framing(start: u64, end: u64, ranges: &[Range<u64>]) -> Result<(), String> {
    let mut previous = start;
    for range in ranges {
        if range.start < start
            || range.end > end
            || range.start >= range.end
            || range.start < previous
        {
            return Err(
                "part framing intervals must be nonempty, ordered, disjoint and inside the part"
                    .into(),
            );
        }
        previous = range.end;
    }
    Ok(())
}

/// Check that parents form a forest and that each `Covered.by` names a
/// given-up proper ancestor.
///
/// One parent walk per node stops at the first node already resolved, so
/// cycle detection visits each node once. An Euler tour then gives every node
/// an interval; `by` is a proper ancestor exactly when its interval strictly
/// encloses the node's interval.
pub(super) fn validate_tree(tree: &BTreeMap<UnknownId, PartRecord>) -> Result<(), String> {
    const UNSEEN: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;
    let ids = tree.keys().collect::<Vec<_>>();
    let index = |id: &UnknownId| ids.binary_search(&id).ok();
    let mut parents = Vec::with_capacity(ids.len());
    for node in tree.values() {
        parents.push(match &node.parent {
            Some(parent) => Some(index(parent).ok_or("part tree parent is absent")?),
            None => None,
        });
    }
    let mut state = vec![UNSEEN; ids.len()];
    let mut path = Vec::new();
    for start in 0..ids.len() {
        let mut current = Some(start);
        while let Some(node) = current {
            match state[node] {
                DONE => break,
                ON_PATH => return Err("part tree has an ancestry cycle".into()),
                _ => {
                    state[node] = ON_PATH;
                    path.push(node);
                    current = parents[node];
                }
            }
        }
        for node in path.drain(..) {
            state[node] = DONE;
        }
    }
    let mut children = vec![Vec::new(); ids.len()];
    let mut roots = Vec::new();
    for (node, parent) in parents.iter().enumerate() {
        match parent {
            Some(parent) => children[*parent].push(node),
            None => roots.push(node),
        }
    }
    let mut enter = vec![0_usize; ids.len()];
    let mut leave = vec![0_usize; ids.len()];
    let mut clock = 0_usize;
    let mut stack = Vec::new();
    for root in roots {
        stack.push((root, false));
        while let Some((node, finished)) = stack.pop() {
            clock += 1;
            if finished {
                leave[node] = clock;
            } else {
                enter[node] = clock;
                stack.push((node, true));
                stack.extend(children[node].iter().map(|&child| (child, false)));
            }
        }
    }
    for (node, part) in tree.values().enumerate() {
        if let PartState::Covered { by } = &part.state {
            let covering = index(by).filter(|&ancestor| {
                enter[ancestor] < enter[node]
                    && leave[node] < leave[ancestor]
                    && matches!(tree[by].state, PartState::GivenUp {})
            });
            if covering.is_none() {
                return Err("covered part must name a given-up ancestor".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
