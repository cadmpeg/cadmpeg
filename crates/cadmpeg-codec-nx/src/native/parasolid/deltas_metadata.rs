// SPDX-License-Identifier: Apache-2.0

use super::deltas_event_id;
use crate::deltas::pmark_state::PmarkState;
use crate::deltas::section::{MapEntry, SectionKind, TerminalWord};
use crate::records::deltas::Metadata;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MetadataWire")]
pub(in crate::native) struct ParasolidDeltasMetadata {
    pub(in crate::native) id: String,
    pub(in crate::native) stream_ordinal: u32,
    pub(in crate::native) inflated_offset: u64,
    byte_len: u64,
    sha256: Sha256Digest,
    #[serde(flatten)]
    production: Production,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "production", rename_all = "snake_case")]
enum Production {
    Map {
        entries: Vec<MapEntry>,
        terminal_word: TerminalWord,
    },
    Pmark {
        state: PmarkState,
    },
    Terminator {
        closes: SectionKind,
        section_offset: u64,
    },
    FinalTerminator,
}

#[derive(Deserialize)]
#[serde(tag = "production", rename_all = "snake_case", deny_unknown_fields)]
enum MetadataWire {
    Map {
        id: String,
        stream_ordinal: u32,
        inflated_offset: u64,
        byte_len: u64,
        sha256: Sha256Digest,
        entries: Vec<MapEntry>,
        terminal_word: TerminalWord,
    },
    Pmark {
        id: String,
        stream_ordinal: u32,
        inflated_offset: u64,
        byte_len: u64,
        sha256: Sha256Digest,
        state: PmarkState,
    },
    Terminator {
        id: String,
        stream_ordinal: u32,
        inflated_offset: u64,
        byte_len: u64,
        sha256: Sha256Digest,
        closes: SectionKind,
        section_offset: u64,
    },
    FinalTerminator {
        id: String,
        stream_ordinal: u32,
        inflated_offset: u64,
        byte_len: u64,
        sha256: Sha256Digest,
    },
}

impl TryFrom<MetadataWire> for ParasolidDeltasMetadata {
    type Error = &'static str;
    fn try_from(wire: MetadataWire) -> Result<Self, Self::Error> {
        let (id, stream_ordinal, inflated_offset, byte_len, sha256, production) = match wire {
            MetadataWire::Map {
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
                entries,
                terminal_word,
            } => (
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
                Production::Map {
                    entries,
                    terminal_word,
                },
            ),
            MetadataWire::Pmark {
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
                state,
            } => (
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
                Production::Pmark { state },
            ),
            MetadataWire::Terminator {
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
                closes,
                section_offset,
            } => (
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
                Production::Terminator {
                    closes,
                    section_offset,
                },
            ),
            MetadataWire::FinalTerminator {
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
            } => (
                id,
                stream_ordinal,
                inflated_offset,
                byte_len,
                sha256,
                Production::FinalTerminator,
            ),
        };
        if id != format!("nx:s{stream_ordinal}:deltas-metadata#{inflated_offset}")
            || inflated_offset.checked_add(byte_len).is_none()
            || production.encoded_len() != Some(byte_len)
            || matches!(production, Production::Terminator { section_offset, .. } if section_offset > inflated_offset)
        {
            return Err("invalid Deltas metadata identity, production, or range");
        }
        Ok(Self {
            id,
            stream_ordinal,
            inflated_offset,
            byte_len,
            sha256,
            production,
        })
    }
}

impl Production {
    fn encoded_len(&self) -> Option<u64> {
        match self {
            Self::Map { entries, .. } => entries.iter().try_fold(6u64, |length, entry| {
                length.checked_add(entry.encoded_len())
            }),
            Self::Pmark { state } => state.encoded_len(),
            Self::Terminator { .. } | Self::FinalTerminator => Some(4),
        }
    }
}

impl ParasolidDeltasMetadata {
    pub(super) fn from_owned(
        source: &[u8],
        stream_ordinal: u32,
        range: Range<usize>,
        metadata: &Metadata,
    ) -> Self {
        let production = match metadata {
            Metadata::Map {
                entries,
                terminal_word,
            } => Production::Map {
                entries: entries.clone(),
                terminal_word: *terminal_word,
            },
            Metadata::Pmark(state) => Production::Pmark { state: *state },
            Metadata::Terminator {
                closes,
                section_start,
            } => Production::Terminator {
                closes: *closes,
                section_offset: u64_from_index(*section_start),
            },
            Metadata::FinalTerminator => Production::FinalTerminator,
        };
        let ordinal = cadmpeg_core::decode::index_from_u32(stream_ordinal);
        Self {
            id: deltas_event_id(ordinal, "deltas-metadata", range.start, None),
            stream_ordinal,
            inflated_offset: u64_from_index(range.start),
            byte_len: u64_from_index(range.len()),
            sha256: Sha256Digest::digest(&source[range]),
            production,
        }
    }
}

pub(in crate::native) fn admit(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
) -> Result<(), NativeConvertError> {
    let mut records = namespace
        .arena_as_for_decode::<ParasolidDeltasMetadata>(ctx, "parasolid_deltas_metadata")?;
    records.sort_unstable_by_key(|record| (record.stream_ordinal, record.inflated_offset));
    let mut previous: Option<(u32, u64)> = None;
    for record in records {
        if previous.is_some_and(|(stream, end)| {
            stream == record.stream_ordinal && end > record.inflated_offset
        }) {
            return Err(NativeConvertError::InvalidCollection(
                "duplicate or overlapping Deltas metadata".into(),
            ));
        }
        previous = Some((
            record.stream_ordinal,
            record.inflated_offset + record.byte_len,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_decode_context;
    use serde_json::json;

    fn map() -> serde_json::Value {
        json!({"id":"nx:s0:deltas-metadata#100", "stream_ordinal":0, "inflated_offset":100,
            "byte_len":10, "sha256":"0".repeat(64), "production":"map", "entries":[[7,19]], "terminal_word":21})
    }

    #[test]
    fn wire_checks_identity_extents_values_and_unknown_fields() {
        let original = map();
        let parsed: ParasolidDeltasMetadata = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), original);
        for (key, value) in [
            ("id", json!("nx:s0:deltas-metadata#101")),
            ("stream_ordinal", json!(1)),
            ("inflated_offset", json!(u64::MAX)),
            ("byte_len", json!(9)),
            ("entries", json!([[1, 19]])),
            ("entries", json!([[7, 1]])),
            ("entries", json!([[u32::MAX, 19]])),
            ("terminal_word", json!(0x8000)),
            ("extra", json!(0)),
        ] {
            let mut bad = original.clone();
            bad[key] = value;
            assert!(
                serde_json::from_value::<ParasolidDeltasMetadata>(bad).is_err(),
                "{key}"
            );
        }
        let mut overflow = original.clone();
        overflow["inflated_offset"] = json!(u64::MAX);
        overflow["id"] = json!(format!("nx:s0:deltas-metadata#{}", u64::MAX));
        assert!(serde_json::from_value::<ParasolidDeltasMetadata>(overflow).is_err());
        let mut terminator = original;
        terminator.as_object_mut().unwrap().remove("entries");
        terminator.as_object_mut().unwrap().remove("terminal_word");
        terminator["production"] = json!("terminator");
        terminator["byte_len"] = json!(4);
        terminator["closes"] = json!("leading");
        terminator["section_offset"] = json!(100);
        assert!(serde_json::from_value::<ParasolidDeltasMetadata>(terminator.clone()).is_ok());
        terminator["section_offset"] = json!(101);
        assert!(serde_json::from_value::<ParasolidDeltasMetadata>(terminator).is_err());
    }

    #[test]
    fn structural_admission_rejects_duplicates_and_overlap_in_physical_order() {
        for offset in [100, 109] {
            with_decode_context(|ctx| {
                let mut second = map();
                second["inflated_offset"] = json!(offset);
                second["id"] = json!(format!("nx:s0:deltas-metadata#{offset}"));
                let mut namespace = NativeNamespace::default();
                namespace
                    .set_arena(ctx, "parasolid_deltas_metadata", &[second, map()])
                    .unwrap();
                assert!(admit(ctx, &namespace).is_err());
            });
        }
        with_decode_context(|ctx| {
            let mut later = map();
            later["inflated_offset"] = json!(110);
            later["id"] = json!("nx:s0:deltas-metadata#110");
            let mut namespace = NativeNamespace::default();
            namespace
                .set_arena(ctx, "parasolid_deltas_metadata", &[later, map()])
                .unwrap();
            admit(ctx, &namespace).unwrap();
        });
    }
}
