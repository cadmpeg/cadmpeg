// SPDX-License-Identifier: Apache-2.0
//! Parameter locator admission for the owning source Directory callback.

use std::ops::Range;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::card::{CardScan, Section};
use crate::directory::DirectoryEntry;

use super::{back_pointer, declared_range, DeclaredRange, ParameterDefect};

pub(crate) struct SourceLocator {
    census: Range<u32>,
    owners: Vec<Option<u32>>,
}

impl SourceLocator {
    pub(crate) fn new(scan: &CardScan<'_>, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let count = scan.section(Section::Parameter).count();
        let mut owners = ctx.alloc_filled(count, None, "IGES Parameter locator census")?;
        let mut bounds: Option<(u32, u32)> = None;
        for ((sequence, line), owner) in scan.section(Section::Parameter).zip(&mut owners) {
            *owner = back_pointer(line);
            bounds = Some(match bounds {
                Some((first, last)) => (first.min(sequence), last.max(sequence)),
                None => (sequence, sequence),
            });
        }
        owners.sort_unstable();
        let census = match bounds {
            Some((first, last)) => {
                first..last.checked_add(1).ok_or_else(|| {
                    CodecError::malformed("IGES Parameter locator census sequence overflows")
                })?
            }
            None => 0..0,
        };
        Ok(Self { census, owners })
    }

    pub(crate) fn admit(&self, entry: &DirectoryEntry) -> Result<(), CodecError> {
        if entry.entity_type == 0 && entry.parameter_line_count == 0 {
            return Ok(());
        }
        let declaration = declared_range(entry, &self.census);
        if matches!(declaration, DeclaredRange::Usable(_))
            || self.owners.binary_search(&Some(entry.sequence)).is_ok()
        {
            return Ok(());
        }
        let defect = if entry.parameter_line_count == 0 {
            ParameterDefect::DeclaredCountZero
        } else if matches!(declaration, DeclaredRange::CardMissing) {
            ParameterDefect::DeclaredCardMissing
        } else {
            ParameterDefect::NoOwnedCards
        };
        Err(CodecError::malformed(format!(
            "Directory D{} cannot locate Parameter Data through its declared range or the source back-pointer census: {}",
            entry.sequence, defect.describe(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::SourceLocator;

    #[test]
    fn null_entities_and_actual_source_locators_use_the_existing_ownership_rules() {
        let bytes = crate::test_support::test_curves_and_surfaces::point_file();
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("source session");
        let scan = crate::card::scan_source(source, &ctx).expect("actual card census");
        let locator = SourceLocator::new(&scan, &ctx).expect("bounded locator census");
        let mut entry = crate::test_support::directory_target(99, 0);
        entry.parameter_start = 0;
        entry.parameter_line_count = 0;
        assert!(locator.admit(&entry).is_ok());
        entry.entity_type = 116;
        assert!(locator.admit(&entry).is_err());
        entry.parameter_line_count = 1;
        assert!(locator.admit(&entry).is_err());
        entry.sequence = 1;
        assert!(locator.admit(&entry).is_ok());
        entry.sequence = 99;
        entry.parameter_start = 1;
        assert!(locator.admit(&entry).is_ok());
        entry.parameter_line_count = 2;
        assert!(locator.admit(&entry).is_err());
    }
}
