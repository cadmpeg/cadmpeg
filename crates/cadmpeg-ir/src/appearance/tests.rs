// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::annotations::{ExactnessNote, StreamProvenance};
use crate::codec::{CadirEncoder, Encoder};
use crate::document::Model;
use crate::examples::{directed_subd_sum, unit_cube};
use crate::features::ExtrudeDirection;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SplineSurfaceParameters, SurfaceGeometry,
};
use crate::ids::{
    CoedgeId, CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SubdId, UnknownId,
};
use crate::math::{Point3, Vector3};
use crate::native::NativeRecord;
use crate::products::{ProductDefinition, ProductDefinitionKind};
use crate::provenance::{Exactness, SourceObjectAssociation};
use crate::report::{Check, LossKind, LossNote, LossTaxonomy, Severity};
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use crate::tessellation::{TessellationChannel, TessellationChannelDomain};
use crate::topology::Color;
use crate::unknown::{NativeUnknownRecord, UnknownRecord};
use crate::validate::validate_neutral;
use crate::{diff, CadIr, SourceProvenance};

use super::*;

#[test]
fn appearance_asset_and_binding_round_trip() {
    use crate::appearance::{
        Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef,
    };
    use crate::ids::AppearanceId;

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("synthetic:test:appearance#prism-001".into()),
        name: Some("Prism-001".into()),
        asset_guid: Some("visual-guid".into()),
        library_id: None,
        visual_guid: Some("visual-guid".into()),
        physical_token: Some("physical-token".into()),
        schema: Some("GenericSchema".into()),
        category: None,
        base_color: Some(crate::topology::Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: vec![TextureRef {
            asset_guid: "texture-guid".into(),
            slot: "generic_bump_map".into(),
            schema: "BumpMapSchema".into(),
            paths: vec!["cloud/resource/texture.png".into()],
            urn: Some("adsk.raas:asset.name:texture".into()),
            mapping: TextureMap2d {
                map_channel: 1,
                uvw_source: 0,
                u_offset: 0.25,
                v_offset: -0.5,
                u_scale: 2.0,
                v_scale: 3.0,
                rotation: std::f64::consts::FRAC_PI_2,
                repeat_u: true,
                repeat_v: false,
                real_world_offset_x: 12.7,
                real_world_offset_y: 25.4,
                real_world_scale_x: 304.8,
                real_world_scale_y: 609.6,
            },
            bump: Some(BumpMap {
                normal_map: true,
                depth: 2.54,
                normal_scale: 0.75,
            }),
        }],
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#0".into(),
        target: AppearanceTarget::Body(body),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Body".into()),
        visible: Some(false),
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#edge".into(),
        target: AppearanceTarget::Edge(ir.model.edges[0].id.clone()),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Edge".into()),
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#vertex".into(),
        target: AppearanceTarget::Vertex(ir.model.vertices[0].id.clone()),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Vertex".into()),
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });

    let json = ir.to_canonical_json().unwrap();
    let decoded = CadIr::from_json(&json).unwrap();
    assert_eq!(decoded.model.appearances, ir.model.appearances);
    assert_eq!(
        decoded.model.appearance_bindings,
        ir.model.appearance_bindings
    );
}
