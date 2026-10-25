// SPDX-License-Identifier: Apache-2.0
//! Parameter values read by their own producers from admitted source fields.

use super::PropertyReader;
use crate::native::PropertyRecord;
use cadmpeg_core::decode::RecordOwner;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(crate) struct ControlSource<'source> {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) text: Option<String>,
    owner: Option<RecordOwner<'source>>,
}

impl<'source> ControlSource<'source> {
    pub(crate) fn owner(&self) -> Option<&RecordOwner<'source>> {
        self.owner.as_ref()
    }
}

enum ControlCapture<'a, 'source> {
    Primary(&'a str),
    Fragment(&'a RecordOwner<'source>),
}

impl<'ctx, 'arena> PropertyReader<'ctx, 'arena> {
    pub(crate) fn feature_object_header(
        &self,
        object: &crate::native::ObjectRecord,
    ) -> Result<Option<(String, String)>, CodecError> {
        let Some(sources) = self.object_sources else {
            return Ok(None);
        };
        let owner = self.feature_owner.borrow();
        let owner = owner
            .as_ref()
            .ok_or_else(|| CodecError::malformed("symbol producer is absent"))?;
        let source = sources
            .get(object.id())
            .ok_or_else(|| CodecError::malformed("symbol owner source is absent"))?;
        let (id, name, kind) = source.feature_header_into(self.context, owner)?;
        if id != *object.id() || kind != object.type_name {
            return Err(CodecError::malformed("symbol source object differs"));
        }
        Ok(Some((name, kind)))
    }

    pub(crate) fn take_feature_owner(&self) -> Option<RecordOwner<'arena>> {
        self.feature_owner.take()
    }

    pub(crate) fn continue_feature<T>(
        &self,
        owner: Option<RecordOwner<'arena>>,
        construct: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        if self.object_sources.is_some() && owner.is_none() {
            return Err(CodecError::malformed(
                "feature continuation has no original owner",
            ));
        }
        let previous = self.feature_owner.replace(owner);
        let result = construct();
        self.feature_owner.replace(previous);
        result
    }

    pub(crate) fn feature_fields(
        &self,
        property: &PropertyRecord,
        order: usize,
        fields: &[&str],
    ) -> Result<Option<ControlSource<'arena>>, CodecError> {
        if self.property_sources.is_none() {
            return Ok(None);
        }
        let capture = self.feature_owner.borrow();
        let Some(owner) = capture.as_ref() else {
            return Ok(None);
        };
        self.control_fields_into(property, order, owner, fields)
            .map(Some)
    }

    pub(crate) fn with_record_producer<T>(
        &self,
        object: &crate::native::ObjectRecord,
        id: &str,
        construct: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let Some(sources) = self.object_sources else {
            return construct();
        };
        let source = sources.get(object.id()).ok_or_else(|| {
            CodecError::malformed("derived record has no original object supplier")
        })?;
        let ((actual_owner, actual_kind), record) =
            source.feature_record_owner(self.context, id)?;
        if actual_owner != *object.id() || actual_kind != object.type_name {
            self.context.retire_record_owner(&record)?;
            return Err(CodecError::malformed("derived source declaration differs"));
        }
        let previous = self.feature_owner.replace(Some(record));
        let result = construct();
        let record = self.feature_owner.replace(previous);
        if result.is_err() {
            if let Some(record) = record {
                self.context.retire_record_owner(&record)?;
            }
        }
        result
    }

    pub(crate) fn feature_field(
        &self,
        property: &PropertyRecord,
        tag: &str,
        attribute: &str,
    ) -> Result<Option<Option<String>>, CodecError> {
        if self.property_sources.is_none() {
            return Ok(None);
        }
        let capture = self.feature_owner.borrow();
        let Some(owner) = capture.as_ref() else {
            return Ok(None);
        };
        if property.body.is_none() {
            self.unavailable(property)?;
            return Ok(Some(None));
        }
        let mut roots = property
            .values()
            .iter()
            .filter(|value| value.parent.is_none() && value.tag == tag);
        let Some(root) = roots.next() else {
            return Ok(Some(None));
        };
        if roots.next().is_some() {
            return Ok(Some(None));
        }
        let mut source = if tag == "String" && attribute == "value" {
            self.control_fields_into(property, root.order, owner, &["value", "Value"])?
        } else {
            self.control_fields_into(property, root.order, owner, &[attribute])?
        };
        let value = source
            .attributes
            .remove(attribute)
            .or_else(|| {
                (tag == "String" && attribute == "value")
                    .then(|| source.attributes.remove("Value"))
                    .flatten()
            })
            .or_else(|| {
                (tag == "String" && attribute == "value")
                    .then_some(source.text)
                    .flatten()
            });
        Ok(Some(value))
    }

    pub(crate) fn parameter_ordinal(
        &self,
        property: &PropertyRecord,
        order: usize,
    ) -> Result<usize, CodecError> {
        let admitted = property
            .values()
            .iter()
            .find(|value| value.order == order)
            .ok_or_else(|| CodecError::malformed("parameter row is not held"))?;
        let Some(ranges) = self.property_sources else {
            return order
                .checked_sub(
                    admitted
                        .parent
                        .and_then(|parent| parent.checked_add(1))
                        .unwrap_or(0),
                )
                .ok_or_else(|| CodecError::malformed("parameter row order precedes its parent"));
        };
        let source = self
            .xml_source
            .ok_or_else(|| CodecError::malformed("control has no original document supplier"))?;
        let range = ranges
            .get(&property.id)
            .and_then(|ranges| {
                ranges
                    .value_ranges()
                    .find_map(|(ordinal, range)| (ordinal == order).then_some(range))
            })
            .ok_or_else(|| CodecError::malformed("parameter row has no original range"))?;
        let index = source
            .elements()
            .iter()
            .position(|element| {
                element.extent().start() == source.view().start() + range.start
                    && element.extent().end() == source.view().start() + range.end
            })
            .ok_or_else(|| CodecError::malformed("parameter row has no original element"))?;
        let parent = source.elements()[index]
            .parent()
            .ok_or_else(|| CodecError::malformed("parameter row has no parent"))?;
        Ok(source
            .children(parent)
            .take_while(|child| *child != index)
            .filter(|child| source.name(*child).is_ok_and(|tag| tag == admitted.tag))
            .count())
    }

    pub(crate) fn control_fields(
        &self,
        property: &PropertyRecord,
        order: usize,
        id: &str,
        fields: &[&str],
    ) -> Result<ControlSource<'arena>, CodecError> {
        self.read_control_fields(property, order, fields, ControlCapture::Primary(id))
    }

    pub(crate) fn control_fields_into(
        &self,
        property: &PropertyRecord,
        order: usize,
        owner: &RecordOwner<'_>,
        fields: &[&str],
    ) -> Result<ControlSource<'arena>, CodecError> {
        self.read_control_fields(property, order, fields, ControlCapture::Fragment(owner))
    }

    fn read_control_fields(
        &self,
        property: &PropertyRecord,
        order: usize,
        fields: &[&str],
        capture: ControlCapture<'_, '_>,
    ) -> Result<ControlSource<'arena>, CodecError> {
        let admitted = property
            .values()
            .iter()
            .find(|value| value.order == order)
            .ok_or_else(|| CodecError::malformed("parameter control is not held"))?;
        let Some(ranges) = self.property_sources else {
            let storage = admitted
                .attributes
                .iter()
                .filter(|(key, _)| fields.contains(&key.as_str()))
                .try_fold(0_u64, |total, (key, value)| {
                    total
                        .checked_add(cadmpeg_core::decode::u64_from_index(
                            std::mem::size_of::<(String, String)>() + key.len() + value.len(),
                        ))
                        .ok_or_else(|| CodecError::malformed("parameter field storage overflows"))
                })?;
            self.context
                .retain_bytes(storage, "typed parameter field index")?;
            return Ok(ControlSource {
                name: property.name.clone(),
                kind: property.type_name.clone(),
                attributes: admitted
                    .attributes
                    .iter()
                    .filter(|(key, _)| fields.contains(&key.as_str()))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                text: admitted.text.clone(),
                owner: None,
            });
        };
        let source = self
            .xml_source
            .ok_or_else(|| CodecError::malformed("control has no original document supplier"))?;
        let ranges = ranges
            .get(&property.id)
            .ok_or_else(|| CodecError::malformed("parameter has no original control ranges"))?;
        let range = ranges.record_range();
        let index = source
            .elements()
            .iter()
            .position(|element| {
                element.extent().start() == source.view().start() + range.start
                    && element.extent().end() == source.view().start() + range.end
            })
            .ok_or_else(|| CodecError::malformed("parameter control has no original element"))?;
        let range = ranges
            .value_ranges()
            .find_map(|(ordinal, range)| (ordinal == order).then_some(range))
            .ok_or_else(|| {
                CodecError::malformed("parameter control value has no original range")
            })?;
        let value_index = source
            .elements()
            .iter()
            .position(|element| {
                element.extent().start() == source.view().start() + range.start
                    && element.extent().end() == source.view().start() + range.end
            })
            .ok_or_else(|| CodecError::malformed("parameter value has no original element"))?;
        let read = || {
            let name = source
                .attribute(self.context, index, "name")?
                .ok_or_else(|| CodecError::malformed("parameter control source has no name"))?;
            let kind = source
                .attribute(self.context, index, "type")?
                .ok_or_else(|| CodecError::malformed("parameter control source has no runtime"))?;
            if name != property.name || kind != property.type_name {
                return Err(CodecError::malformed("parameter control header differs"));
            }
            let tag = source.read_name(value_index)?;
            if tag != admitted.tag {
                return Err(CodecError::malformed("parameter control carrier differs"));
            }
            self.context.retain_bytes(
                cadmpeg_core::decode::u64_from_index(fields.len())
                    .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                        String,
                        String,
                    )>(
                    )))
                    .ok_or_else(|| CodecError::malformed("parameter field storage overflows"))?,
                "parameter source field index",
            )?;
            let mut attributes = BTreeMap::new();
            for field in fields {
                if let Some(value) = crate::persistence::source::value_attribute(
                    self.context,
                    source,
                    value_index,
                    field,
                )? {
                    self.context.retain_bytes(
                        cadmpeg_core::decode::u64_from_index(field.len()),
                        "parameter source field key",
                    )?;
                    attributes.insert((*field).to_owned(), value);
                }
            }
            let text = if tag == "String"
                && fields
                    .iter()
                    .any(|field| matches!(*field, "value" | "Value"))
                && !attributes.contains_key("value")
                && !attributes.contains_key("Value")
            {
                Some(source.text(self.context, value_index)?)
            } else {
                None
            };
            Ok((name, kind, attributes, text))
        };
        let ((name, kind, attributes, text), owner) = match capture {
            ControlCapture::Primary(id) => self
                .context
                .record_owner(id, source.extent(index)?, read)
                .map(|(fields, owner)| (fields, Some(owner))),
            ControlCapture::Fragment(owner) => self
                .context
                .record_fragment(owner, source.extent(index)?, read)
                .map(|fields| (fields, None)),
        }?;
        Ok(ControlSource {
            name,
            kind,
            attributes,
            text,
            owner,
        })
    }

    pub(crate) fn retire_control(&self, source: &ControlSource<'_>) -> Result<(), CodecError> {
        if let Some(owner) = source.owner() {
            self.context.retire_record_owner(owner)?;
        }
        Ok(())
    }
}
