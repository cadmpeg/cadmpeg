// SPDX-License-Identifier: Apache-2.0
//! Decode-local lane Views and pending fidelity pointers for admitted records.

use crate::records::FeatureInputLane;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct BodySources<'a> {
    views: BTreeMap<String, View<'a>>,
    pointers: BTreeMap<String, BTreeMap<String, u64>>,
    classes: super::class_sources::ClassSources,
}

impl<'a> BodySources<'a> {
    pub(crate) fn from_views(
        views: impl IntoIterator<Item = (String, View<'a>)>,
    ) -> Result<Self, CodecError> {
        let mut result = Self::default();
        for (id, view) in views {
            if result.views.insert(id, view).is_some() {
                return Err(CodecError::malformed(
                    "duplicate body-selection lane source",
                ));
            }
        }
        Ok(result)
    }

    /// Split a real root containing the exact concatenation the caller authored.
    /// This does not register an alias or turn a retained copy into source data.
    pub(crate) fn from_concatenated(
        lanes: &[FeatureInputLane],
        source: View<'a>,
    ) -> Result<Self, CodecError> {
        let mut cursor = source.start();
        let mut views = Vec::new();
        for lane in lanes {
            let end = cursor
                .checked_add(lane.native_payload.len())
                .ok_or_else(|| CodecError::malformed("body lane source length overflow"))?;
            let view = source
                .child(cursor, end)
                .ok_or_else(|| CodecError::malformed("body lane source exceeds root"))?;
            if view.window() != lane.native_payload.as_slice() {
                return Err(CodecError::malformed(
                    "body lane source concatenation differs",
                ));
            }
            views.push((lane.id.clone(), view));
            cursor = end;
        }
        if cursor != source.end() {
            return Err(CodecError::malformed(
                "body lane source concatenation has extra bytes",
            ));
        }
        Self::from_views(views)
    }

    pub(crate) fn admit_view(&mut self, id: String, view: View<'a>) -> Result<(), CodecError> {
        if self.views.insert(id, view).is_some() {
            return Err(CodecError::malformed("duplicate feature-input lane source"));
        }
        Ok(())
    }

    pub(crate) fn view(&self, lane: &FeatureInputLane) -> Result<View<'a>, CodecError> {
        let source = self
            .views
            .get(&lane.id)
            .copied()
            .ok_or_else(|| CodecError::malformed("body-selection lane source is missing"))?;
        if source.window() != lane.native_payload.as_slice() {
            return Err(CodecError::malformed(
                "body-selection lane differs from its source View",
            ));
        }
        Ok(source)
    }

    pub(crate) fn capture_classes<'b>(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        lanes: impl IntoIterator<Item = &'b FeatureInputLane>,
    ) -> Result<(), CodecError> {
        for lane in lanes {
            super::class_layout::capture(ctx, lane, self)?;
        }
        Ok(())
    }

    pub(crate) fn pointers(&self) -> &BTreeMap<String, BTreeMap<String, u64>> {
        &self.pointers
    }

    pub(crate) fn pointer(&self, id: &str) -> Option<u64> {
        self.classes.pointer(id).or_else(|| {
            self.pointers
                .get(id)
                .and_then(|fields| fields.get("offset"))
                .copied()
        })
    }

    pub(crate) fn class_sources(&self) -> super::class_sources::ClassSources {
        self.classes.clone()
    }

    pub(super) fn insert_class(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &str,
        offset: u64,
    ) -> Result<(), CodecError> {
        if self.pointers.contains_key(id) {
            return Err(CodecError::malformed(
                "class pointer already has another source owner",
            ));
        }
        self.classes.insert(ctx, id, offset)
    }

    pub(crate) fn insert(&mut self, id: String, offset: u64) -> Result<(), CodecError> {
        if self.classes.pointer(&id).is_some() {
            return Err(CodecError::malformed(
                "source pointer already has a class owner",
            ));
        }
        if self
            .pointers
            .insert(id, BTreeMap::from([("offset".into(), offset)]))
            .is_some()
        {
            return Err(CodecError::malformed(
                "duplicate feature-input source pointer",
            ));
        }
        Ok(())
    }

    pub(crate) fn into_pointers(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<BTreeMap<String, BTreeMap<String, u64>>, CodecError> {
        let mut pointers = self.pointers;
        // insert and insert_class enforce disjoint owner identities.
        pointers.extend(self.classes.into_pointers(ctx)?);
        Ok(pointers)
    }
}

#[cfg(test)]
mod tests;
