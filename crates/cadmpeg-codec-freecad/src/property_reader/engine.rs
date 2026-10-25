// SPDX-License-Identifier: Apache-2.0
//! Binding decisions use original fields and actual finalized failure homes.

use super::{parameters::ControlSource, PropertyReader};
use crate::native::PropertyRecord;
use cadmpeg_core::decode::{DecodeContext, RecordOwner, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::source_fidelity::parts::{FinalizedFailureRegistry, GivenUpHome, PartState};
use cadmpeg_ir::SourceFidelity;

pub(crate) struct EngineFailureSources<'a> {
    parts: std::collections::HashMap<(&'a str, u64, u64, EnginePart), &'a UnknownId>,
    registry: &'a FinalizedFailureRegistry,
}

impl<'a> EngineFailureSources<'a> {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        fidelity: &'a SourceFidelity,
        registry: &'a FinalizedFailureRegistry,
    ) -> Result<Self, CodecError> {
        let count = fidelity
            .parts()
            .values()
            .filter(|part| {
                part.state() == &PartState::GivenUp {}
                    && EnginePart::from_name(part.level().as_str()).is_some()
            })
            .count();
        let bytes = count
            .checked_mul(std::mem::size_of::<(
                (&str, u64, u64, EnginePart),
                &UnknownId,
            )>())
            .ok_or_else(|| CodecError::malformed("Engine failure index storage overflows"))?;
        let mut parts = std::collections::HashMap::new();
        parts.try_reserve(count).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                cadmpeg_core::decode::u64_from_index(bytes),
                "FreeCAD Engine failure index",
            )
        })?;
        for (id, part) in fidelity.parts() {
            let Some(level) = EnginePart::from_name(part.level().as_str())
                .filter(|_| part.state() == &PartState::GivenUp {})
            else {
                continue;
            };
            if parts
                .insert((part.stream(), part.offset(), part.byte_len(), level), id)
                .is_some()
            {
                return Err(CodecError::malformed(
                    "multiple failed Engine parts share an exact registered extent",
                ));
            }
        }
        Ok(Self { parts, registry })
    }

    fn home(
        &self,
        ctx: &DecodeContext<'_>,
        view: View<'_>,
        level: EnginePart,
    ) -> Result<Option<GivenUpHome>, CodecError> {
        let extent = ctx.capture_part_extent(view)?;
        match self
            .parts
            .get(&(extent.stream(), extent.offset(), extent.byte_len(), level))
        {
            Some(id) => self.registry.home(id.as_str()).map(Some).ok_or_else(|| {
                CodecError::malformed("failed Engine part has no finalized source authority")
            }),
            None => Ok(None),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum EnginePart {
    Property,
    Container,
    Expression,
}
impl EnginePart {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "freecad.property" => Self::Property,
            "freecad.property_value" => Self::Container,
            "freecad.value_element" => Self::Expression,
            _ => return None,
        })
    }
}

pub(crate) enum ParameterBinding {
    Bound(String),
    ProvedNoMatch,
    Unavailable,
}

enum BindingLookup {
    Bound(String),
    ProvedNoMatch,
    Unavailable(Vec<GivenUpHome>),
}

pub(crate) struct ParameterRequest<'a> {
    pub(crate) id: &'a ParameterId,
    pub(crate) owner: Option<&'a FeatureId>,
    pub(crate) name: &'a str,
    pub(crate) path: &'a str,
    pub(crate) alias: Option<&'a str>,
}

pub(crate) struct ParameterAttempt {
    pub(crate) id: ParameterId,
    pub(crate) owner: Option<FeatureId>,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) property: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) homes: Vec<GivenUpHome>,
}

impl<'ctx, 'arena> PropertyReader<'ctx, 'arena> {
    pub(crate) fn parameter_phase<'phase>(
        &'phase self,
        failures: Option<&'phase EngineFailureSources<'phase>>,
    ) -> PropertyReader<'phase, 'arena> {
        PropertyReader {
            context: self.context,
            entries: std::borrow::Cow::Borrowed(self.entries.as_ref()),
            failures: self.failures,
            part_failures: self.part_failures,
            engine_failures: failures,
            parameter_attempts: Default::default(),
            projection: Default::default(),
            feature_owner: Default::default(),
            object_sources: self.object_sources,
            xml_source: self.xml_source,
            property_sources: self.property_sources,
            payload_sources: self.payload_sources,
            #[cfg(test)]
            sources: self.sources.clone(),
            #[cfg(test)]
            input: self.input,
        }
    }

    pub(crate) fn take_parameter_attempts(&self) -> Vec<ParameterAttempt> {
        std::mem::take(&mut *self.parameter_attempts.borrow_mut())
    }

    pub(crate) fn parameter_attempts(&self) -> std::cell::Ref<'_, Vec<ParameterAttempt>> {
        self.parameter_attempts.borrow()
    }

    fn missing_binding(
        &self,
        property: &PropertyRecord,
        request: &ParameterRequest<'_>,
        homes: Vec<GivenUpHome>,
    ) -> Result<ParameterBinding, CodecError> {
        let mut attempts = self.parameter_attempts.borrow_mut();
        if !attempts.iter().any(|attempt| &attempt.id == request.id) {
            attempts.try_reserve_exact(1).map_err(|_| {
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    self.context.policy().limits.max_retained_bytes,
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<ParameterAttempt>()),
                    "FreeCAD unavailable parameter attempts",
                )
            })?;
            let mut aliases = vec![request.path.to_owned()];
            if let Some(name) = request.alias.filter(|name| !name.is_empty()) {
                aliases.push(name.to_owned());
                aliases.push(format!("Constraints.{name}"));
            }
            attempts.push(ParameterAttempt {
                id: request.id.clone(),
                owner: request.owner.cloned(),
                name: request.name.to_owned(),
                path: request.path.to_owned(),
                property: property.id.clone(),
                aliases,
                homes,
            });
        }
        Ok(ParameterBinding::Unavailable)
    }

    fn engine_home(
        &self,
        view: View<'_>,
        level: EnginePart,
    ) -> Result<Option<GivenUpHome>, CodecError> {
        self.engine_failures.map_or(Ok(None), |failures| {
            failures.home(self.context, view, level)
        })
    }

    fn source_property_index(&self, property: &PropertyRecord) -> Result<usize, CodecError> {
        let source = self
            .xml_source
            .ok_or_else(|| CodecError::malformed("binding has no original XML supplier"))?;
        let range = self
            .property_sources
            .and_then(|ranges| ranges.get(&property.id))
            .ok_or_else(|| CodecError::malformed("binding has no original property range"))?
            .record_range();
        source
            .elements()
            .iter()
            .position(|element| {
                element.extent().start() == source.view().start() + range.start
                    && element.extent().end() == source.view().start() + range.end
            })
            .ok_or_else(|| CodecError::malformed("binding has no original property element"))
    }

    pub(crate) fn parameter_binding(
        &self,
        parameter: &ControlSource<'_>,
        control: &PropertyRecord,
        properties: &[&PropertyRecord],
        request: ParameterRequest<'_>,
    ) -> Result<ParameterBinding, CodecError> {
        match self.binding_lookup(parameter.owner(), control, properties, request.path)? {
            BindingLookup::Bound(text) => Ok(ParameterBinding::Bound(text)),
            BindingLookup::ProvedNoMatch => Ok(ParameterBinding::ProvedNoMatch),
            BindingLookup::Unavailable(homes) => self.missing_binding(control, &request, homes),
        }
    }

    /// Read a binding under the current genuine Feature or Census producer.
    /// This lookup does not attempt or construct a Parameter.
    pub(crate) fn feature_binding_lookup(
        &self,
        control: &PropertyRecord,
        properties: &[&PropertyRecord],
        path: &str,
    ) -> Result<ParameterBinding, CodecError> {
        let capture = self.feature_owner.borrow();
        if self.property_sources.is_some() && capture.is_none() {
            return Err(CodecError::malformed(
                "source binding lookup has no current record producer",
            ));
        }
        match self.binding_lookup(capture.as_ref(), control, properties, path)? {
            BindingLookup::Bound(text) => Ok(ParameterBinding::Bound(text)),
            BindingLookup::ProvedNoMatch => Ok(ParameterBinding::ProvedNoMatch),
            BindingLookup::Unavailable(_) => Ok(ParameterBinding::Unavailable),
        }
    }

    fn binding_lookup(
        &self,
        owner: Option<&RecordOwner<'_>>,
        control: &PropertyRecord,
        properties: &[&PropertyRecord],
        requested_path: &str,
    ) -> Result<BindingLookup, CodecError> {
        let engine = properties
            .iter()
            .copied()
            .find(|property| property.name == "ExpressionEngine");
        let Some(owner) = owner else {
            let Some(engine) = engine else {
                return Ok(BindingLookup::ProvedNoMatch);
            };
            if engine.body.is_none() {
                return Ok(BindingLookup::Unavailable(Vec::new()));
            }
            let root = engine
                .values()
                .iter()
                .find(|value| value.parent.is_none() && value.tag == "ExpressionEngine");
            let Some(root) = root else {
                return if engine.is_transient()
                    || engine.type_name != "App::PropertyExpressionEngine"
                {
                    Ok(BindingLookup::ProvedNoMatch)
                } else {
                    Ok(BindingLookup::Unavailable(Vec::new()))
                };
            };
            let rows = engine
                .values()
                .iter()
                .filter(|value| value.parent == Some(root.order) && value.tag == "Expression")
                .collect::<Vec<_>>();
            let count = root
                .attributes
                .get("count")
                .and_then(|count| count.parse::<usize>().ok());
            if count != Some(rows.len()) {
                return Ok(BindingLookup::Unavailable(Vec::new()));
            }
            return Ok(rows
                .iter()
                .find(|value| {
                    value
                        .attributes
                        .get("path")
                        .is_some_and(|path| path == requested_path)
                })
                .and_then(|value| value.attributes.get("expression"))
                .map_or(BindingLookup::ProvedNoMatch, |text| {
                    BindingLookup::Bound(text.clone())
                }));
        };
        let source = self
            .xml_source
            .ok_or_else(|| CodecError::malformed("binding has no original XML supplier"))?;
        if self.engine_failures.is_none() {
            return Err(CodecError::malformed(
                "source binding has no finalized part authority",
            ));
        }
        let engine = if let Some(engine) = engine {
            engine
        } else {
            let control_index = self.source_property_index(control)?;
            let parent = source.elements()[control_index].parent();
            if let Some(parent) = parent {
                for index in source.children(parent) {
                    if let Some(home) =
                        self.engine_home(source.extent(index)?, EnginePart::Property)?
                    {
                        return Ok(BindingLookup::Unavailable(vec![home]));
                    }
                    let name =
                        self.context
                            .record_fragment(owner, source.extent(index)?, || {
                                source.attribute(self.context, index, "name")
                            })?;
                    if name.as_deref() == Some("ExpressionEngine") {
                        return Err(CodecError::malformed(
                            "admitted Engine header has no typed property",
                        ));
                    }
                }
            }
            return Ok(BindingLookup::ProvedNoMatch);
        };
        let index = self.source_property_index(engine)?;
        let active = self
            .context
            .record_fragment(owner, source.extent(index)?, || {
                let name = source.attribute(self.context, index, "name")?;
                let kind = source.attribute(self.context, index, "type")?;
                let tag = source.read_name(index)?;
                Ok(name.as_deref() == Some("ExpressionEngine")
                    && kind.as_deref() == Some("App::PropertyExpressionEngine")
                    && tag == "Property")
            })?;
        if !active {
            return Ok(BindingLookup::ProvedNoMatch);
        }
        let root = engine
            .values()
            .iter()
            .find(|value| value.parent.is_none() && value.tag == "ExpressionEngine");
        let root_index = if let Some(root) = root {
            let range = self
                .property_sources
                .and_then(|ranges| ranges.get(&engine.id))
                .and_then(|ranges| {
                    ranges
                        .value_ranges()
                        .find_map(|(order, range)| (order == root.order).then_some(range))
                })
                .ok_or_else(|| CodecError::malformed("binding container has no original range"))?;
            source
                .elements()
                .iter()
                .position(|element| {
                    element.extent().start() == source.view().start() + range.start
                        && element.extent().end() == source.view().start() + range.end
                })
                .ok_or_else(|| CodecError::malformed("binding container has no original element"))?
        } else {
            for child in source.children(index) {
                if let Some(home) =
                    self.engine_home(source.extent(child)?, EnginePart::Container)?
                {
                    return Ok(BindingLookup::Unavailable(vec![home]));
                }
            }
            return Err(CodecError::malformed(
                "binding container unavailable without an actual source home",
            ));
        };
        let count = self
            .context
            .record_fragment(owner, source.extent(root_index)?, || {
                if source.read_name(root_index)? != "ExpressionEngine" {
                    return Err(CodecError::malformed("binding container has another tag"));
                }
                crate::persistence::source::value_attribute(
                    self.context,
                    source,
                    root_index,
                    "count",
                )?
                .and_then(|count| count.parse::<usize>().ok())
                .ok_or_else(|| CodecError::malformed("binding container has no actual count"))
            })?;
        let mut reached = 0;
        for child in source
            .children(root_index)
            .filter(|child| source.name(*child).is_ok_and(|tag| tag == "Expression"))
            .take(count)
        {
            reached += 1;
            if let Some(home) = self.engine_home(source.extent(child)?, EnginePart::Expression)? {
                return Ok(BindingLookup::Unavailable(vec![home]));
            }
            let path = self
                .context
                .record_fragment(owner, source.extent(child)?, || {
                    if source.read_name(child)? != "Expression" {
                        return Err(CodecError::malformed("binding candidate has another tag"));
                    }
                    crate::persistence::source::value_attribute(
                        self.context,
                        source,
                        child,
                        "path",
                    )?
                    .ok_or_else(|| CodecError::malformed("admitted Engine row has no path"))
                })?;
            if path == requested_path {
                let text = self
                    .context
                    .record_fragment(owner, source.extent(child)?, || {
                        crate::persistence::source::value_attribute(
                            self.context,
                            source,
                            child,
                            "expression",
                        )?
                        .ok_or_else(|| {
                            CodecError::malformed("admitted Engine row has no expression")
                        })
                    })?;
                return Ok(BindingLookup::Bound(text));
            }
        }
        if reached != count {
            return Err(CodecError::malformed(
                "binding lookup differs from its admitted count",
            ));
        }
        Ok(BindingLookup::ProvedNoMatch)
    }
}
