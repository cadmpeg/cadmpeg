// SPDX-License-Identifier: Apache-2.0
//! Complete A-family class-25 parameter maps.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::families::consolidated::records::class25::{self, Class25ParameterMap};
use crate::wire::records::{
    ConsolidatedFamily, ConsolidatedFrameFlag, ConsolidatedFrameWidth, ConsolidatedRawFrame,
    ConsolidatedRecord,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "AClass25MapWire")]
pub(crate) struct CatiaConsolidatedAClass25Map {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    frame: ConsolidatedRawFrame<u64>,
    data: Class25ParameterMap,
}

#[derive(Deserialize)]
struct AClass25MapWire {
    id: String,
    source_index: u64,
    source_offset: u64,
    frame: ConsolidatedRawFrame<u64>,
    data: Class25ParameterMap,
}

impl TryFrom<AClass25MapWire> for CatiaConsolidatedAClass25Map {
    type Error = &'static str;

    fn try_from(wire: AClass25MapWire) -> Result<Self, Self::Error> {
        if wire.frame.width() != ConsolidatedFrameWidth::One
            || wire.frame.flag != ConsolidatedFrameFlag::Flag03
            || class25::parse_a(&wire.frame.payload).as_ref() != Some(&wire.data)
        {
            return Err("A-family class-25 map differs from its frame");
        }
        Ok(Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            frame: wire.frame,
            data: wire.data,
        })
    }
}

pub(super) fn derive(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedAClass25Map>, CodecError> {
    let mut maps = Vec::new();
    for record in records {
        if record.family() != ConsolidatedFamily::A
            || record.width() != ConsolidatedFrameWidth::One
            || record.class() != 0x25
            || record.flag() != ConsolidatedFrameFlag::Flag03
        {
            continue;
        }
        let Some(payload) = record.payload() else {
            continue;
        };
        let Some(data) = class25::parse_a(&bytes[payload.clone()]) else {
            continue;
        };
        maps.push(CatiaConsolidatedAClass25Map {
            id: format!("catia:consolidated:a-class25-map#{:00}", maps.len()),
            source_index: u64_from_index(record.source_index()),
            source_offset: u64_from_index(record.source_range().start),
            frame: ConsolidatedRawFrame::from_record(record, bytes[payload].to_vec())?.into(),
            data,
        });
    }
    Ok(maps)
}

#[cfg(test)]
mod tests {
    use super::CatiaConsolidatedAClass25Map;
    use crate::native::CatiaNative;

    #[test]
    fn a5_class25_map_round_trips_with_its_payload() {
        let b_frame = crate::test_support::test_class25::sampled_stream();
        let payload = &b_frame[5..];
        let mut a5 = vec![0xa5, 0x03, 0x25];
        a5.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("small payload")
                .to_le_bytes(),
        );
        a5.push(0x05);
        a5.extend_from_slice(payload);
        let native = CatiaNative::decode(&a5);
        let [map] = native.consolidated_a_class25_maps.as_slice() else {
            panic!("one A-family map");
        };
        let wire = serde_json::to_value(map).expect("serialize map");
        assert_eq!(wire["data"]["selector"], 0x1587);
        let mut contradictory = wire.clone();
        contradictory["data"]["selector"] = serde_json::json!(0x1586);
        assert!(serde_json::from_value::<CatiaConsolidatedAClass25Map>(contradictory).is_err());
        assert_eq!(
            serde_json::from_value::<CatiaConsolidatedAClass25Map>(wire).expect("load map"),
            *map
        );
        assert!(native.consolidated_domain_relations.is_empty());
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        native.store(&mut namespace).expect("store A-family map");
        assert_eq!(
            CatiaNative::load(&namespace).expect("load A-family map"),
            native
        );
    }
}
