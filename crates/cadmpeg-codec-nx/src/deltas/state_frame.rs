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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<ReferenceStateFrame>")]
pub(crate) struct StateFrames(Vec<ReferenceStateFrame>);

impl Serialize for StateFrames {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl StateFrames {
    pub(super) fn new(first: ReferenceStateFrame) -> Self {
        Self(vec![first])
    }
    pub(super) fn push(&mut self, frame: ReferenceStateFrame) {
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

#[cfg(test)]
std::thread_local! {
    static STATE_FRAMES_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<StateFrames> for Vec<ReferenceStateFrame> {
    fn from(frames: StateFrames) -> Self {
        STATE_FRAMES_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        frames.0
    }
}

#[cfg(test)]
mod tests {
    use super::{StateFrames, STATE_FRAMES_INTO_WIRE_COUNT};

    #[test]
    fn state_frames_preserve_array_wire_and_reject_empty_packets() {
        let json = r#"[{"references":[2,3,4,1],"state_words":[34,6,11,22362,1],"state_byte":65}]"#;
        let frames: StateFrames = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&frames).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&frames).unwrap(),
            serde_json::to_vec(&Vec::<super::ReferenceStateFrame>::from(frames.clone())).unwrap()
        );
        assert!(serde_json::from_str::<StateFrames>("[]")
            .unwrap_err()
            .to_string()
            .contains("frames"));
    }

    #[test]
    fn state_frames_native_limit_refuses_before_owned_wire_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            frames: &'a StateFrames,
        }
        let frames: StateFrames = serde_json::from_str(
            r#"[{"references":[2,3,4,1],"state_words":[34,6,11,22362,1],"state_byte":65}]"#,
        )
        .unwrap();
        let record = Record {
            id: "nx:deltas:state-frames#0",
            frames: &frames,
        };
        STATE_FRAMES_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": "nx:deltas:state-frames#0", "frames": frames}),
        );
        STATE_FRAMES_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
