// SPDX-License-Identifier: Apache-2.0
use crate::records::{DataType, SchemaPlan};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum RegionType {
    Solid,
    Void,
}

impl TryFrom<u8> for RegionType {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            b'S' => Ok(Self::Solid),
            b'V' => Ok(Self::Void),
            _ => Err("region_type: expected S or V"),
        }
    }
}

impl From<RegionType> for u8 {
    fn from(value: RegionType) -> Self {
        match value {
            RegionType::Solid => b'S',
            RegionType::Void => b'V',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionTail {
    Base,
    Owned {
        owner: NonZeroU32,
    },
    Framed {
        frame: NonZeroU32,
        owner: NonZeroU32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegionFields {
    pub(super) node_id: u32,
    attributes_features: NonZeroU32,
    body: NonZeroU32,
    next: NonZeroU32,
    previous: NonZeroU32,
    shell: NonZeroU32,
    pub(crate) region_type: RegionType,
    tail: RegionTail,
}

impl RegionFields {
    pub(crate) fn references(&self) -> impl Iterator<Item = u32> + Clone {
        let tail = match self.tail {
            RegionTail::Base => [None, None],
            RegionTail::Owned { owner } => [Some(owner), None],
            RegionTail::Framed { frame, owner } => [Some(frame), Some(owner)],
        };
        [
            self.attributes_features,
            self.body,
            self.next,
            self.previous,
            self.shell,
        ]
        .into_iter()
        .chain(tail.into_iter().flatten())
        .map(NonZeroU32::get)
    }

    pub(crate) fn from_wire(
        node_id: u32,
        references: Vec<u32>,
        region_type: RegionType,
    ) -> Option<Self> {
        let count = references.len();
        let mut references = references.into_iter();
        let mut target = || NonZeroU32::new(references.next()?);
        let attributes_features = target()?;
        let body = target()?;
        let next = target()?;
        let previous = target()?;
        let shell = target()?;
        let tail = match count {
            5 => RegionTail::Base,
            6 => RegionTail::Owned { owner: target()? },
            7 => RegionTail::Framed {
                frame: target()?,
                owner: target()?,
            },
            _ => return None,
        };
        Some(Self {
            node_id,
            attributes_features,
            body,
            next,
            previous,
            shell,
            region_type,
            tail,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionLayout {
    Base,
    Owned,
    Framed,
}

impl RegionLayout {
    pub(crate) fn from_schema(schema: &SchemaPlan) -> Option<Self> {
        [Self::Base, Self::Owned, Self::Framed]
            .into_iter()
            .find(|layout| {
                let leading = [
                    ("node_id", DataType::Integer, 0),
                    ("attributes_features", DataType::Pointer, 1019),
                    ("body", DataType::Pointer, 12),
                    ("next", DataType::Pointer, 19),
                    ("previous", DataType::Pointer, 19),
                    ("shell", DataType::Pointer, 13),
                ];
                schema.matches_fields(
                    19,
                    leading
                        .into_iter()
                        .chain(
                            [("frame", DataType::Pointer, 230)]
                                .into_iter()
                                .filter(|_| matches!(layout, Self::Framed)),
                        )
                        .chain([("type", DataType::Char, 0)])
                        .chain(
                            [("owner", DataType::Pointer, 12)]
                                .into_iter()
                                .filter(|_| !matches!(layout, Self::Base)),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests;
