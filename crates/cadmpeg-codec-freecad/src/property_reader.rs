// SPDX-License-Identifier: Apache-2.0
//! Projection readers serialize current typed property fields.

use crate::native::PropertyRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Deref;

mod engine;
mod entry_lists;
mod failures;
mod headers;
mod mesh_material;
pub(crate) mod parameters;
pub(crate) use engine::{
    EngineFailureSources, ParameterAttempt, ParameterBinding, ParameterRequest,
};
pub(crate) use failures::failure_homes;

#[derive(Default)]
struct ProjectionTrace<'a> {
    owner: Option<String>,
    unavailable_requested: bool,
    source_ids: Vec<&'a str>,
    part_ids: Vec<&'a str>,
    unavailable: Vec<cadmpeg_ir::source_fidelity::parts::GivenUpHome>,
}

pub(crate) struct PropertyReader<'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    entries: Cow<'ctx, [crate::native::EntryRecord]>,
    failures: Option<&'ctx BTreeMap<String, cadmpeg_ir::source_fidelity::parts::GivenUpHome>>,
    part_failures: Option<&'ctx cadmpeg_ir::source_fidelity::parts::FinalizedFailureRegistry>,
    engine_failures: Option<&'ctx EngineFailureSources<'ctx>>,
    parameter_attempts: RefCell<Vec<ParameterAttempt>>,
    projection: RefCell<ProjectionTrace<'ctx>>,
    feature_owner: RefCell<Option<cadmpeg_core::decode::RecordOwner<'arena>>>,
    object_sources:
        Option<&'ctx BTreeMap<String, crate::persistence::object_parts::ObjectSource<'arena>>>,
    xml_source: Option<&'ctx crate::xml_framing::XmlSource<'arena>>,
    property_sources: Option<&'ctx BTreeMap<String, crate::fidelity::ValueSourceRanges>>,
    payload_sources: Option<&'ctx crate::gui::payloads::sources::PayloadSources<'arena>>,
    #[cfg(test)]
    sources: Option<std::collections::BTreeMap<String, cadmpeg_core::decode::View<'ctx>>>,
    #[cfg(test)]
    input: Option<cadmpeg_core::decode::View<'ctx>>,
}

impl<'ctx, 'arena> PropertyReader<'ctx, 'arena> {
    pub(crate) fn typed(context: &'ctx DecodeContext<'arena>) -> Self {
        Self {
            context,
            entries: Cow::Borrowed(&[]),
            failures: None,
            part_failures: None,
            engine_failures: None,
            parameter_attempts: Default::default(),
            projection: RefCell::new(ProjectionTrace::default()),
            feature_owner: RefCell::new(None),
            object_sources: None,
            property_sources: None,
            xml_source: None,
            payload_sources: None,
            #[cfg(test)]
            sources: None,
            #[cfg(test)]
            input: None,
        }
    }

    pub(crate) fn with_object_sources(
        mut self,
        sources: &'ctx BTreeMap<String, crate::persistence::object_parts::ObjectSource<'arena>>,
    ) -> Self {
        self.object_sources = Some(sources);
        self
    }

    pub(crate) fn with_xml_source(
        mut self,
        source: &'ctx crate::xml_framing::XmlSource<'arena>,
    ) -> Self {
        self.xml_source = Some(source);
        self
    }

    pub(crate) fn with_property_sources(
        mut self,
        sources: &'ctx BTreeMap<String, crate::fidelity::ValueSourceRanges>,
    ) -> Self {
        self.property_sources = Some(sources);
        self
    }

    pub(crate) fn begin_feature(
        &self,
        owner: &str,
        id: &str,
        kind: &str,
    ) -> Result<(String, String), CodecError> {
        self.begin_projection(owner)?;
        let Some(sources) = self.object_sources else {
            return Ok((owner.to_owned(), kind.to_owned()));
        };
        let source = sources
            .get(owner)
            .ok_or_else(|| CodecError::malformed("feature has no original object supplier"))?;
        let (object, record) = source.feature_record_owner(self.context, id)?;
        *self.feature_owner.borrow_mut() = Some(record);
        Ok(object)
    }

    pub(crate) fn has_feature_sources(&self) -> bool {
        self.property_sources.is_some() && self.feature_owner.borrow().is_some()
    }

    pub(crate) fn feature_payload_header<'a>(
        &'a self,
        property: &'a PropertyRecord,
    ) -> Result<Option<crate::gui::payloads::sources::ArrayHeader<'a>>, CodecError> {
        let owner = self.feature_owner.borrow();
        let owner = owner
            .as_ref()
            .ok_or_else(|| CodecError::malformed("source feature has no actual producer"))?;
        self.source_payload_header(owner, property)
    }

    pub(crate) fn feature_float(
        &self,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<cadmpeg_ir::scalar::FiniteReal, CodecError> {
        let owner = self.feature_owner.borrow();
        self.source_float(
            owner
                .as_ref()
                .ok_or_else(|| CodecError::malformed("feature producer is absent"))?,
            property,
            ordinal,
            header,
        )?
        .ok_or_else(|| CodecError::malformed("feature float has no original supplier"))
    }

    pub(crate) fn feature_vector(
        &self,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<cadmpeg_ir::units::FiniteVector<3>, CodecError> {
        let owner = self.feature_owner.borrow();
        self.source_vector(
            owner
                .as_ref()
                .ok_or_else(|| CodecError::malformed("feature producer is absent"))?,
            property,
            ordinal,
            header,
        )?
        .ok_or_else(|| CodecError::malformed("feature vector has no original supplier"))
    }

    pub(crate) fn feature_fillets(
        &self,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<[f64; 2], CodecError> {
        if !self
            .fillet_slots(property)
            .is_some_and(|slots| slots.get(ordinal).is_some_and(Option::is_some))
        {
            return Err(CodecError::malformed(
                "feature fillet entry slot is not held",
            ));
        }
        let owner = self.feature_owner.borrow();
        self.payload_sources
            .ok_or_else(|| CodecError::malformed("feature fillet suppliers are absent"))?
            .fillet_radii(
                self.context,
                owner
                    .as_ref()
                    .ok_or_else(|| CodecError::malformed("feature producer is absent"))?,
                ordinal,
                header,
            )
    }

    pub(crate) fn with_payload_sources(
        mut self,
        sources: &'ctx crate::gui::payloads::sources::PayloadSources<'arena>,
    ) -> Self {
        self.payload_sources = Some(sources);
        self
    }

    pub(crate) fn has_payload_sources(&self) -> bool {
        self.payload_sources.is_some()
    }

    pub(crate) fn source_payload_header<'a>(
        &'a self,
        owner: &cadmpeg_core::decode::RecordOwner<'_>,
        property: &'a PropertyRecord,
    ) -> Result<Option<crate::gui::payloads::sources::ArrayHeader<'a>>, CodecError> {
        self.payload_sources
            .map(|sources| sources.header(self.context, owner, &property.id))
            .transpose()
            .map(Option::flatten)
    }

    pub(crate) fn begin_payload_record<'a>(
        &'a self,
        id: &str,
        property: &'a PropertyRecord,
    ) -> Result<
        Option<(
            crate::gui::payloads::sources::ArrayHeader<'a>,
            cadmpeg_core::decode::RecordOwner<'arena>,
        )>,
        CodecError,
    > {
        self.payload_sources
            .map(|sources| sources.begin_record(self.context, id, &property.id))
            .transpose()
            .map(Option::flatten)
    }

    pub(crate) fn source_placement(
        &self,
        owner: &cadmpeg_core::decode::RecordOwner<'_>,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<Option<crate::native::frame::FiniteFrame>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        if !self
            .placement_slots(property)
            .is_some_and(|values| values.get(ordinal).is_some_and(Option::is_some))
        {
            return Err(CodecError::malformed(
                "requested entry list slot is not held",
            ));
        }
        sources
            .placement(self.context, owner, ordinal, header)
            .map(Some)
    }

    pub(crate) fn source_vector(
        &self,
        owner: &cadmpeg_core::decode::RecordOwner<'_>,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<Option<cadmpeg_ir::units::FiniteVector<3>>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        if !self
            .vector_slots(property)
            .is_some_and(|values| values.get(ordinal).is_some_and(Option::is_some))
            && !self.f32_present(property, ordinal, 3)
        {
            return Err(CodecError::malformed(
                "requested entry list slot is not held",
            ));
        }
        sources
            .vector(self.context, owner, ordinal, header)
            .map(Some)
    }

    pub(crate) fn source_float(
        &self,
        owner: &cadmpeg_core::decode::RecordOwner<'_>,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<Option<cadmpeg_ir::scalar::FiniteReal>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        if !self
            .float_slots(property)
            .is_some_and(|values| values.get(ordinal).is_some_and(Option::is_some))
            && !self.f32_present(property, ordinal, 1)
        {
            return Err(CodecError::malformed(
                "requested entry list slot is not held",
            ));
        }
        sources
            .float(self.context, owner, ordinal, header)
            .map(Some)
    }

    pub(crate) fn source_color(
        &self,
        owner: &cadmpeg_core::decode::RecordOwner<'_>,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<Option<u32>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        if !self
            .color_slots(property)
            .is_some_and(|values| values.get(ordinal).is_some_and(Option::is_some))
        {
            return Err(CodecError::malformed(
                "requested entry list slot is not held",
            ));
        }
        sources
            .color(self.context, owner, ordinal, header)
            .map(Some)
    }

    pub(crate) fn source_placement_values(
        &self,
        owner: &cadmpeg_core::decode::RecordOwner<'_>,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<Option<[cadmpeg_ir::scalar::FiniteReal; 7]>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        if !self
            .placement_slots(property)
            .is_some_and(|values| values.get(ordinal).is_some_and(Option::is_some))
        {
            return Err(CodecError::malformed(
                "requested entry list slot is not held",
            ));
        }
        sources
            .placement_values(self.context, owner, ordinal, header)
            .map(Some)
    }

    pub(crate) fn source_curvature(
        &self,
        owner: &cadmpeg_core::decode::RecordOwner<'_>,
        property: &PropertyRecord,
        ordinal: usize,
        header: &crate::gui::payloads::sources::ArrayHeader<'_>,
    ) -> Result<Option<cadmpeg_ir::units::FiniteVector<8>>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        if !self.f32_present(property, ordinal, 8) {
            return Err(CodecError::malformed(
                "requested curvature entry slot is not held",
            ));
        }
        sources
            .curvature(self.context, owner, ordinal, header)
            .map(Some)
    }

    pub(crate) fn with_failures(
        mut self,
        failures: &'ctx BTreeMap<String, cadmpeg_ir::source_fidelity::parts::GivenUpHome>,
    ) -> Self {
        self.failures = Some(failures);
        self
    }

    pub(crate) fn with_part_failures(
        mut self,
        failures: &'ctx cadmpeg_ir::source_fidelity::parts::FinalizedFailureRegistry,
    ) -> Self {
        self.part_failures = Some(failures);
        self
    }

    pub(crate) fn begin_projection(&self, owner: &str) -> Result<(), CodecError> {
        *self.feature_owner.borrow_mut() = None;
        let mut owner_id = String::new();
        owner_id.try_reserve_exact(owner.len()).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                self.context.policy().limits.max_retained_bytes,
                cadmpeg_core::decode::u64_from_index(owner.len()),
                "FreeCAD projection owner authority",
            )
        })?;
        owner_id.push_str(owner);
        *self.projection.borrow_mut() = ProjectionTrace {
            owner: Some(owner_id),
            unavailable_requested: false,
            source_ids: Vec::new(),
            part_ids: Vec::new(),
            unavailable: Vec::new(),
        };
        Ok(())
    }

    /// A held declaration with no value is distinct from an absent role.
    pub(crate) fn missing_value(
        &self,
        property: Option<&PropertyRecord>,
        name: &str,
    ) -> Result<bool, CodecError> {
        match property {
            Some(property) if property.body.is_none() => {
                self.unavailable(property)?;
                Ok(true)
            }
            Some(_) => Ok(false),
            None => self.unavailable_named(name),
        }
    }

    pub(crate) fn unavailable(&self, property: &PropertyRecord) -> Result<bool, CodecError> {
        // Current typed availability is meaningful without source sidecars.
        // A source failure authority remains a separate optional fact.
        self.projection.borrow_mut().unavailable_requested = true;
        self.unavailable_id(&property.id)
    }

    /// Exact refused role headers establish eligibility for optional producers.
    pub(crate) fn unavailable_named_exact(&self, name: &str) -> Result<bool, CodecError> {
        self.unavailable_named_impl(name, false)
    }

    pub(crate) fn unavailable_named(&self, name: &str) -> Result<bool, CodecError> {
        self.unavailable_named_impl(name, true)
    }

    fn unavailable_named_impl(&self, name: &str, include_body: bool) -> Result<bool, CodecError> {
        let projection = self.projection.borrow();
        let Some(owner) = projection.owner.as_deref() else {
            return Ok(false);
        };
        let id = crate::native::native_child_id("property", owner, name);
        // Source-bound body scopes bind their genuinely produced owner IDs.
        // An absent role consults that authority only when actually requested.
        let body_owner = include_body
            .then_some(self.failures)
            .flatten()
            .and_then(|homes| homes.get_key_value(owner))
            .map(|(id, _)| id.as_str());
        drop(projection);
        if self.unavailable_id(&id)? {
            return Ok(true);
        }
        match body_owner {
            Some(owner) => self.unavailable_id(owner),
            None => Ok(false),
        }
    }

    fn unavailable_id(&self, id: &str) -> Result<bool, CodecError> {
        let Some((actual_id, home)) = self
            .failures
            .and_then(|failures| failures.get_key_value(id))
        else {
            return Ok(false);
        };
        let mut projection = self.projection.borrow_mut();
        projection.unavailable_requested = true;
        if !projection.source_ids.contains(&actual_id.as_str()) {
            let failed = |_| {
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    self.context.policy().limits.max_retained_bytes,
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                        &str,
                        cadmpeg_ir::source_fidelity::parts::GivenUpHome,
                    )>()),
                    "FreeCAD projection unavailable field authority",
                )
            };
            projection
                .unavailable
                .try_reserve_exact(1)
                .map_err(failed)?;
            projection.source_ids.try_reserve_exact(1).map_err(failed)?;
            projection.source_ids.push(actual_id.as_str());
            projection.unavailable.push(home.clone());
        }
        Ok(true)
    }

    fn unavailable_part(&self, id: &'ctx str) -> Result<bool, CodecError> {
        let Some(home) = self.part_failures.and_then(|registry| registry.home(id)) else {
            return Ok(false);
        };
        let mut projection = self.projection.borrow_mut();
        if !projection.part_ids.contains(&id) {
            let bytes =
                std::mem::size_of::<(&str, cadmpeg_ir::source_fidelity::parts::GivenUpHome)>();
            let failed = |_| {
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    self.context.policy().limits.max_retained_bytes,
                    cadmpeg_core::decode::u64_from_index(bytes),
                    "FreeCAD projection failed part authority",
                )
            };
            projection.part_ids.try_reserve_exact(1).map_err(failed)?;
            projection
                .unavailable
                .try_reserve_exact(1)
                .map_err(failed)?;
            projection.part_ids.push(id);
            projection.unavailable.push(home);
        }
        projection.unavailable_requested = true;
        Ok(true)
    }

    pub(crate) fn take_unavailable(&self) -> Vec<cadmpeg_ir::source_fidelity::parts::GivenUpHome> {
        let mut projection = self.projection.borrow_mut();
        projection.owner = None;
        projection.unavailable_requested = false;
        projection.source_ids.clear();
        projection.part_ids.clear();
        std::mem::take(&mut projection.unavailable)
    }

    pub(crate) fn has_unavailable(&self) -> bool {
        self.projection.borrow().unavailable_requested
    }

    #[cfg(test)]
    pub(crate) fn fixtures(
        context: &'ctx DecodeContext<'arena>,
        sources: std::collections::BTreeMap<String, cadmpeg_core::decode::View<'ctx>>,
        input: cadmpeg_core::decode::View<'ctx>,
    ) -> Result<Self, CodecError> {
        context.require_source(input)?;
        for source in sources.values() {
            context.require_source(*source)?;
        }
        Ok(Self {
            context,
            entries: Cow::Borrowed(&[]),
            failures: None,
            part_failures: None,
            engine_failures: None,
            parameter_attempts: Default::default(),
            projection: RefCell::new(ProjectionTrace::default()),
            feature_owner: RefCell::new(None),
            object_sources: None,
            property_sources: None,
            xml_source: None,
            payload_sources: None,
            sources: Some(sources),
            input: Some(input),
        })
    }

    #[cfg(test)]
    pub(crate) fn test_input_view(
        &self,
        bytes: &[u8],
    ) -> Result<cadmpeg_core::decode::View<'ctx>, CodecError> {
        let input = self
            .input
            .ok_or_else(|| CodecError::malformed("test reader has no original input"))?;
        let end = input
            .start()
            .checked_add(bytes.len())
            .ok_or_else(|| CodecError::malformed("test input extent overflows"))?;
        let view = input
            .child(input.start(), end)
            .ok_or_else(|| CodecError::malformed("test input escapes original source"))?;
        if view.window() != bytes {
            return Err(CodecError::malformed(
                "test input differs from original source",
            ));
        }
        Ok(view)
    }

    pub(crate) fn property_xml(
        &self,
        property: &PropertyRecord,
    ) -> Result<Cow<'_, str>, CodecError> {
        if property.body.is_none() {
            self.unavailable(property)?;
            return Err(CodecError::malformed("property body is not held"));
        }
        #[cfg(test)]
        if let Some(sources) = &self.sources {
            let source = sources
                .get(&property.id)
                .ok_or_else(|| CodecError::malformed("test property has no genuine source View"))?;
            return std::str::from_utf8(source.window())
                .map(Cow::Borrowed)
                .map_err(|_| CodecError::malformed("test property XML is not UTF-8"));
        }
        match crate::writer::xml::native(property)? {
            Some(value) => Ok(Cow::Owned(value)),
            None => {
                self.unavailable(property)?;
                Err(CodecError::malformed(format_args!(
                    "property {} has no independent typed XML tree",
                    property.id
                )))
            }
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
