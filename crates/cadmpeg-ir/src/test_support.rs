// SPDX-License-Identifier: Apache-2.0
//! Shared IR fixture builders for `#[cfg(test)]` suites.

use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::ids::UnknownId;

/// Push an appearance whose texture mapping states `u_offset`.
///
/// Every float of [`crate::appearance::TextureMap2d`] is a plain public `f64`
/// reached through public fields alone, so the arena holds whatever the caller
/// installs, finite or not. A caller that needs the value changed afterwards
/// writes `ir.model.appearances[0].textures[0].mapping.u_offset`.
pub(crate) fn push_texture_offset(ir: &mut crate::CadIr, u_offset: f64) {
    use crate::appearance::{Appearance, TextureMap2d, TextureRef};
    use crate::ids::AppearanceId;

    ir.model.appearances.push(Appearance {
        id: AppearanceId::mint("test:model:appearance#0").expect("identity grammar"),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: vec![TextureRef {
            asset_guid: "texture-guid".into(),
            slot: "generic_diffuse".into(),
            schema: "UnifiedBitmapSchema".into(),
            paths: Vec::new(),
            urn: None,
            mapping: TextureMap2d {
                map_channel: 1,
                uvw_source: 0,
                u_offset,
                v_offset: 0.0,
                u_scale: 1.0,
                v_scale: 1.0,
                rotation: 0.0,
                repeat_u: true,
                repeat_v: true,
                real_world_offset_x: 0.0,
                real_world_offset_y: 0.0,
                real_world_scale_x: 1.0,
                real_world_scale_y: 1.0,
            },
            bump: None,
        }],
    });
}

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
pub(crate) fn make_first_face_surface_unknown(
    ir: &mut crate::CadIr,
    record: Option<UnknownId>,
) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.as_str().to_owned();
    for s in &mut ir.model.surfaces {
        if s.id.as_str() == surface_id {
            s.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record });
            break;
        }
    }
    surface_id
}

pub(crate) mod nurbs;
