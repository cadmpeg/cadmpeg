// SPDX-License-Identifier: Apache-2.0
//! Roster-owned occurrence lane admission.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::features::NonEmptyMembers;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize, Serializer};

use super::{FastLoadComponentOccurrence, FastLoadComponentOccurrenceWire, OccurrenceLaneForm};

/// Occurrences sharing one roster-level lane form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<FastLoadComponentOccurrenceWire>")]
pub(crate) struct FastLoadOccurrences(Option<OccurrenceLane>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct OccurrenceLane {
    form: OccurrenceLaneForm,
    records: NonEmptyMembers<FastLoadComponentOccurrence>,
}

impl FastLoadOccurrences {
    pub(super) fn from_decoded(
        form: OccurrenceLaneForm,
        records: Vec<FastLoadComponentOccurrence>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let records =
            records
                .try_into()
                .map_err(|_: cadmpeg_ir::features::BodySelectionError| {
                    cadmpeg_core::CodecError::malformed("empty NX fast-load occurrence lane")
                })?;
        Ok(Self(Some(OccurrenceLane { form, records })))
    }

    pub(in crate::native) fn as_slice(&self) -> &[FastLoadComponentOccurrence] {
        self.0.as_ref().map_or(&[], |lane| lane.records.as_slice())
    }

    pub(in crate::native) fn wire_records(
        &self,
    ) -> impl Iterator<Item = FastLoadComponentOccurrenceWire> + '_ {
        self.0.iter().flat_map(|lane| {
            lane.records
                .iter()
                .map(move |record| FastLoadComponentOccurrenceWire::from((record, lane.form)))
        })
    }

    pub(crate) fn from_namespace_with_context(
        ctx: &DecodeContext<'_>,
        namespace: &NativeNamespace,
    ) -> Result<Self, NativeConvertError> {
        let wire: Vec<FastLoadComponentOccurrenceWire> = namespace.arena_as_charged(ctx, "fast_load_component_occurrences")?;
        let work = wire.len().checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("admit NX occurrence roster", u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(u64_from_index(work), "admit NX occurrence roster")?;
        let records = ctx.retained_vec(wire.len(), "NX admitted occurrence records")?;
        Self::from_wire_with_storage(wire, records)
    }

    fn from_wire_with_storage(
        wire: Vec<FastLoadComponentOccurrenceWire>,
        mut records: Vec<FastLoadComponentOccurrence>,
    ) -> Result<Self, NativeConvertError> {
        if records.capacity() < wire.len() {
            return Err(NativeConvertError::InvalidCollection("occurrence output storage is too small".into()));
        }
        let Some(first) = wire.first() else {
            return Ok(Self::default());
        };
        let form = first.occurrence_lane_form;
        if wire
            .iter()
            .any(|record| record.occurrence_lane_form != form)
        {
            return Err(NativeConvertError::InvalidCollection(
                "fast_load_component_occurrences.occurrence_lane_form disagrees across the roster"
                    .into(),
            ));
        }
        for record in wire {
            records.push(FastLoadComponentOccurrence::try_from(record)
                .map_err(|error| NativeConvertError::InvalidCollection(error.into()))?);
        }
        Ok(Self(Some(OccurrenceLane {
            form,
            records: records.try_into().map_err(
                |error: cadmpeg_ir::features::BodySelectionError| {
                    NativeConvertError::InvalidCollection(error.to_string())
                },
            )?,
        })))
    }
}

impl TryFrom<Vec<FastLoadComponentOccurrenceWire>> for FastLoadOccurrences {
    type Error = NativeConvertError;

    fn try_from(wire: Vec<FastLoadComponentOccurrenceWire>) -> Result<Self, Self::Error> {
        let records = DecodeContext::admitted_vec(wire.len(), "NX admitted occurrence records")?;
        Self::from_wire_with_storage(wire, records)
    }
}

impl Serialize for FastLoadOccurrences {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.wire_records())
    }
}

impl TryFrom<&NativeNamespace> for FastLoadOccurrences {
    type Error = NativeConvertError;

    fn try_from(namespace: &NativeNamespace) -> Result<Self, Self::Error> {
        namespace
            .arena_as("fast_load_component_occurrences")?
            .try_into()
    }
}

#[cfg(test)]
mod tests;
