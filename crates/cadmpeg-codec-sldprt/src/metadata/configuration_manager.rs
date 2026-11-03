// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
use cadmpeg_ir::report::loss::LossNote;

use crate::container::Section;
use crate::layout::configuration_manager_scalar_prefix as prefix;

const TOKEN: &[u8] = b"moConfigurationMgr_c";
const MIN_FILETIME: u64 = 124_333_920_000_000_000;
const MAX_FILETIME: u64 = 157_469_184_000_000_000;

pub(crate) fn values(payload: &[u8], offset: usize) -> Option<[i64; 3]> {
    let start = offset.checked_add(TOKEN.len())?;
    (payload.get(offset..start)? == TOKEN).then_some(())?;
    let body = payload.get(start..start.checked_add(prefix::LEN)?)?;
    let minor = View::u32_le_at(body, prefix::MINOR_VERSION)?;
    let states = *body.get(prefix::STATE_COUNT)?;
    let filetime = View::u64_le_at(body, prefix::FILETIME)?;
    (minor < 0x10000 && (MIN_FILETIME..MAX_FILETIME).contains(&filetime)).then_some(())?;
    Some([
        i64::from(minor),
        i64::from(states),
        i64::try_from(filetime).ok()?,
    ])
}

pub(super) fn append(
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let payload = section.payload();
    for (offset, bytes) in payload.windows(TOKEN.len()).enumerate() {
        if bytes != TOKEN {
            continue;
        }
        let Some(values) = values(payload, offset) else {
            continue;
        };
        out.push(super::attribute(
            section,
            offset,
            &cadmpeg_ir::identity_component!("configuration_manager"),
            TOKEN,
            values.into_iter().map(AttributeValue::Integer).collect(),
            annotations,
        )?);
    }
    if super::part::declaration(payload, 0).is_some_and(|(_, name, body)| {
        name == TOKEN && values(payload, body - TOKEN.len()).is_none()
    }) {
        losses.push(crate::loss::SldprtLossCode::MetadataConfigurationManagerOpaque.note(
            "Configuration-manager fields and serializer bounds are unresolved; retained bytes are preserved without editable scalar metadata.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
