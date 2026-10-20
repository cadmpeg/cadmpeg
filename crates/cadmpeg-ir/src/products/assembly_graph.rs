// SPDX-License-Identifier: Apache-2.0
//! Occurrence identity lookup and parent placement resolution.

use super::{Occurrence, OccurrenceParent};
use crate::ids::OccurrenceId;
use crate::index::{DecodeStorage, IndexStorage};
use crate::transform::{Transform, TransformError};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{hash_map::Entry, HashMap};

#[derive(Debug)]
pub(super) enum GraphError<'ir> {
    Duplicate(&'ir OccurrenceId),
    Missing {
        occurrence: &'ir OccurrenceId,
        parent: &'ir OccurrenceId,
    },
    Cycle(&'ir OccurrenceId),
    Transform {
        occurrence: &'ir OccurrenceId,
        source: TransformError,
    },
}

#[derive(Clone, Copy)]
enum Resolution {
    Pending,
    Active,
    Resolved(Transform),
}

pub(super) struct Facts<'ir> {
    positions: HashMap<&'ir str, usize>,
    values: Vec<(&'ir Occurrence, Resolution)>,
}

impl<'ir> Facts<'ir> {
    fn position(&self, id: &OccurrenceId) -> Option<usize> {
        self.positions.get(id.as_str()).copied()
    }

    pub(super) fn occurrence(&self, id: &OccurrenceId) -> Option<&'ir Occurrence> {
        self.position(id).map(|position| self.values[position].0)
    }
}

pub(super) fn build<'ir, S: IndexStorage>(
    rows: &'ir [Occurrence],
    storage: &S,
) -> Result<Result<Facts<'ir>, GraphError<'ir>>, S::Error> {
    let mut facts = Facts {
        positions: HashMap::new(),
        values: Vec::new(),
    };
    for occurrence in rows {
        match facts.positions.entry(occurrence.id.as_str()) {
            Entry::Occupied(_) => return Ok(Err(GraphError::Duplicate(&occurrence.id))),
            Entry::Vacant(entry) => {
                entry.insert(facts.values.len());
                facts.values.push((occurrence, Resolution::Pending));
            }
        }
    }
    for occurrence in rows {
        if let Err(error) = resolve_occurrence(occurrence, &mut facts, storage)? {
            return Ok(Err(error));
        }
    }
    Ok(Ok(facts))
}

pub(super) fn resolve_occurrence<'ir, S: IndexStorage>(
    occurrence: &'ir Occurrence,
    facts: &mut Facts<'ir>,
    storage: &S,
) -> Result<Result<Transform, GraphError<'ir>>, S::Error> {
    let _depth = storage.enter_nested("assembly placement depth")?;
    let Some(position) = facts.position(&occurrence.id) else {
        return Ok(Err(GraphError::Missing {
            occurrence: &occurrence.id,
            parent: &occurrence.id,
        }));
    };
    match facts.values[position].1 {
        Resolution::Resolved(transform) => return Ok(Ok(transform)),
        Resolution::Active => return Ok(Err(GraphError::Cycle(&occurrence.id))),
        Resolution::Pending => facts.values[position].1 = Resolution::Active,
    }
    let parent = match &occurrence.parent {
        OccurrenceParent::Root {} => Transform::identity(),
        OccurrenceParent::Occurrence { occurrence: parent } => {
            let Some(parent_occurrence) = facts.occurrence(parent) else {
                return Ok(Err(GraphError::Missing {
                    occurrence: &occurrence.id,
                    parent,
                }));
            };
            match resolve_occurrence(parent_occurrence, facts, storage)? {
                Ok(transform) => transform,
                Err(error) => return Ok(Err(error)),
            }
        }
    };
    let transform = match occurrence
        .effective_transform()
        .and_then(|local| parent.compose(local))
    {
        Ok(transform) => transform,
        Err(source) => {
            return Ok(Err(GraphError::Transform {
                occurrence: &occurrence.id,
                source,
            }))
        }
    };
    facts.values[position].1 = Resolution::Resolved(transform);
    Ok(Ok(transform))
}

pub(crate) fn validate(
    ctx: &DecodeContext<'_>,
    occurrences: &[Occurrence],
) -> Result<bool, CodecError> {
    Ok(build(occurrences, &DecodeStorage(ctx))?.is_ok())
}

#[cfg(test)]
mod tests;
