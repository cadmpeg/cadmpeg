// SPDX-License-Identifier: Apache-2.0
//! Entity index, Directory Entry references, cycles, and validation states.

pub(crate) mod expectation;
use expectation::{ExpectationLabel, ReferenceExpectation};

use crate::card::{CardScan, Section};

use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

/// The largest sequence address representable by an IGES pointer constant.
const MAX_POINTER_SEQUENCE: i64 = 9_999_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReferenceKind {
    Structure,
    LineFont,
    Level,
    View,
    Transform,
    LabelDisplay,
    Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Resolved(u32),
    OutOfRange,
    EvenSequence(Option<u32>),
    Dangling,
    WrongType(u32),
    Cyclic(u32),
}

impl Resolution {
    /// The report vocabulary this resolution is named by, everywhere it is named.
    const fn key(self) -> &'static str {
        match self {
            Self::Resolved(_) => "resolved",
            Self::OutOfRange => "out_of_range",
            Self::EvenSequence(_) => "even_sequence",
            Self::Dangling => "dangling",
            Self::WrongType(_) => "wrong_type",
            Self::Cyclic(_) => "cyclic",
        }
    }

    fn target_sequence(self) -> Option<u32> {
        match self {
            Self::Resolved(sequence) | Self::WrongType(sequence) | Self::Cyclic(sequence) => {
                Some(sequence)
            }
            Self::EvenSequence(sequence) => sequence,
            Self::OutOfRange | Self::Dangling => None,
        }
    }
}

impl Serialize for Resolution {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

fn classify(
    target_sequence: Option<u32>,
    target: Option<&DirectoryEntry>,
    accepts: impl FnOnce(&DirectoryEntry) -> Result<bool, CodecError>,
) -> Result<Resolution, CodecError> {
    Ok(match (target_sequence, target) {
        (None, _) => Resolution::OutOfRange,
        (Some(sequence), target) if sequence % 2 == 0 => {
            Resolution::EvenSequence(target.map(|entry| entry.sequence))
        }
        (Some(_), None) => Resolution::Dangling,
        (Some(sequence), Some(entry)) => {
            if accepts(entry)? { Resolution::Resolved(sequence) }
            else { Resolution::WrongType(entry.sequence) }
        }
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReferenceOrigin {
    Directory(ReferenceKind),
    Parameter { index: usize },
}
impl ReferenceOrigin {
    fn parameter_index(self) -> Option<usize> {
        match self {
            Self::Directory(_) => None,
            Self::Parameter { index } => Some(index),
        }
    }
}
impl std::fmt::Debug for ReferenceOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Directory(kind) => kind.fmt(f),
            Self::Parameter { .. } => f.write_str("Parameter"),
        }
    }
}
impl Serialize for ReferenceOrigin {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Directory(kind) => kind.serialize(serializer),
            Self::Parameter { .. } => serializer.serialize_str("parameter"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceEdge {
    origin: ReferenceOrigin,
    raw_pointer: i64,
    resolution: Resolution,
    expected: ReferenceExpectation,
}

struct TargetId(u32);

impl std::fmt::Display for TargetId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "iges:entity:directory#{}", self.0)
    }
}

impl Serialize for TargetId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl Serialize for ReferenceEdge {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            kind: ReferenceOrigin,
            raw_pointer: i64,
            target: Option<TargetId>,
            resolution: Resolution,
            expected: &'a ReferenceExpectation,
            #[serde(skip_serializing_if = "Option::is_none")]
            parameter_index: Option<usize>,
        }
        Wire {
            kind: self.origin,
            raw_pointer: self.raw_pointer,
            target: self.resolution.target_sequence().map(TargetId),
            resolution: self.resolution,
            expected: &self.expected,
            parameter_index: self.origin.parameter_index(),
        }
        .serialize(serializer)
    }
}

impl ReferenceEdge {
    /// Copy expectation children into scratch storage for native serialization.
    pub(crate) fn copy_for_native(
        &self,
        ctx: &DecodeContext<'_>,
        storage: &mut ScopedReservation<'_>,
    ) -> Result<Self, CodecError> {
        let expected = match &self.expected {
            ReferenceExpectation::Named(label) => ReferenceExpectation::Named(*label),
            ReferenceExpectation::Type { entity_type, forms } => {
                let copied_forms = storage.with_storage(|| ctx.copy_slice(forms, "iges native reference forms"))?;
                ReferenceExpectation::Type {
                    entity_type: *entity_type,
                    forms: copied_forms,
                }
            }
            ReferenceExpectation::AnyOf {
                first,
                second,
                rest,
            } => {
                let copied_rest = storage.with_storage(|| ctx.copy_slice(rest, "iges native reference types"))?;
                ReferenceExpectation::AnyOf {
                    first: *first,
                    second: *second,
                    rest: copied_rest,
                }
            }
        };
        Ok(Self {
            origin: self.origin,
            raw_pointer: self.raw_pointer,
            resolution: self.resolution,
            expected,
        })
    }

    pub(crate) fn target_sequence(&self) -> Option<u32> {
        self.resolution.target_sequence()
    }

    pub(crate) fn resolved_target_sequence_for(&self, kind: ReferenceKind) -> Option<u32> {
        match (self.origin, self.resolution) {
            (ReferenceOrigin::Directory(actual), Resolution::Resolved(sequence))
                if actual == kind =>
            {
                Some(sequence)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    kind: ReferenceKind,
    raw_pointer: i64,
    target_sequence: Option<u32>,
}

pub(crate) struct ParameterResolver<'directory, 'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    directory: &'directory [DirectoryEntry],
    edges: RefCell<BTreeMap<u32, Vec<ReferenceEdge>>>,
    group_storage: RefCell<ScopedReservation<'ctx>>,
    storage: RefCell<ScopedReservation<'ctx>>,
}

impl<'directory, 'ctx, 'arena> ParameterResolver<'directory, 'ctx, 'arena> {
    pub(crate) fn new(
        directory: &'directory [DirectoryEntry],
        ctx: &'ctx DecodeContext<'arena>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            directory,
            storage: RefCell::new(ctx.reserve_scoped(0, "IGES parameter reference edges")?),
            group_storage: RefCell::new(ctx.reserve_scoped(0, "IGES parameter reference groups")?),
            edges: RefCell::new(BTreeMap::new()),
        })
    }

    /// Resolves a raw pointer to its target sequence, recording the
    /// reference edge. A zero raw pointer resolves to `None` before any edge
    /// is recorded or predicate runs; this check is the single owner of that
    /// rule, and link builders pass raw pointer fields unguarded.
    pub(crate) fn resolve(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        expected: ReferenceExpectation,
        accepts: impl FnOnce(&DirectoryEntry) -> bool,
    ) -> Result<Option<u32>, CodecError> {
        if raw_pointer == 0 {
            return Ok(None);
        }
        let target_sequence = positive_pointer_sequence(raw_pointer);
        self.resolve_sequence(
            source,
            parameter_index,
            raw_pointer,
            target_sequence,
            expected,
            |target| Ok(accepts(target)),
        )
    }

    pub(crate) fn resolve_negative(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        expected: ReferenceExpectation,
        accepts: impl FnOnce(&DirectoryEntry) -> bool,
    ) -> Result<Option<u32>, CodecError> {
        if raw_pointer == 0 {
            return Ok(None);
        }
        let target_sequence = negative_pointer_sequence(raw_pointer);
        self.resolve_sequence(
            source,
            parameter_index,
            raw_pointer,
            target_sequence,
            expected,
            |target| Ok(accepts(target)),
        )
    }

    fn resolve_sequence(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        target_sequence: Option<u32>,
        expected: ReferenceExpectation,
        accepts: impl FnOnce(&DirectoryEntry) -> Result<bool, CodecError>,
    ) -> Result<Option<u32>, CodecError> {
        let target = match target_sequence {
            Some(sequence) => crate::directory::entry_by_sequence(self.directory, sequence, self.ctx)?,
            None => None,
        };
        let resolution = classify(target_sequence, target, accepts)?;
        let mut graph = self.edges.borrow_mut();
        self.group_storage.borrow_mut().with_storage(|| self.ctx
            .admit_btree_entry(&graph, &source, "iges parameter resolver edge groups"))?;
        let edges = match graph.entry(source) {
            Entry::Vacant(slot) => slot.insert(Vec::new()),
            Entry::Occupied(slot) => slot.into_mut(),
        };
        self.ctx.reserve_scoped_vec(&mut self.storage.borrow_mut(), edges, 1, "iges parameter resolver edges")?;
        edges.push(ReferenceEdge {
            origin: ReferenceOrigin::Parameter {
                index: parameter_index,
            },
            raw_pointer,
            resolution,
            expected,
        });
        Ok(match resolution {
            Resolution::Resolved(sequence) => Some(sequence),
            _ => None,
        })
    }

    pub(crate) fn resolve_type(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        entity_type: i64,
        forms: &[i64],
    ) -> Result<Option<u32>, CodecError> {
        if raw_pointer == 0 {
            return Ok(None);
        }
        let expected_forms = self.storage.borrow_mut().with_storage(|| self.ctx.copy_slice(forms, "iges parameter resolver expected forms"))?;
        let expected = ReferenceExpectation::Type {
            entity_type,
            forms: expected_forms,
        };
        self.resolve_sequence(source, parameter_index, raw_pointer,
            positive_pointer_sequence(raw_pointer), expected, |target| {
                Ok(target.entity_type == entity_type && (forms.is_empty() || self.ctx.any_by(forms, |form| Ok(*form == target.form), "iges parameter expected form search")?))
            })
    }

    pub(crate) fn resolve_negative_type(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        entity_type: i64,
        forms: &[i64],
    ) -> Result<Option<u32>, CodecError> {
        if raw_pointer == 0 {
            return Ok(None);
        }
        let expected_forms = self.storage.borrow_mut().with_storage(|| self.ctx.copy_slice(forms, "iges parameter resolver expected forms"))?;
        let expected = ReferenceExpectation::Type {
            entity_type,
            forms: expected_forms,
        };
        self.resolve_sequence(source, parameter_index, raw_pointer,
            negative_pointer_sequence(raw_pointer), expected, |target| {
                Ok(target.entity_type == entity_type && (forms.is_empty() || self.ctx.any_by(forms, |form| Ok(*form == target.form), "iges parameter expected form search")?))
            })
    }

    pub(crate) fn resolve_any_of(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        types: (i64, i64, &[i64]),
        accepts: impl FnOnce(&DirectoryEntry) -> bool,
    ) -> Result<Option<u32>, CodecError> {
        if raw_pointer == 0 {
            return Ok(None);
        }
        let (first, second, rest) = types;
        let expected_rest = self.storage.borrow_mut().with_storage(|| self.ctx.copy_slice(rest, "iges parameter resolver expected types"))?;
        self.resolve(
            source,
            parameter_index,
            raw_pointer,
            ReferenceExpectation::AnyOf {
                first,
                second,
                rest: expected_rest,
            },
            accepts,
        )
    }

    pub(crate) fn resolve_any(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
    ) -> Result<Option<u32>, CodecError> {
        self.resolve(
            source,
            parameter_index,
            raw_pointer,
            ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
            |_| true,
        )
    }

    /// Move the recorded edges into the graph and return their live storage reservation.
    pub(crate) fn append_to(
        self,
        graph: &mut BTreeMap<u32, Vec<ReferenceEdge>>,
    ) -> Result<ScopedReservation<'ctx>, CodecError> {
        let mut storage = self.storage.into_inner();
        for (source, mut edges) in self.ctx.admit_iter(self.edges.into_inner(), "iges parameter resolver graph sources")? {
            storage.with_storage(|| self.ctx
                .admit_btree_entry(graph, &source, "iges parameter resolver graph groups"))?;
            match graph.entry(source) {
                Entry::Vacant(slot) => {
                    slot.insert(edges);
                }
                Entry::Occupied(mut slot) if slot.get().is_empty() => {
                    slot.insert(edges);
                }
                Entry::Occupied(mut slot) => {
                    storage.with_storage(|| self.ctx.append_vec(slot.get_mut(), &mut edges, "iges appended parameter reference edges"))?;
                }
            }
        }
        Ok(storage)
    }
}

fn negative_candidate(kind: ReferenceKind, raw_pointer: i64) -> Candidate {
    Candidate {
        kind,
        raw_pointer,
        target_sequence: negative_pointer_sequence(raw_pointer),
    }
}

fn positive_candidate(kind: ReferenceKind, raw_pointer: i64) -> Candidate {
    Candidate {
        kind,
        raw_pointer,
        target_sequence: positive_pointer_sequence(raw_pointer),
    }
}

fn positive_pointer_sequence(raw_pointer: i64) -> Option<u32> {
    if !(1..=MAX_POINTER_SEQUENCE).contains(&raw_pointer) {
        return None;
    }
    u32::try_from(raw_pointer).ok()
}

fn negative_pointer_sequence(raw_pointer: i64) -> Option<u32> {
    if raw_pointer >= 0 {
        return None;
    }
    let magnitude = raw_pointer.checked_abs()?;
    if !(1..=MAX_POINTER_SEQUENCE).contains(&magnitude) {
        return None;
    }
    u32::try_from(magnitude).ok()
}

fn candidates(entry: &DirectoryEntry) -> impl Iterator<Item = Candidate> {
    [
        (entry.structure < 0)
            .then(|| negative_candidate(ReferenceKind::Structure, entry.structure)),
        (entry.line_font < 0).then(|| negative_candidate(ReferenceKind::LineFont, entry.line_font)),
        (entry.level < 0).then(|| negative_candidate(ReferenceKind::Level, entry.level)),
        (entry.view != 0).then(|| positive_candidate(ReferenceKind::View, entry.view)),
        (entry.transform != 0)
            .then(|| positive_candidate(ReferenceKind::Transform, entry.transform)),
        (entry.label_display != 0)
            .then(|| positive_candidate(ReferenceKind::LabelDisplay, entry.label_display)),
        (entry.color < 0).then(|| negative_candidate(ReferenceKind::Color, entry.color)),
    ]
    .into_iter()
    .flatten()
}

fn expected(
    kind: ReferenceKind,
    source: &DirectoryEntry,
    ctx: &DecodeContext<'_>,
) -> Result<ReferenceExpectation, CodecError> {
    Ok(match kind {
        ReferenceKind::Structure => match source.entity_type {
            422 if matches!(source.form, 0..=1) => ReferenceExpectation::Type {
                entity_type: 322,
                forms: {
                    let mut forms = ctx.collection_vec(1, "iges reference expected forms")?;
                    forms.push(0);
                    forms
                },
            },
            402 if matches!(source.form, 5001..=9999) => {
                ReferenceExpectation::Named(ExpectationLabel::Type302MatchingForm)
            }
            entity_type if crate::profile::macro_instance_type(entity_type) => {
                ReferenceExpectation::AnyOf {
                    first: 306,
                    second: 416,
                    rest: vec![],
                }
            }
            _ => ReferenceExpectation::Named(ExpectationLabel::StructureNotPermitted),
        },
        ReferenceKind::LineFont => ReferenceExpectation::Type {
            entity_type: 304,
            forms: Vec::new(),
        },
        ReferenceKind::Level => ReferenceExpectation::Type {
            entity_type: 406,
            forms: {
                    let mut forms = ctx.collection_vec(1, "iges reference expected forms")?;
                    forms.push(1);
                    forms
                },
        },
        ReferenceKind::View => {
            ReferenceExpectation::Named(ExpectationLabel::Type410OrType402Form3419)
        }
        ReferenceKind::Transform => ReferenceExpectation::Type {
            entity_type: 124,
            forms: Vec::new(),
        },
        ReferenceKind::LabelDisplay => ReferenceExpectation::Type {
            entity_type: 402,
            forms: {
                    let mut forms = ctx.collection_vec(1, "iges reference expected forms")?;
                    forms.push(5);
                    forms
                },
        },
        ReferenceKind::Color => ReferenceExpectation::Type {
            entity_type: 314,
            forms: Vec::new(),
        },
    })
}

fn accepts(kind: ReferenceKind, source: &DirectoryEntry, target: &DirectoryEntry) -> bool {
    match kind {
        ReferenceKind::Structure => match source.entity_type {
            422 if matches!(source.form, 0..=1) => target.entity_type == 322 && target.form == 0,
            402 if matches!(source.form, 5001..=9999) => {
                target.entity_type == 302 && target.form == source.form
            }
            entity_type if crate::profile::macro_instance_type(entity_type) => {
                matches!(target.entity_type, 306 | 416)
            }
            _ => false,
        },
        ReferenceKind::LineFont => target.entity_type == 304 && matches!(target.form, 1 | 2),
        ReferenceKind::Level => target.entity_type == 406 && target.form == 1,
        ReferenceKind::View => {
            target.entity_type == 410
                || (target.entity_type == 402 && matches!(target.form, 3 | 4 | 19))
        }
        ReferenceKind::Transform => target.entity_type == 124,
        ReferenceKind::LabelDisplay => target.entity_type == 402 && target.form == 5,
        ReferenceKind::Color => target.entity_type == 314 && target.form == 0,
    }
}

fn cyclic_transform_nodes(
    edges: &BTreeMap<u32, Vec<ReferenceEdge>>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u32>, CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "IGES transform cycle indices")?;
    let mut next = BTreeMap::new();
    for (source, values) in ctx.admit_iter(edges, "iges transform cycle sources")? {
        if let Some(target) = ctx.find_map(values,
            |edge| Ok(edge.resolved_target_sequence_for(ReferenceKind::Transform)),
            "iges transform successor search",
        )?
        {
            index_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut next,
                    *source,
                    target,
                    "iges transform reference successors",
                )
            })?;
        }
    }
    let mut cyclic = BTreeSet::new();
    let mut completed = BTreeSet::new();
    for start in ctx.admit_iter(&next, "iges transform cycle starts")?.map(|(source, _)| *source) {
        let mut path_storage = ctx.reserve_scoped(0, "IGES transform cycle path")?;
        let mut path = Vec::new();
        let mut active = BTreeMap::<u32, usize>::new();
        let mut successors = std::iter::successors(Some(start), |current| next.get(current).copied());
        while let Some(current) = ctx.next_charged(&mut successors, "iges transform reference cycle walk")? {
            if completed.contains(&current) {
                break;
            }
            if let Some(position) = active.get(&current).copied() {
                for node in ctx.admit_iter(&path[position..], "iges cyclic transform nodes")?.copied() {
                    ctx.insert_btree_set(&mut cyclic, node, "iges cyclic transform references")?;
                }
                break;
            }
            path_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut active,
                    current,
                    path.len(),
                    "iges active transform reference walk",
                )
            })?;
            ctx.reserve_scoped_vec(&mut path_storage, &mut path, 1, "iges transform reference path")?;
            path.push(current);

        }
        for node in ctx.admit_iter(path, "iges completed transform path")? {
            index_storage.with_storage(|| {
                ctx.insert_btree_set(&mut completed, node, "iges completed transform references")
            })?;
        }
    }
    Ok(cyclic)
}

/// Build scratch reference edges and return their live storage reservation.
pub(crate) fn build<'ctx>(
    directory: &[DirectoryEntry],
    ctx: &'ctx DecodeContext<'_>,
) -> Result<(BTreeMap<u32, Vec<ReferenceEdge>>, ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("IGES Directory reference graph", || {
    let mut graph = BTreeMap::new();
    for entry in ctx.admit_iter(directory, "iges directory reference sources")? {
        let mut edges = Vec::new();
        for candidate in candidates(entry) {
            let target = match candidate.target_sequence {
                Some(sequence) => crate::directory::entry_by_sequence(directory, sequence, ctx)?,
                None => None,
            };
            let resolution = classify(candidate.target_sequence, target, |value| {
                Ok(accepts(candidate.kind, entry, value))
            })?;
            let expected = expected(candidate.kind, entry, ctx)?;
            ctx.reserve_vec(&mut edges, 1, "iges directory reference edges")?;
            edges.push(ReferenceEdge {
                origin: ReferenceOrigin::Directory(candidate.kind),
                raw_pointer: candidate.raw_pointer,
                resolution,
                expected,
            });
        }
        ctx.insert_btree_map(
            &mut graph,
            entry.sequence,
            edges,
            "iges directory reference graph",
        )?;
    }
    let (cyclic, _cycle_storage) = ctx.with_scoped_storage("IGES cyclic transform nodes", || cyclic_transform_nodes(&graph, ctx))?;
    for source in ctx.admit_iter(cyclic, "iges cyclic transform sources")? {
        let edge = match graph.get_mut(&source) {
            Some(edges) => ctx.find_by(edges.iter_mut(), |edge| Ok(edge.origin == ReferenceOrigin::Directory(ReferenceKind::Transform)), "iges cyclic transform edge")?,
            None => None,
        };
        if let Some(edge) = edge {
            if let Resolution::Resolved(sequence) = edge.resolution {
                edge.resolution = Resolution::Cyclic(sequence);
            }
        }
    }
    Ok(graph)
    })
}

pub(crate) fn resolved_structure_sequence(
    graph: &BTreeMap<u32, Vec<ReferenceEdge>>,
    source: u32,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    let Some(edges) = graph.get(&source) else { return Ok(None) };
    ctx.find_map(edges, |edge| Ok(edge.resolved_target_sequence_for(ReferenceKind::Structure)), "iges structure reference search")
}

pub(crate) fn summary_notes(
    graph: &BTreeMap<u32, Vec<ReferenceEdge>>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<String>, CodecError> {
    let mut counts = [0_usize; 6];
    for (_, edges) in ctx.admit_iter(graph, "iges reference summary sources")? {
        for edge in ctx.admit_iter(edges, "iges reference summary edges")? {
        let index = match edge.resolution {
            Resolution::Cyclic(_) => 0,
            Resolution::Dangling => 1,
            Resolution::EvenSequence(_) => 2,
            Resolution::OutOfRange => 3,
            Resolution::Resolved(_) => 4,
            Resolution::WrongType(_) => 5,
        };
        counts[index] += 1;
        }
    }
    let mut notes = Vec::new();
    for (resolution, count) in [
        "cyclic",
        "dangling",
        "even_sequence",
        "out_of_range",
        "resolved",
        "wrong_type",
    ]
    .into_iter()
    .zip(counts)
    .filter(|(_, count)| *count > 0)
    {
        ctx.push_formatted_retained(
            &mut notes,
            format_args!("references.{resolution}={count}"),
            "iges reference summary notes",
            "iges reference summary text",
        )?;
    }
    Ok(notes)
}

pub(crate) fn losses(
    graph: &BTreeMap<u32, Vec<ReferenceEdge>>,
    scan: &CardScan<'_>,
    parameters: &[ParameterRecord],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    // A card's sequence is its position in its section, so a section slice
    // answers an offset lookup by index.
    let directory_cards = scan.section(Section::Directory);
    let parameter_cards = scan.section(Section::Parameter);
    let card_offset = |cards: &[crate::card::Card<'_>], sequence: u32| {
        let index = usize::try_from(sequence).ok()?.checked_sub(1)?;
        cards.get(index).map(|card| card.line.offset)
    };
    let mut losses = Vec::new();
    for (source, edges) in ctx.admit_iter(graph, "iges graph loss sources")? {
        let mut parameter_record = None;
        for edge in ctx
            .admit_iter(edges, "iges graph loss edge scan")?
            .filter(|edge| !matches!(edge.resolution, Resolution::Resolved(_)))
        {
            ctx.reserve_vec(&mut losses, 1, "iges graph loss notes")?;
            let record = match edge.origin.parameter_index() {
                Some(_) => match parameter_record {
                    Some(record) => record,
                    None => {
                        let record = crate::parameter::record_by_sequence(parameters, *source, ctx)?;
                        parameter_record = Some(record);
                        record
                    }
                },
                None => None,
            };
            let parameter_location = edge.origin.parameter_index().and_then(|index| {
                let record = record?;
                let span = record.tokens().get(index)?.span.start;
                let card = u32::try_from(span / 64).ok()?;
                let sequence = record.line_range.start.checked_add(card)?;
                let offset = card_offset(parameter_cards, sequence)?
                    .checked_add(cadmpeg_core::decode::u64_from_index(span % 64))?;
                Some((offset, index))
            });
            let location = if let Some((offset, index)) = parameter_location {
                Some((
                    offset,
                    ctx.format_retained(
                        format_args!("D{source}:parameter[{index}]"),
                        "iges graph loss tag",
                    )?,
                ))
            } else {
                card_offset(directory_cards, *source)
                    .map(|offset| {
                        ctx.format_retained(format_args!("D{source}"), "iges graph loss tag")
                            .map(|tag| (offset, tag))
                    })
                    .transpose()?
            };
            let message = ctx.format_retained(
                format_args!(
                    "IGES Directory Entry D{source} {:?} pointer {} has {} resolution; expected {}",
                    edge.origin,
                    edge.raw_pointer,
                    edge.resolution.key(),
                    edge.expected
                ),
                "iges graph loss message",
            )?;
            let code = IgesLossCode::PointerUnresolved;
            ctx.charge_retained(
                4 + cadmpeg_core::decode::u64_from_index(code.code().len()),
                "iges graph loss kind",
            )?;
            let mut note = code.note(message);
            if let Some((offset, tag)) = location {
                let format =
                    ctx.format_retained(format_args!("iges"), "iges graph loss source format")?;
                note = note.with_provenance(
                    SourceProvenance::in_stream(format, cadmpeg_ir::stream_name!("iges"), offset)
                        .with_tag(tag),
                );
            }
            losses.push(note);
        }
    }
    Ok(losses)
}

#[cfg(test)]
mod tests;
