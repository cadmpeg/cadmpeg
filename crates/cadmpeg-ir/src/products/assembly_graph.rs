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
    fn position<S: IndexStorage>(
        &self,
        id: &OccurrenceId,
        storage: &S,
    ) -> Result<(u64, usize, Option<usize>), S::Error> {
        storage.work(id.as_str().len(), "assembly identity hash")?;
        let hash = identity_hash(id.as_str());
        let mut low = 0;
        let mut high = self.values.len();
        while low < high {
            storage.work(1, "assembly identity hash search")?;
            let middle = low + (high - low) / 2;
            if self.values[middle].0 < hash {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        for (offset, (candidate_hash, occurrence, _)) in self.values[low..].iter().enumerate() {
            storage.work(1, "assembly identity collision scan")?;
            if *candidate_hash != hash {
                break;
            }
            storage.work(id.as_str().len(), "assembly identity comparison")?;
            if occurrence.id == *id {
                return Ok((hash, low, Some(low + offset)));
            }
        }
        Ok((hash, low, None))
    }

    pub(super) fn occurrence<S: IndexStorage>(
        &self,
        id: &OccurrenceId,
        storage: &S,
    ) -> Result<Option<&'ir Occurrence>, S::Error> {
        Ok(self
            .position(id, storage)?
            .2
            .map(|position| self.values[position].1))
    }
}

pub(super) fn build<'ir, S: IndexStorage>(
    rows: &'ir [Occurrence],
    storage: &S,
) -> Result<Result<Facts<'ir>, GraphError<'ir>>, S::Error> {
    let mut facts = Facts { values: Vec::new() };
    for occurrence in rows {
        storage.work(1, "assembly occurrence row")?;
        let (hash, low, found) = facts.position(&occurrence.id, storage)?;
        if found.is_some() {
            return Ok(Err(GraphError::Duplicate(&occurrence.id)));
        }
        storage.work(facts.values.len() - low, "assembly identity slot movement")?;
        storage.work(1, "assembly identity slot insertion")?;
        storage.push(
            &mut facts.values,
            (hash, occurrence, Resolution::Pending),
            "assembly identity slots",
        )?;
        facts.values[low..].rotate_right(1);
    }
    for occurrence in rows {
        storage.work(1, "assembly placement root")?;
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
    storage.work(1, "assembly placement visit")?;
    let Some(position) = facts.position(&occurrence.id, storage)?.2 else {
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
            let Some(parent_occurrence) = facts.occurrence(parent, storage)? else {
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
    let (result, _storage) = ctx.with_scoped_storage("assembly validation storage", || {
        build(occurrences, &DecodeStorage(ctx))
            .map(|result| result.is_ok())
            .map_err(CodecError::from)
    })?;
    Ok(result)
}

#[cfg(test)]
mod tests;
