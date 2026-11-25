//! The turn sense a revolution object stores (sldprt.md §2.3).

use super::{is_class_token, DeclaredEnd, CLASS_MARKER};
use crate::layout::revolution_turn_block as turn;
use cadmpeg_core::decode::View;

/// Which way a revolution turns about the axis direction its record stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TurnSense {
    /// The reverse byte is `0`.
    Stored,
    /// The reverse byte is `1`.
    Reversed,
}

impl TurnSense {
    /// The value of [`crate::records::TURN_SENSE_PROPERTY`].
    pub(crate) const fn property(self) -> &'static str {
        match self {
            Self::Stored => "Stored",
            Self::Reversed => "Reversed",
        }
    }
}

/// The turn sense of the revolution object in `payload[start..end]`: the
/// reverse byte of its one turn block, which an empty `suObList`, a u32 `3`
/// or `5`, a u32 and the axis reference follow. `None` where the interval
/// holds no such block or more than one.
pub(crate) fn turn_sense(payload: &[u8], start: usize, end: usize) -> Option<TurnSense> {
    const SUOBLIST: &[u8] = b"suObList";
    let end = DeclaredEnd::of(end, payload.len())?.get();
    let payload = &payload[..end];
    let list_end = |list: usize| {
        let declared = payload.get(list..list + CLASS_MARKER.len()) == Some(CLASS_MARKER)
            && View::u16_le_at(payload, list + CLASS_MARKER.len())
                == u16::try_from(SUOBLIST.len()).ok()
            && payload
                .get(list + CLASS_MARKER.len() + 2..list + CLASS_MARKER.len() + 2 + SUOBLIST.len())
                == Some(SUOBLIST);
        if declared {
            Some(list + CLASS_MARKER.len() + 2 + SUOBLIST.len())
        } else {
            View::u16_le_at(payload, list)
                .is_some_and(is_class_token)
                .then_some(list + 2)
        }
    };
    let sense_at = |block: usize| {
        let field = |offset: usize, len: usize| payload.get(block + offset..block + offset + len);
        if field(turn::LEAD, turn::LEAD_VALUE.len()) != Some(turn::LEAD_VALUE.as_slice())
            || field(turn::PAD, turn::PAD_VALUE.len()) != Some(turn::PAD_VALUE.as_slice())
            || !matches!(View::u32_le_at(payload, block + turn::WORD), Some(0 | 1))
        {
            return None;
        }
        let sense = match payload.get(block + turn::REVERSE)? {
            0 => TurnSense::Stored,
            1 => TurnSense::Reversed,
            _ => return None,
        };
        let list = list_end(block + turn::LEN)?;
        let axis = list + 10;
        (View::u16_le_at(payload, list) == Some(0)
            && matches!(View::u32_le_at(payload, list + 2), Some(3 | 5))
            && (payload.get(axis..axis + CLASS_MARKER.len()) == Some(CLASS_MARKER)
                || View::u16_le_at(payload, axis).is_some_and(is_class_token)))
        .then_some(sense)
    };
    let mut senses = (start..end).filter_map(sense_at);
    let sense = senses.next()?;
    senses.next().is_none().then_some(sense)
}

#[cfg(test)]
mod tests;
