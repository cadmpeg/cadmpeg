// SPDX-License-Identifier: Apache-2.0
//! Original XML source reads while kept property fields are constructed.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) struct Source<'ctx, 'source> {
    ctx: &'ctx DecodeContext<'source>,
    view: View<'source>,
}

impl<'ctx, 'source> Source<'ctx, 'source> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'source>,
        view: View<'source>,
        xml: &roxmltree::Document<'_>,
    ) -> Result<Self, CodecError> {
        ctx.capture_part_extent(view)?;
        let original = xml.input_text().as_bytes();
        if original.len() != view.window().len()
            || !std::ptr::eq(original.as_ptr(), view.window().as_ptr())
        {
            return Err(CodecError::malformed(
                "XML DOM does not borrow its original source View",
            ));
        }
        Ok(Self { ctx, view })
    }

    pub(crate) fn record<T>(
        &self,
        id: &str,
        node: roxmltree::Node<'_, '_>,
        parse: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let extent = crate::fidelity::relative_xml_extent(self.view, node.range())?;
        self.ctx.record(id, extent, parse)
    }

    pub(crate) fn required_attribute(
        &self,
        node: roxmltree::Node<'_, '_>,
        name: &str,
    ) -> Result<String, CodecError> {
        let attribute = node
            .attributes()
            .find(|attribute| attribute.namespace().is_none() && attribute.name() == name)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} element has no {name} attribute",
                    node.tag_name().name()
                ))
            })?;
        self.attribute(&attribute)
    }

    pub(crate) fn status(&self, node: roxmltree::Node<'_, '_>) -> Result<Option<u64>, CodecError> {
        let Some(attribute) = node
            .attributes()
            .find(|attribute| attribute.namespace().is_none() && attribute.name() == "status")
        else {
            return Ok(None);
        };
        if attribute.value().parse::<u64>().is_err() {
            return Ok(None);
        }
        self.attribute(&attribute)?.parse().map(Some).map_err(|_| {
            CodecError::malformed("tracked property status differs from its admitted value")
        })
    }

    pub(crate) fn attribute(
        &self,
        attribute: &roxmltree::Attribute<'_, '_>,
    ) -> Result<String, CodecError> {
        let range = attribute.range();
        let raw = self.view.window().get(range.clone()).ok_or_else(|| {
            CodecError::malformed("XML attribute escapes its original source View")
        })?;
        let equals = raw
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(|| CodecError::malformed("XML attribute has no source equals delimiter"))?;
        let after_equals = equals
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("XML attribute source delimiter overflows"))?;
        let quoted = raw.get(after_equals..).ok_or_else(|| {
            CodecError::malformed("XML attribute source delimiter escapes its extent")
        })?;
        let padding = quoted
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
            .count();
        let quote = *quoted
            .get(padding)
            .ok_or_else(|| CodecError::malformed("XML attribute has no source quote delimiter"))?;
        if !matches!(quote, b'\'' | b'"') || raw.last() != Some(&quote) {
            return Err(CodecError::malformed(
                "XML attribute source quotes do not match",
            ));
        }
        let start = range
            .start
            .checked_add(equals)
            .and_then(|value| value.checked_add(1))
            .and_then(|value| value.checked_add(padding))
            .and_then(|value| value.checked_add(1))
            .and_then(|value| self.view.start().checked_add(value))
            .ok_or_else(|| CodecError::malformed("XML source value start overflows"))?;
        let end = range
            .end
            .checked_sub(1)
            .and_then(|value| self.view.start().checked_add(value))
            .ok_or_else(|| CodecError::malformed("XML source value end overflows"))?;
        if start > end {
            return Err(CodecError::malformed("XML source value is inverted"));
        }
        if start < end {
            self.view.read_slice(start..end).ok_or_else(|| {
                CodecError::malformed("XML value read escapes its original source View")
            })?;
        }
        Ok(attribute.value().to_owned())
    }

    pub(crate) fn retained_attributes(
        &self,
        node: roxmltree::Node<'_, '_>,
    ) -> Result<std::collections::BTreeMap<String, String>, CodecError> {
        let input = node.document().input_text().as_bytes();
        if input.len() != self.view.window().len() || input.as_ptr() != self.view.window().as_ptr()
        {
            return Err(CodecError::malformed(
                "retained attribute node does not borrow its source",
            ));
        }
        let mut values = std::collections::BTreeMap::new();
        for attribute in node.attributes().rev() {
            if values.contains_key(attribute.name()) {
                continue;
            }
            let range = attribute.range();
            let raw = self
                .view
                .window()
                .get(range.clone())
                .ok_or_else(|| CodecError::malformed("attribute name escapes its source"))?;
            let end = raw
                .iter()
                .position(|byte| matches!(byte, b'=' | b' ' | b'\t' | b'\r' | b'\n'))
                .ok_or_else(|| CodecError::malformed("attribute name has no delimiter"))?;
            let start = raw[..end]
                .iter()
                .rposition(|byte| *byte == b':')
                .map_or(0, |at| at + 1);
            if raw.get(start..end) != Some(attribute.name().as_bytes()) {
                return Err(CodecError::malformed(
                    "retained attribute name differs from source",
                ));
            }
            let start = self
                .view
                .start()
                .checked_add(range.start)
                .and_then(|at| at.checked_add(start))
                .ok_or_else(|| CodecError::malformed("attribute name source start overflows"))?;
            let end = self
                .view
                .start()
                .checked_add(range.start)
                .and_then(|at| at.checked_add(end))
                .ok_or_else(|| CodecError::malformed("attribute name source end overflows"))?;
            let key = self
                .view
                .read_slice(start..end)
                .ok_or_else(|| CodecError::malformed("attribute name read escapes source"))?;
            let key = std::str::from_utf8(key)
                .map_err(|_| CodecError::malformed("attribute name is not UTF-8"))?
                .to_owned();
            values.insert(key, self.attribute(&attribute)?);
        }
        Ok(values)
    }

    pub(crate) fn plain_text(
        &self,
        element: roxmltree::Node<'_, '_>,
    ) -> Result<Option<String>, CodecError> {
        let Some(text) = element.first_child().filter(roxmltree::Node::is_text) else {
            return Ok(None);
        };
        let value = text
            .text()
            .ok_or_else(|| CodecError::malformed("XML text node has no value"))?;
        let range = text.range();
        let raw =
            self.view.window().get(range.clone()).ok_or_else(|| {
                CodecError::malformed("XML text escapes its original source View")
            })?;
        let remainder = self
            .view
            .window()
            .get(range.end..element.range().end)
            .ok_or_else(|| CodecError::malformed("XML text suffix escapes its owning element"))?;
        if element.last_child() != Some(text)
            || raw.contains(&b'<')
            || !remainder.starts_with(b"</")
        {
            return Ok(Some(value.to_owned()));
        }
        let start = self
            .view
            .start()
            .checked_add(range.start)
            .ok_or_else(|| CodecError::malformed("XML source text start overflows"))?;
        let end = self
            .view
            .start()
            .checked_add(range.end)
            .ok_or_else(|| CodecError::malformed("XML source text end overflows"))?;
        if start < end {
            self.view.read_slice(start..end).ok_or_else(|| {
                CodecError::malformed("XML text read escapes its original source View")
            })?;
        }
        Ok(Some(value.to_owned()))
    }
}

#[cfg(test)]
mod tests;
