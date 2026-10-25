// SPDX-License-Identifier: Apache-2.0
//! Each decoded asset reads its own source fields; reconstruction is context free.
use super::*;
use crate::gui::payloads::sources::appearance::{self, Control};
use crate::native::entry_contents::EntryGrammar;
use cadmpeg_core::decode::View;

pub(super) struct ColorRequest<'a> {
    pub id: AppearanceId,
    pub provider: &'a str,
    pub property: &'a str,
    pub object: &'a str,
    pub entry: &'a str,
    pub index: usize,
    pub kind: TopologyColorKind,
    pub value: u32,
    pub alpha: bool,
}
pub(super) struct MaterialRequest<'a> {
    pub id: AppearanceId,
    pub provider: &'a str,
    pub property: &'a str,
    pub object: &'a str,
    pub entry: &'a str,
    pub version: u32,
    pub index: usize,
    pub value: &'a GuiMaterial,
    pub alpha: bool,
}
pub(super) trait Producer {
    fn presentation_document(
        &mut self,
        document: &GuiDocumentRecord,
        schema: Option<u32>,
        losses: &mut Vec<LossNote>,
    ) -> Result<PresentationDocument, CodecError>;
    fn presentation_view(
        &mut self,
        provider: &GuiViewProviderRecord,
        properties: &[&GuiPropertyRecord],
        hide_visible: bool,
    ) -> Result<ViewPresentation, CodecError>;

    fn color(&mut self, request: ColorRequest<'_>) -> Result<Appearance, CodecError>;
    fn material(&mut self, request: MaterialRequest<'_>) -> Result<Appearance, CodecError>;
    fn attribute(
        &mut self,
        request: material_attributes::Request<'_>,
    ) -> Result<Option<cadmpeg_ir::attributes::SourceAttribute>, CodecError>;
}
pub(super) struct Current;
impl Producer for Current {
    fn presentation_document(
        &mut self,
        document: &GuiDocumentRecord,
        schema: Option<u32>,
        losses: &mut Vec<LossNote>,
    ) -> Result<PresentationDocument, CodecError> {
        presentation::document(document, schema, losses)
    }
    fn presentation_view(
        &mut self,
        provider: &GuiViewProviderRecord,
        properties: &[&GuiPropertyRecord],
        hide_visible: bool,
    ) -> Result<ViewPresentation, CodecError> {
        presentation::view(provider, properties, hide_visible)
    }

    fn color(&mut self, request: ColorRequest<'_>) -> Result<Appearance, CodecError> {
        color(
            request.id,
            request.provider,
            request.entry,
            request.index,
            request.kind,
            request.value,
            request.alpha,
        )
    }
    fn material(&mut self, request: MaterialRequest<'_>) -> Result<Appearance, CodecError> {
        let mut asset = materials::material_appearance(
            request.id,
            request.provider,
            request.index,
            request.value,
        )?;
        asset.native_ref = Some(request.entry.to_owned());
        Ok(asset)
    }
    fn attribute(
        &mut self,
        request: material_attributes::Request<'_>,
    ) -> Result<Option<cadmpeg_ir::attributes::SourceAttribute>, CodecError> {
        material_attributes::from_material(request)
    }
}
pub(super) struct Decoded<'a, 'ctx, 'refs, 'source> {
    pub ctx: &'a DecodeContext<'ctx>,
    pub sources: &'a GuiSources<'refs, 'source>,
}
impl<'source> Decoded<'_, '_, '_, 'source> {
    fn source(&self, entry: &str, grammar: EntryGrammar) -> Result<View<'source>, CodecError> {
        self.sources
            .gui_entry_interpretations
            .map(|memo| memo.source(self.ctx, entry, grammar))
            .transpose()?
            .flatten()
            .or(self
                .sources
                .entry_interpretations
                .map(|memo| memo.source(self.ctx, entry, grammar))
                .transpose()?
                .flatten())
            .ok_or_else(|| CodecError::malformed("appearance has no original entry supplier"))
    }
    fn control(&self, property: &str) -> Result<&Control<'source>, CodecError> {
        self.sources
            .gui_entry_interpretations
            .and_then(|memo| memo.appearance_control(property))
            .or_else(|| {
                self.sources
                    .entry_interpretations
                    .and_then(|memo| memo.appearance_control(property))
            })
            .ok_or_else(|| CodecError::malformed("appearance has no original XML control supplier"))
    }
    fn row(
        &self,
        property: &str,
        object: &str,
        entry: &str,
        grammar: EntryGrammar,
        index: usize,
    ) -> Result<(appearance::Selection, View<'source>), CodecError> {
        let control = self.control(property)?.read(self.ctx, grammar)?;
        if control.entry != entry {
            return Err(CodecError::malformed(
                "appearance source member changed during construction",
            ));
        }
        if let Some(source) = self
            .sources
            .object_sources
            .and_then(|sources| sources.get(object))
        {
            if source.object_id(self.ctx)? != object {
                return Err(CodecError::malformed(
                    "appearance application object changed during construction",
                ));
            }
        }
        let source = self.source(&control.entry, grammar)?;
        if index >= appearance::count(source, grammar)? {
            return Err(CodecError::malformed(
                "appearance ordinal escapes its actual count",
            ));
        }
        Ok((control, appearance::row(source, grammar, index)?))
    }
}
impl Producer for Decoded<'_, '_, '_, '_> {
    fn presentation_document(
        &mut self,
        document: &GuiDocumentRecord,
        _schema: Option<u32>,
        losses: &mut Vec<LossNote>,
    ) -> Result<PresentationDocument, CodecError> {
        let control = self
            .sources
            .gui_entry_interpretations
            .and_then(|memo| memo.presentation_control(&document.id))
            .ok_or_else(|| {
                CodecError::malformed("GUI document has no original presentation supplier")
            })?;
        let id = PresentationId::compose(
            &cadmpeg_ir::identity_namespace!("fcstd", "presentation", "document"),
            cadmpeg_ir::identity_key!("0"),
        );
        self.ctx
            .record(id.as_str(), control.primary(self.ctx)?, || {
                let document = control.read_document(self.ctx, document, self.sources)?;
                let schema =
                    schema::classify(document.schema_version.as_deref()).neutral_schema_version();
                presentation::document(&document, schema, losses)
            })
    }
    fn presentation_view(
        &mut self,
        provider: &GuiViewProviderRecord,
        properties: &[&GuiPropertyRecord],
        hide_visible: bool,
    ) -> Result<ViewPresentation, CodecError> {
        let memo = self.sources.gui_entry_interpretations.ok_or_else(|| {
            CodecError::malformed("GUI provider has no presentation source overlay")
        })?;
        let control = memo.presentation_control(&provider.id).ok_or_else(|| {
            CodecError::malformed("GUI provider has no original presentation supplier")
        })?;
        let id = presentation::view_id(provider)?;
        self.ctx
            .record(id.as_str(), control.primary(self.ctx)?, || {
                let mut provider = control.read_provider(self.ctx, provider)?;
                provider.id = crate::native::native_id("gui-view-provider", &provider.name);
                if let Some(object) = &provider.object {
                    if let Some(source) = self
                        .sources
                        .object_sources
                        .and_then(|sources| sources.get(object.as_str()))
                    {
                        provider.object = Some(
                            cadmpeg_core::text::NonBlankString::new(source.object_id(self.ctx)?)
                                .ok_or_else(|| {
                                    CodecError::malformed("presentation object identity is blank")
                                })?,
                        );
                    }
                }
                let mut values = Vec::new();
                for property in properties {
                    let used = matches!(
                        (property.name.as_str(), property.type_name.as_str()),
                        ("DisplayMode" | "SelectionStyle", "App::PropertyEnumeration")
                            | ("LineWidth" | "PointSize", "App::PropertyFloatConstraint")
                    ) || (!hide_visible
                        && property.name == "Visibility"
                        && property.type_name == "App::PropertyBool");
                    if used {
                        let control = memo.presentation_control(&property.id).ok_or_else(|| {
                            CodecError::malformed(
                                "GUI scalar has no original presentation supplier",
                            )
                        })?;
                        values.push(control.read_scalar_property(self.ctx, property)?);
                    }
                }
                presentation::view(&provider, &values.iter().collect::<Vec<_>>(), hide_visible)
            })
    }

    fn color(&mut self, request: ColorRequest<'_>) -> Result<Appearance, CodecError> {
        let grammar = EntryGrammar::Color;
        let primary =
            appearance::row(self.source(request.entry, grammar)?, grammar, request.index)?;
        self.ctx.record(request.id.as_str(), primary, || {
            let (control, mut row) = self.row(
                request.property,
                request.object,
                request.entry,
                grammar,
                request.index,
            )?;
            let expected = match request.kind {
                TopologyColorKind::Face => "DiffuseColor",
                TopologyColorKind::Edge => "LineColorArray",
                TopologyColorKind::Vertex => "PointColorArray",
            };
            if control.property != expected {
                return Err(CodecError::malformed(
                    "appearance color property selects another topology domain",
                ));
            }
            let id = topology_appearance_id(
                request.kind,
                &provider_identity_key(&control.provider),
                request.index,
            )?;
            if id != request.id {
                return Err(CodecError::malformed(
                    "appearance source identity changed during construction",
                ));
            }
            color(
                id,
                &control.provider,
                &control.entry,
                request.index,
                request.kind,
                row.req_u32_le()?,
                request.alpha,
            )
        })
    }
    fn material(&mut self, request: MaterialRequest<'_>) -> Result<Appearance, CodecError> {
        let grammar = EntryGrammar::Material(request.version);
        let primary =
            appearance::row(self.source(request.entry, grammar)?, grammar, request.index)?;
        self.ctx.record(request.id.as_str(), primary, || {
            let (control, mut row) = self.row(
                request.property,
                request.object,
                request.entry,
                grammar,
                request.index,
            )?;
            let mut value = GuiMaterial {
                ambient: convert_packed_alpha(row.req_u32_le()?, request.alpha),
                diffuse: convert_packed_alpha(row.req_u32_le()?, request.alpha),
                specular: convert_packed_alpha(row.req_u32_le()?, request.alpha),
                emissive: convert_packed_alpha(row.req_u32_le()?, request.alpha),
                shininess: payloads::finite32(&mut row)?,
                transparency: payloads::finite32(&mut row)?,
                image: None,
                image_path: None,
                uuid: None,
            };
            if request
                .value
                .uuid
                .as_ref()
                .is_some_and(|uuid| !uuid.is_empty())
            {
                let source = self
                    .sources
                    .gui_entry_interpretations
                    .map(|memo| {
                        memo.material_string(
                            self.ctx,
                            &control.entry,
                            control.version,
                            request.index,
                            2,
                        )
                    })
                    .transpose()?
                    .flatten()
                    .or(self
                        .sources
                        .entry_interpretations
                        .map(|memo| {
                            memo.material_string(
                                self.ctx,
                                &control.entry,
                                control.version,
                                request.index,
                                2,
                            )
                        })
                        .transpose()?
                        .flatten())
                    .ok_or_else(|| {
                        CodecError::malformed("material asset has no original UUID supplier")
                    })?;
                value.uuid = Some(material_attributes::string(self.ctx, source)?);
            }
            let id = shape_material_appearance_id(
                &provider_identity_key(&control.provider),
                request.index,
            )?;
            if id != request.id {
                return Err(CodecError::malformed(
                    "material source identity changed during construction",
                ));
            }
            let mut asset =
                materials::material_appearance(id, &control.provider, request.index, &value)?;
            asset.native_ref = Some(control.entry);
            Ok(asset)
        })
    }
    fn attribute(
        &mut self,
        request: material_attributes::Request<'_>,
    ) -> Result<Option<cadmpeg_ir::attributes::SourceAttribute>, CodecError> {
        material_attributes::decode(self.ctx, self.sources, request)
    }
}
fn color(
    id: AppearanceId,
    provider: &str,
    entry: &str,
    index: usize,
    kind: TopologyColorKind,
    value: u32,
    alpha: bool,
) -> Result<Appearance, CodecError> {
    Ok(Appearance {
        native_ref: Some(entry.to_owned()),
        id,
        name: Some(format!(
            "{provider} {}{} appearance",
            kind.name(),
            index + 1
        )),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(kind.schema().into()),
        category: None,
        base_color: Some(decode_color(convert_packed_alpha(value, alpha), None)?),
        textures: Vec::new(),
        properties: BTreeMap::new(),
    })
}
