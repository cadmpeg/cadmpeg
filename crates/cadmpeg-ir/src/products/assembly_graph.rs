// SPDX-License-Identifier: Apache-2.0
//! Occurrence identity lookup and parent placement resolution.

use super::{Occurrence, OccurrenceParent};
use crate::ids::OccurrenceId;
use crate::index::{identity_hash, DecodeStorage, IndexStorage};
use crate::transform::{Transform, TransformError};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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
    values: Vec<(u64, &'ir Occurrence, Resolution)>,
}

impl<'ir> Facts<'ir> {
    fn position(&self, id: &OccurrenceId) -> (u64, usize, Option<usize>) {
        let hash = identity_hash(id.as_str());
        let mut low = 0;
        let mut high = self.values.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.values[middle].0 < hash {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        for (offset, (candidate_hash, occurrence, _)) in self.values[low..].iter().enumerate() {
            if *candidate_hash != hash {
                break;
            }
            if occurrence.id == *id {
                return (hash, low, Some(low + offset));
            }
        }
        (hash, low, None)
    }

    pub(super) fn occurrence(&self, id: &OccurrenceId) -> Option<&'ir Occurrence> {
        self.position(id).2.map(|position| self.values[position].1)
    }
}

pub(super) fn build<'ir, S: IndexStorage>(
    rows: &'ir [Occurrence],
    storage: &S,
) -> Result<Result<Facts<'ir>, GraphError<'ir>>, S::Error> {
    let mut facts = Facts { values: Vec::new() };
    for occurrence in rows {
        let (hash, low, found) = facts.position(&occurrence.id);
        if found.is_some() {
            return Ok(Err(GraphError::Duplicate(&occurrence.id)));
        }
        facts.values.push((hash, occurrence, Resolution::Pending));
        facts.values[low..].rotate_right(1);
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
    let Some(position) = facts.position(&occurrence.id).2 else {
        return Ok(Err(GraphError::Missing {
            occurrence: &occurrence.id,
            parent: &occurrence.id,
        }));
    };
    match facts.values[position].2 {
        Resolution::Resolved(transform) => return Ok(Ok(transform)),
        Resolution::Active => return Ok(Err(GraphError::Cycle(&occurrence.id))),
        Resolution::Pending => facts.values[position].2 = Resolution::Active,
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
    facts.values[position].2 = Resolution::Resolved(transform);
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
