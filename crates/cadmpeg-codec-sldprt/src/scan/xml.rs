// SPDX-License-Identifier: Apache-2.0
//! Keywords XML listing with shared tree admission.

use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::scan::{DesignScan, FeatureCensus, ScanBudget, StopReason, Suppressed};

fn malformed() -> StopReason {
    StopReason::Malformed("SolidWorks history XML framing".into())
}

pub(super) fn census(
    source: View<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
) -> Result<bool, StopReason> {
    let mut source = source;
    if source.window().starts_with(&[0x86]) {
        source.req_u8()?;
    }
    let mut converted = String::new();
    let bytes = if source.unread().starts_with(&[0xff, 0xfe]) {
        source.req_take(2)?;
        if !source.remaining().is_multiple_of(2) {
            return Err(StopReason::Truncated("SolidWorks UTF16 unit".into()));
        }
        let unit_count = source.remaining() / 2;
        let utf8_capacity = unit_count.checked_mul(3).ok_or(StopReason::RetainedBytes)?;
        budget.retain(
            u64_from_index(unit_count)
                .checked_mul(5)
                .ok_or(StopReason::RetainedBytes)?,
        )?;
        let mut units = Vec::new();
        units
            .try_reserve_exact(unit_count)
            .map_err(|_| StopReason::RetainedBytes)?;
        converted
            .try_reserve_exact(utf8_capacity)
            .map_err(|_| StopReason::RetainedBytes)?;
        while source.remaining() != 0 {
            budget.check()?;
            units.push(source.req_u16_le()?);
        }
        for ch in char::decode_utf16(units) {
            budget.check()?;
            converted.push(ch.map_err(|_| malformed())?);
        }
        converted.as_bytes()
    } else {
        source
            .unread()
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(source.unread())
    };
    let text = std::str::from_utf8(bytes).map_err(|_| malformed())?;
    let document = budget.parse_xml(text)?;
    let history = document
        .root_element()
        .tag_name()
        .name()
        .contains("Keywords");
    if !history {
        return Ok(false);
    }
    scan.census.get_or_insert_with(FeatureCensus::default);
    let applications = document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "Application")
        .collect::<Vec<_>>();
    for node in document.descendants().filter(roxmltree::Node::is_element) {
        budget.check()?;
        let name = node.tag_name().name();
        if matches!(name, "Keywords" | "Configuration" | "Dimension")
            || node
                .ancestors()
                .any(|ancestor| applications.contains(&ancestor))
        {
            continue;
        }
        let visit = budget.feature()?;
        let suppressed = match node.attribute("Suppressed") {
            Some("1" | "true" | "True") => Suppressed::Yes,
            Some("0" | "false" | "False") => Suppressed::No,
            _ => Suppressed::Unknown,
        };
        scan.record_feature(visit, node.attribute("Type").unwrap_or(name), suppressed)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
