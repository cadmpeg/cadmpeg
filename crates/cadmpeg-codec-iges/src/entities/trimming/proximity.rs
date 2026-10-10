// SPDX-License-Identifier: Apache-2.0
//! Endpoint cells with fixed storage for a small boundary.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

const INLINE_ENDPOINTS: usize = 32;

struct InlineGrid {
    cells: [Option<([i64; 3], usize)>; INLINE_ENDPOINTS],
    links: [Option<usize>; INLINE_ENDPOINTS],
}

pub(super) struct ProximityGrid {
    inline: Option<InlineGrid>,
    cells: HashMap<[i64; 3], usize>,
    links: Vec<Option<usize>>,
}

impl ProximityGrid {
    pub(super) fn new(count: usize, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let mut cells = HashMap::new();
        let (inline, links) = if count <= INLINE_ENDPOINTS {
            (
                Some(InlineGrid {
                    cells: [None; INLINE_ENDPOINTS],
                    links: [None; INLINE_ENDPOINTS],
                }),
                Vec::new(),
            )
        } else {
            let links = ctx.alloc_filled(count, None, "iges boundary proximity links")?;
            ctx.reserve_map(&mut cells, count, "iges boundary proximity cells")?;
            (None, links)
        };
        Ok(Self {
            inline,
            cells,
            links,
        })
    }

    pub(super) fn head(&self, key: &[i64; 3]) -> Option<usize> {
        match &self.inline {
            Some(inline) => inline
                .cells
                .iter()
                .flatten()
                .find_map(|(cell, head)| (cell == key).then_some(*head)),
            None => self.cells.get(key).copied(),
        }
    }

    pub(super) fn previous(&self, index: usize) -> Option<usize> {
        match &self.inline {
            Some(inline) => inline.links[index],
            None => self.links[index],
        }
    }

    pub(super) fn insert(&mut self, key: [i64; 3], index: usize) -> Result<(), CodecError> {
        match &mut self.inline {
            Some(inline) => {
                if let Some(slot) = inline
                    .cells
                    .iter_mut()
                    .find(|slot| slot.as_ref().is_some_and(|(cell, _)| *cell == key))
                {
                    inline.links[index] = slot.replace((key, index)).map(|(_, head)| head);
                } else {
                    let slot = inline
                        .cells
                        .iter_mut()
                        .find(|slot| slot.is_none())
                        .ok_or_else(|| {
                            CodecError::malformed("boundary inline cell capacity is exhausted")
                        })?;
                    *slot = Some((key, index));
                }
            }
            None => {
                // The table holds one cell per endpoint; insertion cannot grow it.
                self.links[index] = self.cells.insert(key, index);
            }
        }
        Ok(())
    }
}
