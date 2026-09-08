// SPDX-License-Identifier: Apache-2.0
//! Fixed datum coordinate-system reference frame.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

/// Position in the eight-reference datum-CSYS construction lane.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub(crate) enum DatumCsysSlot {
    Zero = 0,
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
}

impl DatumCsysSlot {
    pub(crate) const ALL: [Self; 8] = [
        Self::Zero,
        Self::One,
        Self::Two,
        Self::Three,
        Self::Four,
        Self::Five,
        Self::Six,
        Self::Seven,
    ];
}

impl From<DatumCsysSlot> for u8 {
    fn from(value: DatumCsysSlot) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for DatumCsysSlot {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            4 => Ok(Self::Four),
            5 => Ok(Self::Five),
            6 => Ok(Self::Six),
            7 => Ok(Self::Seven),
            _ => Err("DatumCsysSlot: expected 0..=7"),
        }
    }
}

impl std::fmt::Display for DatumCsysSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&u8::from(*self), f)
    }
}

const HEADER_SUFFIX: [u8; 13] = [
    0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
];
const TRAILER: [u8; 8] = [0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatumCsysFrame<B> {
    control: u8,
    origin: u64,
    references: [(PayloadIndexToken, B); 8],
}

impl<B> DatumCsysFrame<B> {
    pub(crate) fn new(
        control: u8,
        origin: u64,
        references: [(PayloadIndexToken, B); 8],
    ) -> Result<Self, &'static str> {
        let width: u64 = references
            .iter()
            .map(|(token, _)| token.raw().len() as u64)
            .sum();
        origin
            .checked_add(14 + width + TRAILER.len() as u64)
            .ok_or("source_offsets: datum-CSYS frame end overflows")?;
        Ok(Self {
            control,
            origin,
            references,
        })
    }

    pub(crate) fn control(&self) -> u8 {
        self.control
    }
    pub(crate) fn members(&self) -> &[(PayloadIndexToken, B); 8] {
        &self.references
    }
    pub(crate) fn offsets(&self) -> [u64; 8] {
        let mut at = self.origin + 14;
        self.references.each_ref().map(|(token, _)| {
            let offset = at;
            at += token.raw().len() as u64;
            offset
        })
    }
    pub(crate) fn references(&self) -> impl Iterator<Item = (&PayloadIndexToken, &B, u64)> {
        self.references
            .iter()
            .zip(self.offsets())
            .map(|((token, binding), offset)| (token, binding, offset))
    }
    pub(crate) fn relocate(self, base: u64) -> Option<Self> {
        Self::new(
            self.control,
            base.checked_add(self.origin)?,
            self.references,
        )
        .ok()
    }
    pub(crate) fn resolve<C>(
        self,
        mut resolve: impl FnMut(u32) -> Option<C>,
    ) -> Option<DatumCsysFrame<C>> {
        let [slot0, slot1, slot2, slot3, slot4, slot5, slot6, slot7] = self
            .references
            .map(|(token, _)| Some((token, resolve(token.value())?)));
        Some(DatumCsysFrame {
            control: self.control,
            origin: self.origin,
            references: [
                slot0?, slot1?, slot2?, slot3?, slot4?, slot5?, slot6?, slot7?,
            ],
        })
    }
}

pub(crate) fn datum_csys_references(record: OperationPayload<'_>) -> Option<DatumCsysFrame<()>> {
    if record.name() != "DATUM_CSYS" || record.payload().get(1..14) != Some(&HEADER_SUFFIX) {
        return None;
    }
    let mut at = 14;
    let [slot0, slot1, slot2, slot3, slot4, slot5, slot6, slot7] =
        std::array::from_fn::<_, 8, _>(|_| {
            let token = PayloadIndexToken::read(record.payload().get(at..)?)?;
            at += token.raw().len();
            Some((token, ()))
        });
    let references = [
        slot0?, slot1?, slot2?, slot3?, slot4?, slot5?, slot6?, slot7?,
    ];
    if record.payload().get(at..at + TRAILER.len()) != Some(&TRAILER) {
        return None;
    }
    DatumCsysFrame::new(
        record.payload()[0],
        record.payload_offset() as u64,
        references,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn construction_slots_reject_out_of_lane_wire_indices() {
        for value in 0..=u8::MAX {
            let decoded = serde_json::from_value::<super::DatumCsysSlot>(value.into());
            assert_eq!(decoded.is_ok(), value < 8);
            if let Ok(slot) = decoded {
                assert_eq!(
                    serde_json::to_value(slot).unwrap(),
                    serde_json::json!(value)
                );
            }
        }
    }

    use super::*;

    #[test]
    fn frame_derives_mixed_width_positions_and_relocates_the_complete_span() {
        let raw = [
            vec![0xf0, 0],
            vec![0xf1, 1, 0],
            vec![0xf0, 1],
            vec![0xf1, 2, 0],
            vec![0xf0, 2],
            vec![0xf1, 3, 0],
            vec![0xf0, 3],
            vec![0xf1, 4, 0],
        ];
        let mut payload = vec![0xff];
        payload.extend_from_slice(&HEADER_SUFFIX);
        payload.extend(raw.iter().flatten());
        payload.extend_from_slice(&TRAILER);
        let frame =
            datum_csys_references(OperationPayload::new(&payload, 100, "DATUM_CSYS").unwrap())
                .unwrap();
        assert_eq!(frame.control(), 0xff);
        assert_eq!(frame.offsets(), [114, 116, 119, 121, 124, 126, 129, 131]);
        assert_eq!(
            frame.members().each_ref().map(|(token, ())| token.value()),
            [0, 256, 1, 512, 2, 768, 3, 1024]
        );
        let relocated = frame.clone().relocate(1000).unwrap();
        assert_eq!(
            relocated.offsets(),
            [1114, 1116, 1119, 1121, 1124, 1126, 1129, 1131]
        );
        assert!(frame.clone().relocate(u64::MAX - 142).is_some());
        assert!(frame.clone().relocate(u64::MAX - 141).is_none());
        assert!(frame
            .clone()
            .resolve(|value| (value != 768).then_some(value))
            .is_none());
        let resolved = frame.resolve(|value| Some(value.to_string())).unwrap();
        assert_eq!(resolved.members()[0].1, "0");
        assert_eq!(resolved.offsets(), [114, 116, 119, 121, 124, 126, 129, 131]);
        payload.pop();
        assert!(
            datum_csys_references(OperationPayload::new(&payload, 100, "DATUM_CSYS").unwrap())
                .is_none()
        );
    }
}
