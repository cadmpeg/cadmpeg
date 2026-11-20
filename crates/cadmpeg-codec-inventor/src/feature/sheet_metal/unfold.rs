// SPDX-License-Identifier: Apache-2.0
//! `PmApp` unfold-rule records and the active unfold-rule selection.

use cadmpeg_core::decode::{bounded_len, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

use crate::pmdc::{Cursor, PmDcReference};
use crate::record_identity::{Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

pub(crate) const RULE_TYPE: [u8; 16] = [
    0x3e, 0x0b, 0x6b, 0xda, 0xf5, 0x41, 0x7a, 0x64, 0x8d, 0xa9, 0x1f, 0xbd, 0x09, 0x3f, 0xe2, 0x13,
];
pub(crate) const SELECTION_TYPE: [u8; 16] = [
    0x20, 0xd0, 0xa6, 0xfd, 0xd2, 0x11, 0x8f, 0x93, 0x60, 0x00, 0xa1, 0xb7, 0x0b, 0x02, 0xec, 0xb0,
];
/// Style category of the unfold-rule selection.
const UNFOLD_CATEGORY: &[u8] = b"SheetMetalUnfold";
/// Unit suffix of a unitless expression text.
const UNITLESS_SUFFIX: &str = " ul";
/// First `PmApp` segment major version with the unfold-rule layout.
const FIRST_SUPPORTED_MAJOR: u8 = 17;
/// Last `PmApp` segment major version without the style-prefix padding word.
const LAST_UNPADDED_MAJOR: u8 = 23;

pub(crate) type Rule = Located<RulePayload>;
pub(crate) type Selection = Located<SelectionPayload>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RulePayload {
    pub(crate) save_version_major: u8,
    header_value: u32,
    header_id: u16,
    state: u8,
    flags: u16,
    values: [u16; 2],
    default_state: u32,
    value: u32,
    name_reference: PmDcReference,
    pub(crate) name: String,
    long_name: String,
    style_state: u16,
    key: String,
    form: u32,
    pub(crate) expression: String,
    display: String,
    suffix: NativeBytes<Vec<u8>>,
}

impl RecordPayload for RulePayload {
    const KIND: &'static str = "sheet-metal-unfold-rule";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SelectionEntry {
    pub(crate) name: String,
    pub(crate) rule: PmDcReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SelectionPayload {
    header_value: u32,
    header_id: u16,
    value: u32,
    reference: PmDcReference,
    form: u32,
    entry_values: Option<[u32; 2]>,
    pub(crate) entries: Vec<SelectionEntry>,
    suffix: NativeBytes<Vec<u8>>,
}

impl RecordPayload for SelectionPayload {
    const KIND: &'static str = "sheet-metal-unfold-selection";
}

fn retain(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<NativeBytes<Vec<u8>>, CodecError> {
    ctx.retain_bytes(
        u64_from_index(bytes.len()),
        "retain Inventor unfold-rule fields",
    )?;
    let mut copy = ctx.alloc_filled(bytes.len(), 0, "copy Inventor unfold-rule fields")?;
    copy.copy_from_slice(bytes);
    Ok(NativeBytes::new(copy))
}

fn suffix(ctx: &DecodeContext<'_>, cursor: Cursor<'_>) -> Result<NativeBytes<Vec<u8>>, CodecError> {
    let mut tail = cursor.into_view();
    let remaining = tail.remaining();
    retain(
        ctx,
        crate::reader::take(&mut tail, remaining, "unfold-rule suffix")?,
    )
}

/// Whether a `PmApp` segment major version has the unfold-rule layout.
pub(crate) const fn version_supported(version: u8) -> bool {
    version >= FIRST_SUPPORTED_MAJOR
}

pub(crate) fn parse_rule(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<RulePayload, CodecError> {
    if !version_supported(version) {
        return Err(CodecError::malformed(
            "unsupported Inventor unfold-rule segment version",
        ));
    }
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("unfold-rule header value")?;
    let header_id = cursor.u16("unfold-rule header id")?;
    let state = cursor.u8("unfold-rule state")?;
    let flags = cursor.u16("unfold-rule flags")?;
    if version > LAST_UNPADDED_MAJOR && cursor.u16("unfold-rule padding")? != 0 {
        return Err(CodecError::malformed(
            "Inventor unfold-rule padding is not zero",
        ));
    }
    let values = [
        cursor.u16("unfold-rule value 0")?,
        cursor.u16("unfold-rule value 1")?,
    ];
    let default_state = cursor.u32("unfold-rule default state")?;
    let value = cursor.u32("unfold-rule value")?;
    let name_reference = cursor.reference("unfold-rule name reference")?;
    let name = cursor.utf16(ctx, "unfold-rule name")?;
    let long_name = cursor.utf16(ctx, "unfold-rule long name")?;
    let style_state = cursor.u16("unfold-rule style state")?;
    let key = cursor.utf16(ctx, "unfold-rule key")?;
    let form = cursor.u32("unfold-rule form")?;
    let expression = cursor.utf16(ctx, "unfold-rule expression")?;
    let display = cursor.utf16(ctx, "unfold-rule display text")?;
    Ok(RulePayload {
        save_version_major: version,
        header_value,
        header_id,
        state,
        flags,
        values,
        default_state,
        value,
        name_reference,
        name,
        long_name,
        style_state,
        key,
        form,
        expression,
        display,
        suffix: suffix(ctx, cursor)?,
    })
}

/// The selection payload of a style-category record, when its category is
/// the unfold-rule category.
pub(crate) fn parse_selection(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
) -> Result<Option<SelectionPayload>, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("style-category header value")?;
    let header_id = cursor.u16("style-category header id")?;
    let count = cursor.u32("style-category name length")?;
    let mut view = cursor.into_view();
    let len = bounded_len(u64::from(count), 1, view.remaining())
        .ok_or_else(|| CodecError::malformed("Inventor style-category name exceeds record"))?;
    if crate::reader::take(&mut view, len, "style-category name")? != UNFOLD_CATEGORY {
        return Ok(None);
    }
    let mut cursor = Cursor::new(view);
    let value = cursor.u32("style-category value")?;
    let reference = cursor.reference("style-category reference")?;
    let form = cursor.u32("style-category form")?;
    let count = cursor.u32("style-category entry count")?;
    let entry_values = (count > 0)
        .then(|| cursor.u32_array::<2>("style-category entry values"))
        .transpose()?;
    let count = cursor
        .view()
        .counted(u64::from(count), 8)
        .ok_or_else(|| CodecError::malformed("Inventor style-category entries exceed record"))?;
    let mut entries = Vec::new();
    for _ in 0..count.get() {
        ctx.charge_entities(1, "admit Inventor style-category entry")?;
        let name = cursor.utf16(ctx, "style-category entry name")?;
        let rule = cursor.reference("style-category entry reference")?;
        entries.push(SelectionEntry { name, rule });
    }
    Ok(Some(SelectionPayload {
        header_value,
        header_id,
        value,
        reference,
        form,
        entry_values,
        entries,
        suffix: suffix(ctx, cursor)?,
    }))
}

/// Admit the unfold-rule and unfold-selection records of every `PmApp`
/// segment.
pub(crate) fn admit(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
    rules: &mut Vec<Rule>,
    selections: &mut Vec<Selection>,
    issues: &mut Vec<RecordIssue>,
) -> Result<(), CodecError> {
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmApp {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let token = segment.pair.token.key();
        for record in &table.records {
            let parsed = match record.type_id {
                RULE_TYPE if version_supported(version) => parse_rule(ctx, record.payload, version)
                    .and_then(|rule| {
                        crate::record_identity::push_record(
                            ctx,
                            rules,
                            rule,
                            record.type_id,
                            token,
                            record.ordinal,
                            "admit Inventor unfold rule",
                        )
                    }),
                SELECTION_TYPE => parse_selection(ctx, record.payload).and_then(|selection| {
                    selection.map_or(Ok(()), |selection| {
                        crate::record_identity::push_record(
                            ctx,
                            selections,
                            selection,
                            record.type_id,
                            token,
                            record.ordinal,
                            "admit Inventor unfold-rule selection",
                        )
                    })
                }),
                _ => continue,
            };
            if let Err(error) = parsed {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_entities(1, "admit Inventor unfold-rule issue")?;
                issues.push(RecordIssue {
                    family: RecordIssueFamily::Feature {
                        type_id: crate::record_identity::RecordTypeId::from_bytes(record.type_id),
                    },
                    segment_token: token.clone(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(())
}

/// The K-factor of an unfold-rule expression text: a decimal with an
/// optional unitless suffix.
pub(crate) fn factor(text: &str) -> Option<FiniteReal> {
    let number = text.strip_suffix(UNITLESS_SUFFIX).unwrap_or(text);
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
        return None;
    }
    FiniteReal::new(number.parse().ok()?)
}

/// The active unfold rule. The document has one unfold-rule selection with
/// one entry, and the entry names a rule record of the same segment by its
/// name and by its one-based record index.
pub(crate) fn active_rule<'a>(rules: &'a [Rule], selections: &[Selection]) -> Option<&'a Rule> {
    let [selection] = selections else {
        return None;
    };
    let [entry] = selection.entries.as_slice() else {
        return None;
    };
    let ordinal = entry.rule.record_ordinal()?;
    let mut matches = rules.iter().filter(|rule| {
        rule.identity.segment_token == selection.identity.segment_token
            && rule.identity.record_ordinal == ordinal
    });
    let rule = matches.next()?;
    if matches.next().is_some() || rule.name != entry.name {
        return None;
    }
    Some(rule)
}

#[cfg(test)]
pub(crate) mod tests;
