// SPDX-License-Identifier: Apache-2.0
//! Transient exact-shape source associations from actual property owners.

use crate::native::{ExtensionRecord, GuiPropertyRecord, GuiViewProviderRecord, PropertyRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(crate) type GuiOwners = BTreeMap<String, Option<NonBlankString>>;

pub(crate) fn gui(
    ctx: &DecodeContext<'_>,
    namespace: Option<&cadmpeg_ir::native::NativeNamespace>,
) -> Result<GuiOwners, CodecError> {
    let Some(namespace) = namespace else {
        return Ok(BTreeMap::new());
    };
    let properties = namespace.arena_as_for_decode::<GuiPropertyRecord>(ctx, "gui_properties")?;
    let providers =
        namespace.arena_as_for_decode::<GuiViewProviderRecord>(ctx, "gui_view_providers")?;
    let extensions = namespace.arena_as_for_decode::<ExtensionRecord>(ctx, "extensions")?;
    let mut owners = BTreeMap::new();
    for property in properties.into_iter().filter(|property| {
        matches!(
            property.type_name.as_str(),
            "Part::PropertyPartShape" | "Part::PropertyTopoShapeList"
        )
    }) {
        let provider = extensions
            .iter()
            .find(|extension| extension.id == property.owner)
            .map_or(property.owner.as_str(), |extension| {
                extension.owner.as_str()
            });
        let object = providers
            .iter()
            .find(|record| record.id == provider)
            .and_then(|record| record.object.as_ref());
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                String,
                Option<NonBlankString>,
            )>()),
            "GUI exact-shape owner pair",
        )?;
        if let Some(object) = object {
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(object.as_str().len()),
                "GUI exact-shape object reference",
            )?;
        }
        owners.insert(property.id, object.cloned());
    }
    Ok(owners)
}

pub(crate) struct Properties<'a> {
    application: &'a [PropertyRecord],
    gui: Option<&'a GuiOwners>,
}
impl<'a> Properties<'a> {
    pub(crate) fn new(application: &'a [PropertyRecord], gui: &'a GuiOwners) -> Self {
        Self {
            application,
            gui: Some(gui),
        }
    }
    pub(crate) fn owner(
        &self,
        ctx: &DecodeContext<'_>,
        property: &str,
    ) -> Result<Option<NonBlankString>, CodecError> {
        if let Some(owner) = self.gui.and_then(|gui| gui.get(property)) {
            if let Some(owner) = owner {
                ctx.retain_bytes(
                    cadmpeg_core::decode::u64_from_index(owner.as_str().len()),
                    "exact-shape source association",
                )?;
            }
            return Ok(owner.clone());
        }
        let owner = self
            .application
            .iter()
            .find(|record| record.id == property)
            .map_or(property, |record| record.owner.as_str());
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(owner.len()),
            "exact-shape source association",
        )?;
        NonBlankString::new(owner.to_owned())
            .map(Some)
            .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))
    }
}
impl<'a> From<&'a [PropertyRecord]> for Properties<'a> {
    fn from(application: &'a [PropertyRecord]) -> Self {
        Self {
            application,
            gui: None,
        }
    }
}
impl<'a> From<&'a Vec<PropertyRecord>> for Properties<'a> {
    fn from(application: &'a Vec<PropertyRecord>) -> Self {
        Self::from(application.as_slice())
    }
}
impl<'a, const N: usize> From<&'a [PropertyRecord; N]> for Properties<'a> {
    fn from(application: &'a [PropertyRecord; N]) -> Self {
        Self::from(application.as_slice())
    }
}
