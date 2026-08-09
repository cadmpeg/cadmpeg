// SPDX-License-Identifier: Apache-2.0
//! Stable Design identities and library markers for body presentation records.

/// Stable Design type of a body record that owns its presentation envelope.
pub(crate) const BODY_PRESENTATION_TYPE_GUID: &str = "D3937028-C20C-4E65-B010-94AD418A5C20";
pub(crate) const BODY_PRESENTATION_TYPE_VERSION: u32 = 19;

/// Stable Design type of the B-rep container referenced by a body
/// presentation.
pub(crate) const BREP_CONTAINER_TYPE_GUID: &str = "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA";
pub(crate) const BREP_CONTAINER_TYPE_VERSION: u32 = 4;

/// Stable Design type of the scene entity referenced by a body presentation.
pub(crate) const BODY_SCENE_NODE_TYPE_GUID: &str = "702b9cd2-537c-429e-8cc4-22beeeb98c37";
pub(crate) const BODY_SCENE_NODE_TYPE_VERSION: u32 = 1;

/// Stable Design type of a browser node that carries body visibility.
pub(crate) const BROWSER_NODE_TYPE_GUID: &str = "D26351F0-5940-4D23-AA20-2C35475A6D9E";
pub(crate) const BROWSER_NODE_BASE_TYPE_GUID: &str = "CB844AB6-240D-4fc9-9C9F-3679DC896D6F";
pub(crate) const BROWSER_NODE_TYPE_VERSION: u32 = 2;

/// Physical-material library identifier in a body presentation envelope.
pub(crate) const PHYSICAL_MATERIAL_LIBRARY_ID: &str = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
/// Appearance library identifier in a legacy body presentation envelope.
pub(crate) const APPEARANCE_LIBRARY_ID: &str = "BA5EE55E-9982-449B-9D66-9F036540E140";
/// Appearance-library identifier pair in a current body presentation envelope.
pub(crate) const MODERN_APPEARANCE_LIBRARY_IDS: [&str; 2] = [
    "08861000-1D69-CF2A-C082-CBD98E7E5D7F",
    "005E1000-55CE-AFB6-81A1-36E3EF077C5F",
];

/// Whether a token names a physical material rather than one of its aspects.
pub(crate) fn is_physical_material_token(value: &str) -> bool {
    value.starts_with("PrismMaterial") && !value.contains("_physmat_aspects")
}
