// SPDX-License-Identifier: Apache-2.0
//! Emit placement matrices as regular entities so references can be ordered.

use super::{number, Entity, EntityStatus};
use cadmpeg_core::CodecError;

pub(super) fn materialize(entities: &mut Vec<Entity>) -> Result<(), CodecError> {
    for index in 0..entities.len() {
        let Some(placement) = entities[index].transform.take() else {
            continue;
        };
        if entities[index].transform_link.is_some() {
            return Err(CodecError::malformed(
                "IGES entity has both a placement and a transformation link",
            ));
        }
        let body = placement
            .rows
            .iter()
            .flatten()
            .map(|value| number(*value))
            .collect::<Vec<_>>()
            .join(",");
        let transform = entities.len();
        entities[index].transform_link = Some(transform);
        entities.push(Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: Vec::new(),
            type_code: 124,
            form: 0,
            label: String::new(),
            status: EntityStatus::Independent,
            parameter_body: format!("{body};").into_bytes(),
            transform: None,
            transform_link: None,
            subscript: 0,
        });
    }
    Ok(())
}
