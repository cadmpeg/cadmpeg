// SPDX-License-Identifier: Apache-2.0
//! Patch eligibility compares the current typed graph with its neutral view.
//!
//! This module reads no retained source image, historical digest, or report.

use crate::{brep, native};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{ArenaName, CadIr};
use std::collections::BTreeMap;

pub(super) fn agrees(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<(), CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return decline("there is no typed FreeCAD graph");
    };
    let objects = namespace.arena_as::<native::ObjectRecord>("objects")?;
    let properties = namespace.arena_as::<native::PropertyRecord>("properties")?;
    let property_reader = crate::property_reader::PropertyReader::typed(ctx);
    let entry_views = BTreeMap::new();
    let entry_sources = crate::entry_sources::EntrySources::new(&entry_views);
    for property in &properties {
        if super::super::xml::native(property)?.is_none() {
            return decline("the typed property has no independent projection");
        }
    }
    let payloads = namespace.arena_as::<brep::ShapePayloadRecord>("shape_payloads")?;
    let products = namespace.arena_as::<native::ProductNodeRecord>("product_nodes")?;
    let joints = namespace.arena_as::<native::joint::JointRecord>("joints")?;
    let drawings = namespace.arena_as::<native::DrawingRecord>("drawings")?;
    let annotations = namespace.arena_as::<native::SemanticAnnotationRecord>("annotations")?;
    let documents = namespace.arena_as::<native::DocumentFacts>("document")?;
    let version = documents
        .first()
        .and_then(|document| document.program_version.as_deref());
    let mut projected = CadIr::empty();
    projected.native.clone_from(&ir.native);
    projected.source.clone_from(&ir.source);
    let mut curves = brep::transfer_text_curves(ctx, &payloads, &properties)?;
    let surfaces = brep::transfer_text_surfaces(ctx, &payloads, &properties, &mut curves)?;
    projected.model.curves = curves.curves;
    projected
        .model
        .add_procedural_curves(curves.procedural)?
        .map_err(CodecError::malformed)?;
    projected.model.surfaces = surfaces.surfaces;
    projected
        .model
        .add_procedural_surfaces(surfaces.procedural)?
        .map_err(CodecError::malformed)?;
    let mut losses = Vec::new();
    let topology_occurrences = crate::topology_transfer::transfer_projection(
        ctx,
        &mut projected,
        &payloads,
        &properties,
        &mut losses,
    )?;
    crate::topology_transfer::apply_body_names(ctx, &mut projected, &payloads, &properties)?;
    if ir.scope.design {
        let extensions = namespace.arena_as::<native::ExtensionRecord>("extensions")?;
        crate::design::transfer(
            &property_reader,
            &mut projected,
            crate::design::DesignObjects {
                objects: &objects,
                extensions: &extensions,
            },
            &properties,
            &payloads,
            crate::entry_sources::ArchiveEntries {
                records: &[],
                sources: &entry_sources,
            },
            version,
        )?;
    }
    let (definitions, occurrences) = crate::product::transfer_neutral(
        &property_reader,
        &products,
        &joints,
        &objects,
        &properties,
        &payloads,
        &projected.model.bodies,
    )?;
    projected.model.product_definitions = definitions;
    projected.model.occurrences = occurrences;
    projected.model.assembly_joints =
        crate::joint::transfer_neutral(&joints, &projected.model.occurrences)?;
    crate::drawing::transfer_neutral(
        &property_reader,
        &mut projected.model,
        &drawings,
        &properties,
    )?;
    crate::annotation::transfer_neutral(
        &property_reader,
        &mut projected.model,
        &annotations,
        &properties,
        &drawings,
    )?;
    crate::writer::ordinals::apply(&properties, &mut projected)?;
    if let Some(gui) = gui_xml(ctx, namespace)? {
        let entries = BTreeMap::new();
        crate::gui::transfer_rendered(
            ctx,
            &mut projected,
            gui.xml.as_bytes(),
            &crate::gui::GuiSources {
                entries: &entries,
                objects: &objects,
                properties: &properties,
                payloads: &payloads,
                topology_occurrences: &topology_occurrences,
                requires_alpha_conversion: crate::gui::requires_alpha_conversion(version),
            },
            &gui.origins,
        )?;
    }
    crate::writer::body_styles::apply(&mut projected, &payloads, &properties)?;
    crate::attributes::transfer(ctx, &mut projected, &objects, &properties)?;
    let current = serde_json::to_value(&ir.model).map_err(CodecError::malformed)?;
    let candidate = serde_json::to_value(&projected.model).map_err(CodecError::malformed)?;
    for arena in ArenaName::ALL {
        if ir.scope.holds(arena.class()) && current[arena.as_str()] != candidate[arena.as_str()] {
            return decline(&format!(
                "current typed FreeCAD projection disagrees with neutral arena {}",
                arena.as_str()
            ));
        }
    }
    if ir.tolerances != projected.tolerances {
        return decline("current typed FreeCAD projection has different document tolerances");
    }
    Ok(())
}

fn decline<T>(reason: &str) -> Result<T, CodecError> {
    Err(CodecError::UnsupportedContent(reason.to_owned()))
}

pub(in crate::writer) struct RenderedGui {
    pub(in crate::writer) xml: String,
    pub(in crate::writer) origins: crate::gui::RenderedGuiOrigins,
}

trait GuiOriginObserver {
    fn provider(&mut self) -> Result<(), CodecError>;
    fn property(&mut self, id: &str) -> Result<(), CodecError>;
}

struct GuiOriginCollector<'ctx, 'input> {
    ctx: &'ctx DecodeContext<'input>,
    origins: crate::gui::RenderedGuiOrigins,
}

impl GuiOriginObserver for GuiOriginCollector<'_, '_> {
    fn provider(&mut self) -> Result<(), CodecError> {
        self.origins.provider(self.ctx)
    }
    fn property(&mut self, id: &str) -> Result<(), CodecError> {
        self.origins.property(self.ctx, id)
    }
}

pub(in crate::writer) fn gui_xml(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<Option<RenderedGui>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut observer = GuiOriginCollector {
        ctx,
        origins: crate::gui::RenderedGuiOrigins::default(),
    };
    Ok(
        gui_xml_impl(namespace, None, &mut observer)?.map(|xml| RenderedGui {
            xml,
            origins: observer.origins,
        }),
    )
}

pub(in crate::writer) fn gui_xml_lossy(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<RenderedGui>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut observer = GuiOriginCollector {
        ctx,
        origins: crate::gui::RenderedGuiOrigins::default(),
    };
    Ok(
        gui_xml_impl(namespace, Some(losses), &mut observer)?.map(|xml| RenderedGui {
            xml,
            origins: observer.origins,
        }),
    )
}

fn gui_xml_impl(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    mut losses: Option<&mut Vec<cadmpeg_ir::report::loss::LossNote>>,
    observer: &mut impl GuiOriginObserver,
) -> Result<Option<String>, CodecError> {
    use crate::writer::xml;
    let documents = namespace.arena_as::<native::GuiDocumentRecord>("gui_documents")?;
    let providers = namespace.arena_as::<native::GuiViewProviderRecord>("gui_view_providers")?;
    let properties = namespace.arena_as::<native::GuiPropertyRecord>("gui_properties")?;
    let Some(document) = documents.first() else {
        return Ok(None);
    };
    if documents.len() != 1 {
        if let Some(losses) = losses.as_deref_mut() {
            losses.push(crate::loss::FreecadLossCode::ExportArenaNotTransferred.note("multiple native GUI documents share one target document; additional GUI roots are omitted"));
        } else {
            return decline("multiple typed GUI documents require synthesis");
        }
    }
    let mut output = format!(
        "<Document SchemaVersion=\"{}\"><ViewProviderData Count=\"{}\">",
        xml::escaped(document.schema_version.as_deref().unwrap_or("1")),
        providers.len()
    );
    for provider in &providers {
        observer.provider()?;
        let mut encoded_properties = Vec::new();
        for property in properties
            .iter()
            .filter(|property| property.owner == provider.id)
        {
            let Some(value) = xml::native_gui(property)? else {
                if let Some(losses) = losses.as_deref_mut() {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportNativePropertyNotTransferred.note(
                            format!(
                                "GUI property {} has no independent typed projection",
                                property.id
                            ),
                        ),
                    );
                    continue;
                }
                return decline("GUI property needs source-only payload");
            };
            observer.property(&property.id)?;
            encoded_properties.push(value);
        }
        output.push_str("<ViewProvider name=\"");
        output.push_str(&xml::escaped(&provider.name));
        output.push('"');
        if let Some(expanded) = provider.expanded {
            output.push_str(" expanded=\"");
            output.push_str(&u8::from(expanded).to_string());
            output.push('"');
        }
        output.push_str("><Properties Count=\"");
        output.push_str(&encoded_properties.len().to_string());
        output.push_str("\">");
        for value in encoded_properties {
            output.push_str(&value);
        }
        output.push_str("</Properties></ViewProvider>");
    }
    output.push_str("</ViewProviderData>");
    for state in &document.states {
        if !state.values.is_empty() || !state.side_entries.is_empty() {
            if let Some(losses) = losses.as_deref_mut() {
                losses.push(
                    crate::loss::FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "GUI state {} has no independent neutral projection",
                        state.id
                    )),
                );
                continue;
            }
            return decline("nested GUI state needs an independent typed projection");
        }
        let attributes = state
            .attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>();
        output.push_str(&xml::element(&state.kind, &attributes));
    }
    output.push_str("</Document>");
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    const GUI_SOURCE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
    ));

    struct IgnoreOrigins;
    impl GuiOriginObserver for IgnoreOrigins {
        fn provider(&mut self) -> Result<(), CodecError> {
            Ok(())
        }
        fn property(&mut self, _: &str) -> Result<(), CodecError> {
            Ok(())
        }
    }

    fn gui_document() -> CadIr {
        crate::FcstdCodec
            .decode(&mut Cursor::new(GUI_SOURCE), &DecodeOptions::default())
            .unwrap()
            .ir()
            .clone()
    }

    #[test]
    fn emitted_gui_origin_observation_preserves_exact_existing_xml_bytes_and_losses() {
        let ir = gui_document();
        let namespace = ir.native.namespace("fcstd").unwrap();
        let mut original_losses = Vec::new();
        let original = gui_xml_impl(namespace, Some(&mut original_losses), &mut IgnoreOrigins)
            .unwrap()
            .unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut actual_losses = Vec::new();
        let actual = gui_xml_lossy(&ctx, namespace, &mut actual_losses)
            .unwrap()
            .unwrap();
        assert_eq!(actual.xml.as_bytes(), original.as_bytes());
        assert_eq!(actual_losses, original_losses);
        let providers = namespace
            .arena_as::<native::GuiViewProviderRecord>("gui_view_providers")
            .unwrap();
        let properties = namespace
            .arena_as::<native::GuiPropertyRecord>("gui_properties")
            .unwrap();
        for (provider_index, provider) in providers.iter().enumerate() {
            let emitted = properties
                .iter()
                .filter(|property| property.owner == provider.id)
                .filter(|property| crate::writer::xml::native_gui(property).unwrap().is_some())
                .collect::<Vec<_>>();
            for (index, property) in emitted.iter().enumerate() {
                assert_eq!(
                    actual.origins.property_id(provider_index, index),
                    Some(property.id.as_str())
                );
            }
            assert!(actual
                .origins
                .property_id(provider_index, emitted.len())
                .is_none());
        }
    }

    #[test]
    fn skipped_duplicate_gui_rows_preserve_emitted_order_and_actual_opaque_native_ids() {
        let mut ir = gui_document();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut losses = Vec::new();
        let original = gui_xml_lossy(&ctx, ir.native.namespace("fcstd").unwrap(), &mut losses)
            .unwrap()
            .unwrap();
        let original_losses = losses.len();
        let mut properties = ir
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<native::GuiPropertyRecord>("gui_properties")
            .unwrap();
        let mut duplicate = properties
            .iter()
            .find(|property| property.name == "LineColorArray")
            .unwrap()
            .clone();
        duplicate.id = "opaque:native:GUI-field#skipped-duplicate".to_owned();
        assert!(crate::writer::xml::native_gui(&duplicate)
            .unwrap()
            .is_none());
        let side_entries = duplicate.side_entries.clone();
        let duplicate_id = duplicate.id.clone();
        properties.push(duplicate);
        let mut entries = ir
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<native::EntryRecord>("entries")
            .unwrap();
        for name in &side_entries {
            entries
                .iter_mut()
                .find(|entry| entry.name() == name)
                .unwrap()
                .add_reference(&duplicate_id)
                .unwrap();
        }
        let namespace = ir.native.namespace_mut("fcstd");
        namespace
            .set_arena(&ctx, "gui_properties", &properties)
            .unwrap();
        namespace.set_arena(&ctx, "entries", &entries).unwrap();
        assert!(crate::test_support::validate_native(&ir).is_empty());
        let namespace = ir.native.namespace("fcstd").unwrap();
        let mut actual_losses = Vec::new();
        let rendered = gui_xml_lossy(&ctx, namespace, &mut actual_losses)
            .unwrap()
            .unwrap();
        assert_eq!(rendered.xml.as_bytes(), original.xml.as_bytes());
        assert_eq!(actual_losses.len(), original_losses + 1);
        assert!(actual_losses
            .iter()
            .any(|loss| loss.message.contains(&duplicate_id)));
        let providers = namespace
            .arena_as::<native::GuiViewProviderRecord>("gui_view_providers")
            .unwrap();
        let properties = namespace
            .arena_as::<native::GuiPropertyRecord>("gui_properties")
            .unwrap();
        for (provider_index, provider) in providers.iter().enumerate() {
            let emitted = properties
                .iter()
                .filter(|property| property.owner == provider.id)
                .filter(|property| crate::writer::xml::native_gui(property).unwrap().is_some())
                .collect::<Vec<_>>();
            for (index, property) in emitted.iter().enumerate() {
                assert_eq!(
                    rendered.origins.property_id(provider_index, index),
                    Some(property.id.as_str())
                );
                assert_ne!(
                    rendered.origins.property_id(provider_index, index),
                    Some(duplicate_id.as_str())
                );
            }
        }
    }
}

#[cfg(test)]
mod native_gui_projection_tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    #[test]
    fn rendered_gui_opaque_refs_are_working_only_in_the_actual_encode_context() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
        ));
        let mut ir = crate::FcstdCodec
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .unwrap()
            .ir()
            .clone();
        let setup = cadmpeg_test_support::service_decode_context();
        let mut fields = ir
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<native::GuiPropertyRecord>("gui_properties")
            .unwrap();
        let field = fields
            .iter_mut()
            .find(|field| field.name == "LineColor")
            .unwrap();
        field.id = "opaque:native:GUI-property#actual-rendered-line".to_owned();
        let opaque = field.id.clone();
        ir.native
            .namespace_mut("fcstd")
            .set_arena(&setup, "gui_properties", &fields)
            .unwrap();
        let native_before = serde_json::to_value(&ir.native).unwrap();
        let namespace = ir.native.namespace("fcstd").unwrap();
        let objects = namespace
            .arena_as::<native::ObjectRecord>("objects")
            .unwrap();
        let properties = namespace
            .arena_as::<native::PropertyRecord>("properties")
            .unwrap();
        let payloads = namespace
            .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
            .unwrap();
        let documents = namespace
            .arena_as::<native::DocumentFacts>("document")
            .unwrap();
        let version = documents
            .first()
            .and_then(|document| document.program_version.as_deref());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        policy.limits.max_retained_bytes = 0;
        let ctx = DecodeContext::for_encode(&arena, &policy);
        assert!(ctx.is_encoding());
        let mut losses = Vec::new();
        let rendered = gui_xml_lossy(&ctx, namespace, &mut losses)
            .unwrap()
            .unwrap();
        let mut projected = ir.clone();
        projected.model.appearances.clear();
        projected.model.appearance_bindings.clear();
        let entries = BTreeMap::new();
        let sources = crate::gui::GuiSources {
            entries: &entries,
            objects: &objects,
            properties: &properties,
            payloads: &payloads,
            topology_occurrences: &[],
            requires_alpha_conversion: crate::gui::requires_alpha_conversion(version),
        };
        crate::gui::transfer_rendered(
            &ctx,
            &mut projected,
            rendered.xml.as_bytes(),
            &sources,
            &rendered.origins,
        )
        .unwrap();
        assert!(projected
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.native_ref.as_deref() == Some(opaque.as_str())));
        assert!(projected
            .model
            .appearance_bindings
            .iter()
            .any(|binding| binding.native_ref.as_deref() == Some(opaque.as_str())));
        assert_eq!(
            serde_json::to_value(&projected.native).unwrap(),
            native_before
        );
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().unwrap();
    }
}
