// SPDX-License-Identifier: Apache-2.0
//! Source-proven part extents and exact framing intervals.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, PartCapture, View};
use cadmpeg_ir::source_fidelity::parts::{PartLevel, PartSpec};

/// Index-based containment in a parent-first frame inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameParent {
    /// This row is directly contained by the physical input file.
    File,
    /// One declared `parent` alternative contains this part.
    Part(usize),
    /// An embedding unit hosts a row whose declared parent is `file`.
    Host(usize),
}

/// One exact source part, including its independent framing and dispatch bytes.
#[derive(Debug)]
pub struct FramedPart<'a> {
    level: PartLevel,
    view: View<'a>,
    capture: PartCapture,
    parent: FrameParent,
    framing: Vec<Range<u64>>,
    dispatch: Vec<Range<u64>>,
    external_dispatch: Vec<View<'a>>,
}

impl<'a> FramedPart<'a> {
    /// Bind an exact registered level to a genuine session view.
    pub fn new(
        ctx: &DecodeContext<'_>,
        level: PartLevel,
        view: View<'a>,
        parent: FrameParent,
        framing: Vec<Range<u64>>,
        dispatch: Vec<Range<u64>>,
    ) -> Result<Self, String> {
        let capture = ctx.capture_part_extent(view).map_err(|e| e.to_string())?;
        validate_ranges(&capture, &framing)?;
        validate_ranges(&capture, &dispatch)?;
        Ok(Self {
            level,
            view,
            capture,
            parent,
            framing,
            dispatch,
            external_dispatch: Vec::new(),
        })
    }

    /// Retain dispatch bytes that reside outside this part's own address range.
    pub fn with_external_dispatch(
        mut self,
        ctx: &DecodeContext<'_>,
        views: Vec<View<'a>>,
    ) -> Result<Self, String> {
        for view in &views {
            let capture = ctx.capture_part_extent(*view).map_err(|e| e.to_string())?;
            if capture.byte_len() == 0 {
                return Err("external dispatch has no bytes".into());
            }
        }
        self.external_dispatch = views;
        Ok(self)
    }

    /// Exact qualified table level.
    pub fn level(&self) -> PartLevel {
        self.level
    }

    /// Original view, including its session and source address space.
    pub fn view(&self) -> View<'a> {
        self.view
    }

    /// Canonical stream and extent captured from the source session.
    pub fn capture(&self) -> &PartCapture {
        &self.capture
    }

    /// Declared enclosing frame or physical file.
    pub fn parent(&self) -> FrameParent {
        self.parent
    }

    /// Absolute intervals in this view's address space that are framing.
    pub fn framing(&self) -> &[Range<u64>] {
        &self.framing
    }

    /// Absolute intervals in this view's address space that select content grammar.
    pub fn dispatch(&self) -> &[Range<u64>] {
        &self.dispatch
    }

    /// Original views of dispatch bytes outside this part's own extent.
    pub fn external_dispatch(&self) -> &[View<'a>] {
        &self.external_dispatch
    }

    /// Supply the IR helper with the exact table identity and framing intervals.
    pub fn spec(&self) -> PartSpec {
        PartSpec::new(self.level, self.framing.clone())
    }

    /// Bytes inside this part that are not its own framing.
    pub fn content_ranges(&self) -> Vec<Range<u64>> {
        subtract_ranges(
            u64_from_index(self.view.start())..u64_from_index(self.view.end()),
            &self.framing,
        )
    }
}

fn validate_ranges(capture: &PartCapture, ranges: &[Range<u64>]) -> Result<(), String> {
    let end = capture
        .offset()
        .checked_add(capture.byte_len())
        .ok_or("part extent overflows")?;
    let mut previous = capture.offset();
    for range in ranges {
        if range.start < previous || range.start >= range.end || range.end > end {
            return Err(
                "framing or dispatch ranges are unordered, overlapping or outside the part".into(),
            );
        }
        previous = range.end;
    }
    Ok(())
}

pub(super) fn subtract_ranges(extent: Range<u64>, ranges: &[Range<u64>]) -> Vec<Range<u64>> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|range| (range.start, range.end));
    let mut result = Vec::new();
    let mut start = extent.start;
    for range in sorted {
        let end = range.start.min(extent.end);
        if start < end {
            result.push(start..end);
        }
        start = start.max(range.end).min(extent.end);
    }
    if start < extent.end {
        result.push(start..extent.end);
    }
    result
}
