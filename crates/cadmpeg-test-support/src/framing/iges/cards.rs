// SPDX-License-Identifier: Apache-2.0
//! Physical ASCII line and packed-card boundaries.

use cadmpeg_core::decode::DecodeContext;

use super::append;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Card(u8),
    Unmarked,
    PostTerminate,
    Unframed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Card {
    pub(super) start: usize,
    pub(super) body_end: usize,
    pub(super) end: usize,
    pub(super) kind: Kind,
}

fn marker(bytes: &[u8]) -> Option<u8> {
    bytes
        .get(72)
        .copied()
        .filter(|byte| matches!(byte, b'S' | b'G' | b'D' | b'P' | b'T'))
}

fn line(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut body_end = start;
    while body_end < bytes.len() && !matches!(bytes[body_end], b'\r' | b'\n') {
        body_end += 1;
    }
    let end = if bytes[body_end..].starts_with(b"\r\n") {
        body_end + 2
    } else if body_end < bytes.len() {
        body_end + 1
    } else {
        body_end
    };
    (body_end, end)
}

pub(super) fn scan(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<Card>, String> {
    if bytes.get(72) == Some(&b'C') {
        return Err("compressed ASCII IGES is outside the physical-card framer".into());
    }
    if bytes.first() == Some(&b'B')
        && bytes.get(1..5) == Some(&[0, 0, 0, 75])
        && bytes.get(72) == Some(&b'B')
    {
        return Err("binary IGES is outside the physical-card framer".into());
    }
    let bare = !bytes.iter().any(|byte| matches!(byte, b'\r' | b'\n'));
    let mut out = Vec::new();
    let mut start = 0;
    let mut terminated = false;
    while start < bytes.len() {
        let (body_end, end) = line(bytes, start);
        if terminated {
            append(
                ctx,
                &mut out,
                Card {
                    start,
                    body_end,
                    end,
                    kind: Kind::PostTerminate,
                },
            )?;
            start = end;
            continue;
        }
        let length = body_end - start;
        let packed = length > 80
            && length.is_multiple_of(80)
            && bytes[start..body_end]
                .chunks_exact(80)
                .all(|card| marker(card).is_some());
        if bare || packed || length <= 80 || marker(&bytes[start..body_end]) == Some(b'T') {
            let mut at = start;
            while at < body_end {
                let card_end = at
                    .checked_add(80)
                    .ok_or("IGES card offset overflows")?
                    .min(body_end);
                let card_marker = marker(&bytes[at..card_end]);
                let next = if card_end == body_end { end } else { card_end };
                append(
                    ctx,
                    &mut out,
                    Card {
                        start: at,
                        body_end: card_end,
                        end: next,
                        kind: card_marker.map_or(
                            if bare && length > 80 {
                                Kind::Unframed
                            } else {
                                Kind::Unmarked
                            },
                            Kind::Card,
                        ),
                    },
                )?;
                if card_marker == Some(b'T') {
                    terminated = true;
                    if next < end {
                        append(
                            ctx,
                            &mut out,
                            Card {
                                start: next,
                                body_end,
                                end,
                                kind: Kind::PostTerminate,
                            },
                        )?;
                    }
                    break;
                }
                at = next;
            }
            if length == 0 {
                append(
                    ctx,
                    &mut out,
                    Card {
                        start,
                        body_end,
                        end,
                        kind: Kind::Unmarked,
                    },
                )?;
            }
        } else {
            let kind = if marker(&bytes[start..body_end]).is_some() {
                Kind::Unframed
            } else {
                Kind::Unmarked
            };
            append(
                ctx,
                &mut out,
                Card {
                    start,
                    body_end,
                    end,
                    kind,
                },
            )?;
        }
        start = end;
    }
    Ok(out)
}
