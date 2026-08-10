// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` feature records and feature-list terminators.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    content_header, reference_list, type_id_string, Cursor, PmDcContentHeader, PmDcReferenceList,
};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const FEATURE_TYPE: [u8; 16] = [
    0x91, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const END_OF_FEATURES_TYPE: [u8; 16] = [
    0x24, 0xfd, 0x41, 0x8f, 0xd2, 0x11, 0xac, 0x6e, 0x00, 0x08, 0x2a, 0xab, 0x32, 0xa3, 0xdc, 0x09,
];

#[derive(Debug)]
pub(crate) struct FeatureInventory {
    pub(crate) features: Vec<PmDcFeature>,
    pub(crate) terminators: Vec<PmDcFeatureTerminator>,
    pub(crate) issues: Vec<FeatureRecordIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeature {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) state: i32,
    pub(crate) outline_value: u32,
    pub(crate) properties: PmDcReferenceList,
    pub(crate) value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeatureTerminator {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) state: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureRecordIssue {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<FeatureInventory, CodecError> {
    let mut inventory = FeatureInventory {
        features: Vec::new(),
        terminators: Vec::new(),
        issues: Vec::new(),
    };
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry_version_major else {
            continue;
        };
        if !(15..=22).contains(&version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let parsed = match record.type_id {
                FEATURE_TYPE => parse_feature(ctx, record.payload, version).map(|mut feature| {
                    feature.id = format!(
                        "inventor:pmdc:feature#{}-{}",
                        segment.pair.token.as_str(),
                        record.ordinal
                    );
                    feature.type_id = type_id_string(record.type_id);
                    feature.segment_token = segment.pair.token.as_str().into();
                    feature.record_ordinal = record.ordinal;
                    inventory.features.push(feature);
                }),
                END_OF_FEATURES_TYPE => {
                    parse_terminator(record.payload, version).map(|mut terminator| {
                        terminator.id = format!(
                            "inventor:pmdc:feature-terminator#{}-{}",
                            segment.pair.token.as_str(),
                            record.ordinal
                        );
                        terminator.type_id = type_id_string(record.type_id);
                        terminator.segment_token = segment.pair.token.as_str().into();
                        terminator.record_ordinal = record.ordinal;
                        inventory.terminators.push(terminator);
                    })
                }
                _ => continue,
            };
            if let Err(error) = parsed {
                inventory.issues.push(FeatureRecordIssue {
                    id: format!(
                        "inventor:pmdc:feature-record-issue#{}-{}",
                        segment.pair.token.as_str(),
                        record.ordinal
                    ),
                    type_id: type_id_string(record.type_id),
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: error.to_string(),
                });
            }
        }
    }
    ctx.charge_collection_items(
        inventory
            .features
            .len()
            .saturating_add(inventory.terminators.len())
            .saturating_add(inventory.issues.len()) as u64,
        "admit Inventor feature records",
    )?;
    Ok(inventory)
}

fn parse_feature(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeature, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature state")? as i32;
    let outline_value = cursor.u32("feature outline value")?;
    let properties = reference_list(ctx, &mut cursor, 2, "feature property list")?;
    let value = cursor.u32("feature value")?;
    cursor.finish("feature")?;
    Ok(PmDcFeature {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header,
        state,
        outline_value,
        properties,
        value,
    })
}

fn parse_terminator(source: View<'_>, version: u8) -> Result<PmDcFeatureTerminator, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature-terminator state")? as i32;
    cursor.finish("feature terminator")?;
    Ok(PmDcFeatureTerminator {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    fn content(index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes()[..2]);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0x0002_0200u32.to_le_bytes());
        bytes.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes
    }

    fn parse<T>(bytes: &[u8], parser: impl FnOnce(&DecodeContext<'_>, View<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("view");
        parser(&ctx, source)
    }

    #[test]
    fn parses_generated_feature_and_terminator() {
        let mut feature = content(7);
        feature.extend_from_slice(&(-1i32).to_le_bytes());
        feature.extend_from_slice(&42u32.to_le_bytes());
        feature.extend_from_slice(&2u16.to_le_bytes());
        feature.extend_from_slice(&0x3000u16.to_le_bytes());
        feature.extend_from_slice(&2u32.to_le_bytes());
        feature.extend_from_slice(&[0; 8]);
        feature.extend_from_slice(&0x8000_0004u32.to_le_bytes());
        feature.extend_from_slice(&5u32.to_le_bytes());
        feature.extend_from_slice(&9u32.to_le_bytes());
        let parsed = parse(&feature, |ctx, source| {
            parse_feature(ctx, source, 16).expect("feature")
        });
        assert_eq!(parsed.state, -1);
        assert_eq!(parsed.outline_value, 42);
        assert_eq!(parsed.properties.references.len(), 2);
        assert!(parsed.properties.references[0].qualified);
        assert_eq!(parsed.value, 9);

        let mut terminator = content(8);
        terminator.extend_from_slice(&(-1i32).to_le_bytes());
        let parsed = parse(&terminator, |_, source| {
            parse_terminator(source, 16).expect("terminator")
        });
        assert_eq!(parsed.state, -1);
    }
}
