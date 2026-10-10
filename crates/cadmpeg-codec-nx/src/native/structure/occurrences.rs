// SPDX-License-Identifier: Apache-2.0
//! Roster-owned occurrence lane admission.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::features::NonEmptyMembers;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize, Serializer};

use super::{
    FastLoadComponentOccurrence, FastLoadComponentOccurrenceView, FastLoadComponentOccurrenceWire,
    OccurrenceLaneForm,
};

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
    ) -> impl Iterator<Item = FastLoadComponentOccurrenceView<'_>> + '_ {
        self.0.iter().flat_map(|lane| {
            lane.records
                .iter()
                .map(move |record| record.view(lane.form))
        })
    }

    pub(crate) fn from_namespace_with_context(
        ctx: &DecodeContext<'_>,
        namespace: &NativeNamespace,
    ) -> Result<Self, NativeConvertError> {
        const OPERATION: &str = "admit NX occurrence roster";
        let wire: Vec<FastLoadComponentOccurrenceWire> =
            namespace.arena_as_for_decode(ctx, "fast_load_component_occurrences")?;
        let Some(form) = wire.first().map(|record| record.occurrence_lane_form) else {
            return Ok(Self::default());
        };
        if ctx.any_by(
            &wire,
            |record| Ok(record.occurrence_lane_form != form),
            OPERATION,
        )? {
            return Err(lane_form_disagreement());
        }
        let count = wire.len();
        let mut records = ctx.vector_storage(count, "NX admitted occurrence records")?;
        let mut wire = wire.into_iter();
        for _ in ctx
            .admit_iter(&(0..count), OPERATION)
            .map_err(cadmpeg_core::CodecError::from)?
        {
            ctx.reserve_vec(&mut records, 1, "NX admitted occurrence records")?;
            let Some(record) = wire.next() else {
                break;
            };
            match FastLoadComponentOccurrence::try_from(record) {
                Ok(record) => records.push(record),
                Err(error) => {
                    return Err(NativeConvertError::InvalidCollection(
                        ctx.copy_retained_text(error, OPERATION)?,
                    ))
                }
            }
        }
        Self::from_lane(form, records)
    }

    fn from_wire_with_storage(
        wire: Vec<FastLoadComponentOccurrenceWire>,
        mut records: Vec<FastLoadComponentOccurrence>,
    ) -> Result<Self, NativeConvertError> {
        if records.capacity() < wire.len() {
            return Err(NativeConvertError::InvalidCollection(
                "occurrence output storage is too small".into(),
            ));
        }
        let Some(first) = wire.first() else {
            return Ok(Self::default());
        };
        let form = first.occurrence_lane_form;
        if wire
            .iter()
            .any(|record| record.occurrence_lane_form != form)
        {
            return Err(lane_form_disagreement());
        }
        for record in wire {
            records.push(
                FastLoadComponentOccurrence::try_from(record)
                    .map_err(|error| NativeConvertError::InvalidCollection(error.into()))?,
            );
        }
        Self::from_lane(form, records)
    }

    /// Admit the nonempty occurrence lane that shares one lane form.
    fn from_lane(
        form: OccurrenceLaneForm,
        records: Vec<FastLoadComponentOccurrence>,
    ) -> Result<Self, NativeConvertError> {
        let records =
            records
                .try_into()
                .map_err(|_: cadmpeg_ir::features::BodySelectionError| {
                    NativeConvertError::InvalidCollection(
                        "fast_load_component_occurrences: empty lane".into(),
                    )
                })?;
        Ok(Self(Some(OccurrenceLane { form, records })))
    }
}

fn lane_form_disagreement() -> NativeConvertError {
    NativeConvertError::InvalidCollection(
        "fast_load_component_occurrences.occurrence_lane_form disagrees across the roster".into(),
    )
}

impl TryFrom<Vec<FastLoadComponentOccurrenceWire>> for FastLoadOccurrences {
    type Error = NativeConvertError;

    fn try_from(wire: Vec<FastLoadComponentOccurrenceWire>) -> Result<Self, Self::Error> {
        let records = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(wire.len()).map(|()| storage)
        }
        .map_err(|_| {
            NativeConvertError::InvalidCollection("occurrence storage allocation failed".into())
        })?;
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
