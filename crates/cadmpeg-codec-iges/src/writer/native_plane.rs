// SPDX-License-Identifier: Apache-2.0
//! Reconstruct an exact canonical Type 108 plane from current geometry.

use super::{
    number, orthonormal_pair, CadIr, Entity, EntityStatus, FiniteReal, SolvedSurfaceGeometry,
    Surface,
};
use serde_json::Value;

fn numeric(parameters: &[Value], index: usize) -> Option<f64> {
    let value = parameters.get(index)?.get("value")?;
    match value.get("kind")?.as_str()? {
        "integer" => cadmpeg_core::convert::f64_from_i64(value.get("value")?.as_i64()?),
        "real" => value.get("value")?.as_f64(),
        _ => None,
    }
}

pub(super) fn regenerate(ir: &CadIr, surface: &Surface) -> Option<Entity> {
    let SolvedSurfaceGeometry::Plane(plane) = surface.geometry.solved()? else {
        return None;
    };
    let origin = plane.origin().get();
    let (normal, u_axis) = orthonormal_pair(plane.frame());
    if origin.x != 0.0
        || origin.y != 0.0
        || [normal.x, normal.y, normal.z] != [0.0, 0.0, 1.0]
        || [u_axis.x, u_axis.y, u_axis.z] != [1.0, 0.0, 0.0]
    {
        return None;
    }
    let source = surface.source_object.as_ref()?;
    if source.format != cadmpeg_ir::CodecFormat::Iges {
        return None;
    }
    let source_id = format!(
        "iges:entity:directory#{}",
        source.object_id.as_str().strip_prefix('D')?
    );
    let record = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("entities")?
        .iter()
        .find(|record| record.id() == source_id)?;
    if record.field("entity_type")?.as_u64()? != 108
        || record.field("form")?.as_i64()? != 0
        || record.field("transform")?.as_i64()? != 0
    {
        return None;
    }
    let parameters = record.field("parameters")?;
    let parameters = parameters.as_array()?;
    let expected = [0.0, 0.0, 1.0, origin.z, 0.0, 0.0, 0.0, origin.z, 0.0];
    if expected
        .iter()
        .enumerate()
        .any(|(index, value)| numeric(parameters, index + 1) != Some(*value))
    {
        return None;
    }
    let z = number(FiniteReal::new(origin.z)?);
    let mut entity = Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: vec![source_id],
        type_code: 108,
        form: 0,
        label: String::new(),
        status: EntityStatus::Independent,
        parameter_body: format!("0,0,1,{z},0,0,0,{z},0;").into_bytes(),
        transform: None,
        transform_link: None,
        subscript: 0,
    };
    super::support_records::retain_numeric_tokens(&mut entity, record);
    Some(entity)
}
