// SPDX-License-Identifier: Apache-2.0

use super::{base, positive, BaseVersion, PrimaryRecords};
use crate::records::group::GroupSelector;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) struct PrimaryGroup {
    pub(crate) xmt: u32,
    pub(crate) node_id: u32,
    pub(crate) references: [u32; 5],
    pub(crate) selector: GroupSelector,
    pub(crate) offset: usize,
    pub(crate) end: usize,
}

impl PrimaryRecords<'_> {
    pub(crate) fn groups(&self, ctx: &DecodeContext<'_>) -> Result<Vec<PrimaryGroup>, CodecError> {
        ctx.collect_retained_vec(
            self.records.iter().filter_map(|record| {
                if record.definition.kind != 90 {
                    return None;
                }
                let expected = base::fields(BaseVersion::V13006, 90)?;
                if record.definition.fields.len() != expected.len()
                    || !record
                        .definition
                        .fields
                        .iter()
                        .zip(expected)
                        .all(|(actual, base)| {
                            actual.name.as_str() == base.name
                                && actual.data_type == base.data_type
                                && actual.pointer_class == base.pointer_class
                                && actual.n_elements == base.n_elements
                                && actual.transmit == base.transmit
                        })
                {
                    return None;
                }
                let field = |index: usize| {
                    Some(View::over_retained(
                        self.source.get(record.fields.get(index)?.clone())?,
                    ))
                };
                let pointer =
                    |index| positive(&mut field(index)?).and_then(|value| value.checked_add(1));
                Some(PrimaryGroup {
                    xmt: u32::from(record.xmt),
                    node_id: field(0)?.u32_be()?,
                    references: [
                        pointer(1)?,
                        pointer(2)?,
                        pointer(3)?,
                        pointer(4)?,
                        pointer(6)?,
                    ],
                    selector: GroupSelector::try_from(field(5)?.u8()?).ok()?,
                    offset: record.range.start,
                    end: record.range.end,
                })
            }),
            "NX primary GROUP projections",
        )
    }
}
