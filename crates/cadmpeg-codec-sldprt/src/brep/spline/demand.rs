// SPDX-License-Identifier: Apache-2.0
//! Progressive discovery and cached resolution of demanded spline sources.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

use super::{
    exact_f64_array, framed_array_span, parse_curve_descriptor, parse_surface_descriptor,
    surface_knot_values, ArrayCandidate, Arrays, CompactArray, CurveDescriptor,
    FramedArray, SurfaceDescriptor, SurfaceKnotValues,
};
use crate::layout::bspline_compact_array_header as compact_arr;

/// A compact control result owns its backing. Framed results borrow the
/// immutable framed payload cache; absence includes ambiguous aliases.
enum ControlValues<'ctx> {
    Framed,
    Compact {
        values: Vec<f64>,
        _storage: ScopedReservation<'ctx>,
    },
    Absent,
}

struct KnotValues<'ctx> {
    values: SurfaceKnotValues,
    _storage: ScopedReservation<'ctx>,
}

/// Source table nodes and compact header lists share a shell lifetime.
/// Decoded payloads and resolved lanes have independent value owners.
/// Discovery costs one source pass per distinct demand in the worst case.
/// The immutable byte source cannot change while its cached offsets live.
/// All cache discovery and storage use the one constructor context.
pub(super) struct Sources<'input, 'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    bytes: &'input [u8],
    curve_descriptors: BTreeMap<u16, Option<CurveDescriptor>>,
    surface_descriptors: BTreeMap<u16, Option<SurfaceDescriptor>>,
    arrays: Arrays<'ctx>,
    compact: bool,
    demanded_arrays: BTreeSet<u16>,
    controls: BTreeMap<(u16, usize), ControlValues<'ctx>>,
    knots: BTreeMap<(u16, u16, usize), Option<KnotValues<'ctx>>>,
    shell: ScopedReservation<'ctx>,
}

impl<'input, 'ctx, 'arena> Sources<'input, 'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>, bytes: &'input [u8], compact: bool) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            bytes,
            curve_descriptors: BTreeMap::new(),
            surface_descriptors: BTreeMap::new(),
            arrays: Arrays::default(),
            compact,
            demanded_arrays: BTreeSet::new(),
            controls: BTreeMap::new(),
            knots: BTreeMap::new(),
            shell: ctx.reserve_scoped(0, "hold Parasolid demanded sources")?,
        })
    }

    pub(super) fn arrays(&self) -> &Arrays<'ctx> {
        &self.arrays
    }

    fn curve(
        &mut self,
        attr: u16,
    ) -> Result<Option<CurveDescriptor>, CodecError> {
        let ctx = self.ctx;
        if let Some(limit) = ctx.resource_refusal() { return Err(limit.into()); }
        let bytes = self.bytes;
        if attr <= 1 {
            return Ok(None);
        }
        if let Some(cached) = ctx.get_btree_map(
            &self.curve_descriptors, &attr, "find Parasolid curve descriptors",
        )? {
            return Ok(*cached);
        }
        let mut found = None;
        let mut starts = 0..bytes.len().checked_sub(29).map_or(0, |end| end);
        while starts.len() != 0 {
            let Some(off) = ctx.next_charged(&mut starts, "scan Parasolid curve descriptors")? else {
                break;
            };
            if let Some((key, descriptor)) = parse_curve_descriptor(bytes, off) {
                if key == attr {
                    found = Some(descriptor);
                    break;
                }
            }
        }
        self.shell.with_storage(|| ctx.insert_btree_map(
            &mut self.curve_descriptors, attr, found, "collect Parasolid curve descriptors",
        ))?;
        Ok(found)
    }

    /// The wrapper has at most22 fixed reference positions. Each demanded
    /// descriptor is scanned once, including references with no descriptor.
    pub(super) fn curve_for_wrapper(
        &mut self,
        attr_at: usize,
    ) -> Result<Option<CurveDescriptor>, CodecError> {
        let ctx = self.ctx;
        if let Some(limit) = ctx.resource_refusal() { return Err(limit.into()); }
        let bytes = self.bytes;
        for at in attr_at + 2..(attr_at + 24).min(bytes.len().checked_sub(1).map_or(0, |end| end)) {
            let Some(reference) = View::u16_be_at(bytes, at) else { continue; };
            if let Some(descriptor) = self.curve(reference)? {
                return Ok(Some(descriptor));
            }
        }
        Ok(None)
    }

    pub(super) fn surface(
        &mut self,
        attr: u16,
    ) -> Result<Option<SurfaceDescriptor>, CodecError> {
        let ctx = self.ctx;
        if let Some(limit) = ctx.resource_refusal() { return Err(limit.into()); }
        let bytes = self.bytes;
        if attr <= 1 {
            return Ok(None);
        }
        if let Some(cached) = ctx.get_btree_map(
            &self.surface_descriptors, &attr, "find Parasolid surface descriptors",
        )? {
            return Ok(*cached);
        }
        let mut found = None;
        let mut starts = 0..bytes.len().checked_sub(1).map_or(0, |end| end);
        while starts.len() != 0 {
            let Some(off) = ctx.next_charged(&mut starts, "scan Parasolid surface descriptors")? else {
                break;
            };
            if let Some(descriptor) = parse_surface_descriptor(bytes, off) {
                if descriptor.attr == attr {
                    found = Some(descriptor);
                    break;
                }
            }
        }
        self.shell.with_storage(|| ctx.insert_btree_map(
            &mut self.surface_descriptors, attr, found, "collect Parasolid surface descriptors",
        ))?;
        Ok(found)
    }

    /// Gather only this demanded attribute. Compact aliases require the full
    /// source pass. Framed payloads remain unread until descriptor binding.
    pub(super) fn array(
        &mut self,
        attr: u16,
    ) -> Result<(), CodecError> {
        let ctx = self.ctx;
        let bytes = self.bytes;
        let reference_operation = if self.compact { "collect Parasolid surface array references" } else { "collect Parasolid curve array references" };
        if ctx.contains_btree_set(&self.demanded_arrays, &attr, reference_operation)? {
            return Ok(());
        }
        let mut starts = 0..bytes.len().checked_sub(9).map_or(0, |end| end);
        while starts.len() != 0 {
            let Some(off) = ctx.next_charged(&mut starts, "scan Parasolid spline arrays")? else {
                break;
            };
            if self.compact && bytes.get(off) == Some(&0) {
                let count = bytes.get(off + compact_arr::COUNT).copied().map_or(0, usize::from);
                if count > 0 && View::u16_be_at(bytes, off + compact_arr::ATTR) == Some(attr) {
                    self.shell.with_storage(|| ctx.push_btree_group(
                        &mut self.arrays.compact, attr, CompactArray { offset: off, count },
                        "index Parasolid compact arrays", "scan Parasolid compact arrays",
                    ))?;
                }
            }
            let Some((key, tag, span)) = framed_array_span(bytes, off) else { continue; };
            if key != attr { continue; }
            if tag == 0x7f {
                if !ctx.contains_key_btree_map(&self.arrays.u16s, &attr, "collect Parasolid integer arrays")? {
                    self.shell.with_storage(|| ctx.insert_btree_map(
                        &mut self.arrays.u16s, attr,
                        FramedArray { span, operation: "decode Parasolid integer array values", values: std::cell::OnceCell::new() },
                        "collect Parasolid integer arrays",
                    ))?;
                }
            } else if !ctx.contains_key_btree_map(&self.arrays.f64s, &attr, "collect Parasolid scalar arrays")? {
                self.shell.with_storage(|| ctx.insert_btree_map(
                    &mut self.arrays.f64s, attr,
                    FramedArray { span, operation: "decode Parasolid scalar array values", values: std::cell::OnceCell::new() },
                    "collect Parasolid scalar arrays",
                ))?;
            }
        }
        self.shell.with_storage(|| ctx.insert_btree_set(
            &mut self.demanded_arrays, attr, reference_operation,
        ))?;
        Ok(())
    }

    pub(super) fn prepare_control(
        &mut self,
        attr: u16,
        count: usize,
    ) -> Result<(), CodecError> {
        let ctx = self.ctx;
        let key = (attr, count);
        if ctx.contains_key_btree_map(&self.controls, &key, "find Parasolid resolved control arrays")? {
            return Ok(());
        }
        let mut workspace = ctx.reserve_scoped(0, "hold Parasolid surface array candidates")?;
        let selected = exact_f64_array(ctx, &mut workspace, self.bytes, &self.arrays, attr, count)?;
        let result = match selected {
            Some(ArrayCandidate::Borrowed(_)) => ControlValues::Framed,
            Some(ArrayCandidate::Owned { values, _storage }) => ControlValues::Compact { values, _storage },
            None => ControlValues::Absent,
        };
        drop(workspace);
        self.shell.with_storage(|| ctx.insert_btree_map(
            &mut self.controls, key, result, "collect Parasolid resolved control arrays",
        ))?;
        Ok(())
    }

    pub(super) fn control(
        &self,
        attr: u16,
        count: usize,
    ) -> Result<Option<&[f64]>, CodecError> {
        let ctx = self.ctx;
        let bytes = self.bytes;
        let Some(result) = ctx.get_btree_map(&self.controls, &(attr, count), "find Parasolid resolved control arrays")? else {
            return Ok(None);
        };
        match result {
            ControlValues::Framed => {
                let Some(array) = ctx.get_btree_map(&self.arrays.f64s, &attr, "find Parasolid resolved control arrays")? else {
                    return Ok(None);
                };
                Ok(Some(array.read(ctx, |at| View::f64_be_at(bytes, at))?))
            }
            ControlValues::Compact { values, .. } => Ok(Some(values)),
            ControlValues::Absent => Ok(None),
        }
    }

    pub(super) fn prepare_knots(
        &mut self,
        knot_attr: u16,
        multiplicity_attr: u16,
        count: usize,
    ) -> Result<(), CodecError> {
        let ctx = self.ctx;
        let key = (knot_attr, multiplicity_attr, count);
        if ctx.contains_key_btree_map(&self.knots, &key, "find Parasolid resolved knot lanes")? {
            return Ok(());
        }
        let mut workspace = ctx.reserve_scoped(0, "hold Parasolid surface array candidates")?;
        let mut output = ctx.reserve_scoped(0, "hold Parasolid resolved knot lanes")?;
        let resolved = surface_knot_values(
            ctx, &mut workspace, &mut output, self.bytes, &self.arrays, (knot_attr, multiplicity_attr, count),
        )?;
        let result = resolved.map(|values| KnotValues { values, _storage: output });
        drop(workspace);
        self.shell.with_storage(|| ctx.insert_btree_map(
            &mut self.knots, key, result, "collect Parasolid resolved knot lanes",
        ))?;
        Ok(())
    }

    pub(super) fn knots(
        &self,
        knot_attr: u16,
        multiplicity_attr: u16,
        count: usize,
    ) -> Result<Option<&SurfaceKnotValues>, CodecError> {
        let ctx = self.ctx;
        Ok(ctx.get_btree_map(
            &self.knots, &(knot_attr, multiplicity_attr, count), "find Parasolid resolved knot lanes",
        )?.and_then(|result| result.as_ref().map(|owned| &owned.values)))
    }
}

#[cfg(test)]
mod tests;
