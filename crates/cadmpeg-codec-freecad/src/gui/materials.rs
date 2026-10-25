// SPDX-License-Identifier: Apache-2.0
//! Material assets and bindings from the current typed entry interpretation.
use super::*;

pub(super) fn transfer_shape_appearances(
    ir: &CadIr,
    plan: &mut AppearancePlan,
    graph: &Graph,
    material_lists: &HashMap<String, payloads::MaterialList>,
    sources: &GuiSources<'_, '_>,
    losses: &mut Vec<LossNote>,
    producer: &mut impl appearance_producer::Producer,
) -> Result<(), CodecError> {
    let properties = sources.properties;
    let payloads = sources.payloads;
    let topology_occurrences = sources.topology_occurrences;
    for provider in &graph.providers {
        let provider_key = provider_identity_key(&provider.name);
        let Some(object_id) = provider
            .object
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str)
        else {
            continue;
        };
        let Some(property) = graph.properties.iter().find(|property| {
            property.owner == provider.id
                && property.name == "ShapeAppearance"
                && property.type_name == "App::PropertyMaterialList"
        }) else {
            continue;
        };
        let Some(materials) = material_lists.get(&property.id) else {
            continue;
        };
        let body_ids = displayed_shape_bodies(ir, object_id, properties, payloads)?;
        let group = displayed_shape_topology(
            object_id,
            properties,
            payloads,
            topology_occurrences,
            "Face",
        )?;
        let mapped_count = group.len();
        if materials.count == 1 && materials.materials.contains_key(&0) {
            let legacy_id = object_appearance_id(&provider_key)?;
            plan.bindings
                .retain(|binding| binding.appearance != legacy_id);
            plan.appearances
                .retain(|appearance| appearance.id != legacy_id);
            plan.remove_appearances.insert(legacy_id);
        }
        let bind_faces =
            materials.count != 1 && mapped_count > 0 && materials.count == mapped_count;
        if materials.count != 1 && !bind_faces {
            push_gui_appearance_loss(losses, FreecadLossCode::AppearanceTopologyColorCountMismatch, format_args!(
                        "FCStd provider {} ShapeAppearance material count {} does not match {} mapped Face subelements; native material list retained and neutral face override withheld",
                        provider.name, materials.count, mapped_count
                    ), SourceProvenance::in_stream(
                        "fcstd",
                        cadmpeg_ir::stream_name!("GuiDocument.xml"),
                        cadmpeg_core::decode::u64_from_index(
                            graph.property_sources.get(&property.id).map_or(0, |source| source.record_range().start),
                        ),
                    )
                    .with_tag(property.id.clone()));
        }
        for (&index, material) in &materials.materials {
            let appearance_id = shape_material_appearance_id(&provider_key, index)?;
            let appearance = producer.material(appearance_producer::MaterialRequest {
                id: appearance_id.clone(),
                provider: &provider.name,
                property: &property.id,
                object: object_id,
                entry: &materials.entry,
                version: materials.version,
                index,
                value: material,
                alpha: sources.requires_alpha_conversion,
            })?;
            if appearance.base_color.is_none() {
                losses.push(FreecadLossCode::SourceGuiMetadataUnresolved.note(format!("GUI provider {} material {index} has transparency outside [0, 1]; its typed material fields and appearance are retained without a derived base color",provider.name)));
            }
            if let Some(attribute) = producer.attribute(material_attributes::Request {
                property: &property.id,
                name: &property.name,
                entry: &materials.entry,
                version: materials.version,
                index,
                material,
                appearance: &appearance,
            })? {
                plan.attributes.push(attribute);
            }
            plan.appearances.push(appearance);
            if materials.count == 1 {
                for (body_index, body) in body_ids.iter().enumerate() {
                    plan.bindings.push(AppearanceBinding {
                        native_ref: None,
                        id: binding_id(format_args!(
                            "fcstd:appearance:binding#shape-material:{provider_key}:{body_index}"
                        ))?,
                        target: AppearanceTarget::Body(body.clone()),
                        appearance: appearance_id.clone(),
                        source_entity_id: Some(object_id.to_owned()),
                        object_type: Some("ViewProvider ShapeAppearance".into()),
                        visible: None,
                        channels: BTreeMap::new(),
                    });
                }
            } else if let Some(topology_ids) = group.get(index).filter(|_| bind_faces) {
                bind_material_faces(
                    ir,
                    plan,
                    topology_ids,
                    &appearance_id,
                    (&provider_key, object_id),
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn material_appearance(
    id: AppearanceId,
    provider_name: &str,
    index: usize,
    material: &GuiMaterial,
) -> Result<Appearance, CodecError> {
    let scalar = |name: &str, value: f64| {
        cadmpeg_ir::scalar::FiniteReal::new(value)
            .ok_or_else(|| CodecError::malformed(format_args!("GUI material {name} is non-finite")))
    };
    Ok(Appearance {
        native_ref: None,
        id,
        name: Some(format!("{provider_name} face {} material", index + 1)),
        asset_guid: material
            .uuid
            .as_ref()
            .filter(|uuid| !uuid.is_empty())
            .cloned(),
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ShapeAppearance".into()),
        category: None,
        base_color: if (0.0..=1.0).contains(&material.transparency.get()) {
            Some(decode_color(
                material.diffuse,
                Some(material.transparency.get()),
            )?)
        } else {
            None
        },
        textures: Vec::new(),
        properties: [
            (
                cadmpeg_core::nonblank_literal!("ambient_packed"),
                scalar("ambient_packed", f64::from(material.ambient))?,
            ),
            (
                cadmpeg_core::nonblank_literal!("diffuse_packed"),
                scalar("diffuse_packed", f64::from(material.diffuse))?,
            ),
            (
                cadmpeg_core::nonblank_literal!("specular_packed"),
                scalar("specular_packed", f64::from(material.specular))?,
            ),
            (
                cadmpeg_core::nonblank_literal!("emissive_packed"),
                scalar("emissive_packed", f64::from(material.emissive))?,
            ),
            (
                cadmpeg_core::nonblank_literal!("shininess"),
                material.shininess.into(),
            ),
            (
                cadmpeg_core::nonblank_literal!("transparency"),
                material.transparency.into(),
            ),
        ]
        .into(),
    })
}

fn bind_material_faces(
    ir: &CadIr,
    plan: &mut AppearancePlan,
    topology_ids: &[&str],
    appearance_id: &AppearanceId,
    provider: (&IdentityKey, &str),
) -> Result<(), CodecError> {
    let (provider_key, object_id) = provider;
    let mut bound = HashSet::new();
    for &topology_id in topology_ids {
        if !bound.insert(topology_id) {
            continue;
        }
        let Some(face) = ir
            .model
            .faces
            .iter()
            .find(|face| face.id.as_str() == topology_id)
        else {
            continue;
        };
        let face = face.id.clone();
        let binding_index = ir.model.appearance_bindings.len() + plan.bindings.len();
        plan.bindings.push(AppearanceBinding {
            native_ref: None,
            id: binding_id(format_args!(
                "fcstd:appearance:binding#shape-material:{provider_key}:{binding_index}"
            ))?,
            target: AppearanceTarget::Face(face),
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some("ViewProvider ShapeAppearance".into()),
            visible: None,
            channels: [(
                cadmpeg_core::nonblank_literal!("precedence"),
                "face_over_object".into(),
            )]
            .into(),
        });
    }
    Ok(())
}
