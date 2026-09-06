// SPDX-License-Identifier: Apache-2.0
//! Optional counted state bytes of a leading SWP104 branch.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Swp104StateLane {
    witnessed_bytes: Option<Vec<u8>>,
}

impl Swp104StateLane {
    pub(crate) fn from_parts(count: Option<u8>, bytes: Vec<u8>) -> Result<Self, &'static str> {
        match count {
            None if bytes == [0; 5] => Ok(Self { witnessed_bytes: None }),
            Some(count) if count >= 2 && bytes.len() == usize::from(count) + 3 => {
                Ok(Self { witnessed_bytes: Some(bytes) })
            }
            _ => Err("state_lane must contain five zero bytes without witnessed_count, or witnessed_count plus three bytes with a count of at least two"),
        }
    }

    pub(crate) fn witnessed_count(&self) -> Option<u8> {
        self.witnessed_bytes
            .as_ref()
            .map(|bytes| (bytes.len() - 3) as u8)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self.witnessed_bytes.as_deref().unwrap_or(&[0; 5])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_lane_preserves_witness_presence_for_zero_bytes() {
        let absent = Swp104StateLane::from_parts(None, vec![0; 5]).unwrap();
        let present = Swp104StateLane::from_parts(Some(2), vec![0; 5]).unwrap();
        assert_eq!(absent.bytes(), present.bytes());
        assert_eq!(absent.witnessed_count(), None);
        assert_eq!(present.witnessed_count(), Some(2));
    }

    #[test]
    fn state_lane_rejects_inconsistent_witnesses() {
        for (count, bytes) in [
            (None, vec![0; 4]),
            (None, vec![1; 5]),
            (Some(0), vec![0; 3]),
            (Some(1), vec![0; 4]),
            (Some(2), vec![0; 6]),
            (Some(255), vec![0; 257]),
        ] {
            assert!(Swp104StateLane::from_parts(count, bytes).is_err());
        }
        let lane = Swp104StateLane::from_parts(Some(255), vec![1; 258]).unwrap();
        assert_eq!(lane.witnessed_count(), Some(255));
    }
}
