// SPDX-License-Identifier: Apache-2.0
//! Encode CADIR's canonical length unit when no other entity is writable.

use super::{Entity, EntityStatus};

pub(super) fn ensure_nonempty(entities: &mut Vec<Entity>, version: crate::IgesVersion) {
    if !entities.is_empty() || version == crate::IgesVersion::V4_0 {
        return;
    }
    entities.push(Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: Vec::new(),
        type_code: 316,
        form: 0,
        label: "UNITS".into(),
        status: EntityStatus::Definition,
        parameter_body: b"1,6HLENGTH,1HM,0.001;".to_vec(),
        transform: None,
        transform_link: None,
        subscript: 0,
    });
}
