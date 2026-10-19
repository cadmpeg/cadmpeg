// SPDX-License-Identifier: Apache-2.0
//! Primary PARTITION-mark fields, shared by current and historical nodes.

use crate::framing::read_and_advance;
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// One status-free type-4 instance after its schema information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmarkState {
    xmt: NonNullXmt,
    preceding: NonZeroU32,
    first_following: NonZeroU32,
    next_sibling: NonZeroU32,
    prev_sibling: NonZeroU32,
    n_new_nodes: u32,
    n_del_nodes: u32,
    n_copy_mod_nodes: u32,
    delta_key: u32,
    delta_is_forward: bool,
    identifier: u32,
}

impl PmarkState {
    /// Decode fields at an already established instance cursor.
    ///
    /// This reader assigns no ownership to a type-shaped payload occurrence.
    pub(super) fn read(stream: &[u8], mut at: usize) -> Option<(Self, usize)> {
        let xmt = NonNullXmt::try_from(read_and_advance(stream, &mut at)?).ok()?;
        let preceding = NonZeroU32::new(read_and_advance(stream, &mut at)?)?;
        let first_following = NonZeroU32::new(read_and_advance(stream, &mut at)?)?;
        let next_sibling = NonZeroU32::new(read_and_advance(stream, &mut at)?)?;
        let prev_sibling = NonZeroU32::new(read_and_advance(stream, &mut at)?)?;
        let mut view = View::over_retained(stream);
        view.seek(at)?;
        let n_new_nodes = view.u32_be()?;
        let n_del_nodes = view.u32_be()?;
        let n_copy_mod_nodes = view.u32_be()?;
        let delta_key = view.u32_be()?;
        let delta_is_forward = match view.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let identifier = view.u32_be()?;
        Some((
            Self {
                xmt,
                preceding,
                first_following,
                next_sibling,
                prev_sibling,
                n_new_nodes,
                n_del_nodes,
                n_copy_mod_nodes,
                delta_key,
                delta_is_forward,
                identifier,
            },
            view.position(),
        ))
    }

    pub(crate) fn xmt(self) -> u32 {
        self.xmt.into()
    }

    pub(super) fn preceding(self) -> u32 {
        self.preceding.get()
    }

    pub(super) fn first_following(self) -> u32 {
        self.first_following.get()
    }

    pub(crate) fn identifier(self) -> u32 {
        self.identifier
    }
}

#[cfg(test)]
mod tests {
    use super::PmarkState;

    #[test]
    fn primary_fields_keep_the_logical_byte_before_the_full_identifier() {
        let mut bytes = Vec::new();
        for value in [2_u16, 1, 3, 4, 1] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        for value in [0_u32, 1, 2, 0x0102_0304] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.push(1);
        bytes.extend_from_slice(&0x1234_5678_u32.to_be_bytes());
        let (state, end) = PmarkState::read(&bytes, 0).unwrap();
        assert_eq!(end, bytes.len());
        assert_eq!(state.xmt(), 2);
        assert_eq!(state.preceding(), 1);
        assert_eq!(state.first_following(), 3);
        assert_eq!(state.identifier(), 0x1234_5678);
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!({
                "xmt": 2, "preceding": 1, "first_following": 3,
                "next_sibling": 4, "prev_sibling": 1,
                "n_new_nodes": 0, "n_del_nodes": 1, "n_copy_mod_nodes": 2,
                "delta_key": 0x0102_0304, "delta_is_forward": true,
                "identifier": 0x1234_5678,
            })
        );
        for len in 0..bytes.len() {
            assert!(PmarkState::read(&bytes[..len], 0).is_none());
        }
        bytes[26] = 2;
        assert!(PmarkState::read(&bytes, 0).is_none());
    }

    #[test]
    fn wire_preserves_null_links_and_rejects_zero_or_null_identity() {
        let value = serde_json::json!({
            "xmt": 2, "preceding": 1, "first_following": 1,
            "next_sibling": 1, "prev_sibling": 1,
            "n_new_nodes": 0, "n_del_nodes": 0, "n_copy_mod_nodes": 0,
            "delta_key": 0, "delta_is_forward": false, "identifier": u32::MAX,
        });
        let state: PmarkState = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), value);
        for (field, invalid) in [
            ("xmt", 0),
            ("xmt", 1),
            ("preceding", 0),
            ("first_following", 0),
            ("next_sibling", 0),
            ("prev_sibling", 0),
        ] {
            let mut invalid_value = value.clone();
            invalid_value[field] = invalid.into();
            assert!(serde_json::from_value::<PmarkState>(invalid_value).is_err());
        }
    }
}
