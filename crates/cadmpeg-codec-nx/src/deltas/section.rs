// SPDX-License-Identifier: Apache-2.0

use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SectionKind {
    Leading,
    Map,
    Pmark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub(crate) struct MapKind(u16);

impl TryFrom<u16> for MapKind {
    type Error = &'static str;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value < 2 {
            return Err("map kind must be at least two");
        }
        Ok(Self(value))
    }
}

impl From<MapKind> for u16 {
    fn from(value: MapKind) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "[u32; 2]", into = "[u32; 2]")]
pub(crate) struct MapEntry(NonNullXmt, MapKind);

pub(crate) enum MapRead {
    End,
    Entry(MapEntry),
}

const MAX_MAP_XMT: u32 = 65_535 * 32_767 + 32_767;

impl TryFrom<[u32; 2]> for MapEntry {
    type Error = &'static str;
    fn try_from([xmt, kind]: [u32; 2]) -> Result<Self, Self::Error> {
        if xmt > MAX_MAP_XMT {
            return Err("map XMT exceeds the canonical encoding range");
        }
        Ok(Self(
            NonNullXmt::try_from(xmt)?,
            MapKind::try_from(u16::try_from(kind).map_err(|_| "map kind exceeds u16")?)?,
        ))
    }
}

impl From<MapEntry> for [u32; 2] {
    fn from(value: MapEntry) -> Self {
        [u32::from(value.0), u32::from(u16::from(value.1))]
    }
}

impl MapEntry {
    pub(crate) fn read(view: &mut View<'_>, maximum: u16) -> Option<MapRead> {
        let mut probe = *view;
        if probe.i16_be()? == i16::MIN {
            return None;
        }
        let value = crate::records::positive(view)?.checked_add(1)?;
        if value == 1 {
            return Some(MapRead::End);
        }
        let xmt = NonNullXmt::try_from(value).ok()?;
        let kind = view.u16_be().filter(|kind| *kind <= maximum)?;
        Some(MapRead::Entry(Self(xmt, MapKind::try_from(kind).ok()?)))
    }

    pub(crate) fn encoded_len(self) -> u64 {
        if u32::from(self.0) <= 32_767 {
            4
        } else {
            6
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub(crate) struct TerminalWord(u16);

impl TryFrom<u16> for TerminalWord {
    type Error = &'static str;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value >= 0x8000 {
            return Err("map terminal word width is unresolved");
        }
        Ok(Self(value))
    }
}

impl From<TerminalWord> for u16 {
    fn from(value: TerminalWord) -> Self {
        value.0
    }
}
