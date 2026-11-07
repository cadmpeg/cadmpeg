// SPDX-License-Identifier: Apache-2.0
//! Sheet boundaries selected from outer sketch and fill records.

mod emit;
mod geometry;
mod source;

use cadmpeg_ir::math::{Point2, Point3, Vector3};

#[derive(Clone)]
struct SheetPlan {
    source_offset: u64,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
    components: Vec<Vec<SheetLine>>,
}

#[derive(Clone, Copy)]
struct SheetLine {
    source_offset: u64,
    start_id: u32,
    end_id: u32,
    start: Point2,
    end: Point2,
    origin: Point2,
    direction: Point2,
    interval: [f64; 2],
    forward: bool,
}

pub(crate) fn transfer(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source_data: cadmpeg_core::decode::View<'_>,
    ir: &mut cadmpeg_ir::document::CadIr,
    annotations: &mut cadmpeg_ir::Annotations,
    native: &crate::native::CatiaNative,
    scope: &crate::decode::ModelingGraphScope,
) -> Result<usize, cadmpeg_core::CodecError> {
    let plans = source::plans(ctx, source_data, native, scope, ir.tolerances.linear.get())?;
    if plans.is_empty() {
        return Ok(0);
    }
    let mut builder = cadmpeg_ir::AnnotationBuilder::resume(std::mem::take(annotations));
    let count = emit::emit(ctx, ir, &mut builder, &plans)?;
    *annotations = builder.build();
    Ok(count)
}
