//! Scalar ownership and record boundaries for cosmetic-thread diameter children.

use crate::records::{Feature, FeatureInputLane, FeatureInputScalar, ObjectId};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

struct DiameterRecords<'a> {
    scalars: HashMap<u32, Option<&'a FeatureInputScalar>>,
    boundaries: Vec<u64>,
}

/// A lane's last name definitions, scalar owners and following record offsets.
pub(in crate::resolved_features) struct CosmeticDiameterIndex<'a, 'ctx> {
    lane: &'a FeatureInputLane,
    records: OnceCell<DiameterRecords<'a>>,
    storage: RefCell<ScopedReservation<'ctx>>,
}

impl<'a, 'ctx> CosmeticDiameterIndex<'a, 'ctx> {
    pub(in crate::resolved_features) fn new(
        ctx: &'ctx DecodeContext<'_>,
        lane: &'a FeatureInputLane,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            lane,
            records: OnceCell::new(),
            storage: RefCell::new(
                ctx.reserve_scoped(0, "index SLDPRT cosmetic diameter intervals")?,
            ),
        })
    }

    pub(in crate::resolved_features) fn tail(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &Feature,
    ) -> Result<Option<std::ops::Range<usize>>, CodecError> {
        const OPERATION: &str = "resolve SLDPRT cosmetic diameter interval";
        let Some(diameter_id) = feature.source_value().and_then(|id| id.checked_sub(1)) else {
            return Ok(None);
        };
        let records = match self.records.get() {
            Some(records) => records,
            None => {
                let records = self.storage.borrow_mut().with_storage(|| {
                    const OPERATION: &str = "index SLDPRT cosmetic diameter intervals";
                    let mut name_storage = ctx.reserve_scoped(0, OPERATION)?;
                    let mut names = HashMap::new();
                    let mut scalars = HashMap::new();
                    let mut boundaries = Vec::new();
                    for name in ctx.admit_iter(&self.lane.names, OPERATION)? {
                        name_storage.with_storage(|| {
                            ctx.insert_hash_map(&mut names, name.id.as_str(), name, OPERATION)
                        })?;
                        if name.object_id != Some(ObjectId::Absent) {
                            ctx.push_vec(&mut boundaries, name.offset, OPERATION)?;
                        }
                    }
                    for scalar in ctx.admit_iter(&self.lane.scalars, OPERATION)? {
                        if ctx
                            .get_hash_map(&names, scalar.name.as_str(), OPERATION)?
                            .is_some_and(|name| name.value == "D2")
                        {
                            if let Some(selected) =
                                ctx.get_mut_hash_map(&mut scalars, &scalar.object_id, OPERATION)?
                            {
                                *selected = None;
                            } else {
                                ctx.insert_hash_map(
                                    &mut scalars,
                                    scalar.object_id,
                                    Some(scalar),
                                    OPERATION,
                                )?;
                            }
                        }
                        ctx.push_vec(&mut boundaries, scalar.offset, OPERATION)?;
                    }
                    drop(names);
                    drop(name_storage);
                    ctx.sort_unstable_by(&mut boundaries, |offset| offset, Ord::cmp, OPERATION)?;
                    Ok::<_, CodecError>(DiameterRecords {
                        scalars,
                        boundaries,
                    })
                })?;
                self.records.get_or_init(|| records)
            }
        };
        let Some(selected) = ctx
            .get_hash_map(&records.scalars, &diameter_id, OPERATION)?
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let Some(start) = usize::try_from(selected.offset)
            .ok()
            .and_then(|offset| offset.checked_add(8))
        else {
            return Ok(None);
        };
        let next = ctx.partition_point(
            &records.boundaries,
            |offset| Ok(*offset < u64_from_index(start)),
            OPERATION,
        )?;
        let end = records
            .boundaries
            .get(next)
            .and_then(|offset| usize::try_from(*offset).ok())
            .unwrap_or(self.lane.native_payload.len());
        Ok((start < end).then_some(start..end))
    }
}

#[cfg(test)]
mod tests;
