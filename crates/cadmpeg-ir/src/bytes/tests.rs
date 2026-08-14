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

fn assert_base64_round_trip_and_rejection<T>(value: &T, field: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut json = serde_json::to_value(value).unwrap();
    assert_eq!(json[field], "AQID");
    assert_eq!(serde_json::from_value::<T>(json.clone()).unwrap(), *value);
    json[field] = serde_json::Value::String("%%%".into());
    assert!(serde_json::from_value::<T>(json).is_err());
}

#[test]
fn byte_payloads_use_nonempty_base64_and_reject_invalid_text() {
    assert_base64_round_trip_and_rejection(
        &UnknownRecord {
            id: UnknownId("synthetic:test:unknown#0".into()),
            offset: 0,
            byte_len: 3,
            sha256: "00".repeat(32),
            data: Some(vec![1, 2, 3]),
            links: Vec::new(),
        },
        "data",
    );
    assert_base64_round_trip_and_rejection(
        &TessellationChannel {
            domain: TessellationChannelDomain::default(),
            item_size: 3,
            kind: 0,
            flags: 0,
            count: 1,
            data: vec![1, 2, 3],
            indices: Vec::new(),
        },
        "data",
    );
}
