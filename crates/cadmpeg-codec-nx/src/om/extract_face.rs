// SPDX-License-Identifier: Apache-2.0
//! Ordered `EXTRACT_FACE` construction relations without authored topology roles.

use super::operation_record::OperationPayload;
use super::reference_index::FeatureReferenceToken;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

pub(crate) const PARAMETER_CLASS: &str = "UGS::ModlFeature::ExtractFaceParms";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct ExtractFaceRelation {
    pub(crate) key: u8,
    pub(crate) left: FeatureReferenceToken,
    pub(crate) right: FeatureReferenceToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<ExtractFaceRelation>")]
pub(crate) struct ExtractFaceLane(Vec<ExtractFaceRelation>);

impl Serialize for ExtractFaceLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl TryFrom<Vec<ExtractFaceRelation>> for ExtractFaceLane {
    type Error = &'static str;

    fn try_from(members: Vec<ExtractFaceRelation>) -> Result<Self, Self::Error> {
        if members.len() > 254 {
            return Err("EXTRACT_FACE lane must fit its nonzero count byte");
        }
        Ok(Self(members))
    }
}

impl ExtractFaceLane {
    fn byte_len(&self) -> usize {
        2 + self
            .0
            .iter()
            .map(|r| 1 + r.left.raw().len() + r.right.raw().len())
            .sum::<usize>()
    }

    fn read(bytes: &[u8], at: &mut usize) -> Option<Self> {
        let Some(&[1, count @ 1..=255]) = bytes.get(*at..*at + 2) else {
            return None;
        };
        *at += 2;
        let mut members = Vec::new();
        for _ in 1..count {
            let &key = bytes.get(*at)?;
            *at += 1;
            let left = bytes.get(*at..).and_then(FeatureReferenceToken::read)?;
            *at += left.raw().len();
            let right = bytes.get(*at..).and_then(FeatureReferenceToken::read)?;
            *at += right.raw().len();
            members.push(ExtractFaceRelation { key, left, right });
        }
        Some(Self(members))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractFaceEnvelope {
    pub(crate) parameter_class_index: NonZeroU8,
    pub(crate) control_words: [u32; 4],
    pub(crate) lanes: [ExtractFaceLane; 2],
    pub(crate) terminal: FeatureReferenceToken,
    navigator_text: String,
}

impl ExtractFaceEnvelope {
    pub(crate) fn new(
        parameter_class_index: NonZeroU8,
        control_words: [u32; 4],
        lanes: [ExtractFaceLane; 2],
        terminal: FeatureReferenceToken,
        navigator_text: String,
    ) -> Result<Self, &'static str> {
        if navigator_text.len() > 253 {
            return Err("EXTRACT_FACE navigator text must fit its length byte");
        }
        Ok(Self {
            parameter_class_index,
            control_words,
            lanes,
            terminal,
            navigator_text,
        })
    }

    pub(crate) fn navigator_text(&self) -> &str {
        &self.navigator_text
    }

    pub(crate) fn byte_len(&self) -> usize {
        5 + 20
            + 6
            + self
                .lanes
                .iter()
                .map(ExtractFaceLane::byte_len)
                .sum::<usize>()
            + 1
            + self.terminal.raw().len()
            + 5
            + 3
            + self.navigator_text.len()
    }

    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        if record.name() != "EXTRACT_FACE" {
            return None;
        }
        let bytes = record.payload();
        // Maximum: two 254-member lanes of seven-byte triples and 253 UTF-8 bytes.
        let parameter_class_index = bytes.first().copied().and_then(NonZeroU8::new)?;
        if bytes.get(1..5) != Some(&[0, 0, 1, 0]) {
            return None;
        }
        let mut at = 5;
        let mut control_words = [0; 4];
        for word in &mut control_words {
            if bytes.get(at) != Some(&1) {
                return None;
            }
            let value = View::u32_be_at(bytes, at + 1)?;
            *word = value;
            at += 5;
        }
        if bytes.get(at..at + 6) != Some(&[0; 6]) {
            return None;
        }
        at += 6;
        let first = ExtractFaceLane::read(bytes, &mut at)?;
        let second = ExtractFaceLane::read(bytes, &mut at)?;
        if bytes.get(at) != Some(&255) {
            return None;
        }
        at += 1;
        let terminal = bytes.get(at..).and_then(FeatureReferenceToken::read)?;
        at += terminal.raw().len();
        if bytes.get(at..at + 5) != Some(&[0; 5]) {
            return None;
        }
        at += 5;
        let Some(&[3, len @ 2..=255]) = bytes.get(at..at + 2) else {
            return None;
        };
        let end = at + usize::from(len);
        if bytes.get(end) != Some(&0) {
            return None;
        }
        let text = bytes
            .get(at + 2..end)
            .and_then(|b| std::str::from_utf8(b).ok())?;
        let mut navigator_text = String::new();
        navigator_text.push_str(text);
        Some(Self {
            parameter_class_index,
            control_words,
            lanes: [first, second],
            terminal,
            navigator_text,
        })
    }
}

#[cfg(test)]
mod tests;
