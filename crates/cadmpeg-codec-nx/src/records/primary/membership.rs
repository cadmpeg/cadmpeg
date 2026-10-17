// SPDX-License-Identifier: Apache-2.0

use super::{DataType, PrimaryRecord, PrimaryRecords};
use crate::records::group::{GroupSelector, MembershipKind, MembershipRecord};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

impl PrimaryRecords<'_> {
    pub(crate) fn membership_records(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<MembershipRecord>, CodecError> {
        ctx.charge_work(
            u64_from_index(self.records.len()),
            "NX primary GROUP membership projection",
        )?;
        ctx.collect_retained_vec(
            self.records.iter().map(|record| MembershipRecord {
                xmt: record.xmt,
                kind: self
                    .membership_kind(record)
                    .unwrap_or(MembershipKind::Other),
            }),
            "NX primary GROUP membership records",
        )
    }

    fn membership_kind(&self, record: &PrimaryRecord) -> Option<MembershipKind> {
        match record.definition.kind {
            90 if self.matches_base(record) => Some(MembershipKind::Group {
                node_id: self.field(record, 0)?.u32_be()?,
                selector: GroupSelector::try_from(self.field(record, 5)?.u8()?).ok()?,
                first_member: self.pointer(record, 6)?,
            }),
            91 if self.matches_base(record) => Some(MembershipKind::Member {
                owning_group: self.pointer(record, 1)?,
                owner: self.pointer(record, 2)?,
                next_member: self.pointer(record, 5)?,
                previous_member: self.pointer(record, 6)?,
            }),
            kind => {
                let first = record.definition.fields.first()?;
                if first.name.as_str() != "node_id"
                    || first.data_type != DataType::Integer
                    || first.n_elements != 0
                    || !first.transmit
                {
                    return None;
                }
                Some(MembershipKind::Node {
                    kind,
                    node_id: self.field(record, 0)?.u32_be()?,
                })
            }
        }
    }
}
