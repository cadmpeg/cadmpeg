// SPDX-License-Identifier: Apache-2.0
//! Wire adapters for checked palette records.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PartColorTableWire {
    /// Globally unique table identity.
    pub id: String,
    /// Registered `UGS::COLOR_table` declaration in `class_definitions`.
    pub class_definition: String,
    /// Name of the separately encoded background color.
    pub background_name: String,
    /// Normalized background RGB components.
    pub background_rgb: [f32; 3],
    /// Exact serialized background component atoms.
    pub raw_background_components: [Vec<u8>; 3],
    /// Absolute file offsets of the background component atoms.
    pub background_component_source_offsets: [u64; 3],
    /// Ordered entries in the native `part_color_definitions` arena.
    pub definitions: Vec<String>,
    /// Directory entry containing the table.
    pub source_entry: String,
    /// Absolute file offset of the counted name roster.
    pub source_offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PartColorDefinitionWire {
    /// Globally unique color-definition identity.
    pub id: String,
    /// Owning table in the native `part_color_tables` arena.
    pub color_table: String,
    /// One-based NX color index.
    pub color_index: u16,
    /// Serialized color name.
    pub name: String,
    /// Normalized RGB components.
    pub rgb: [f32; 3],
    /// Exact serialized index token.
    pub raw_color_index: Vec<u8>,
    /// Exact serialized component atoms.
    pub raw_components: [Vec<u8>; 3],
    /// Absolute file offset of the opening `05` marker.
    pub source_offset: u64,
    /// Absolute file offsets of the three component atoms.
    pub component_source_offsets: [u64; 3],
}

fn components_from_wire(
    rgb: [f32; 3],
    raw: &[Vec<u8>; 3],
    offsets: [u64; 3],
) -> Result<[(ColorComponent, u64); 3], &'static str> {
    let [red, green, blue] = std::array::from_fn(|i| {
        ColorComponent::from_wire(rgb[i], &raw[i]).map(|component| (component, offsets[i]))
    });
    Ok([red?, green?, blue?])
}

impl TryFrom<PartColorTableWire> for PartColorTable {
    type Error = String;
    fn try_from(wire: PartColorTableWire) -> Result<Self, Self::Error> {
        if wire.background_name != BACKGROUND_NAME {
            return Err("background_name: must be Background".into());
        }
        Ok(Self {
            id: wire.id,
            class_definition: wire.class_definition,
            background: components_from_wire(
                wire.background_rgb,
                &wire.raw_background_components,
                wire.background_component_source_offsets,
            )
            .map_err(|error| format!("background_rgb/raw_background_components: {error}"))?,
            definitions: wire
                .definitions
                .try_into()
                .map_err(|_: Vec<String>| "definitions: must contain 216 entries")?,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        })
    }
}
impl From<PartColorTable> for PartColorTableWire {
    fn from(value: PartColorTable) -> Self {
        Self {
            id: value.id,
            class_definition: value.class_definition,
            background_name: BACKGROUND_NAME.into(),
            background_rgb: value.background.map(|(component, _)| component.value()),
            raw_background_components: value
                .background
                .map(|(component, _)| component.raw().to_vec()),
            background_component_source_offsets: value.background.map(|(_, offset)| offset),
            definitions: Vec::from(value.definitions),
            source_entry: value.source_entry,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<PartColorDefinitionWire> for PartColorDefinition {
    type Error = String;
    fn try_from(wire: PartColorDefinitionWire) -> Result<Self, Self::Error> {
        let color_index =
            PaletteIndex::new(wire.color_index).ok_or("color_index: must be in 1..=216")?;
        if color_index.definition_raw() != wire.raw_color_index {
            return Err("raw_color_index: differs from color_index definition token".into());
        }
        Ok(Self {
            id: wire.id,
            color_table: wire.color_table,
            color_index,
            name: wire.name,
            components: components_from_wire(
                wire.rgb,
                &wire.raw_components,
                wire.component_source_offsets,
            )?,
            source_offset: wire.source_offset,
        })
    }
}
impl From<PartColorDefinition> for PartColorDefinitionWire {
    fn from(value: PartColorDefinition) -> Self {
        Self {
            id: value.id,
            color_table: value.color_table,
            color_index: value.color_index.value(),
            name: value.name,
            rgb: value.components.map(|(component, _)| component.value()),
            raw_color_index: value.color_index.definition_raw(),
            raw_components: value
                .components
                .map(|(component, _)| component.raw().to_vec()),
            source_offset: value.source_offset,
            component_source_offsets: value.components.map(|(_, offset)| offset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_definition_keeps_wire_and_rejects_derived_field_mismatches() {
        let json = r#"{"id":"color","color_table":"table","color_index":128,"name":"test","rgb":[0.0,1.0,0.5],"raw_color_index":[128,127],"raw_components":[[0],[1],[48,0,0,0,0,0,0,0]],"source_offset":10,"component_source_offsets":[20,30,40]}"#;
        let definition: PartColorDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&definition).unwrap(), json);
        for (field, invalid) in [
            ("rgb", serde_json::json!([0.0, 1.0, 0.25])),
            ("raw_color_index", serde_json::json!([128, 128])),
            ("color_index", serde_json::json!(0)),
            ("raw_components", serde_json::json!([[0], [1], [2]])),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = invalid;
            let error = serde_json::from_value::<PartColorDefinition>(wire).unwrap_err();
            assert!(error.to_string().contains(field), "{error}");
        }
    }

    #[test]
    fn palette_table_rejects_incomplete_definition_lists_and_wrong_background_name() {
        let table = PartColorTable {
            id: "table".into(),
            class_definition: "class".into(),
            background: [(ColorComponent::read(&[1]).unwrap(), 10); 3],
            definitions: std::array::from_fn(|i| format!("color-{}", i + 1)),
            source_entry: "entry".into(),
            source_offset: 0,
        };
        let wire = serde_json::to_value(&table).unwrap();
        assert_eq!(
            serde_json::from_value::<PartColorTable>(wire.clone()).unwrap(),
            table
        );
        for (field, invalid) in [
            ("definitions", serde_json::json!([])),
            ("background_name", serde_json::json!("background")),
            ("background_rgb", serde_json::json!([0.0, 1.0, 1.0])),
        ] {
            let mut invalid_wire = wire.clone();
            invalid_wire[field] = invalid;
            let error = serde_json::from_value::<PartColorTable>(invalid_wire).unwrap_err();
            assert!(error.to_string().contains(field), "{error}");
        }
    }
}
