// SPDX-License-Identifier: Apache-2.0
//! Ordered EXTREFSTREAM handle tokens and their derived prefix fields.

use serde::{Deserialize, Serialize};

use crate::layout::extrefstream_handle_set_record;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "HandlesWire", into = "HandlesWire")]
pub(crate) struct ExtrefHandles(Vec<u32>);

impl ExtrefHandles {
    pub(crate) fn new(tokens: Vec<u32>) -> Result<Self, &'static str> {
        if !(1..=254).contains(&tokens.len()) {
            return Err("handles: encoded token count must be in 1..=254");
        }
        if !tokens.windows(2).all(|pair| pair[0] <= pair[1]) {
            return Err("handles: must be non-decreasing");
        }
        Ok(Self(tokens))
    }

    pub(crate) fn serialized(&self) -> &[u32] {
        &self.0
    }

    pub(crate) fn closing_duplicate(&self) -> bool {
        self.0.len() >= 2 && self.0[self.0.len() - 1] == self.0[self.0.len() - 2]
    }

    pub(crate) fn values(&self) -> &[u32] {
        &self.0[..self.0.len() - usize::from(self.closing_duplicate())]
    }

    pub(crate) fn prefix_byte_len(&self) -> usize {
        extrefstream_handle_set_record::LEN + self.0.len() * 5 + 1
    }
}

#[derive(Serialize, Deserialize)]
struct HandlesWire {
    handles: Vec<u32>,
    closing_duplicate: bool,
    prefix_byte_len: u64,
}

impl From<ExtrefHandles> for HandlesWire {
    fn from(value: ExtrefHandles) -> Self {
        Self {
            handles: value.values().to_vec(),
            closing_duplicate: value.closing_duplicate(),
            prefix_byte_len: value.prefix_byte_len() as u64,
        }
    }
}

impl TryFrom<HandlesWire> for ExtrefHandles {
    type Error = &'static str;

    fn try_from(mut wire: HandlesWire) -> Result<Self, Self::Error> {
        if wire.closing_duplicate {
            let last = *wire
                .handles
                .last()
                .ok_or("closing_duplicate: requires a handle")?;
            wire.handles.push(last);
        }
        let value = Self::new(wire.handles)?;
        if value.closing_duplicate() != wire.closing_duplicate {
            return Err("closing_duplicate: must match the final encoded handle pair");
        }
        if value.prefix_byte_len() as u64 != wire.prefix_byte_len {
            return Err("prefix_byte_len: must match the encoded handle count");
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::ExtrefHandles;

    #[test]
    fn wire_preserves_handle_occurrences_and_derived_fields() {
        for (tokens, handles, closing, length) in [
            (vec![7], vec![7], false, 31),
            (vec![7, 7], vec![7], true, 36),
            (vec![7, 7, 7], vec![7, 7], true, 41),
            (vec![7, 7, 9], vec![7, 7, 9], false, 41),
            (vec![7; 254], vec![7; 253], true, 1296),
        ] {
            let value = ExtrefHandles::new(tokens.clone()).unwrap();
            assert_eq!(value.serialized(), tokens);
            let wire = serde_json::json!({
                "handles": handles, "closing_duplicate": closing, "prefix_byte_len": length,
            });
            assert_eq!(serde_json::to_value(&value).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<ExtrefHandles>(wire).unwrap(),
                value
            );
        }
    }

    #[test]
    fn construction_rejects_inconsistent_prefixes() {
        for tokens in [vec![], vec![7; 255], vec![9, 7]] {
            assert!(ExtrefHandles::new(tokens).is_err());
        }
        for (handles, closing, length, field) in [
            (vec![], true, 26, "closing_duplicate"),
            (vec![7, 7], false, 36, "closing_duplicate"),
            (vec![7], false, 36, "prefix_byte_len"),
            (vec![7; 254], true, 1301, "handles"),
        ] {
            let error = serde_json::from_value::<ExtrefHandles>(serde_json::json!({
                "handles": handles, "closing_duplicate": closing, "prefix_byte_len": length,
            }))
            .unwrap_err();
            assert!(error.to_string().contains(field));
        }
    }
}
