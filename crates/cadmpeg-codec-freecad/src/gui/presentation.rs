// SPDX-License-Identifier: Apache-2.0
//! Neutral presentation construction shared by real source and current facts.
use super::*;

pub(super) fn view_id(provider: &GuiViewProviderRecord) -> Result<PresentationId, CodecError> {
    PresentationId::mint(crate::native::model_id(
        "presentation-view",
        &provider.id,
        "state",
    ))
    .map_err(CodecError::malformed)
}
pub(super) fn document(
    document: &GuiDocumentRecord,
    neutral_schema_version: Option<u32>,
    losses: &mut Vec<LossNote>,
) -> Result<PresentationDocument, CodecError> {
    let mut presentation = PresentationDocument::new(PresentationId::compose(
        &cadmpeg_ir::identity_namespace!("fcstd", "presentation", "document"),
        cadmpeg_ir::identity_key!("0"),
    ));
    presentation.schema_version = neutral_schema_version;
    presentation.native_ref = Some(document.id.clone());
    let mut states = Vec::new();
    let camera_count = document
        .states
        .iter()
        .filter(|state| state.kind == "Camera")
        .count();
    if camera_count > 1 {
        losses.push(FreecadLossCode::SourceGuiMetadataUnresolved.note(
            "GUI camera selection is unresolved because more than one Camera record is present",
        ));
    }
    for (order, state) in document.states.iter().enumerate() {
        let (attributes, refused) = gui_named_entries(
            || format!("the gui {} state", state.kind),
            state
                .attributes
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        );
        note_refused_gui_keys(losses, &refused);
        let kind = if state.kind == "Camera" && camera_count == 1 {
            match camera_state_value(state, losses) {
                Ok(camera) => PresentationStateKind::Camera(camera),
                Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })) => {
                    losses.push(
                        FreecadLossCode::SourceGuiMetadataUnresolved
                            .note(format!("GUI Camera remains native: {error}")),
                    );
                    PresentationStateKind::Native(state.kind.clone())
                }
                Err(error) => return Err(error),
            }
        } else {
            PresentationStateKind::Native(state.kind.clone())
        };
        let mut assets = Vec::new();
        for entry in &state.side_entries {
            assets.push(crate::native::native_id("entry", entry));
        }
        states.push(PresentationState {
            kind,
            order: u32::try_from(order)
                .map_err(|_| CodecError::malformed("GUI state order exceeds u32"))?,
            attributes,
            assets,
        });
    }
    presentation
        .set_states(states)
        .map_err(CodecError::malformed)?;
    Ok(presentation)
}
pub(super) fn view(
    provider: &GuiViewProviderRecord,
    owned: &[&GuiPropertyRecord],
    hide_visible: bool,
) -> Result<ViewPresentation, CodecError> {
    let property_value = |name: &str, type_name: &str| {
        owned
            .iter()
            .find(|property| property.name == name && property.type_name == type_name)
            .and_then(|property| gui_property_value(property))
    };
    let line_width = property_value("LineWidth", "App::PropertyFloatConstraint")
        .and_then(|value| value.parse::<f64>().ok());
    let point_size = property_value("PointSize", "App::PropertyFloatConstraint")
        .and_then(|value| value.parse::<f64>().ok());
    let line_width = line_width
        .map(|value| {
            cadmpeg_ir::scalar::NonNegativeReal::new(value)
                .ok_or_else(|| CodecError::malformed("line_width must be finite and nonnegative"))
        })
        .transpose()?;
    let point_size = point_size
        .map(|value| {
            cadmpeg_ir::scalar::NonNegativeReal::new(value)
                .ok_or_else(|| CodecError::malformed("point_size must be finite and nonnegative"))
        })
        .transpose()?;
    Ok(ViewPresentation {
        id: view_id(provider)?,
        object: provider
            .object
            .as_ref()
            .map(|object| object.as_str().to_owned()),
        order: u32::try_from(provider.order).map_err(|_| {
            CodecError::malformed(format_args!(
                "FreeCAD ordinal {} exceeds u32",
                provider.order
            ))
        })?,
        expanded: provider.expanded,
        visible: if hide_visible {
            None
        } else {
            property_value("Visibility", "App::PropertyBool").and_then(parse_bool)
        },
        display_mode: property_value("DisplayMode", "App::PropertyEnumeration")
            .map(ToOwned::to_owned),
        selection_style: property_value("SelectionStyle", "App::PropertyEnumeration")
            .map(ToOwned::to_owned),
        line_width,
        point_size,
        // Every original property remains typed in gui_properties. Neutral
        // presentation owns only its dedicated semantic fields (§8).
        properties: BTreeMap::new(),
        native_ref: Some(provider.id.clone()),
    })
}
pub(super) fn transfer(
    plan: &mut AppearancePlan,
    graph: &Graph,
    neutral_schema_version: Option<u32>,
    losses: &mut Vec<LossNote>,
    producer: &mut impl appearance_producer::Producer,
) -> Result<(), CodecError> {
    for document in &graph.documents {
        plan.presentation_documents
            .push(producer.presentation_document(document, neutral_schema_version, losses)?);
    }
    for provider in &graph.providers {
        let owned = graph
            .properties
            .iter()
            .filter(|property| property.owner == provider.id)
            .collect::<Vec<_>>();
        for property in owned
            .iter()
            .filter(|property| property.name.trim().is_empty())
        {
            losses.push(FreecadLossCode::SourceGuiPropertyKeyBlank.note(format!("GUI property {} states a property with a blank key; the property value is not transferred",property.id)));
        }
        let hide_visible = provider
            .object
            .as_ref()
            .is_some_and(|object| plan.body_visibility_owners.contains(object.as_str()));
        plan.view_presentations
            .push(producer.presentation_view(provider, &owned, hide_visible)?);
    }
    Ok(())
}
