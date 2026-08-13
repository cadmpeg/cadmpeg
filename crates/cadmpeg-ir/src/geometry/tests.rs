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

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn unknown_surface_json_round_trips() {
    let mut ir = unit_cube();
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.set_native_unknowns(
        "synthetic",
        &[NativeUnknownRecord {
            id: rec.clone(),
            links: Vec::new(),
        }],
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the unknown surface");
}

#[test]
fn ordered_pcurve_uses_round_trip_with_isoparametric_state() {
    let uses = vec![
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#first".into()),
            isoparametric: Some(true),
            parameter_range: None,
        },
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#second".into()),
            isoparametric: Some(false),
            parameter_range: Some([0.0, 1.0]),
        },
    ];
    let json = serde_json::to_string(&uses).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<crate::topology::PcurveUse>>(&json).unwrap(),
        uses
    );
}
