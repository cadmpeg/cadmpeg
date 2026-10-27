// SPDX-License-Identifier: Apache-2.0
//! Primary PARTITION-mark fields, shared by current and historical nodes.

use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// One status-free type-4 instance after its schema information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

const MAX_XMT: u32 = 65_535 * 32_767 + 32_768;

impl PmarkState {
    /// Decode fields at an already established instance cursor.
    ///
    /// This reader assigns no ownership to a type-shaped payload occurrence.
    pub(crate) fn read(view: &mut View<'_>) -> Option<Self> {
        let xmt = NonNullXmt::try_from(crate::records::positive(view)?.checked_add(1)?).ok()?;
        let preceding = NonZeroU32::new(crate::records::positive(view)?.checked_add(1)?)?;
        let first_following = NonZeroU32::new(crate::records::positive(view)?.checked_add(1)?)?;
        let next_sibling = NonZeroU32::new(crate::records::positive(view)?.checked_add(1)?)?;
        let prev_sibling = NonZeroU32::new(crate::records::positive(view)?.checked_add(1)?)?;
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
        Some(Self {
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
        })
    }

    pub(crate) fn encoded_len(self) -> Option<u64> {
        [
            u32::from(self.xmt),
            self.preceding.get(),
            self.first_following.get(),
            self.next_sibling.get(),
            self.prev_sibling.get(),
        ]
        .into_iter()
        .try_fold(23u64, |length, value| {
            (value <= MAX_XMT).then(|| length + if value <= 32_767 { 2 } else { 4 })
        })
    }

    pub(crate) fn xmt(self) -> u32 {
        self.xmt.into()
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
        let mut view = cadmpeg_core::decode::View::over_retained(&bytes);
        let state = PmarkState::read(&mut view).unwrap();
        assert_eq!(view.position(), bytes.len());
        assert_eq!(state.xmt(), 2);
        assert_eq!(state.preceding.get(), 1);
        assert_eq!(state.first_following.get(), 3);
        assert_eq!(state.identifier, 0x1234_5678);
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
            assert!(
                PmarkState::read(&mut cadmpeg_core::decode::View::over_retained(
                    &bytes[..len]
                ))
                .is_none()
            );
        }
        bytes[26] = 2;
        assert!(PmarkState::read(&mut cadmpeg_core::decode::View::over_retained(&bytes)).is_none());
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

    #[test]
    fn accepted_xmt_spellings_have_value_determined_widths() {
        for spelling in [
            vec![0x7f, 0xff],
            vec![0xff, 0xff, 0, 1],
            vec![0xff, 0xff, 0, 2],
            vec![0x80, 0, 0, 1],
            vec![0x80, 0, 0xff, 0xff],
        ] {
            let mut bytes = spelling;
            for _ in 0..4 {
                bytes.extend([0, 1]);
            }
            bytes.extend([0; 21]);
            let mut view = cadmpeg_core::decode::View::over_retained(&bytes);
            let state = PmarkState::read(&mut view).unwrap();
            assert_eq!(view.position(), bytes.len());
            assert_eq!(
                state.encoded_len(),
                Some(u64::try_from(bytes.len() + 2).unwrap())
            );
        }
        for spelling in [[0xff, 0xff, 0, 0], [0x80, 0, 0, 0]] {
            let mut bytes = spelling.to_vec();
            bytes.extend([0; 29]);
            assert!(
                PmarkState::read(&mut cadmpeg_core::decode::View::over_retained(&bytes)).is_none()
            );
        }
    }
}
