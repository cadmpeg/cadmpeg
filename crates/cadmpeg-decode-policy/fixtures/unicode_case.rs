// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub struct ScopedReservation;
struct UnicodeCaseAdmission<'a> { text: &'a str, _workspace: ScopedReservation }
impl DecodeContext {
    fn admit_moves<T>(&self, _: &[T], _: u64, _: &str) -> Result<(), ()> { Ok(()) }
    fn unicode_case_admission<'a>(&self, text: &'a str) -> Result<UnicodeCaseAdmission<'a>, ()> {
        Ok(UnicodeCaseAdmission { text, _workspace: ScopedReservation })
    }
}
pub fn admitted(ctx: &DecodeContext, text: &str) -> Result<String, ()> {
    let admission = ctx.unicode_case_admission(text)?;
    Ok(admission.text.to_lowercase())
}
pub fn upper(ctx: &DecodeContext, text: &str) -> Result<String, ()> {
    let admission = ctx.unicode_case_admission(text)?;
    Ok(admission.text.to_uppercase())
}
pub fn other(ctx: &DecodeContext, text: &str, other: &str) -> Result<String, ()> {
    let _admission = ctx.unicode_case_admission(text)?;
    Ok(other.to_lowercase()) // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn reused(ctx: &DecodeContext, text: &str) -> Result<String, ()> {
    let admission = ctx.unicode_case_admission(text)?;
    drop(admission.text.to_lowercase());
    Ok(admission.text.to_uppercase()) // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn discarded(ctx: &DecodeContext, text: &str) -> Result<String, ()> {
    let admission = ctx.unicode_case_admission(text)?;
    drop(admission._workspace);
    Ok(admission.text.to_uppercase()) // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn changed(ctx: &DecodeContext, text: &str, other: &str) -> Result<String, ()> {
    let mut admission = ctx.unicode_case_admission(text)?;
    admission.text = other;
    Ok(admission.text.to_lowercase()) // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn forged(_ctx: &DecodeContext, text: &str) -> String {
    let admission = UnicodeCaseAdmission { text, _workspace: ScopedReservation };
    admission.text.to_lowercase() // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn child_fill(ctx: &DecodeContext, values: &mut [String], value: &String) -> Result<(), ()> {
    ctx.admit_moves(values, 1, "fill")?;
    let value = value.clone(); // finding: uncharged_decode_allocation, uncharged_decode_work
    values.fill(value); // finding: uncharged_decode_work
    Ok(())
}
