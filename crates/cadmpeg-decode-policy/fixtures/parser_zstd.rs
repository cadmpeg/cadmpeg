// SPDX-License-Identifier: Apache-2.0
use zstd_safe::{DCtx, InBuffer, OutBuffer};
pub struct DecodeContext;
pub struct ScopedReservation;
struct ZstdStepAdmission<'step, 'input> {
    decoder: &'step mut DCtx<'static>,
    input: &'step mut InBuffer<'input>,
    output: OutBuffer<'step, [u8]>,
    _workspace: &'step ScopedReservation,
}
impl DecodeContext {
    fn charge_work(&self, _: u64, _: &str) -> Result<(), ()> { Ok(()) }
    fn zstd_step_admission<'step, 'input>(&self, decoder: &'step mut DCtx<'static>, input: &'step mut InBuffer<'input>, output: &'step mut [u8], workspace: &'step ScopedReservation) -> Result<ZstdStepAdmission<'step, 'input>, ()> {
        Ok(ZstdStepAdmission { decoder, input, output: OutBuffer::around(output), _workspace: workspace })
    }
}
pub fn admitted(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    drop(step.decoder.decompress_stream(&mut step.output, step.input));
    Ok(())
}
pub fn wrong_input(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, other: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    drop(step.decoder.decompress_stream(&mut step.output, other)); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn wrong_output(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    let mut bytes = [0; 1];
    let mut other = OutBuffer::around(&mut bytes[..]);
    drop(step.decoder.decompress_stream(&mut other, step.input)); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn wrong_decoder(ctx: &DecodeContext, decoder: &mut DCtx<'static>, other: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    drop(other.decompress_stream(&mut step.output, step.input)); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn reused(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    drop(step.decoder.decompress_stream(&mut step.output, step.input));
    drop(step.decoder.decompress_stream(&mut step.output, step.input)); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn discarded_workspace_field(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    drop(step._workspace);
    drop(step.decoder.decompress_stream(&mut step.output, step.input)); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn borrowed_workspace_stays_live(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    consume(step._workspace);
    drop(step.decoder.decompress_stream(&mut step.output, step.input));
    Ok(())
}
fn consume(_workspace: &ScopedReservation) {}
pub fn forged(_ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) {
    let mut step = ZstdStepAdmission { decoder, input, output: OutBuffer::around(bytes), _workspace: workspace };
    drop(step.decoder.decompress_stream(&mut step.output, step.input)); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn changed(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
    step.input.pos = 0;
    drop(step.decoder.decompress_stream(&mut step.output, step.input)); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn fresh_steps(ctx: &DecodeContext, decoder: &mut DCtx<'static>, input: &mut InBuffer<'_>, bytes: &mut [u8], workspace: &ScopedReservation) -> Result<(), ()> {
    loop {
        ctx.charge_work(1, "step")?;
        let mut step = ctx.zstd_step_admission(decoder, input, bytes, workspace)?;
        drop(step.decoder.decompress_stream(&mut step.output, step.input));
        break;
    }
    Ok(())
}
