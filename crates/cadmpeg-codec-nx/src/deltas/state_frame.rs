// SPDX-License-Identifier: Apache-2.0
//! Reference-state frames and nonempty packet contents.

use super::state_references::StateReferences;
use serde::{Deserialize, Serialize};

/// One four-reference frame in a deltas state packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReferenceStateFrame {
    pub(crate) references: StateReferences,
    pub(crate) state_words: [u32; 5],
    pub(crate) state_byte: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<ReferenceStateFrame>",
    into = "Vec<ReferenceStateFrame>"
)]
pub(crate) struct StateFrames(Vec<ReferenceStateFrame>);

impl StateFrames {
    pub(crate) fn new(first: ReferenceStateFrame) -> Self {
        Self(vec![first])
    }
    pub(crate) fn push(&mut self, frame: ReferenceStateFrame) {
        self.0.push(frame);
    }
    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[ReferenceStateFrame] {
        &self.0
    }
}

impl TryFrom<Vec<ReferenceStateFrame>> for StateFrames {
    type Error = &'static str;
    fn try_from(frames: Vec<ReferenceStateFrame>) -> Result<Self, Self::Error> {
        if frames.is_empty() {
            return Err("frames: require at least one frame");
        }
        Ok(Self(frames))
    }
}

impl From<StateFrames> for Vec<ReferenceStateFrame> {
    fn from(frames: StateFrames) -> Self {
        frames.0
    }
}

#[cfg(test)]
mod tests {
    use super::StateFrames;

    #[test]
    fn state_frames_preserve_array_wire_and_reject_empty_packets() {
        let json = r#"[{"references":[2,3,4,1],"state_words":[34,6,11,22362,1],"state_byte":65}]"#;
        let frames: StateFrames = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&frames).unwrap(), json);
        assert!(serde_json::from_str::<StateFrames>("[]")
            .unwrap_err()
            .to_string()
            .contains("frames"));
    }
}
