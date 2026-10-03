// SPDX-License-Identifier: Apache-2.0
use std::io::{Read, Seek, SeekFrom};
pub struct DecodeContext;
impl DecodeContext {
    fn charge_work(&self, _: u64, _: &str) -> Result<(), ()> { Ok(()) }
    fn read_input<R: Read + ?Sized>(&self, reader: &mut R, bytes: &mut [u8]) -> Result<(), ()> {
        self.charge_work(bytes.len() as u64, "read")?;
        drop(reader.read(bytes));
        Ok(())
    }
}
pub trait ReadSeek: Read + Seek {}
impl<R: Read + Seek + ?Sized> ReadSeek for R {}
pub fn root<R: ReadSeek + ?Sized>(ctx: &DecodeContext, reader: &mut R, bytes: &mut [u8]) -> Result<(), ()> {
    drop(reader.seek(SeekFrom::Start(0)));
    drop(reader.rewind());
    ctx.read_input(reader, bytes)
}
pub fn raw_extent<R: Read + ?Sized>(_ctx: &DecodeContext, reader: &mut R, bytes: &mut [u8]) {
    drop(reader.read(bytes)); // finding: uncharged_decode_work
}
pub struct Checked;
impl Read for Checked {
    fn read(&mut self, _bytes: &mut [u8]) -> std::io::Result<usize> { Ok(0) }
}
impl Seek for Checked {
    fn seek(&mut self, _position: SeekFrom) -> std::io::Result<u64> { Ok(0) }
}
pub struct Scanning;
impl Read for Scanning {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let copied = bytes.to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
        Ok(copied.len())
    }
}
pub struct ScanningRewind { bytes: Vec<u8> }
impl Read for ScanningRewind {
    fn read(&mut self, _bytes: &mut [u8]) -> std::io::Result<usize> { Ok(0) }
}
impl Seek for ScanningRewind {
    fn seek(&mut self, _position: SeekFrom) -> std::io::Result<u64> { Ok(0) }
    fn rewind(&mut self) -> std::io::Result<()> {
        let copied = self.bytes.to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
        drop(copied);
        Ok(())
    }
}
pub struct CustomBytes;
impl AsRef<[u8]> for CustomBytes {
    fn as_ref(&self) -> &[u8] { &[] }
}
pub fn callers(vector: &Vec<u8>, ctx: &DecodeContext, file: &mut std::fs::File, bytes: &mut [u8], opaque: &mut dyn Read, opaque_seek: &mut dyn ReadSeek, scanning_rewind: &mut ScanningRewind) -> Result<(), ()> {
    ctx.read_input(&mut std::io::Cursor::new(vector), bytes)?;
    root(ctx, &mut std::io::Cursor::new(vector), bytes)?;
    ctx.read_input(file, bytes)?;
    root(ctx, file, bytes)?;
    ctx.read_input(&mut std::io::Cursor::new(b"fixed"), bytes)?;
    root(ctx, &mut std::io::Cursor::new(b"fixed"), bytes)?;
    ctx.read_input(&mut Checked, bytes)?;
    root(ctx, &mut Checked, bytes)?;
    root(ctx, scanning_rewind, bytes)?;
    ctx.read_input(&mut Scanning, bytes)?;
    ctx.read_input(opaque, bytes)?; // finding: unproven_decode_charge
    root(ctx, opaque_seek, bytes)?; // finding: unproven_decode_charge
    ctx.read_input(&mut std::io::Cursor::new(CustomBytes), bytes)?; // finding: unproven_decode_charge
    Ok(())
}
