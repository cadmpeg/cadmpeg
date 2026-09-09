// SPDX-License-Identifier: Apache-2.0
//! Roster-owned occurrence lane admission.

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
    pub(crate) fn as_slice(&self) -> &[FastLoadComponentOccurrence] {
        self.0.as_ref().map_or(&[], |lane| lane.records.as_slice())
    }

    pub(crate) fn wire_records(
        &self,
    ) -> impl Iterator<Item = FastLoadComponentOccurrenceWire> + '_ {
        self.0.iter().flat_map(|lane| {
            lane.records
                .iter()
                .map(move |record| FastLoadComponentOccurrenceWire::from((record, lane.form)))
        })
    }
}

impl TryFrom<Vec<FastLoadComponentOccurrenceWire>> for FastLoadOccurrences {
    type Error = NativeConvertError;

    fn try_from(wire: Vec<FastLoadComponentOccurrenceWire>) -> Result<Self, Self::Error> {
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
        let records = wire
            .into_iter()
            .map(FastLoadComponentOccurrence::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| NativeConvertError::InvalidCollection(error.into()))?;
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
