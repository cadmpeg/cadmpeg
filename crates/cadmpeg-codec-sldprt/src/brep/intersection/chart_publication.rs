// SPDX-License-Identifier: Apache-2.0
//! Chart point backing and concrete candidate publication.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use super::{finite_point, Chart};

pub(super) struct ChartHeader {
    pub(super) endpoints: [[f64; 3]; 2],
    pub(super) base_parameter: f64,
    pub(super) base_scale: f64,
    pub(super) chordal_error: f64,
}

impl ChartHeader {
    fn with_points(self, interior_points: Vec<[f64; 3]>) -> Chart {
        Chart {
            endpoints: self.endpoints,
            interior_points,
            base_parameter: self.base_parameter,
            base_scale: self.base_scale,
            chordal_error: self.chordal_error,
        }
    }
}

pub(super) struct ScopedChart<'ctx> {
    pub(super) value: Chart,
    _points: ScopedReservation<'ctx>,
}

pub(super) struct ScopedCharts<'ctx> {
    values: Vec<ScopedChart<'ctx>>,
    shell: ScopedReservation<'ctx>,
}

impl<'ctx> ScopedCharts<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self { values: Vec::new(), shell: ctx.reserve_scoped(0, "hold Parasolid chart candidates")? })
    }

    pub(super) fn into_entries(self) -> ChartEntries<'ctx> {
        ChartEntries { values: self.values.into_iter(), _shell: self.shell }
    }
}

pub(super) struct ChartEntries<'ctx> {
    values: std::vec::IntoIter<ScopedChart<'ctx>>,
    _shell: ScopedReservation<'ctx>,
}

impl<'ctx> ChartEntries<'ctx> {
    pub(super) fn next(
        &mut self, ctx: &DecodeContext<'_>, operation: &'static str,
    ) -> Result<Option<ScopedChart<'ctx>>, CodecError> {
        if let Some(limit) = ctx.resource_refusal() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if self.values.len() == 0 { return Ok(None); }
        ctx.next_charged(&mut self.values, operation)
    }
}

pub(super) enum ChartSink<'out, 'ctx> {
    #[cfg(test)]
    Retained(&'out mut Vec<Chart>),
    Scoped(&'out mut ScopedCharts<'ctx>),
}

fn interior_points(
    ctx: &DecodeContext<'_>, bytes: &[u8], block: usize, count: usize, stride: usize,
) -> Result<Vec<[f64; 3]>, CodecError> {
    let mut values = ctx.vector_storage(count - 2, "decode Parasolid chart interior points")?;
    for index in ctx.admit_iter(1..count - 1, "decode Parasolid chart interior points")? {
        if let Some(point) = finite_point(bytes, block + index * stride) {
            ctx.push_vec(&mut values, point, "decode Parasolid chart interior points")?;
        }
    }
    Ok(values)
}

impl<'ctx> ChartSink<'_, 'ctx> {
    pub(super) fn publish(
        &mut self, ctx: &'ctx DecodeContext<'_>, bytes: &[u8],
        block: usize, count: usize, stride: usize, header: ChartHeader,
    ) -> Result<(), CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(out) => {
                let value = header.with_points(interior_points(ctx, bytes, block, count, stride)?);
                ctx.push_vec(out, value, "collect Parasolid chart stride candidates")
            }
            Self::Scoped(out) => {
                let mut storage = ctx.reserve_scoped(0, "hold Parasolid chart interior points")?;
                let points = storage.with_storage(|| interior_points(ctx, bytes, block, count, stride))?;
                let value = ScopedChart { value: header.with_points(points), _points: storage };
                out.shell.with_storage(|| ctx.push_vec(&mut out.values, value, "collect Parasolid chart stride candidates"))
            }
        }
    }
}
