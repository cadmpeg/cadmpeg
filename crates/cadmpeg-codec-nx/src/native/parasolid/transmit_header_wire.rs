// SPDX-License-Identifier: Apache-2.0
//! Transmit headers always start at inflated offset zero.

use super::ParasolidDeltasTransmitHeader;
use crate::deltas::transmit_state::TransmitState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct TransmitHeaderWire {
    id: String,
    stream_ordinal: u32,
    #[serde(flatten)]
    state: TransmitState,
    byte_len: u64,
    sha256: crate::native::hex::Sha256Hex,
    inflated_offset: u64,
}

impl From<ParasolidDeltasTransmitHeader> for TransmitHeaderWire {
    fn from(header: ParasolidDeltasTransmitHeader) -> Self {
        Self {
            id: header.id,
            stream_ordinal: header.stream_ordinal,
            state: header.state,
            byte_len: header.byte_len,
            sha256: header.sha256,
            inflated_offset: 0,
        }
    }
}

impl TryFrom<TransmitHeaderWire> for ParasolidDeltasTransmitHeader {
    type Error = &'static str;
    fn try_from(wire: TransmitHeaderWire) -> Result<Self, Self::Error> {
        if wire.inflated_offset != 0 {
            return Err("inflated_offset: transmit header must start at zero");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            state: wire.state,
            byte_len: wire.byte_len,
            sha256: wire.sha256,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ParasolidDeltasTransmitHeader;

    #[test]
    fn header_wire_emits_zero_offset_and_rejects_displaced_header() {
        let json = r#"{"id":"header","stream_ordinal":0,"description":"Transmit (deltas)","schema":"SCH_1","references":[2,3],"byte_len":42,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","inflated_offset":0}"#;
        let header: ParasolidDeltasTransmitHeader = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&header).unwrap(), json);
        let displaced = json.replace("\"inflated_offset\":0", "\"inflated_offset\":1");
        assert!(
            serde_json::from_str::<ParasolidDeltasTransmitHeader>(&displaced)
                .unwrap_err()
                .to_string()
                .contains("inflated_offset")
        );
    }
}
