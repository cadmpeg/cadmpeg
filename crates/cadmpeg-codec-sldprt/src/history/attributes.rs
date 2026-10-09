// SPDX-License-Identifier: Apache-2.0
//! One attribute grammar with retained and borrowed property publication.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

pub(super) type Properties = BTreeMap<NonBlankString, String>;

pub(super) struct BorrowedProperties<'xml, 'ctx> {
    entries: BTreeMap<&'xml str, &'xml str>,
    shell: ScopedReservation<'ctx>,
}

struct PropertyEntries<'xml, 'ctx> {
    entries: std::collections::btree_map::IntoIter<&'xml str, &'xml str>,
    _shell: ScopedReservation<'ctx>,
}

impl BorrowedProperties<'_, '_> {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn into_retained(self, ctx: &DecodeContext<'_>) -> Result<Properties, CodecError> {
        let mut source = PropertyEntries { entries: self.entries.into_iter(), _shell: self.shell };
        let mut output = BTreeMap::new();
        while source.entries.len() != 0 {
            let Some((name, value)) = ctx.next_charged(&mut source.entries, "publish SLDPRT dimension properties")? else { break; };
            retain_property(ctx, &mut output, name, value)?;
        }
        Ok(output)
    }
}

enum PropertySink<'out, 'xml, 'ctx> {
    Retained(&'out mut Properties),
    Borrowed(&'out mut BorrowedProperties<'xml, 'ctx>),
}

impl<'xml> PropertySink<'_, 'xml, '_> {
    fn contains(&self, ctx: &DecodeContext<'_>, name: &str) -> Result<bool, CodecError> {
        match self {
            Self::Retained(output) => ctx.contains_key_btree_map(output, name, "test SLDPRT map key"),
            Self::Borrowed(output) => ctx.contains_key_btree_map(&output.entries, name, "test SLDPRT map key"),
        }
    }

    fn insert(&mut self, ctx: &DecodeContext<'_>, name: &'xml str, value: &'xml str) -> Result<(), CodecError> {
        match self {
            Self::Retained(output) => retain_property(ctx, output, name, value),
            Self::Borrowed(output) => {
                output.shell.with_storage(|| ctx.insert_btree_map(&mut output.entries, name, value, "index SLDPRT history properties"))?;
                Ok(())
            }
        }
    }
}

fn retain_property(ctx: &DecodeContext<'_>, output: &mut Properties, name: &str, value: &str) -> Result<(), CodecError> {
    let name = ctx.copy_retained_text(name, "retain SLDPRT history property name")?;
    let value = ctx.copy_retained_text(value, "retain SLDPRT history property value")?;
    if let Some(name) = NonBlankString::for_decode(ctx, name, "validate nonblank text")? {
        ctx.insert_btree_map(output, name, value, "index SLDPRT history properties")?;
    }
    Ok(())
}

fn scan_attributes<'xml>(
    ctx: &DecodeContext<'_>, losses: &mut Vec<LossNote>, record: &str,
    node: roxmltree::Node<'xml, '_>, excluded: &[&str], output: &mut PropertySink<'_, 'xml, '_>,
) -> Result<(), CodecError> {
    let mut attributes = node.attributes();
    while let Some(attribute) = ctx.next_charged(&mut attributes, "scan SLDPRT element attributes")? {
        let (name, value) = (attribute.name(), attribute.value());
        if excluded.contains(&name) { continue; }
        if ctx.all_by(name.chars(), |character| Ok(character.is_whitespace()), "scan SLDPRT property name characters")? {
            report_unkeyed_property(ctx, losses, record, None)?;
            continue;
        }
        if output.contains(ctx, name)? {
            report_unkeyed_property(ctx, losses, record, Some(name))?;
            continue;
        }
        output.insert(ctx, name, value)?;
    }
    Ok(())
}

pub(super) fn keyed_attributes(
    ctx: &DecodeContext<'_>, losses: &mut Vec<LossNote>, record: &str,
    node: roxmltree::Node<'_, '_>, excluded: &[&str],
) -> Result<Properties, CodecError> {
    let mut output = BTreeMap::new();
    scan_attributes(ctx, losses, record, node, excluded, &mut PropertySink::Retained(&mut output))?;
    Ok(output)
}

pub(super) fn borrowed_attributes<'xml, 'ctx>(
    ctx: &'ctx DecodeContext<'_>, losses: &mut Vec<LossNote>, record: &str,
    node: roxmltree::Node<'xml, '_>, excluded: &[&str],
) -> Result<BorrowedProperties<'xml, 'ctx>, CodecError> {
    let mut output = BorrowedProperties { entries: BTreeMap::new(), shell: ctx.reserve_scoped(0, "hold SLDPRT dimension properties")? };
    scan_attributes(ctx, losses, record, node, excluded, &mut PropertySink::Borrowed(&mut output))?;
    Ok(output)
}

fn report_unkeyed_property(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    record: &str,
    duplicate: Option<&str>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "retain SLDPRT history property loss";
    const BLANK: &str = " states a property with a blank key; the property is not transferred";
    const RESTATED_BEFORE: &str = " states the property ";
    const RESTATED_AFTER: &str = " a second time; the property is not transferred";
    let message = match duplicate {
        Some(key) => ctx.format_retained(
            format_args!("{record}{RESTATED_BEFORE}{key}{RESTATED_AFTER}"),
            OPERATION,
        )?,
        None => ctx.format_retained(format_args!("{record}{BLANK}"), OPERATION)?,
    };
    ctx.push_vec(
        losses,
        crate::loss::SldprtLossCode::SourcePropertyKeyBlank.note(message),
        "collect SLDPRT history property losses",
    )
}

