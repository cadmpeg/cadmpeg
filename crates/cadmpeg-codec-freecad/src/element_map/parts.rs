// SPDX-License-Identifier: Apache-2.0
//! Framed naming streams decoded from their registered source Views.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{PartLevel, PartSpec};
use std::ops::Range;

mod maps;
mod tables;
pub(super) use maps::{entry_framing, legacy, map};
#[cfg(test)]
pub(super) use tables::record as read_record;
pub(super) use tables::table;

/// Lookup facts from actually admitted tables and source-proven missing rows.
#[derive(Clone, Copy, Default)]
pub(super) struct StringIds<'a> {
    pub(super) known: Option<&'a std::collections::BTreeSet<i64>>,
    pub(super) unavailable: Option<&'a std::collections::BTreeSet<i64>>,
}

impl StringIds<'_> {
    pub(super) fn missing(self, ids: &[i64]) -> bool {
        self.unavailable
            .is_some_and(|missing| ids.iter().any(|id| missing.contains(id)))
    }
}

type Spacing<'scope, 'source> = dyn Fn(View<'source>) -> Result<(), CodecError> + 'scope;

#[derive(Clone)]
struct Scanner<'scope, 'source> {
    view: View<'source>,
    at: usize,
    spacing: Option<&'scope Spacing<'scope, 'source>>,
}

impl<'scope, 'source> Scanner<'scope, 'source> {
    fn new(view: View<'source>) -> Self {
        Self {
            view,
            at: 0,
            spacing: None,
        }
    }
    fn tracking(view: View<'source>, spacing: Option<&'scope Spacing<'scope, 'source>>) -> Self {
        Self {
            view,
            at: 0,
            spacing,
        }
    }
    fn skip(&mut self) -> Result<(), CodecError> {
        let start = self.at;
        while self.at < self.view.window().len() {
            let rest = &self.view.window()[self.at..];
            let width = if rest[0].is_ascii_whitespace() {
                1
            } else {
                first_character(rest)
                    .filter(|value| value.is_whitespace())
                    .map_or(0, char::len_utf8)
            };
            if width == 0 {
                break;
            }
            self.at += width;
        }
        if start < self.at {
            if let Some(spacing) = self.spacing {
                spacing(self.extent(start..self.at)?)?;
            }
        }
        Ok(())
    }
    fn token_range(&mut self) -> Result<Range<usize>, CodecError> {
        self.skip()?;
        let start = self.at;
        while self.at < self.view.window().len() {
            let rest = &self.view.window()[self.at..];
            if rest[0].is_ascii_whitespace() {
                break;
            }
            if rest[0] >= 0x80 {
                if let Some(value) = first_character(rest) {
                    if value.is_whitespace() {
                        break;
                    }
                    self.at += value.len_utf8();
                    continue;
                }
            }
            self.at += 1;
        }
        if start == self.at {
            return Err(CodecError::malformed(
                "naming stream ends before its next token",
            ));
        }
        Ok(start..self.at)
    }
    fn navigation(&self, range: Range<usize>) -> Result<&'source str, CodecError> {
        std::str::from_utf8(
            self.view
                .window()
                .get(range)
                .ok_or_else(|| CodecError::malformed("naming token escapes source"))?,
        )
        .map_err(|_| CodecError::malformed("naming framing token is not UTF-8"))
    }
    fn token(&mut self) -> Result<&'source str, CodecError> {
        let range = self.token_range()?;
        text(self.view, range)
    }
    fn count(&mut self, maximum: usize) -> Result<usize, CodecError> {
        let value = self
            .token()?
            .parse::<usize>()
            .map_err(|_| CodecError::malformed("invalid naming stream count"))?;
        if value > maximum {
            return Err(CodecError::malformed(
                "naming stream count exceeds its part limit",
            ));
        }
        Ok(value)
    }
    fn expect(&mut self, expected: &str) -> Result<(), CodecError> {
        if self.token()? != expected {
            return Err(CodecError::malformed(format_args!(
                "expected naming stream token {expected}"
            )));
        }
        Ok(())
    }
    fn extent(&self, range: Range<usize>) -> Result<View<'source>, CodecError> {
        self.view
            .child(
                self.view.start() + range.start,
                self.view.start() + range.end,
            )
            .ok_or_else(|| CodecError::malformed("naming record escapes source"))
    }
    fn whitespace_end(&mut self) -> Result<usize, CodecError> {
        self.skip()?;
        Ok(self.at)
    }
    fn done(&mut self) -> Result<bool, CodecError> {
        self.skip()?;
        Ok(self.at == self.view.window().len())
    }
}

fn first_character(bytes: &[u8]) -> Option<char> {
    let first = *bytes.first()?;
    let width = match first {
        0..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    std::str::from_utf8(bytes.get(..width)?)
        .ok()?
        .chars()
        .next()
}

fn text(view: View<'_>, range: Range<usize>) -> Result<&str, CodecError> {
    let bytes = view
        .read_slice(view.start() + range.start..view.start() + range.end)
        .ok_or_else(|| CodecError::malformed("naming field read escapes source"))?;
    std::str::from_utf8(bytes).map_err(|_| CodecError::malformed("naming field is not UTF-8"))
}

pub(super) fn spec(
    level: &str,
    view: View<'_>,
    fields: &[Range<usize>],
) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(level)
        .ok_or_else(|| CodecError::malformed("naming level is absent from Appendix C"))?;
    let framing = fields
        .iter()
        .map(|range| {
            u64_from_index(view.start() + range.start)..u64_from_index(view.start() + range.end)
        })
        .collect();
    Ok(PartSpec::new(level, framing))
}

fn checked_count(
    ctx: &DecodeContext<'_>,
    count: usize,
    source: View<'_>,
    what: &'static str,
) -> Result<(), CodecError> {
    cadmpeg_core::decode::bounded_len(u64_from_index(count), 1, source.window().len())
        .ok_or_else(|| CodecError::malformed(what))?;
    ctx.charge_entities(u64_from_index(count), what)?;
    Ok(())
}

#[cfg(test)]
mod tests;
