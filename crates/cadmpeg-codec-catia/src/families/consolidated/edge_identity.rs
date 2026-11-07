// SPDX-License-Identifier: Apache-2.0
//! Serialized CGM edge and endpoint allocation identities.

use super::allocation::AllocationJournal;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::ConsolidatedFamily;

pub(crate) fn source_tag(
    source: Option<&cadmpeg_ir::SourceObjectAssociation>,
    kind: &str,
) -> Option<u64> {
    let source = source?;
    let prefix = format!("cgm-{kind}:");
    u64::from_str_radix(source.object_id.as_str().strip_prefix(&prefix)?, 16).ok()
}

pub(crate) fn endpoint_address(
    journal: &AllocationJournal<'_>,
    origin: usize,
    value: u32,
    encoding: AllocationReferenceEncoding,
) -> Option<u64> {
    let base = journal.address(origin)?;
    let address = match encoding {
        AllocationReferenceEncoding::BackwardDistance | AllocationReferenceEncoding::WidthCoded => {
            base.checked_sub(u64::from(value))?
        }
        AllocationReferenceEncoding::OwnedChild
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16 => {
            base.checked_add(u64::from(value))?.checked_add(1)?
        }
        AllocationReferenceEncoding::Selector2 => return None,
    };
    let record = journal.resolve_address(origin, address)?;
    if record.family() != ConsolidatedFamily::B || record.class() != 0x5d {
        return None;
    }
    let index = journal.index(record.source_index(), record.source_range().start)?;
    journal.absolute_address(index)
}
