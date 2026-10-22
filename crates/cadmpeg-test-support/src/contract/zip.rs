// SPDX-License-Identifier: Apache-2.0
//! Source-addressed ZIP and embedded-document framing for contract checks.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};

use crate::framing::freecad::{frame_xml_member, XmlFraming};
use crate::framing::zip::{scan, ZipScan};
use crate::framing::{step, FramedPart, PartTable};

/// Physical archive observations and reached XML boundaries.
#[derive(Debug)]
pub struct ArchiveFraming<'a> {
    scan: ZipScan<'a>,
    xml: Vec<XmlFraming<'a>>,
}
impl<'a> ArchiveFraming<'a> {
    /// Complete-directory members or complete local members and their exact stop.
    pub fn scan(&self) -> &ZipScan<'a> {
        &self.scan
    }

    /// Lexical XML observations, in the corresponding member traversal order.
    pub fn xml(&self) -> &[XmlFraming<'a>] {
        &self.xml
    }
}

/// Frame an archive and its exact stored STEP and FreeCAD XML root members.
///
/// Complete archives retain central-directory authority. Incomplete archives
/// use only the complete local members proved by the recovering ZIP scanner.
/// A cut-short payload remains a ZIP stop, never a complete embedded document.
/// Other entry names remain in the returned ZIP inventory without interpretation.
/// No semantic decoder or source rebuilder runs here.
///
/// # Errors
/// Returns a framing, provenance or budget error. Recognized members requiring
/// decryption or decompression are refused until an actual opening supplies a
/// provenance-bearing expanded view.
pub fn frame_stored_members<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<ArchiveFraming<'a>, String> {
    let scan = scan(ctx, source, out)?;
    let mut xml = Vec::new();
    match &scan {
        ZipScan::Complete { entries } => {
            for entry in entries {
                member(
                    ctx,
                    entry.name(),
                    entry.payload(),
                    entry.frame_index(),
                    (entry.method(), entry.flags()),
                    out,
                    &mut xml,
                )?;
            }
        }
        ZipScan::Local { entries, .. } => {
            for entry in entries {
                member(
                    ctx,
                    entry.name(),
                    entry.payload(),
                    entry.frame_index(),
                    (entry.method(), entry.flags()),
                    out,
                    &mut xml,
                )?;
            }
        }
    }
    PartTable::load()?.validate(ctx, out)?;
    Ok(ArchiveFraming { scan, xml })
}

fn member<'a>(
    ctx: &DecodeContext<'_>,
    name: View<'a>,
    payload: View<'a>,
    parent: usize,
    (method, flags): (u16, u16),
    out: &mut Vec<FramedPart<'a>>,
    xml: &mut Vec<XmlFraming<'a>>,
) -> Result<(), String> {
    if !matches!(
        name.window(),
        b"ISO-10303.p21" | b"Document.xml" | b"GuiDocument.xml"
    ) {
        return Ok(());
    }
    if method != 0 || flags & (1 | 64) != 0 {
        return Err("contract member framing requires an opened unencrypted stored member".into());
    }
    if name.window() == b"ISO-10303.p21" {
        step::frame_zip_root(ctx, payload, name, parent, out)?;
    } else {
        let framing = frame_xml_member(ctx, payload, name, parent, out)?;
        ctx.retain_bytes(
            u64_from_index(size_of::<XmlFraming<'_>>()),
            "contract XML inventories",
        )
        .map_err(|error| error.to_string())?;
        xml.try_reserve(1).map_err(|error| error.to_string())?;
        xml.push(framing);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
