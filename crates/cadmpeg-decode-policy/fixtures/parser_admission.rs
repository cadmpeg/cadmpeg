// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub struct ScopedReservation;
pub struct XmlParserAdmission<'a> {
    text: &'a str,
    reservation: ScopedReservation,
}
impl DecodeContext {
    pub fn xml_parser_admission<'a>(&self, text: &'a str) -> Result<XmlParserAdmission<'a>, ()> {
        Ok(XmlParserAdmission { text, reservation: ScopedReservation })
    }
}
pub fn admitted<'a>(ctx: &DecodeContext, text: &'a str) -> Result<roxmltree::Document<'a>, ()> {
    let admission = ctx.xml_parser_admission(text)?;
    roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default()).map_err(|_| ())
}
pub fn wrong_source(ctx: &DecodeContext, text: &str, other: &str) -> Result<(), ()> {
    let _admission = ctx.xml_parser_admission(text)?;
    drop(roxmltree::Document::parse_with_options(other, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn reused(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.xml_parser_admission(text)?;
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default()));
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn released_storage(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.xml_parser_admission(text)?;
    drop(admission.reservation);
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn forged(_ctx: &DecodeContext, text: &str) {
    let admission = XmlParserAdmission { text, reservation: ScopedReservation };
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn transferred(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.xml_parser_admission(text)?;
    consume(admission.reservation);
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
fn consume(_storage: ScopedReservation) {}
pub fn changed_input(ctx: &DecodeContext, text: &str, other: &str) -> Result<(), ()> {
    let mut admission = ctx.xml_parser_admission(text)?;
    admission.text = other;
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn expired_scope(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = { let admission = ctx.xml_parser_admission(text)?; admission };
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn loop_reuse(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.xml_parser_admission(text)?;
    for _ in 0..2 {
        drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    }
    Ok(())
}
pub fn fresh_each_iteration(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    for _ in 0..2 {
        let admission = ctx.xml_parser_admission(text)?;
        drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default()));
    }
    Ok(())
}
pub fn branch_reuse(ctx: &DecodeContext, text: &str, condition: bool) -> Result<(), ()> {
    let admission = ctx.xml_parser_admission(text)?;
    if condition { drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); }
    drop(roxmltree::Document::parse_with_options(admission.text, roxmltree::ParsingOptions::default())); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
