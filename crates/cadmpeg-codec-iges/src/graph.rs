// SPDX-License-Identifier: Apache-2.0
//! Entity index, Directory Entry references, cycles, and validation states.

pub(crate) mod expectation;
use expectation::{ExpectationLabel, ReferenceExpectation};

use crate::card::{CardScan, Section};
use crate::decode_resource::{
    format_retained, insert_optional_btree_map, insert_optional_btree_set, push_formatted_note,
    reserve_vec, reserve_vec_growth,
};
use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{refuse_local_limit, u64_from_index, DecodeContext};
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
    accepts: impl FnOnce(&DirectoryEntry) -> bool,
) -> Resolution {
    match (target_sequence, target) {
        (None, _) => Resolution::OutOfRange,
        (Some(sequence), target) if sequence % 2 == 0 => {
            Resolution::EvenSequence(target.map(|entry| entry.sequence))
        }
        (Some(_), None) => Resolution::Dangling,
        (Some(_), Some(entry)) if !accepts(entry) => Resolution::WrongType(entry.sequence),
        (Some(sequence), Some(_)) => Resolution::Resolved(sequence),
    }
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
    pub(crate) fn copy_for_native(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let expected = match &self.expected {
            ReferenceExpectation::Named(label) => ReferenceExpectation::Named(*label),
            ReferenceExpectation::Type { entity_type, forms } => {
                let mut copied_forms =
                    reserve_vec(ctx, forms.len(), "iges native reference forms")?;
                copied_forms.extend_from_slice(forms);
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
                let mut copied_rest = reserve_vec(ctx, rest.len(), "iges native reference types")?;
                copied_rest.extend_from_slice(rest);
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

pub(crate) struct ParameterResolver<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    directory: BTreeMap<u32, &'a DirectoryEntry>,
    edges: RefCell<BTreeMap<u32, Vec<ReferenceEdge>>>,
}

impl<'a, 'ctx> ParameterResolver<'a, 'ctx> {
    pub(crate) fn new(
        directory: &'a [DirectoryEntry],
        ctx: &'a DecodeContext<'ctx>,
    ) -> Result<Self, CodecError> {
        let mut index = BTreeMap::new();
        for entry in directory {
            insert_optional_btree_map(
                Some(ctx),
                &mut index,
                entry.sequence,
                entry,
                "iges parameter resolver directory index",
            )?;
        }
        Ok(Self {
            ctx,
            directory: index,
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
            accepts,
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
            accepts,
        )
    }

    fn resolve_sequence(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        target_sequence: Option<u32>,
        expected: ReferenceExpectation,
        accepts: impl FnOnce(&DirectoryEntry) -> bool,
    ) -> Result<Option<u32>, CodecError> {
        let target = target_sequence.and_then(|sequence| self.directory.get(&sequence).copied());
        let resolution = classify(target_sequence, target, accepts);
        let mut graph = self.edges.borrow_mut();
        let edges = match graph.entry(source) {
            Entry::Vacant(slot) => {
                self.ctx
                    .charge_collection_items(1, "iges parameter resolver edge groups")?;
                slot.insert(Vec::new())
            }
            Entry::Occupied(slot) => slot.into_mut(),
        };
        reserve_vec_growth(self.ctx, edges, 1, "iges parameter resolver edges")?;
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
        let mut expected_forms = reserve_vec(
            self.ctx,
            forms.len(),
            "iges parameter resolver expected forms",
        )?;
        expected_forms.extend_from_slice(forms);
        let expected = ReferenceExpectation::Type {
            entity_type,
            forms: expected_forms,
        };
        self.resolve(source, parameter_index, raw_pointer, expected, |target| {
            target.entity_type == entity_type && (forms.is_empty() || forms.contains(&target.form))
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
        let mut expected_forms = reserve_vec(
            self.ctx,
            forms.len(),
            "iges parameter resolver expected forms",
        )?;
        expected_forms.extend_from_slice(forms);
        let expected = ReferenceExpectation::Type {
            entity_type,
            forms: expected_forms,
        };
        self.resolve_negative(source, parameter_index, raw_pointer, expected, |target| {
            target.entity_type == entity_type && (forms.is_empty() || forms.contains(&target.form))
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
        let mut expected_rest = reserve_vec(
            self.ctx,
            rest.len(),
            "iges parameter resolver expected types",
        )?;
        expected_rest.extend_from_slice(rest);
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

    pub(crate) fn append_to(
        self,
        graph: &mut BTreeMap<u32, Vec<ReferenceEdge>>,
    ) -> Result<(), CodecError> {
        for (source, mut edges) in self.edges.into_inner() {
            match graph.entry(source) {
                Entry::Vacant(slot) => {
                    self.ctx
                        .charge_collection_items(1, "iges parameter resolver graph groups")?;
                    slot.insert(edges);
                }
                Entry::Occupied(mut slot) => {
                    reserve_vec_growth(
                        self.ctx,
                        slot.get_mut(),
                        edges.len(),
                        "iges appended parameter reference edges",
                    )?;
                    slot.get_mut().append(&mut edges);
                }
            }
        }
        Ok(())
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
                forms: expected_forms(ctx, &[0])?,
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
            forms: expected_forms(ctx, &[1])?,
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
            forms: expected_forms(ctx, &[5])?,
        },
        ReferenceKind::Color => ReferenceExpectation::Type {
            entity_type: 314,
            forms: Vec::new(),
        },
    })
}

fn expected_forms(ctx: &DecodeContext<'_>, forms: &[i64]) -> Result<Vec<i64>, CodecError> {
    let mut copied = reserve_vec(ctx, forms.len(), "iges reference expected forms")?;
    copied.extend_from_slice(forms);
    Ok(copied)
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
    let mut next = BTreeMap::new();
    for (source, values) in edges {
        if let Some(target) = values
            .iter()
            .find_map(|edge| edge.resolved_target_sequence_for(ReferenceKind::Transform))
        {
            insert_optional_btree_map(
                Some(ctx),
                &mut next,
                *source,
                target,
                "iges transform reference successors",
            )?;
        }
    }
    let mut cyclic = BTreeSet::new();
    let mut completed = BTreeSet::new();
    let mut active = BTreeMap::<u32, usize>::new();
    for start in next.keys().copied() {
        let mut path = Vec::new();
        let mut current = start;
        loop {
            ctx.charge_work(1, "iges transform reference cycle walk")?;
            if completed.contains(&current) {
                break;
            }
            if let Some(position) = active.get(&current).copied() {
                for node in path[position..].iter().copied() {
                    insert_optional_btree_set(
                        Some(ctx),
                        &mut cyclic,
                        node,
                        "iges cyclic transform references",
                    )?;
                }
                break;
            }
            insert_optional_btree_map(
                Some(ctx),
                &mut active,
                current,
                path.len(),
                "iges active transform reference walk",
            )?;
            reserve_vec_growth(ctx, &mut path, 1, "iges transform reference path")?;
            path.push(current);
            let Some(target) = next.get(&current).copied() else {
                break;
            };
            current = target;
        }
        for node in path {
            active.remove(&node);
            insert_optional_btree_set(
                Some(ctx),
                &mut completed,
                node,
                "iges completed transform references",
            )?;
        }
    }
    Ok(cyclic)
}

pub(crate) fn build(
    directory: &[DirectoryEntry],
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u32, Vec<ReferenceEdge>>, CodecError> {
    let mut index = BTreeMap::new();
    for entry in directory {
        insert_optional_btree_map(
            Some(ctx),
            &mut index,
            entry.sequence,
            entry,
            "iges reference directory index",
        )?;
    }
    let mut graph = BTreeMap::new();
    for entry in directory {
        let mut edges = Vec::new();
        for candidate in candidates(entry) {
            let target = candidate
                .target_sequence
                .and_then(|value| index.get(&value).copied());
            let resolution = classify(candidate.target_sequence, target, |value| {
                accepts(candidate.kind, entry, value)
            });
            let expected = expected(candidate.kind, entry, ctx)?;
            reserve_vec_growth(ctx, &mut edges, 1, "iges directory reference edges")?;
            edges.push(ReferenceEdge {
                origin: ReferenceOrigin::Directory(candidate.kind),
                raw_pointer: candidate.raw_pointer,
                resolution,
                expected,
            });
        }
        insert_optional_btree_map(
            Some(ctx),
            &mut graph,
            entry.sequence,
            edges,
            "iges directory reference graph",
        )?;
    }
    let cyclic = cyclic_transform_nodes(&graph, ctx)?;
    for source in cyclic {
        if let Some(edge) = graph.get_mut(&source).and_then(|edges| {
            edges
                .iter_mut()
                .find(|edge| edge.origin == ReferenceOrigin::Directory(ReferenceKind::Transform))
        }) {
            if let Resolution::Resolved(sequence) = edge.resolution {
                edge.resolution = Resolution::Cyclic(sequence);
            }
        }
    }
    Ok(graph)
}

pub(crate) fn resolved_structure_sequence(
    graph: &BTreeMap<u32, Vec<ReferenceEdge>>,
    source: u32,
) -> Option<u32> {
    graph
        .get(&source)?
        .iter()
        .find_map(|edge| edge.resolved_target_sequence_for(ReferenceKind::Structure))
}

pub(crate) fn summary_notes(
    graph: &BTreeMap<u32, Vec<ReferenceEdge>>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<String>, CodecError> {
    let mut counts = [0_usize; 6];
    for edge in graph.values().flatten() {
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
        push_formatted_note(
            ctx,
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
    let scan_work = u64_from_index(scan.lines.len())
        .checked_mul(2)
        .ok_or_else(|| refuse_local_limit("iges graph loss offset scans", u64::MAX, 1))?;
    ctx.charge_work(scan_work, "iges graph loss offset scans")?;
    let mut directory_offsets = BTreeMap::new();
    for (sequence, line) in scan.section(Section::Directory) {
        insert_optional_btree_map(
            Some(ctx),
            &mut directory_offsets,
            sequence,
            line.offset,
            "iges graph loss directory offsets",
        )?;
    }
    let mut parameter_lines = BTreeMap::new();
    for (sequence, line) in scan.section(Section::Parameter) {
        insert_optional_btree_map(
            Some(ctx),
            &mut parameter_lines,
            sequence,
            line.offset,
            "iges graph loss parameter offsets",
        )?;
    }
    let mut records = BTreeMap::new();
    ctx.charge_work(
        u64_from_index(parameters.len()),
        "iges graph loss record index",
    )?;
    for record in parameters {
        insert_optional_btree_map(
            Some(ctx),
            &mut records,
            record.directory_sequence,
            record,
            "iges graph loss parameter records",
        )?;
    }
    let mut losses = Vec::new();
    for (source, edges) in graph {
        ctx.charge_work(u64_from_index(edges.len()), "iges graph loss edge scan")?;
        for edge in edges
            .iter()
            .filter(|edge| !matches!(edge.resolution, Resolution::Resolved(_)))
        {
            reserve_vec_growth(ctx, &mut losses, 1, "iges graph loss notes")?;
            let parameter_location = edge.origin.parameter_index().and_then(|index| {
                let record = records.get(source)?;
                let span = record.tokens().get(index)?.span.start;
                let card = u32::try_from(span / 64).ok()?;
                let sequence = record.line_range.start.checked_add(card)?;
                let offset = parameter_lines
                    .get(&sequence)?
                    .checked_add((span % 64) as u64)?;
                Some((offset, index))
            });
            let location = if let Some((offset, index)) = parameter_location {
                Some((
                    offset,
                    format_retained(
                        ctx,
                        format_args!("D{source}:parameter[{index}]"),
                        "iges graph loss tag",
                    )?,
                ))
            } else {
                directory_offsets
                    .get(source)
                    .copied()
                    .map(|offset| {
                        format_retained(ctx, format_args!("D{source}"), "iges graph loss tag")
                            .map(|tag| (offset, tag))
                    })
                    .transpose()?
            };
            let message = format_retained(
                ctx,
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
            ctx.charge_retained(4 + code.code().len() as u64, "iges graph loss kind")?;
            let mut note = code.note(message);
            if let Some((offset, tag)) = location {
                let format =
                    format_retained(ctx, format_args!("iges"), "iges graph loss source format")?;
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
