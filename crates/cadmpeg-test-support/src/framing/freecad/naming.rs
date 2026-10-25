// SPDX-License-Identifier: Apache-2.0
//! Independent Appendix C boundaries for FreeCAD persistent naming streams.
//!
//! These routines inspect source windows only. They do not admit identifiers,
//! resolve references, decode names, or import a codec parser. On an unprovable
//! located boundary they return an error and leave the proved prefix in `out`.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};

use crate::framing::{FrameParent, FramedPart};

mod legacy;
mod maps;
mod strings;

pub use legacy::frame_legacy_records;
pub use maps::{frame_element_map_entry, frame_inline_map};
pub use strings::{frame_string_records, frame_string_table_entry, StringRecords};

struct Tokens<'a> {
    source: View<'a>,
    position: usize,
}
impl<'a> Tokens<'a> {
    fn new(source: View<'a>) -> Self {
        Self {
            source,
            position: 0,
        }
    }
    fn bytes(&self) -> &'a [u8] {
        self.source.window()
    }
    fn skip(&mut self) -> Range<usize> {
        let start = self.position;
        while self.position < self.bytes().len() {
            let tail = &self.bytes()[self.position..];
            let width = if tail[0].is_ascii_whitespace() {
                1
            } else {
                tail.utf8_chunks()
                    .next()
                    .and_then(|chunk| chunk.valid().chars().next())
                    .filter(|character| character.is_whitespace())
                    .map_or(0, char::len_utf8)
            };
            if width == 0 {
                break;
            }
            self.position += width;
        }
        start..self.position
    }
    fn token(&mut self) -> Result<Range<usize>, String> {
        self.skip();
        let start = self.position;
        while self.position < self.bytes().len() {
            let mut probe = Self {
                source: self.source,
                position: self.position,
            };
            if !probe.skip().is_empty() {
                break;
            }
            self.position += 1;
        }
        if start == self.position {
            return Err(format!(
                "naming source ends before token at {}",
                self.position
            ));
        }
        Ok(start..self.position)
    }
    fn text(&self, range: Range<usize>) -> Result<&'a str, String> {
        std::str::from_utf8(&self.bytes()[range]).map_err(|error| error.to_string())
    }
    fn count(&self, range: Range<usize>) -> Result<usize, String> {
        let count = self
            .text(range)?
            .parse::<usize>()
            .map_err(|error| error.to_string())?;
        if count > self.bytes().len() {
            return Err("naming count cannot fit in its source extent".into());
        }
        Ok(count)
    }
    fn expect(&mut self, expected: &[u8]) -> Result<Range<usize>, String> {
        let range = self.token()?;
        if &self.bytes()[range.clone()] != expected {
            return Err(format!(
                "naming boundary lacks token {}",
                String::from_utf8_lossy(expected)
            ));
        }
        Ok(range)
    }
    fn view(&self, range: Range<usize>) -> Result<View<'a>, String> {
        let start = self
            .source
            .start()
            .checked_add(range.start)
            .ok_or("naming start overflows")?;
        let end = self
            .source
            .start()
            .checked_add(range.end)
            .ok_or("naming end overflows")?;
        self.source
            .child(start, end)
            .ok_or_else(|| "naming extent escapes source".into())
    }
}

fn emit<'a>(
    ctx: &DecodeContext<'_>,
    tokens: &Tokens<'a>,
    extent: Range<usize>,
    name: &str,
    parent: usize,
    framing: Vec<Range<usize>>,
    dispatch: Vec<Range<usize>>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<usize, String> {
    let absolute = |ranges: Vec<Range<usize>>| -> Result<Vec<Range<u64>>, String> {
        ranges
            .into_iter()
            .filter(|range| !range.is_empty())
            .map(|range| {
                let start = tokens
                    .source
                    .start()
                    .checked_add(range.start)
                    .ok_or("naming field start overflows")?;
                let end = tokens
                    .source
                    .start()
                    .checked_add(range.end)
                    .ok_or("naming field end overflows")?;
                Ok(u64_from_index(start)..u64_from_index(end))
            })
            .collect()
    };
    let frame = FramedPart::new(
        ctx,
        super::level(name)?,
        tokens.view(extent)?,
        FrameParent::Part(parent),
        absolute(framing)?,
        absolute(dispatch)?,
    )?;
    let index = out.len();
    super::append(ctx, out, frame)?;
    Ok(index)
}

fn retain_push<T>(ctx: &DecodeContext<'_>, values: &mut Vec<T>, value: T) -> Result<(), String> {
    ctx.retain_bytes(
        u64_from_index(size_of::<T>()),
        "FreeCAD naming framing metadata",
    )
    .map_err(|error| error.to_string())?;
    values.try_reserve(1).map_err(|error| error.to_string())?;
    values.push(value);
    Ok(())
}

fn entry<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    file_attribute: View<'a>,
    zip_parent: usize,
    name: &str,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<usize, String> {
    let frame = FramedPart::new(
        ctx,
        super::level(name)?,
        source,
        FrameParent::Part(zip_parent),
        vec![],
        vec![],
    )?
    .with_external_dispatch(ctx, vec![file_attribute])?;
    let index = out.len();
    super::append(ctx, out, frame)?;
    Ok(index)
}

#[cfg(test)]
mod tests;
