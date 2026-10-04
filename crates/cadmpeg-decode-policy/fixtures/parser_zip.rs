// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub struct ScopedReservation;
pub struct ZipParserAdmission<'a> {
    bytes: &'a [u8],
    workspace: ScopedReservation,
}
fn zip_parser_admission<'a>(
    _ctx: &DecodeContext,
    bytes: &'a [u8],
) -> Result<ZipParserAdmission<'a>, ()> {
    Ok(ZipParserAdmission {
        bytes,
        workspace: ScopedReservation,
    })
}
pub fn admitted(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    let admission = zip_parser_admission(ctx, bytes)?;
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes)));
    Ok(())
}
pub fn wrong_source(ctx: &DecodeContext, bytes: &[u8], other: &[u8]) -> Result<(), ()> {
    let _admission = zip_parser_admission(ctx, bytes)?;
    drop(zip::ZipArchive::new(std::io::Cursor::new(other))); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn reused(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    let admission = zip_parser_admission(ctx, bytes)?;
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes)));
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes))); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn released(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    let admission = zip_parser_admission(ctx, bytes)?;
    drop(admission.workspace);
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes))); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn forged(_ctx: &DecodeContext, bytes: &[u8]) {
    let admission = ZipParserAdmission {
        bytes,
        workspace: ScopedReservation,
    };
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes))); // finding: uncharged_decode_allocation, uncharged_decode_work
}
pub fn wrong_reader<R: std::io::Read + std::io::Seek>(
    ctx: &DecodeContext,
    bytes: &[u8],
    reader: R,
) -> Result<(), ()> {
    let _admission = zip_parser_admission(ctx, bytes)?;
    drop(zip::ZipArchive::new(reader)); // finding: unproven_decode_charge
    Ok(())
}
pub fn changed_input(ctx: &DecodeContext, bytes: &[u8], other: &[u8]) -> Result<(), ()> {
    let mut admission = zip_parser_admission(ctx, bytes)?;
    admission.bytes = other;
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes))); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn transferred(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    let admission = zip_parser_admission(ctx, bytes)?;
    consume(admission.workspace);
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes))); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
fn consume(_workspace: ScopedReservation) {}
pub fn not_propagated(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    let admission = match zip_parser_admission(ctx, bytes) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes))); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn expired(ctx: &DecodeContext, bytes: &[u8]) -> Result<(), ()> {
    let admission = {
        let admission = zip_parser_admission(ctx, bytes)?;
        admission
    };
    drop(zip::ZipArchive::new(std::io::Cursor::new(admission.bytes))); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
