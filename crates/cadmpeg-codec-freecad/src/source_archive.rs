// SPDX-License-Identifier: Apache-2.0
//! Original archive access for source processing, never semantic synthesis.

use std::collections::BTreeMap;

mod part_integrity;
mod property_integrity;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::source_fidelity::RecordLayout;
use cadmpeg_ir::SourceFidelity;

/// Original physical input; expanded member copies are not retained.
pub(crate) fn image(fidelity: &SourceFidelity) -> Result<&[u8], CodecError> {
    let record = original_record(fidelity)?;
    let bytes = record
        .data()
        .ok_or_else(|| CodecError::malformed("original FCStd archive has no bytes"))?;
    if record.byte_len() != cadmpeg_core::decode::u64_from_index(bytes.len())
        || record.sha256() != cadmpeg_ir::hash::sha256_hex(bytes)
    {
        return Err(CodecError::malformed(
            "original FCStd archive evidence disagrees",
        ));
    }
    Ok(bytes)
}

fn original_record(
    fidelity: &SourceFidelity,
) -> Result<&cadmpeg_ir::source_fidelity::RetainedSourceRecord, CodecError> {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let mut failed = fidelity.parts().iter().filter(|(_, part)| {
        part.parent().is_none()
            && part.level().as_str() == "zip.archive"
            && part.state() == &PartState::GivenUp {}
    });
    let root = failed.next();
    if failed.next().is_some() {
        return Err(CodecError::malformed(
            "original FCStd archive has multiple failed root owners",
        ));
    }
    match (
        fidelity.retained_record(crate::fidelity::SOURCE_IMAGE),
        root,
    ) {
        (Some(record), None) => Ok(record),
        (None, Some((id, part))) => {
            let record = fidelity
                .retained_record(id.as_str())
                .ok_or_else(|| CodecError::malformed("failed archive has no retained source"))?;
            if record.stream() != part.stream()
                || record.offset() != part.offset()
                || record.byte_len() != part.byte_len()
            {
                return Err(CodecError::malformed(
                    "failed archive source disagrees with its actual part",
                ));
            }
            Ok(record)
        }
        (Some(_), Some(_)) => Err(CodecError::malformed(
            "original FCStd archive has competing source images",
        )),
        (None, None) => Err(CodecError::malformed(
            "original FCStd archive is unavailable",
        )),
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn entry_pointers(
    entry: &cadmpeg_container::EntryRecord,
    byte_len: u64,
) -> Result<BTreeMap<String, u64>, CodecError> {
    let declaration = entry.declaration_range();
    let stored = entry.stored_range()?;
    Ok(BTreeMap::from([
        ("byte_len".into(), byte_len),
        ("archive.data_start".into(), stored.start),
        ("archive.data_end".into(), stored.end),
        ("archive.declaration_start".into(), declaration.start),
        ("archive.declaration_end".into(), declaration.end),
    ]))
}

/// Reopens named members through the genuine original ZIP and decode budget.
pub(crate) struct OriginalArchive<'source, 'fidelity> {
    archive: ArchiveSnapshot<'source>,
    fidelity: &'fidelity SourceFidelity,
}

impl<'source, 'fidelity> OriginalArchive<'source, 'fidelity> {
    pub(crate) fn new(
        ctx: &DecodeContext<'source>,
        root: View<'source>,
        fidelity: &'fidelity SourceFidelity,
    ) -> Result<Self, CodecError> {
        if root.window() != image(fidelity)? {
            return Err(CodecError::malformed(
                "source processing input differs from original archive",
            ));
        }
        let record = original_record(fidelity)?;
        let actual = ctx.capture_part_extent(root)?;
        if actual.offset() != record.offset() || actual.byte_len() != record.byte_len() {
            return Err(CodecError::UnsupportedContent(
                "original FCStd archive needs its actual source origin for layout reuse".into(),
            ));
        }
        let root = if actual.stream() == record.stream() {
            root
        } else {
            ctx.name_stream(root, record.stream())?
        };
        let archive = ArchiveSnapshot::new(ctx, root)?;
        part_integrity::validate(ctx, root, &archive, fidelity)?;
        property_integrity::validate(ctx, &archive, fidelity)?;
        Ok(Self { archive, fidelity })
    }

    #[cfg(test)]
    pub(crate) fn part(
        &self,
        ctx: &DecodeContext<'source>,
        id: &str,
        name: &str,
    ) -> Result<&'fidelity [u8], CodecError> {
        part_integrity::named(ctx, &self.archive, self.fidelity, id, name)
    }

    pub(crate) fn open(
        &self,
        ctx: &DecodeContext<'source>,
        name: &str,
    ) -> Result<View<'source>, CodecError> {
        let entry = self.archive.entry(name).ok_or_else(|| {
            CodecError::malformed(format_args!("original ZIP member {name} is absent"))
        })?;
        let layout = self
            .fidelity
            .record_layout()
            .get(&crate::native::native_id("entry", name))
            .ok_or_else(|| CodecError::malformed("original ZIP member has no source layout"))?;
        if entry_pointers(entry, layout.byte_len())? != *layout.pointers() {
            return Err(CodecError::malformed(
                "original ZIP member disagrees with archive source ranges",
            ));
        }
        let view = self.archive.open(ctx, name)?;
        let capture = ctx.capture_part_extent(view)?;
        if capture.stream() != layout.stream()
            || capture.offset() != layout.offset()
            || capture.byte_len() != layout.byte_len()
        {
            return Err(CodecError::malformed(
                "original ZIP member disagrees with source layout",
            ));
        }
        Ok(view)
    }

    #[cfg(test)]
    pub(crate) fn stream(
        &self,
        ctx: &DecodeContext<'source>,
        layout: &RecordLayout,
    ) -> Result<View<'source>, CodecError> {
        for entry in self.archive.entries() {
            let id = crate::native::native_id("entry", &entry.name);
            if self
                .fidelity
                .record_layout()
                .get(&id)
                .is_some_and(|candidate| candidate.stream() == layout.stream())
            {
                return self.open(ctx, &entry.name);
            }
        }
        Err(CodecError::malformed(
            "source layout has no actual ZIP member stream",
        ))
    }
}
