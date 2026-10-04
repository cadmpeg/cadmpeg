// SPDX-License-Identifier: Apache-2.0
//! Nonempty typed reference lanes and maps.

use super::record_kind::RecordKind;
use crate::framing::xmt_reference::NonNullXmt;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaggedKind {
    Record(RecordKind),
    Type79,
    Type80,
}
impl TaggedKind {
    fn new(kind: u16) -> Result<Self, &'static str> {
        match kind {
            79 => Ok(Self::Type79),
            80 => Ok(Self::Type80),
            98 => Err("references.kind: type 98 is not a tagged reference"),
            _ => RecordKind::try_from(kind)
                .map(Self::Record)
                .map_err(|_| "references.kind: unknown tagged reference type"),
        }
    }
    fn code(self) -> u16 {
        match self {
            Self::Record(kind) => u16::from(kind.code()),
            Self::Type79 => 79,
            Self::Type80 => 80,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapKind {
    Tagged(TaggedKind),
    Type11,
    Type35,
    Type55,
    Type61,
    Type100,
}
impl MapKind {
    fn new(kind: u16) -> Result<Self, &'static str> {
        match kind {
            11 => Ok(Self::Type11),
            35 => Ok(Self::Type35),
            55 => Ok(Self::Type55),
            61 => Ok(Self::Type61),
            100 => Ok(Self::Type100),
            _ => TaggedKind::new(kind)
                .map(Self::Tagged)
                .map_err(|_| "entries.kind: unknown map entry type"),
        }
    }
    fn code(self) -> u16 {
        match self {
            Self::Tagged(kind) => kind.code(),
            Self::Type11 => 11,
            Self::Type35 => 35,
            Self::Type55 => 55,
            Self::Type61 => 61,
            Self::Type100 => 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<(u16, u32)>")]
pub(crate) struct TaggedReferences {
    first: (TaggedKind, NonNullXmt),
    rest: Vec<(TaggedKind, NonNullXmt)>,
}

impl Serialize for TaggedReferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(1 + self.rest.len()))?;
        for (kind, reference) in std::iter::once(&self.first).chain(&self.rest) {
            sequence.serialize_element(&(kind.code(), u32::from(*reference)))?;
        }
        sequence.end()
    }
}
impl TryFrom<Vec<(u16, u32)>> for TaggedReferences {
    type Error = &'static str;
    fn try_from(raw: Vec<(u16, u32)>) -> Result<Self, Self::Error> {
        let mut entries = raw.into_iter().map(|(kind, reference)| {
            Ok((
                TaggedKind::new(kind)?,
                NonNullXmt::try_from(reference)
                    .map_err(|_| "references.reference: must exceed one")?,
            ))
        });
        let first = entries
            .next()
            .ok_or("references: require at least one tagged reference")??;
        let rest = entries.collect::<Result<_, _>>()?;
        Ok(Self { first, rest })
    }
}
#[cfg(test)]
std::thread_local! {
    static TAGGED_REFERENCES_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static MAP_ENTRIES_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<TaggedReferences> for Vec<(u16, u32)> {
    fn from(lane: TaggedReferences) -> Self {
        TAGGED_REFERENCES_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        std::iter::once(lane.first)
            .chain(lane.rest)
            .map(|(kind, reference)| (kind.code(), reference.into()))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<(u32, u16)>")]
pub(crate) struct MapEntries {
    first: (u32, MapKind),
    rest: Vec<(u32, MapKind)>,
}

impl Serialize for MapEntries {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(1 + self.rest.len()))?;
        for (reference, kind) in std::iter::once(&self.first).chain(&self.rest) {
            sequence.serialize_element(&(*reference, kind.code()))?;
        }
        sequence.end()
    }
}
impl MapEntries {
    pub(super) fn last_kind(&self) -> u16 {
        self.rest.last().unwrap_or(&self.first).1.code()
    }
}
impl TryFrom<Vec<(u32, u16)>> for MapEntries {
    type Error = &'static str;
    fn try_from(raw: Vec<(u32, u16)>) -> Result<Self, Self::Error> {
        let mut entries = raw.into_iter().map(|(reference, kind)| {
            if reference == 1 {
                return Err("entries.reference: one is the terminal clause");
            }
            Ok((reference, MapKind::new(kind)?))
        });
        let first = entries
            .next()
            .ok_or("entries: require at least one map entry")??;
        let rest = entries.collect::<Result<_, _>>()?;
        Ok(Self { first, rest })
    }
}
#[cfg(test)]
impl From<MapEntries> for Vec<(u32, u16)> {
    fn from(map: MapEntries) -> Self {
        MAP_ENTRIES_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        std::iter::once(map.first)
            .chain(map.rest)
            .map(|(reference, kind)| (reference, kind.code()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MapEntries, TaggedReferences, MAP_ENTRIES_INTO_WIRE_COUNT,
        TAGGED_REFERENCES_INTO_WIRE_COUNT,
    };

    #[test]
    fn reference_lanes_preserve_distinct_kind_and_reference_domains() {
        for json in ["[[79,2],[80,40000]]", "[[67,4],[81,5]]"] {
            let lane: TaggedReferences = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&lane).unwrap(), json);
            assert_eq!(
                serde_json::to_vec(&lane).unwrap(),
                serde_json::to_vec(&Vec::<(u16, u32)>::from(lane.clone())).unwrap()
            );
        }
        for json in ["[]", "[[98,2]]", "[[100,2]]", "[[79,1]]", "[[79,0]]"] {
            assert!(serde_json::from_str::<TaggedReferences>(json)
                .unwrap_err()
                .to_string()
                .contains("references"));
        }
        for json in ["[[0,11]]", "[[40000,81],[3,100]]", "[[2,67],[3,61]]"] {
            let map: MapEntries = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&map).unwrap(), json);
            assert_eq!(
                serde_json::to_vec(&map).unwrap(),
                serde_json::to_vec(&Vec::<(u32, u16)>::from(map.clone())).unwrap()
            );
        }
        for json in ["[]", "[[1,81]]", "[[2,98]]", "[[2,612]]"] {
            assert!(serde_json::from_str::<MapEntries>(json)
                .unwrap_err()
                .to_string()
                .contains("entries"));
        }
    }

    #[test]
    fn tagged_references_native_limit_refuses_before_owned_wire_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            references: &'a TaggedReferences,
        }
        let references: TaggedReferences = serde_json::from_str("[[79,2],[80,40000]]").unwrap();
        let record = Record {
            id: "nx:deltas:tagged-references#0",
            references: &references,
        };
        TAGGED_REFERENCES_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id":"nx:deltas:tagged-references#0", "references": [[79,2],[80,40000]]}),
        );
        TAGGED_REFERENCES_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn map_entries_native_limit_refuses_before_owned_wire_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            entries: &'a MapEntries,
        }
        let entries: MapEntries = serde_json::from_str("[[40000,81],[3,100]]").unwrap();
        let record = Record {
            id: "nx:deltas:map-entries#0",
            entries: &entries,
        };
        MAP_ENTRIES_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id":"nx:deltas:map-entries#0", "entries": [[40000,81],[3,100]]}),
        );
        MAP_ENTRIES_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
