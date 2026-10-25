// SPDX-License-Identifier: Apache-2.0
//! Source-proven part extents and exact framing intervals.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, PartCapture, View, XmlElementScan};
use cadmpeg_ir::source_fidelity::parts::{PartLevel, PartSpec};

/// Index-based containment in a parent-first frame inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameParent {
    /// This row is directly contained by the physical input file.
    File,
    /// The physical containing frame; a source proof can bind a declared ancestor.
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
    xml_containment: Option<XmlContainment>,
    zip_containment: Option<ZipContainment>,
}

#[derive(Debug)]
pub(super) struct XmlContainment {
    pub(super) owner: usize,
    pub(super) parent: PartCapture,
}

#[derive(Debug)]
pub(super) struct ZipContainment {
    pub(super) owner: usize,
    pub(super) parent: PartCapture,
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
            xml_containment: None,
            zip_containment: None,
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

    /// Bind actual immediate XML containment separately from the declared owner.
    ///
    /// Only a node from the source scanner can supply this proof. The table
    /// validator still checks the owner level and its physical ancestor chain.
    pub fn with_xml_parent(
        mut self,
        ctx: &DecodeContext<'_>,
        scan: &XmlElementScan<'a>,
        node: usize,
        declared_owner: usize,
    ) -> Result<Self, String> {
        if !matches!(self.parent, FrameParent::Part(_)) {
            return Err("XML containment requires a physical part parent".into());
        }
        let child = scan.elements().get(node).ok_or("missing XML child")?;
        let actual = ctx
            .capture_part_extent(child.extent())
            .map_err(|error| error.to_string())?;
        if !same_extent(&actual, &self.capture) {
            return Err("XML child proof does not describe this frame".into());
        }
        let parent = child
            .parent()
            .and_then(|index| scan.elements().get(index))
            .ok_or("XML child has no physical parent")?;
        self.xml_containment = Some(XmlContainment {
            owner: declared_owner,
            parent: ctx
                .capture_part_extent(parent.extent())
                .map_err(|error| error.to_string())?,
        });
        Ok(self)
    }

    pub(super) fn xml_containment(&self) -> Option<&XmlContainment> {
        self.xml_containment.as_ref()
    }

    pub(super) fn bind_zip_directory_parent(
        &mut self,
        ctx: &DecodeContext<'_>,
        proof: &super::zip::DirectorySignature<'a>,
        declared_owner: usize,
    ) -> Result<(), String> {
        if self.level.as_str() != "zip.digital_signature"
            || !matches!(self.parent, FrameParent::Part(_))
        {
            return Err("ZIP directory proof requires a physical signature child".into());
        }
        let actual = ctx
            .capture_part_extent(proof.signature())
            .map_err(|error| error.to_string())?;
        if !same_extent(&actual, &self.capture) {
            return Err("ZIP signature proof describes a different source part".into());
        }
        self.zip_containment = Some(ZipContainment {
            owner: declared_owner,
            parent: ctx
                .capture_part_extent(proof.directory())
                .map_err(|error| error.to_string())?,
        });
        Ok(())
    }

    pub(super) fn zip_containment(&self) -> Option<&ZipContainment> {
        self.zip_containment.as_ref()
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

    /// Actual enclosing frame or physical file.
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

    /// Content eligible for damage, preserving this part's framing and every
    /// descendant's framing in the undamaged independent inventory (#215).
    ///
    /// Descendants in another source address space contribute no byte offsets
    /// to this view. The receiver must be an actual member of `inventory`.
    ///
    /// # Errors
    /// Rejects a foreign receiver, invalid parent index, or parent cycle.
    pub fn content_ranges_in(&self, inventory: &[Self]) -> Result<Vec<Range<u64>>, String> {
        let index = inventory
            .iter()
            .position(|part| std::ptr::eq(self, part))
            .ok_or("damage part is not in its independent inventory")?;
        let mut framing = self.framing.clone();
        for (child, part) in inventory.iter().enumerate() {
            if child == index {
                continue;
            }
            let mut current = child;
            let mut remaining = inventory.len();
            loop {
                if remaining == 0 {
                    return Err("framer parent cycle".into());
                }
                remaining -= 1;
                let parent = inventory
                    .get(current)
                    .ok_or("framer parent outside inventory")?
                    .parent;
                let FrameParent::Part(parent) = parent else {
                    break;
                };
                if parent == index {
                    if part.capture.stream() == self.capture.stream() {
                        framing.extend(part.framing.iter().cloned());
                    }
                    break;
                }
                current = parent;
            }
        }
        Ok(subtract_ranges(
            self.capture.offset()
                ..self
                    .capture
                    .offset()
                    .checked_add(self.capture.byte_len())
                    .ok_or("damage part extent overflows")?,
            &framing,
        ))
    }

    /// Bytes inside this part that are not its own framing.
    pub fn content_ranges(&self) -> Vec<Range<u64>> {
        subtract_ranges(
            u64_from_index(self.view.start())..u64_from_index(self.view.end()),
            &self.framing,
        )
    }
}

pub(super) fn same_extent(first: &PartCapture, second: &PartCapture) -> bool {
    first.stream() == second.stream()
        && first.offset() == second.offset()
        && first.byte_len() == second.byte_len()
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
