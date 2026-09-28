// SPDX-License-Identifier: Apache-2.0
//! Borrowed pattern lane columns retain the existing flat JSON shape.

use super::{
    FeatureIdenticalInstanceOutputLane, FeaturePatternConstructionFixedLane,
    FeaturePatternCountedReferenceLane,
};
use crate::iter_wire::IterWire;
use serde::ser::SerializeMap;
use serde::Serialize;

struct RawPatternIndex(crate::om::reference_index::PayloadIndexToken);

impl Serialize for RawPatternIndex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.raw().serialize(serializer)
    }
}

impl Serialize for FeaturePatternCountedReferenceLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "declared_count",
            &usize::from(self.references.declared_count()),
        )?;
        wire.serialize_entry(
            "object_indices",
            &IterWire(self.references.iter().map(|(_, token, _)| token.value())),
        )?;
        wire.serialize_entry(
            "raw_object_indices",
            &IterWire(
                self.references
                    .iter()
                    .map(|(_, token, _)| RawPatternIndex(token)),
            ),
        )?;
        wire.serialize_entry(
            "data_blocks",
            &IterWire(
                self.references
                    .iter()
                    .map(|(_, _, target)| target.as_deref()),
            ),
        )?;
        wire.serialize_entry("source_offset", &self.references.offset())?;
        wire.serialize_entry(
            "object_index_source_offsets",
            &IterWire(self.references.iter().map(|(offset, _, _)| offset)),
        )?;
        wire.end()
    }
}

impl Serialize for FeaturePatternConstructionFixedLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("construction_payload", &self.construction_payload)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry(
            "values",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.scalar.value())),
        )?;
        wire.serialize_entry(
            "markers",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.marker.byte())),
        )?;
        wire.serialize_entry(
            "raw_values",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.scalar.raw())),
        )?;
        wire.serialize_entry("payload_offset", &self.lane.offset())?;
        wire.serialize_entry(
            "value_payload_offsets",
            &IterWire(self.lane.iter().map(|(offset, _, _)| offset)),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "value_source_offsets",
            &IterWire(self.lane.iter().map(|(_, _, source)| *source)),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureIdenticalInstanceOutputLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("leading_schema_index", &self.leading_schema_index)?;
        wire.serialize_entry("count_schema_index", &self.count_schema_index.value())?;
        wire.serialize_entry("row_schema_indices", &self.count_schema_index.row_indices())?;
        wire.serialize_entry(
            "declared_count",
            &(usize::from(self.selectors.declared_count()) - 1),
        )?;
        wire.serialize_entry(
            "selectors",
            &IterWire(
                self.selectors
                    .as_slice()
                    .iter()
                    .map(|token| token.atom.value()),
            ),
        )?;
        wire.serialize_entry(
            "raw_selectors",
            &IterWire(
                self.selectors
                    .as_slice()
                    .iter()
                    .map(|token| token.atom.raw()),
            ),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "selector_source_offsets",
            &IterWire(self.selectors.as_slice().iter().map(|token| token.offset)),
        )?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;
    use serde::de::DeserializeOwned;

    fn bytes_and_limit<T, W>(json: &str)
    where
        T: DeserializeOwned + Serialize + Clone + Into<W>,
        W: Serialize + From<T>,
    {
        let json = json.replace("\"id\":\"lane\"", "\"id\":\"nx:feature:pattern#0\"");
        let value: T = serde_json::from_str(&json).unwrap();
        let borrowed = serde_json::to_vec(&value).unwrap();
        let owned = serde_json::to_vec(&W::from(value.clone())).unwrap();
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed, json.as_bytes());
        assert_native_limit(
            &value,
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        );
    }

    #[test]
    fn pattern_fixed_borrowed_bytes_and_limit() {
        bytes_and_limit::<
            FeaturePatternConstructionFixedLane,
            super::super::FeaturePatternConstructionFixedLaneWire,
        >(
            r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,176],"raw_values":[[32,0,0,0,0,0,0],[64,0,0,0,0,0,0]],"payload_offset":0,"value_payload_offsets":[18,26],"source_offset":100,"value_source_offsets":[118,126]}"#,
        );
    }

    #[test]
    fn pattern_transform_scalar_borrowed_bytes_and_limit() {
        bytes_and_limit::<
            super::super::FeaturePatternTransformLane,
            super::super::FeaturePatternTransformLaneWire,
        >(
            r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":3,"encodings":["binary32","binary64"],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[48,16,0,0,0,0,0,0]],"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"value_source_offsets":[110,120],"selector_source_offsets":[114,128]}"#,
        );
    }

    #[test]
    fn pattern_transform_wide_borrowed_bytes_and_limit() {
        bytes_and_limit::<
            super::super::FeaturePatternTransformLane,
            super::super::FeaturePatternTransformLaneWire,
        >(
            r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#,
        );
    }

    #[test]
    fn multi_instance_borrowed_bytes_and_limit() {
        bytes_and_limit::<
            super::super::FeatureMultiInstanceOutputLane,
            super::super::FeatureMultiInstanceOutputLaneWire,
        >(
            r#"{"id":"lane","operation_label":"operation","declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"ordinals":[2,2],"row_indices":[2,3],"instance_count":2,"trailing_object_indices":[9],"raw_trailing_object_indices":[[9]],"source_offset":100,"selector_source_offsets":[110,120],"trailing_object_index_source_offsets":[130]}"#,
        );
    }

    #[test]
    fn identical_instance_borrowed_bytes_and_limit() {
        bytes_and_limit::<
            FeatureIdenticalInstanceOutputLane,
            super::super::FeatureIdenticalInstanceOutputLaneWire,
        >(
            r#"{"id":"lane","operation_label":"operation","leading_schema_index":4,"count_schema_index":5,"row_schema_indices":[6,7,8],"declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"selector_source_offsets":[110,120]}"#,
        );
    }

    #[test]
    fn pattern_counted_reference_borrowed_bytes_and_limit() {
        bytes_and_limit::<
            FeaturePatternCountedReferenceLane,
            super::super::FeaturePatternCountedReferenceLaneWire,
        >(
            r#"{"id":"nx:feature:pattern-counted#0","operation_label":"pattern","declared_count":2,"object_indices":[1],"raw_object_indices":[[240,1]],"data_blocks":[null],"source_offset":18,"object_index_source_offsets":[20]}"#,
        );
    }
}

#[derive(Clone)]
struct PatternScalars<'a> {
    rows: &'a super::PatternRows<crate::om::compact::LocatedCompactIndex<u64>, u64>,
    at: usize,
}

struct RawPatternScalar<'a>(PatternScalarRef<'a>);

impl Serialize for RawPatternScalar<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.raw().serialize(serializer)
    }
}

enum PatternScalarRef<'a> {
    Shifted(&'a super::PatternValue<super::ShiftedScalar, u64>),
    Binary64(&'a super::PatternValue<super::ShiftedBinary64, u64>),
    Terminal(&'a super::PatternValue<super::PatternTerminal, u64>),
}

impl PatternScalarRef<'_> {
    fn encoding(&self) -> super::PatternScalarEncoding {
        match self {
            Self::Shifted(value) => match value.scalar {
                super::ShiftedScalar::Binary32(_) => super::PatternScalarEncoding::Binary32,
                super::ShiftedScalar::Binary64(_) => super::PatternScalarEncoding::Binary64,
            },
            Self::Binary64(_) => super::PatternScalarEncoding::Binary64,
            Self::Terminal(value) => value.scalar.encoding(),
        }
    }
    fn value(&self) -> f64 {
        match self {
            Self::Shifted(value) => value.scalar.value().get(),
            Self::Binary64(value) => value.scalar.value().get(),
            Self::Terminal(value) => value.scalar.value().get(),
        }
    }
    fn raw(&self) -> &[u8] {
        match self {
            Self::Shifted(value) => value.scalar.raw(),
            Self::Binary64(value) => value.scalar.as_bytes(),
            Self::Terminal(value) => value.scalar.raw(),
        }
    }
    fn offset(&self) -> u64 {
        match self {
            Self::Shifted(value) => value.offset,
            Self::Binary64(value) => value.offset,
            Self::Terminal(value) => value.offset,
        }
    }
}

impl<'a> Iterator for PatternScalars<'a> {
    type Item = PatternScalarRef<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let value = match self.rows {
            super::PatternRows::Scalar(rows) => {
                Self::Item::Shifted(&rows.as_slice().get(self.at)?.values)
            }
            super::PatternRows::Wide(rows) => {
                let row = rows.as_slice().get(self.at / 5)?;
                match self.at % 5 {
                    0..=3 => Self::Item::Binary64(&row.values.first[self.at % 5]),
                    _ => Self::Item::Terminal(&row.values.terminal),
                }
            }
        };
        self.at += 1;
        Some(value)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = match self.rows {
            super::PatternRows::Scalar(rows) => rows.len(),
            super::PatternRows::Wide(rows) => rows.len() * 5,
        } - self.at;
        (len, Some(len))
    }
}

impl Serialize for super::FeaturePatternTransformLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("row_schema_index", &self.row_schema_index)?;
        let layout = match self.rows {
            super::PatternRows::Scalar(_) => super::FeaturePatternTransformLayout::ScalarRows,
            super::PatternRows::Wide(_) => super::FeaturePatternTransformLayout::WideRows,
        };
        wire.serialize_entry("layout", &layout)?;
        wire.serialize_entry("declared_count", &usize::from(self.rows.declared_count()))?;
        let scalars = PatternScalars {
            rows: &self.rows,
            at: 0,
        };
        wire.serialize_entry(
            "encodings",
            &IterWire(scalars.clone().map(|value| value.encoding())),
        )?;
        wire.serialize_entry(
            "values",
            &IterWire(scalars.clone().map(|value| value.value())),
        )?;
        wire.serialize_entry(
            "raw_values",
            &IterWire(scalars.clone().map(RawPatternScalar)),
        )?;
        match &self.rows {
            super::PatternRows::Scalar(rows) => {
                wire.serialize_entry(
                    "selectors",
                    &IterWire(rows.as_slice().iter().map(|row| row.selector.atom.value())),
                )?;
                wire.serialize_entry(
                    "raw_selectors",
                    &IterWire(rows.as_slice().iter().map(|row| row.selector.atom.raw())),
                )?;
            }
            super::PatternRows::Wide(rows) => {
                wire.serialize_entry(
                    "selectors",
                    &IterWire(rows.as_slice().iter().map(|row| row.selector.atom.value())),
                )?;
                wire.serialize_entry(
                    "raw_selectors",
                    &IterWire(rows.as_slice().iter().map(|row| row.selector.atom.raw())),
                )?;
            }
        }
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "value_source_offsets",
            &IterWire(scalars.map(|value| value.offset())),
        )?;
        match &self.rows {
            super::PatternRows::Scalar(rows) => wire.serialize_entry(
                "selector_source_offsets",
                &IterWire(rows.as_slice().iter().map(|row| row.selector.offset)),
            )?,
            super::PatternRows::Wide(rows) => wire.serialize_entry(
                "selector_source_offsets",
                &IterWire(rows.as_slice().iter().map(|row| row.selector.offset)),
            )?,
        }
        wire.end()
    }
}

impl Serialize for super::FeatureMultiInstanceOutputLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        let selectors = self.outputs.selectors();
        let references = self.outputs.references();
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("declared_count", &(selectors.len() + 1))?;
        wire.serialize_entry(
            "selectors",
            &IterWire(selectors.iter().map(|token| token.atom.value())),
        )?;
        wire.serialize_entry(
            "raw_selectors",
            &IterWire(selectors.iter().map(|token| token.atom.raw())),
        )?;
        wire.serialize_entry(
            "ordinals",
            &IterWire((0..selectors.len()).map(|index| {
                let value = selectors[index].atom.value();
                selectors[..index]
                    .iter()
                    .filter(|token| token.atom.value() == value)
                    .fold(2_u8, |ordinal, _| ordinal + 1)
            })),
        )?;
        wire.serialize_entry("row_indices", &IterWire(2..selectors.len() + 2))?;
        wire.serialize_entry("instance_count", &(references.len() + 1))?;
        wire.serialize_entry(
            "trailing_object_indices",
            &IterWire(references.iter().map(|token| token.token.value())),
        )?;
        wire.serialize_entry(
            "raw_trailing_object_indices",
            &IterWire(references.iter().map(|token| token.token.raw())),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "selector_source_offsets",
            &IterWire(selectors.iter().map(|token| token.offset)),
        )?;
        wire.serialize_entry(
            "trailing_object_index_source_offsets",
            &IterWire(references.iter().map(|token| token.offset)),
        )?;
        wire.end()
    }
}
