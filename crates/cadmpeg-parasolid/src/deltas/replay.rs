// SPDX-License-Identifier: Apache-2.0
//! Main-line replay of a partition's transmitted deltas.
//!
//! A main-line delta is backward: it turns the state at its mark into the
//! state at the mark's preceding mark. The partition and its deltas share one
//! node-index space, so one step removes the nodes the delta's map lists, adds
//! the images it restores, and overwrites shared nodes with their older
//! images.

use std::collections::{BTreeMap, BTreeSet};

use super::pmark_state::PmarkState;
use super::section::{MapEntry, SectionKind};
use crate::records::deltas::node::DeltasNode;
use crate::records::deltas::{DeltasEnd, DeltasRecords, EventContent, Metadata};
use crate::records::node_type::{BODY, EDGE, FACE, GROUP, VERTEX};
use crate::records::primary::Node;
use crate::records::{PrimaryRecords, SchemaPlan};

/// Why a Deltas stream yields no main line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainLineError {
    /// The stream has no partition and current-mark prefix.
    MissingPrefix,
    /// Two marks in the stream share one node index.
    DuplicateMark {
        /// Node index of the mark.
        xmt: u32,
    },
    /// A mark on the main line names a preceding mark absent from the stream.
    MissingMark {
        /// Node index of the absent mark.
        xmt: u32,
    },
    /// The `preceding` links from the current mark return to a visited mark.
    Cycle {
        /// Node index of the revisited mark.
        xmt: u32,
    },
    /// The stream stopped before the sections of a main-line delta closed,
    /// or ended after a PMARK section whose last mark is not the initial
    /// mark.
    IncompleteDelta {
        /// Identifier of the mark whose delta is incomplete.
        identifier: u32,
    },
    /// The initial mark carries delta content.
    InitialDelta {
        /// Identifier of the initial mark.
        identifier: u32,
    },
    /// A main-line mark other than the initial mark has a forward delta.
    ForwardDelta {
        /// Identifier of the mark.
        identifier: u32,
    },
}

impl std::fmt::Display for MainLineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPrefix => formatter.write_str("the deltas stream has no partition prefix"),
            Self::DuplicateMark { xmt } => write!(formatter, "two marks share node index {xmt}"),
            Self::MissingMark { xmt } => {
                write!(
                    formatter,
                    "the preceding mark {xmt} is absent from the stream"
                )
            }
            Self::Cycle { xmt } => write!(formatter, "the preceding links revisit mark {xmt}"),
            Self::IncompleteDelta { identifier } => {
                write!(formatter, "the delta of mark {identifier} is incomplete")
            }
            Self::InitialDelta { identifier } => {
                write!(
                    formatter,
                    "the initial mark {identifier} carries delta content"
                )
            }
            Self::ForwardDelta { identifier } => {
                write!(
                    formatter,
                    "the main-line mark {identifier} has a forward delta"
                )
            }
        }
    }
}

impl std::error::Error for MainLineError {}

/// Why a replay step was refused. The state stays at the mark before the step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    /// The partition's node sequence did not reach its terminator.
    PartitionIncomplete,
    /// Two partition nodes share one node index.
    DuplicateNode {
        /// Node index.
        xmt: u32,
    },
    /// A map entry names a node absent from the state.
    RemovedAbsent {
        /// Identifier of the mark whose delta was applied.
        mark: u32,
        /// Node index.
        xmt: u32,
    },
    /// A map entry names a node of another type.
    RemovedKind {
        /// Identifier of the mark whose delta was applied.
        mark: u32,
        /// Node index.
        xmt: u32,
        /// Node type the map entry names.
        expected: u16,
        /// Node type in the state.
        found: u16,
    },
    /// A restored image names a node present in the state.
    RestoredPresent {
        /// Identifier of the mark whose delta was applied.
        mark: u32,
        /// Node index.
        xmt: u32,
    },
    /// An older image names a node absent from the state.
    OlderAbsent {
        /// Identifier of the mark whose delta was applied.
        mark: u32,
        /// Node index.
        xmt: u32,
    },
    /// Two older images of one delta name one node.
    OlderRepeated {
        /// Identifier of the mark whose delta was applied.
        mark: u32,
        /// Node index.
        xmt: u32,
    },
    /// An older image has another type than the node it overwrites.
    OlderKind {
        /// Identifier of the mark whose delta was applied.
        mark: u32,
        /// Node index.
        xmt: u32,
        /// Node type of the older image.
        expected: u16,
        /// Node type in the state.
        found: u16,
    },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PartitionIncomplete => {
                formatter.write_str("the partition node sequence is incomplete")
            }
            Self::DuplicateNode { xmt } => write!(formatter, "two partition nodes share index {xmt}"),
            Self::RemovedAbsent { mark, xmt } => {
                write!(formatter, "delta of mark {mark} removes absent node {xmt}")
            }
            Self::RemovedKind {
                mark,
                xmt,
                expected,
                found,
            } => write!(
                formatter,
                "delta of mark {mark} removes node {xmt} as type {expected}, the state holds type {found}"
            ),
            Self::RestoredPresent { mark, xmt } => {
                write!(formatter, "delta of mark {mark} restores present node {xmt}")
            }
            Self::OlderAbsent { mark, xmt } => {
                write!(formatter, "delta of mark {mark} overwrites absent node {xmt}")
            }
            Self::OlderRepeated { mark, xmt } => {
                write!(formatter, "delta of mark {mark} overwrites node {xmt} twice")
            }
            Self::OlderKind {
                mark,
                xmt,
                expected,
                found,
            } => write!(
                formatter,
                "delta of mark {mark} overwrites node {xmt} with type {expected}, the state holds type {found}"
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

/// Why a state does not write as a partition transmit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransmitError {
    /// A node's field list differs from the field list its type was written
    /// with.
    SchemaMismatch {
        /// Node type.
        kind: u16,
    },
    /// The written stream does not frame back to the state's node types and
    /// indices.
    Reframe,
}

impl std::fmt::Display for TransmitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaMismatch { kind } => write!(
                formatter,
                "a type {kind} node has another field list than the type was written with"
            ),
            Self::Reframe => {
                formatter.write_str("the written partition does not frame back to the state")
            }
        }
    }
}

impl std::error::Error for TransmitError {}

/// Why the state at a wanted mark was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    /// A step toward the mark was refused. The replay stops: no earlier
    /// state follows.
    Replay(ReplayError),
    /// The state at the mark does not write as a partition transmit. Earlier
    /// states follow.
    Transmit {
        /// Identifier of the mark.
        mark: u32,
        /// Why the state does not write.
        error: TransmitError,
    },
}

impl std::fmt::Display for StateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Replay(error) => write!(formatter, "{error}"),
            Self::Transmit { mark, error } => write!(formatter, "state at mark {mark}: {error}"),
        }
    }
}

impl std::error::Error for StateError {}

/// The content of one delta.
#[derive(Debug, Clone, Default)]
struct Content<'r> {
    restored: Vec<DeltasNode<'r>>,
    removed: Vec<MapEntry>,
    older: Vec<DeltasNode<'r>>,
}

impl Content<'_> {
    fn is_empty(&self) -> bool {
        self.restored.is_empty() && self.removed.is_empty() && self.older.is_empty()
    }
}

/// One main-line delta: the change from the state at [`Delta::mark`] to the
/// state at [`Delta::target`].
#[derive(Debug, Clone)]
pub struct Delta<'r> {
    mark: PmarkState,
    target: PmarkState,
    content: Content<'r>,
}

impl<'r> Delta<'r> {
    /// The mark the delta belongs to; the step starts at its state.
    #[must_use]
    pub const fn mark(&self) -> PmarkState {
        self.mark
    }

    /// The preceding mark; the step ends at its state.
    #[must_use]
    pub const fn target(&self) -> PmarkState {
        self.target
    }

    /// Images of the nodes absent at the mark and present at the target.
    #[must_use]
    pub fn restored(&self) -> &[DeltasNode<'r>] {
        &self.content.restored
    }

    /// The nodes present at the mark and absent at the target: the nodes the
    /// change from the target to the mark creates.
    #[must_use]
    pub fn created(&self) -> &[MapEntry] {
        &self.content.removed
    }

    /// Images, at the target, of nodes present at both marks.
    #[must_use]
    pub fn older(&self) -> &[DeltasNode<'r>] {
        &self.content.older
    }

    /// Node indices of the GROUP nodes the change from the target to the mark
    /// creates.
    pub fn created_groups(&self) -> impl Iterator<Item = u32> + '_ {
        self.content
            .removed
            .iter()
            .filter(|entry| entry.kind() == GROUP)
            .map(|entry| entry.index())
    }

    /// Whether the states at the mark and at the target are equal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

/// One closed section of a Deltas stream.
struct Section<'r> {
    kind: SectionKind,
    marks: Vec<PmarkState>,
    map: Vec<MapEntry>,
    nodes: Vec<DeltasNode<'r>>,
}

impl Section<'_> {
    const fn open() -> Self {
        Self {
            kind: SectionKind::Leading,
            marks: Vec::new(),
            map: Vec::new(),
            nodes: Vec::new(),
        }
    }
}

/// The closed sections of `records` and the prefix's current mark.
fn sections<'r>(
    records: &'r DeltasRecords<'_>,
) -> Result<(PmarkState, Vec<Section<'r>>), MainLineError> {
    let mut current = None;
    let mut out = Vec::new();
    let mut open = Section::open();
    for event in records.events() {
        match event.content() {
            EventContent::PartitionPrefix(prefix) => current = Some(prefix.current_pmark()),
            EventContent::Instance(_) => {
                open.nodes
                    .extend(DeltasNode::of_event(records.source(), event));
            }
            EventContent::Metadata(Metadata::Map { entries, .. }) => {
                open.map.extend_from_slice(entries);
            }
            EventContent::Metadata(Metadata::Pmark(mark)) => open.marks.push(*mark),
            EventContent::Metadata(Metadata::Terminator { closes, .. }) => {
                open.kind = *closes;
                out.push(std::mem::replace(&mut open, Section::open()));
            }
            EventContent::Header(_) | EventContent::Metadata(Metadata::FinalTerminator) => {}
        }
    }
    Ok((current.ok_or(MainLineError::MissingPrefix)?, out))
}

/// The main line of one partition's Deltas stream: the marks from the current
/// mark to the initial mark, and the delta between each adjacent pair.
#[derive(Debug, Clone)]
pub struct MainLine<'r> {
    marks: Vec<PmarkState>,
    deltas: Vec<Delta<'r>>,
    off_line: usize,
}

impl<'r> MainLine<'r> {
    /// Assign the stream's sections to its marks and follow `preceding` from
    /// the current mark to the initial mark.
    ///
    /// The leading section and the first map section hold the current mark's
    /// delta. Each PMARK section's nodes and the map section after it hold
    /// the delta of the section's last mark; its other marks have empty
    /// deltas. A PMARK section that ends the stream holds the initial mark
    /// last.
    ///
    /// # Errors
    ///
    /// Returns [`MainLineError`] when the stream has no prefix, a mark is
    /// repeated or absent, the links cycle, a main-line delta is incomplete
    /// or forward, or the initial mark carries content.
    pub fn read(records: &'r DeltasRecords<'_>) -> Result<Self, MainLineError> {
        let complete = *records.end() == DeltasEnd::Complete;
        let (current, sections) = sections(records)?;
        let mut sections = sections.into_iter().peekable();
        let mut by_xmt = BTreeMap::<u32, (PmarkState, Option<Content<'r>>)>::new();
        let mut insert = |mark: PmarkState, content| {
            by_xmt
                .insert(mark.xmt(), (mark, content))
                .map_or(Ok(()), |_| {
                    Err(MainLineError::DuplicateMark { xmt: mark.xmt() })
                })
        };
        let leading = sections
            .next_if(|section| section.kind == SectionKind::Leading)
            .map_or_else(Vec::new, |section| section.nodes);
        let first = sections.next_if(|section| section.kind == SectionKind::Map);
        let content = first.map(|map| Content {
            restored: leading,
            removed: map.map,
            older: map.nodes,
        });
        insert(current, content)?;
        while let Some(section) = sections.next() {
            if section.kind != SectionKind::Pmark {
                continue;
            }
            let map = sections.next_if(|next| next.kind == SectionKind::Map);
            let Some((last, earlier)) = section.marks.split_last() else {
                continue;
            };
            for mark in earlier {
                insert(*mark, Some(Content::default()))?;
            }
            let content = match map {
                Some(map) => Some(Content {
                    restored: section.nodes,
                    removed: map.map,
                    older: map.nodes,
                }),
                None if complete && last.preceding().is_none() => Some(Content {
                    restored: section.nodes,
                    ..Content::default()
                }),
                None => None,
            };
            insert(*last, content)?;
        }
        let total = by_xmt.len();
        let mut marks = Vec::new();
        let mut contents = Vec::new();
        let mut visited = BTreeSet::new();
        let mut xmt = current.xmt();
        loop {
            if !visited.insert(xmt) {
                return Err(MainLineError::Cycle { xmt });
            }
            let (mark, content) = by_xmt
                .remove(&xmt)
                .ok_or(MainLineError::MissingMark { xmt })?;
            marks.push(mark);
            let Some(preceding) = mark.preceding() else {
                if content.is_some_and(|content| !content.is_empty()) {
                    return Err(MainLineError::InitialDelta {
                        identifier: mark.identifier(),
                    });
                }
                break;
            };
            if mark.delta_is_forward() {
                return Err(MainLineError::ForwardDelta {
                    identifier: mark.identifier(),
                });
            }
            contents.push(content.ok_or(MainLineError::IncompleteDelta {
                identifier: mark.identifier(),
            })?);
            xmt = preceding;
        }
        let deltas = contents
            .into_iter()
            .zip(marks.windows(2))
            .map(|(content, pair)| Delta {
                mark: pair[0],
                target: pair[1],
                content,
            })
            .collect();
        Ok(Self {
            off_line: total - marks.len(),
            marks,
            deltas,
        })
    }

    /// The main-line marks from the current mark to the initial mark.
    #[must_use]
    pub fn marks(&self) -> &[PmarkState] {
        &self.marks
    }

    /// The deltas in application order; delta `k` runs from mark `k` to
    /// mark `k + 1`.
    #[must_use]
    pub fn deltas(&self) -> &[Delta<'r>] {
        &self.deltas
    }

    /// The number of marks in the stream that are not on the main line.
    #[must_use]
    pub const fn off_line(&self) -> usize {
        self.off_line
    }

    /// The deltas whose change creates the node at `xmt`.
    pub fn deltas_creating(&self, xmt: u32) -> impl Iterator<Item = &Delta<'r>> + '_ {
        self.deltas
            .iter()
            .filter(move |delta| delta.created().iter().any(|entry| entry.index() == xmt))
    }

    /// Start a replay at the current mark from the partition's node sequence.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError::PartitionIncomplete`] when the partition's node
    /// sequence did not reach its terminator, and
    /// [`ReplayError::DuplicateNode`] when two of its nodes share an index.
    pub fn replay<'a>(
        &'a self,
        partition: &'r PrimaryRecords<'_>,
    ) -> Result<Replay<'a, 'r>, ReplayError> {
        let (header, end) = partition.frame().ok_or(ReplayError::PartitionIncomplete)?;
        let mut nodes = BTreeMap::new();
        let mut declarations = BTreeMap::new();
        for node in partition.nodes() {
            let xmt = node.index();
            declarations
                .entry(node.kind())
                .or_insert_with(|| node.declaration());
            if nodes.insert(xmt, StateNode::Partition(node)).is_some() {
                return Err(ReplayError::DuplicateNode { xmt });
            }
        }
        Ok(Replay {
            line: self,
            state: State { nodes },
            applied: 0,
            frame: Frame {
                header,
                end,
                declarations,
            },
        })
    }
}

impl<'r> MainLine<'r> {
    /// The partition transmit of the state at each main-line mark whose
    /// identifier `wanted` holds, from the current mark back. The replay ends
    /// at the earliest wanted mark.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError`] where [`MainLine::replay`] does.
    pub fn states<'a>(
        &'a self,
        partition: &'r PrimaryRecords<'_>,
        wanted: BTreeSet<u32>,
    ) -> Result<States<'a, 'r>, ReplayError> {
        Ok(States {
            replay: self.replay(partition)?,
            wanted,
            stepped: false,
            done: false,
        })
    }
}

/// The states at the wanted marks of one main line, from the current mark
/// back: each mark with its partition transmit. After a refused step the
/// iterator ends.
#[derive(Debug, Clone)]
pub struct States<'a, 'r> {
    replay: Replay<'a, 'r>,
    wanted: BTreeSet<u32>,
    stepped: bool,
    done: bool,
}

impl Iterator for States<'_, '_> {
    type Item = Result<(PmarkState, Vec<u8>), StateError>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.done && !self.wanted.is_empty() {
            if self.stepped {
                match self.replay.step() {
                    Ok(Some(_)) => {}
                    Ok(None) => self.done = true,
                    Err(error) => {
                        self.done = true;
                        return Some(Err(StateError::Replay(error)));
                    }
                }
            }
            self.stepped = true;
            let mark = self.replay.mark();
            if !self.done && self.wanted.remove(&mark.identifier()) {
                return Some(
                    self.replay
                        .transmit()
                        .map(|bytes| (mark, bytes))
                        .map_err(|error| StateError::Transmit {
                            mark: mark.identifier(),
                            error,
                        }),
                );
            }
        }
        None
    }
}

/// One node of a replayed state: a partition node, or a delta's image.
#[derive(Debug, Clone)]
pub enum StateNode<'r> {
    /// A node of the partition's node sequence.
    Partition(Node<'r>),
    /// A restored or older image from the Deltas stream.
    Delta(DeltasNode<'r>),
}

impl<'r> StateNode<'r> {
    /// The partition node, when the node is one.
    #[must_use]
    pub const fn as_partition(&self) -> Option<&Node<'r>> {
        match self {
            Self::Partition(node) => Some(node),
            Self::Delta(_) => None,
        }
    }

    /// The delta image, when the node is one.
    #[must_use]
    pub const fn as_delta(&self) -> Option<&DeltasNode<'r>> {
        match self {
            Self::Partition(_) => None,
            Self::Delta(node) => Some(node),
        }
    }

    /// The node type.
    #[must_use]
    pub fn kind(&self) -> u16 {
        match self {
            Self::Partition(node) => node.kind(),
            Self::Delta(node) => node.kind(),
        }
    }

    /// The encoded index of every transmitted pointer element, in field
    /// order; the null index is `1`.
    #[must_use]
    pub fn pointer_targets(&self) -> Option<Vec<u32>> {
        match self {
            Self::Partition(node) => node.pointer_targets(),
            Self::Delta(node) => node.pointer_targets(),
        }
    }
}

/// The node set at one mark, keyed by node index.
#[derive(Debug, Clone)]
pub struct State<'r> {
    nodes: BTreeMap<u32, StateNode<'r>>,
}

impl<'r> State<'r> {
    /// The node at `xmt`.
    #[must_use]
    pub fn get(&self, xmt: u32) -> Option<&StateNode<'r>> {
        self.nodes.get(&xmt)
    }

    /// Every node, in node-index order.
    pub fn nodes(&self) -> impl Iterator<Item = &StateNode<'r>> + '_ {
        self.nodes.values()
    }

    /// The number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the state holds no node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The number of nodes of type `kind`.
    #[must_use]
    pub fn count(&self, kind: u16) -> usize {
        self.nodes
            .values()
            .filter(|node| node.kind() == kind)
            .count()
    }

    /// Whether the state holds no BODY, FACE, EDGE, or VERTEX node.
    #[must_use]
    pub fn is_empty_part(&self) -> bool {
        !self
            .nodes
            .values()
            .any(|node| matches!(node.kind(), BODY | FACE | EDGE | VERTEX))
    }

    /// Each non-null pointer whose target is absent, as the holding node's
    /// index and the target index. A node whose pointer fields do not decode
    /// yields target `0`.
    pub fn dangling(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.nodes.iter().flat_map(move |(&xmt, node)| {
            let targets = node.pointer_targets();
            let undecoded = targets.is_none().then_some((xmt, 0));
            targets
                .unwrap_or_default()
                .into_iter()
                .filter(move |target| *target != 1 && !self.nodes.contains_key(target))
                .map(move |target| (xmt, target))
                .chain(undecoded)
        })
    }

    /// Apply `delta` when every entry is consistent with the state.
    fn apply(&mut self, delta: &Delta<'r>) -> Result<(), ReplayError> {
        let mark = delta.mark.identifier();
        let mut removed = BTreeMap::new();
        for entry in &delta.content.removed {
            let xmt = entry.index();
            let found = self
                .nodes
                .get(&xmt)
                .filter(|_| !removed.contains_key(&xmt))
                .ok_or(ReplayError::RemovedAbsent { mark, xmt })?
                .kind();
            if found != entry.kind() {
                return Err(ReplayError::RemovedKind {
                    mark,
                    xmt,
                    expected: entry.kind(),
                    found,
                });
            }
            removed.insert(xmt, found);
        }
        let mut restored = BTreeSet::new();
        for node in &delta.content.restored {
            let xmt = node.index();
            if self.nodes.contains_key(&xmt) || !restored.insert(xmt) {
                return Err(ReplayError::RestoredPresent { mark, xmt });
            }
        }
        let mut older = BTreeSet::new();
        for node in &delta.content.older {
            let xmt = node.index();
            if !older.insert(xmt) {
                return Err(ReplayError::OlderRepeated { mark, xmt });
            }
            let found = self
                .nodes
                .get(&xmt)
                .filter(|_| !removed.contains_key(&xmt))
                .ok_or(ReplayError::OlderAbsent { mark, xmt })?
                .kind();
            if found != node.kind() {
                return Err(ReplayError::OlderKind {
                    mark,
                    xmt,
                    expected: node.kind(),
                    found,
                });
            }
        }
        for xmt in removed.keys() {
            self.nodes.remove(xmt);
        }
        for node in delta.content.restored.iter().chain(&delta.content.older) {
            self.nodes
                .insert(node.index(), StateNode::Delta(node.clone()));
        }
        Ok(())
    }
}

/// A walk down one main line: the state at each mark, from the current mark
/// to the initial mark.
#[derive(Debug, Clone)]
pub struct Replay<'a, 'r> {
    line: &'a MainLine<'r>,
    state: State<'r>,
    applied: usize,
    frame: Frame<'r>,
}

/// The partition bytes a written state reuses.
#[derive(Debug, Clone)]
struct Frame<'r> {
    /// The bytes before the partition's first node.
    header: &'r [u8],
    /// The partition's terminator and the bytes after it.
    end: &'r [u8],
    /// The declaration of each node type, from its first node in the
    /// partition.
    declarations: BTreeMap<u16, Option<&'r [u8]>>,
}

impl<'r> Replay<'_, 'r> {
    /// The mark whose state [`Replay::state`] holds.
    #[must_use]
    pub fn mark(&self) -> PmarkState {
        self.line.marks[self.applied]
    }

    /// The state at [`Replay::mark`].
    #[must_use]
    pub const fn state(&self) -> &State<'r> {
        &self.state
    }

    /// Apply the next delta and return the mark it reaches, or `None` at the
    /// initial mark.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError`] when the delta is inconsistent with the state;
    /// the state then stays at [`Replay::mark`].
    pub fn step(&mut self) -> Result<Option<PmarkState>, ReplayError> {
        let Some(delta) = self.line.deltas.get(self.applied) else {
            return Ok(None);
        };
        self.state.apply(delta)?;
        self.applied += 1;
        Ok(Some(self.mark()))
    }

    /// The state at [`Replay::mark`] as a partition transmit: the
    /// partition's header, the state's partition nodes in partition order,
    /// its delta images in node-index order, and the partition's terminator.
    /// Each node type carries its declaration at its first node. A delta
    /// image loses the status byte after each pointer element, and each of
    /// its Hvec values keeps the `(x, y, z)` row that a partition stores.
    /// At the current mark the result is the partition's bytes.
    ///
    /// # Errors
    ///
    /// Returns [`TransmitError`] when two nodes of one type have different
    /// field lists, or when the written stream does not frame back to the
    /// state's node types and indices.
    pub fn transmit(&self) -> Result<Vec<u8>, TransmitError> {
        let mut partition = Vec::new();
        let mut images = Vec::new();
        for node in self.state.nodes() {
            match node {
                StateNode::Partition(node) => partition.push(node),
                StateNode::Delta(node) => images.push(node),
            }
        }
        partition.sort_by_key(|node| node.range().start);
        let mut out = self.frame.header.to_vec();
        let mut written = BTreeMap::<u16, &SchemaPlan>::new();
        let mut expected = Vec::new();
        let mut write = |kind: u16,
                         xmt: u32,
                         plan: &'r SchemaPlan,
                         declaration: Option<&[u8]>,
                         encoding: Option<&[u8]>|
         -> Result<(), TransmitError> {
            out.extend(kind.to_be_bytes());
            match written.get(&kind) {
                Some(first) if *first != plan => {
                    return Err(TransmitError::SchemaMismatch { kind });
                }
                Some(_) => {}
                None => {
                    written.insert(kind, plan);
                    out.extend_from_slice(declaration.ok_or(TransmitError::Reframe)?);
                }
            }
            out.extend_from_slice(encoding.ok_or(TransmitError::Reframe)?);
            expected.push((xmt, kind));
            Ok(())
        };
        for node in partition {
            let declaration = self.frame.declarations.get(&node.kind()).copied().flatten();
            write(
                node.kind(),
                node.index(),
                node.plan(),
                declaration,
                node.encoding(),
            )?;
        }
        for node in images {
            write(
                node.kind(),
                node.index(),
                node.plan(),
                node.declaration(),
                node.primary_encoding().as_deref(),
            )?;
        }
        out.extend_from_slice(self.frame.end);
        let framed = PrimaryRecords::read(&out)
            .ok()
            .flatten()
            .filter(PrimaryRecords::is_terminated)
            .is_some_and(|records| {
                records
                    .nodes()
                    .map(|node| (node.index(), node.kind()))
                    .eq(expected.iter().copied())
            });
        framed.then_some(out).ok_or(TransmitError::Reframe)
    }
}

#[cfg(test)]
mod tests;
