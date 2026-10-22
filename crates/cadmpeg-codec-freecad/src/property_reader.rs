// SPDX-License-Identifier: Apache-2.0
//! Explicit, transient XML input for property interpretation.
//!
//! Decoder readers borrow genuine source Views and original parser ranges.
//! Writer projections instead serialize current typed fields. Neither path
//! attaches source bytes or physical positions to native semantic records.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::{Deref, Range};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::fidelity::ValueSourceRanges;
use crate::native::PropertyRecord;

pub(crate) struct PropertyReader<'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    trees: PropertyTrees<'ctx>,
}

enum PropertyTrees<'source> {
    Source {
        document: View<'source>,
        ranges: &'source BTreeMap<String, ValueSourceRanges>,
    },
    Typed,
    #[cfg(test)]
    Fixtures(BTreeMap<String, View<'source>>),
}

impl<'ctx, 'arena> PropertyReader<'ctx, 'arena> {
    pub(crate) fn source(
        context: &'ctx DecodeContext<'arena>,
        document: View<'ctx>,
        ranges: &'ctx BTreeMap<String, ValueSourceRanges>,
    ) -> Result<Self, CodecError> {
        // Capturing provenance validates the live source session; it reads no bytes.
        context.capture_part_extent(document)?;
        Ok(Self {
            context,
            trees: PropertyTrees::Source { document, ranges },
        })
    }

    pub(crate) fn typed(context: &'ctx DecodeContext<'arena>) -> Self {
        Self {
            context,
            trees: PropertyTrees::Typed,
        }
    }

    #[cfg(test)]
    pub(crate) fn fixtures(
        context: &'ctx DecodeContext<'arena>,
        sources: BTreeMap<String, View<'ctx>>,
    ) -> Result<Self, CodecError> {
        for source in sources.values() {
            context.capture_part_extent(*source)?;
        }
        Ok(Self {
            context,
            trees: PropertyTrees::Fixtures(sources),
        })
    }

    pub(crate) fn property_xml(
        &self,
        property: &PropertyRecord,
    ) -> Result<Cow<'_, str>, CodecError> {
        if property.body.is_none() {
            return Err(CodecError::malformed("property body is not held"));
        }
        match &self.trees {
            PropertyTrees::Source { document, ranges } => {
                let source = ranges.get(&property.id).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "property {} has no original source range",
                        property.id
                    ))
                })?;
                let extent =
                    crate::fidelity::relative_xml_extent(*document, source.record_range())?;
                std::str::from_utf8(extent.window())
                    .map(Cow::Borrowed)
                    .map_err(|_| CodecError::malformed("property source XML is not UTF-8"))
            }
            #[cfg(test)]
            PropertyTrees::Fixtures(sources) => {
                let source = sources.get(&property.id).ok_or_else(|| {
                    CodecError::malformed("test property has no genuine source View")
                })?;
                std::str::from_utf8(source.window())
                    .map(Cow::Borrowed)
                    .map_err(|_| CodecError::malformed("test property XML is not UTF-8"))
            }
            PropertyTrees::Typed => crate::writer::xml::native(property)?
                .map(Cow::Owned)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "property {} has no independent typed XML tree",
                        property.id
                    ))
                }),
        }
    }

    pub(crate) fn source_range(
        &self,
        property: &PropertyRecord,
    ) -> Result<Option<Range<usize>>, CodecError> {
        match &self.trees {
            PropertyTrees::Source { ranges, .. } => ranges
                .get(&property.id)
                .map(|source| Some(source.record_range()))
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "property {} has no original source range",
                        property.id
                    ))
                }),
            #[cfg(test)]
            PropertyTrees::Fixtures(sources) => sources
                .get(&property.id)
                .map(|source| Some(source.start()..source.end()))
                .ok_or_else(|| CodecError::malformed("test property has no genuine source View")),
            PropertyTrees::Typed => Ok(None),
        }
    }
}

impl<'arena> Deref for PropertyReader<'_, 'arena> {
    type Target = DecodeContext<'arena>;

    fn deref(&self) -> &Self::Target {
        self.context
    }
}

#[cfg(test)]
mod tests;
