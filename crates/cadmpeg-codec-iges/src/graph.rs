// SPDX-License-Identifier: Apache-2.0
//! Entity index, Directory Entry references, cycles, and validation states.

use crate::card::{CardScan, Section};
use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

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
    Parameter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Resolution {
    Resolved,
    Null,
    OutOfRange,
    EvenSequence,
    Dangling,
    WrongType,
    Cyclic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ReferenceEdge {
    kind: ReferenceKind,
    raw_pointer: i64,
    target: Option<String>,
    resolution: Resolution,
    expected: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameter_index: Option<usize>,
}

impl ReferenceEdge {
    pub(crate) fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub(crate) fn resolved_target_sequence_for(&self, kind: ReferenceKind) -> Option<u32> {
        (self.kind == kind && self.resolution == Resolution::Resolved)
            .then(|| self.raw_pointer.checked_abs())
            .flatten()
            .and_then(|value| u32::try_from(value).ok())
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    kind: ReferenceKind,
    raw_pointer: i64,
    target_sequence: Option<u32>,
}

pub(crate) struct ParameterResolver<'a> {
    directory: BTreeMap<u32, &'a DirectoryEntry>,
    edges: RefCell<BTreeMap<u32, Vec<ReferenceEdge>>>,
}

impl<'a> ParameterResolver<'a> {
    pub(crate) fn new(directory: &'a [DirectoryEntry]) -> Self {
        Self {
            directory: directory
                .iter()
                .map(|entry| (entry.sequence, entry))
                .collect(),
            edges: RefCell::new(BTreeMap::new()),
        }
    }

    pub(crate) fn resolve(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        expected: impl Into<String>,
        accepts: impl FnOnce(&DirectoryEntry) -> bool,
    ) -> Option<u32> {
        if raw_pointer == 0 {
            return None;
        }
        let target_sequence = u32::try_from(raw_pointer).ok();
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
        expected: impl Into<String>,
        accepts: impl FnOnce(&DirectoryEntry) -> bool,
    ) -> Option<u32> {
        if raw_pointer == 0 {
            return None;
        }
        let target_sequence = raw_pointer
            .checked_neg()
            .and_then(|value| u32::try_from(value).ok());
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
        expected: impl Into<String>,
        accepts: impl FnOnce(&DirectoryEntry) -> bool,
    ) -> Option<u32> {
        let target = target_sequence.and_then(|sequence| self.directory.get(&sequence).copied());
        let resolution = if target_sequence.is_none() {
            Resolution::OutOfRange
        } else if target_sequence.is_some_and(|sequence| sequence % 2 == 0) {
            Resolution::EvenSequence
        } else if target.is_none() {
            Resolution::Dangling
        } else if target.is_some_and(|entry| !accepts(entry)) {
            Resolution::WrongType
        } else {
            Resolution::Resolved
        };
        self.edges
            .borrow_mut()
            .entry(source)
            .or_default()
            .push(ReferenceEdge {
                kind: ReferenceKind::Parameter,
                raw_pointer,
                target: target.map(|entry| format!("iges:entity:directory#{}", entry.sequence)),
                resolution,
                expected: expected.into(),
                parameter_index: Some(parameter_index),
            });
        if resolution == Resolution::Resolved {
            target_sequence
        } else {
            None
        }
    }

    pub(crate) fn resolve_type(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
        entity_type: i64,
        forms: &[i64],
    ) -> Option<u32> {
        let expected = if forms.is_empty() {
            format!("type-{entity_type}")
        } else {
            let forms = forms
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join("-or-");
            format!("type-{entity_type}-form-{forms}")
        };
        self.resolve(source, parameter_index, raw_pointer, expected, |target| {
            target.entity_type == entity_type && (forms.is_empty() || forms.contains(&target.form))
        })
    }

    pub(crate) fn resolve_any(
        &self,
        source: u32,
        parameter_index: usize,
        raw_pointer: i64,
    ) -> Option<u32> {
        self.resolve(
            source,
            parameter_index,
            raw_pointer,
            "existing-directory-entry",
            |_| true,
        )
    }

    pub(crate) fn append_to(self, graph: &mut BTreeMap<u32, Vec<ReferenceEdge>>) {
        for (source, mut edges) in self.edges.into_inner() {
            graph.entry(source).or_default().append(&mut edges);
        }
    }
}

fn negative_candidate(kind: ReferenceKind, raw_pointer: i64) -> Candidate {
    Candidate {
        kind,
        raw_pointer,
        target_sequence: raw_pointer
            .checked_abs()
            .and_then(|value| u32::try_from(value).ok()),
    }
}

fn positive_candidate(kind: ReferenceKind, raw_pointer: i64) -> Candidate {
    Candidate {
        kind,
        raw_pointer,
        target_sequence: (raw_pointer != 0)
            .then(|| u32::try_from(raw_pointer).ok())
            .flatten(),
    }
}

fn candidates(entry: &DirectoryEntry) -> Vec<Candidate> {
    let mut values = Vec::new();
    if entry.structure < 0 {
        values.push(negative_candidate(
            ReferenceKind::Structure,
            entry.structure,
        ));
    }
    if entry.line_font < 0 {
        values.push(negative_candidate(ReferenceKind::LineFont, entry.line_font));
    }
    if entry.level < 0 {
        values.push(negative_candidate(ReferenceKind::Level, entry.level));
    }
    for (kind, pointer) in [
        (ReferenceKind::View, entry.view),
        (ReferenceKind::Transform, entry.transform),
        (ReferenceKind::LabelDisplay, entry.label_display),
    ] {
        if pointer != 0 {
            values.push(positive_candidate(kind, pointer));
        }
    }
    if entry.color < 0 {
        values.push(negative_candidate(ReferenceKind::Color, entry.color));
    }
    values
}

fn expected(kind: ReferenceKind, source: &DirectoryEntry) -> &'static str {
    match kind {
        ReferenceKind::Structure => match source.entity_type {
            422 if matches!(source.form, 0..=1) => "type-322-form-0",
            402 if matches!(source.form, 5001..=9999) => "type-302-matching-form",
            600..=699 | 10_000..=99_999 => "type-306-or-type-416",
            _ => "structure-not-permitted",
        },
        ReferenceKind::LineFont => "type-304",
        ReferenceKind::Level => "type-406-form-1",
        ReferenceKind::View => "type-410-or-type-402-form-3-4-19",
        ReferenceKind::Transform => "type-124",
        ReferenceKind::LabelDisplay => "type-402-form-5",
        ReferenceKind::Color => "type-314",
        ReferenceKind::Parameter => unreachable!("parameter edges carry their field contract"),
    }
}

fn accepts(kind: ReferenceKind, source: &DirectoryEntry, target: &DirectoryEntry) -> bool {
    match kind {
        ReferenceKind::Structure => match source.entity_type {
            422 if matches!(source.form, 0..=1) => target.entity_type == 322 && target.form == 0,
            402 if matches!(source.form, 5001..=9999) => {
                target.entity_type == 302 && target.form == source.form
            }
            600..=699 | 10_000..=99_999 => matches!(target.entity_type, 306 | 416),
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
        ReferenceKind::Parameter => unreachable!("parameter edges use their field contract"),
    }
}

fn cyclic_transform_nodes(edges: &BTreeMap<u32, Vec<ReferenceEdge>>) -> BTreeSet<u32> {
    let next = edges
        .iter()
        .filter_map(|(source, values)| {
            values
                .iter()
                .find(|edge| {
                    edge.kind == ReferenceKind::Transform && edge.resolution == Resolution::Resolved
                })
                .and_then(|edge| edge.target.as_deref())
                .and_then(|id| id.rsplit_once('#'))
                .and_then(|(_, value)| value.parse::<u32>().ok())
                .map(|target| (*source, target))
        })
        .collect::<BTreeMap<_, _>>();
    let mut cyclic = BTreeSet::new();
    let mut completed = BTreeSet::new();
    let mut active = BTreeMap::<u32, usize>::new();
    for start in next.keys().copied() {
        let mut path = Vec::new();
        let mut current = start;
        loop {
            if completed.contains(&current) {
                break;
            }
            if let Some(position) = active.get(&current).copied() {
                cyclic.extend(path[position..].iter().copied());
                break;
            }
            active.insert(current, path.len());
            path.push(current);
            let Some(target) = next.get(&current).copied() else {
                break;
            };
            current = target;
        }
        for node in path {
            active.remove(&node);
            completed.insert(node);
        }
    }
    cyclic
}

pub(crate) fn build(directory: &[DirectoryEntry]) -> BTreeMap<u32, Vec<ReferenceEdge>> {
    let index = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut graph = directory
        .iter()
        .map(|entry| {
            let edges = candidates(entry)
                .into_iter()
                .map(|candidate| {
                    let target = candidate
                        .target_sequence
                        .and_then(|value| index.get(&value).copied());
                    let resolution = if candidate.raw_pointer == 0 {
                        Resolution::Null
                    } else if candidate.target_sequence.is_none() {
                        Resolution::OutOfRange
                    } else if candidate
                        .target_sequence
                        .is_some_and(|value| value % 2 == 0)
                    {
                        Resolution::EvenSequence
                    } else if target.is_none() {
                        Resolution::Dangling
                    } else if target.is_some_and(|value| !accepts(candidate.kind, entry, value)) {
                        Resolution::WrongType
                    } else {
                        Resolution::Resolved
                    };
                    ReferenceEdge {
                        kind: candidate.kind,
                        raw_pointer: candidate.raw_pointer,
                        target: target
                            .map(|value| format!("iges:entity:directory#{}", value.sequence)),
                        resolution,
                        expected: expected(candidate.kind, entry).into(),
                        parameter_index: None,
                    }
                })
                .collect();
            (entry.sequence, edges)
        })
        .collect::<BTreeMap<_, Vec<_>>>();
    let cyclic = cyclic_transform_nodes(&graph);
    for source in cyclic {
        if let Some(edge) = graph.get_mut(&source).and_then(|edges| {
            edges
                .iter_mut()
                .find(|edge| edge.kind == ReferenceKind::Transform)
        }) {
            edge.resolution = Resolution::Cyclic;
        }
    }
    graph
}

pub(crate) fn resolved_structure_sequence(
    graph: &BTreeMap<u32, Vec<ReferenceEdge>>,
    source: u32,
) -> Option<u32> {
    graph.get(&source)?.iter().find_map(|edge| {
        (edge.kind == ReferenceKind::Structure && edge.resolution == Resolution::Resolved)
            .then(|| edge.raw_pointer.checked_abs())
            .flatten()
            .and_then(|value| u32::try_from(value).ok())
    })
}

pub(crate) fn summary_notes(graph: &BTreeMap<u32, Vec<ReferenceEdge>>) -> Vec<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for edge in graph.values().flatten() {
        *counts
            .entry(format!("{:?}", edge.resolution).to_lowercase())
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(resolution, count)| format!("references.{resolution}={count}"))
        .collect()
}

pub(crate) fn losses(
    graph: &BTreeMap<u32, Vec<ReferenceEdge>>,
    scan: &CardScan<'_>,
    parameters: &[ParameterRecord],
) -> Vec<LossNote> {
    let directory_offsets = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Directory))
        .filter_map(|line| line.sequence.map(|sequence| (sequence, line.offset)))
        .collect::<BTreeMap<_, _>>();
    let parameter_lines = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Parameter))
        .filter_map(|line| line.sequence.map(|sequence| (sequence, line.offset)))
        .collect::<BTreeMap<_, _>>();
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    graph
        .iter()
        .flat_map(|(source, edges)| {
            let directory_offsets = &directory_offsets;
            let parameter_lines = &parameter_lines;
            let records = &records;
            edges
                .iter()
                .filter(|edge| {
                    !matches!(edge.resolution, Resolution::Resolved | Resolution::Null)
                })
                .map(move |edge| {
                    let parameter_location = edge.parameter_index.and_then(|index| {
                        let record = records.get(source)?;
                        let span = record.tokens.get(index)?.span.start;
                        let card = u32::try_from(span / 64).ok()?;
                        let sequence = record.line_range.start.checked_add(card)?;
                        let offset = parameter_lines
                            .get(&sequence)?
                            .checked_add((span % 64) as u64)?;
                        Some((offset, format!("D{source}:parameter[{index}]")))
                    });
                    let location = parameter_location.or_else(|| {
                        directory_offsets
                            .get(source)
                            .copied()
                            .map(|offset| (offset, format!("D{source}")))
                    });
                    let mut note = IgesLossCode::PointerUnresolved.note(format!(
                        "IGES Directory Entry D{source} {:?} pointer {} has {:?} resolution; expected {}",
                        edge.kind, edge.raw_pointer, edge.resolution, edge.expected
                    ));
                    if let Some((offset, tag)) = location {
                        note = note.with_provenance(SourceProvenance {
                            format: "iges".into(),
                            stream: "iges".into(),
                            offset,
                            tag: Some(tag),
                        });
                    }
                    note
                })
        })
        .collect()
}

#[cfg(test)]
mod tests;
