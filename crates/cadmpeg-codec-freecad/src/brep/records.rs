// SPDX-License-Identifier: Apache-2.0
//! Individually admitted exact-shape table rows.

use super::*;
use cadmpeg_ir::schema::rewrite::typed::prune::{DependencyPosition, DependencyReference};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
pub(crate) mod polygons;
use polygons::{polygon_rows, polygon_rows_for_decode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BrepRowWire")]
pub(crate) struct BrepRow {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) index: usize,
    pub(crate) value: BrepValue,
}

#[derive(Deserialize)]
struct BrepRowWire {
    id: String,
    owner: String,
    index: usize,
    value: BrepValue,
}

impl TryFrom<BrepRowWire> for BrepRow {
    type Error = String;
    fn try_from(wire: BrepRowWire) -> Result<Self, Self::Error> {
        if wire.id.trim().is_empty() || wire.owner.trim().is_empty() || wire.index == 0 {
            return Err("B-rep row requires identities and a nonzero table index".to_owned());
        }
        match &wire.value {
            BrepValue::TShape(shapes) if shapes.len() != 1 => {
                return Err("B-rep topology row requires exactly one shape".to_owned())
            }
            BrepValue::TShape(shapes)
                if shapes[0]
                    .children
                    .iter()
                    .any(|child| child.shape == 0 || child.shape >= wire.index) =>
            {
                return Err("B-rep topology row requires prior child references".to_owned())
            }
            BrepValue::Roots(values) if values.iter().any(|value| value.shape == 0) => {
                return Err("B-rep root uses require positive shape references".to_owned())
            }
            BrepValue::Location(value)
                if value
                    .factors
                    .iter()
                    .any(|factor| factor.location == 0 || factor.location >= wire.index) =>
            {
                return Err("B-rep location factors require prior references".to_owned())
            }
            BrepValue::Polygon3d(polygon)
                if polygon
                    .parameters
                    .as_ref()
                    .is_some_and(|values| values.len() != polygon.nodes.len()) =>
            {
                return Err("B-rep polygon parameters must align with nodes".to_owned())
            }
            BrepValue::PolygonOnTriangulation(polygon)
                if polygon.nodes.contains(&0)
                    || polygon
                        .parameters
                        .as_ref()
                        .is_some_and(|values| values.len() != polygon.nodes.len()) =>
            {
                return Err(
                    "B-rep indexed polygon requires positive indices and aligned parameters"
                        .to_owned(),
                )
            }
            _ => {}
        }
        Ok(Self {
            id: wire.id,
            owner: wire.owner,
            index: wire.index,
            value: wire.value,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "table", content = "value", rename_all = "snake_case")]
pub(crate) enum BrepValue {
    Location(TextLocation),
    Curve2d(TextCurve2d),
    Curve(TextCurve),
    Polygon3d(TextPolygon3d),
    PolygonOnTriangulation(TextPolygonOnTriangulation),
    Surface(TextSurface),
    Triangulation(TextTriangulation),
    TShape(TextTShapes),
    Roots(Vec<TextShapeUse>),
}

impl BrepRow {
    pub(crate) fn validate_arena(&self, arena: &str) -> Result<(), String> {
        let valid = match arena {
            "polygons3d" => matches!(self.value, BrepValue::Polygon3d(_)),
            "polygons_on_triangulations" => {
                matches!(self.value, BrepValue::PolygonOnTriangulation(_))
            }

            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err("B-rep row does not belong to its native arena".to_owned())
        }
    }

    pub(super) fn new(owner: &str, table: &str, index: usize, value: BrepValue) -> Self {
        Self {
            id: row_id(owner, table, index),
            owner: owner.to_owned(),
            index,
            value,
        }
    }
}

pub(super) fn row_id(owner: &str, table: &str, index: usize) -> String {
    let arena = match table {
        "Polygon3D" => "polygons3d",
        "PolygonOnTriangulations" => "polygons_on_triangulations",
        _ => return owner.to_owned(),
    };
    let key = cadmpeg_ir::ids::Identity::new(owner).map_or_else(
        |_| cadmpeg_ir::ids::IdentityKey::encode_segment(owner).to_string(),
        |identity| identity.key().to_string(),
    );
    native::native_id(arena, &format!("{key}:{index}"))
}

impl RewriteIdentities for BrepRow {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.owner)
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(DependencyReference {
            target: &self.owner,
            position: DependencyPosition::Required,
            field: Some("owner"),
        })
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.owner = map.identity(&self.owner)?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        rewrite_fields(value, &["id", "owner"], map)
    }
}

impl RewriteIdentities for ShapePayloadRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.property)?;
        visitor(&self.entry)?;
        if let Some(facts) = self.payload.shape_set() {
            for id in facts
                .polygon3d_refs
                .iter()
                .chain(&facts.indexed_polygon_refs)
            {
                visitor(id)?;
            }
        }
        Ok(())
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for (field, target) in [
            ("property", self.property.as_str()),
            ("entry", self.entry.as_str()),
        ] {
            visitor(DependencyReference {
                target,
                position: DependencyPosition::Required,
                field: Some(field),
            })?;
        }
        if let Some(facts) = self.payload.shape_set() {
            for (field, refs) in [
                ("polygons3d", &facts.polygon3d_refs),
                ("polygons_on_triangulations", &facts.indexed_polygon_refs),
            ] {
                for (index, target) in refs.iter().enumerate() {
                    visitor(DependencyReference {
                        target,
                        position: DependencyPosition::List {
                            length: refs.len(),
                            index,
                        },
                        field: Some(field),
                    })?;
                }
            }
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.property = map.identity(&self.property)?;
        self.entry = map.identity(&self.entry)?;
        if let ShapePayload::Text { facts, .. } | ShapePayload::Binary { facts, .. } =
            &mut self.payload
        {
            for id in facts
                .polygon3d_refs
                .iter_mut()
                .chain(&mut facts.indexed_polygon_refs)
            {
                *id = map.identity(id)?;
            }
        }
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        rewrite_fields(value, &["id", "property", "entry"], map)?;
        for carrier in ["text", "binary"] {
            if let Some(facts) = value
                .get_mut(carrier)
                .and_then(serde_json::Value::as_object_mut)
            {
                for key in ["polygons3d", "polygons_on_triangulations"] {
                    if let Some(refs) = facts.get_mut(key).and_then(serde_json::Value::as_array_mut)
                    {
                        for id in refs {
                            let text = id.as_str().ok_or_else(|| {
                                CodecError::malformed("polygon reference is not text")
                            })?;
                            *id = map.identity(text)?.into();
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn rewrite_fields<F: FnMut(&str) -> Result<String, CodecError>>(
    value: &mut serde_json::Value,
    fields: &[&str],
    map: &mut IdentityMap<'_, F>,
) -> Result<(), CodecError> {
    for field in fields {
        let target = value
            .get_mut(*field)
            .ok_or_else(|| CodecError::malformed("B-rep identity is absent"))?;
        let original = target
            .as_str()
            .ok_or_else(|| CodecError::malformed("B-rep identity is not text"))?;
        *target = map.identity(original)?.into();
    }
    Ok(())
}

pub(crate) fn validate(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<(String, String)>, cadmpeg_ir::native::NativeConvertError> {
    let mut failures = Vec::new();
    let mut slots = std::collections::BTreeSet::new();
    let rows = polygon_rows_for_decode(ctx, namespace, payloads)?;
    for row in &rows {
        let arena = if matches!(row.value, BrepValue::Polygon3d(_)) {
            "polygons3d"
        } else {
            "polygons_on_triangulations"
        };
        if !slots.insert((row.owner.clone(), row.value.table_order(), row.index)) {
            failures.push((row.id.clone(), "duplicate B-rep table element slot".into()));
        }
        if let Err(error) = row.validate_arena(arena) {
            failures.push((row.id.clone(), error));
        }
        if !payloads.iter().any(|payload| payload.id == row.owner) {
            failures.push((
                row.id.clone(),
                "polygon row has no shape payload owner".to_owned(),
            ));
        }
        if payloads.iter().any(|payload| {
            payload.id == row.owner
                && matches!(&payload.payload, ShapePayload::Text { facts, .. }
                        if source::TABLES.get(usize::from(row.value.table_order()))
                            .and_then(|table| facts.declared_counts.get(*table))
                            .is_some_and(|count| row.index > *count))
        }) {
            failures.push((
                row.id.clone(),
                "polygon row index exceeds the original text table count".to_owned(),
            ));
        }
    }
    for payload in payloads {
        if let Some(facts) = payload.payload.shape_set() {
            if let Err(error) = facts.validate() {
                failures.push((payload.id.clone(), error));
            }
            for (table, refs) in [(3, &facts.polygon3d_refs), (4, &facts.indexed_polygon_refs)] {
                let mut seen = std::collections::BTreeSet::new();
                for reference in refs {
                    if !seen.insert(reference) {
                        failures.push((
                            payload.id.clone(),
                            format!("shape payload repeats polygon row {reference}"),
                        ));
                    }
                    if !rows.iter().any(|row| {
                        row.id == *reference
                            && row.owner == payload.id
                            && row.value.table_order() == table
                    }) {
                        failures.push((
                            payload.id.clone(),
                            format!(
                                "shape payload refers to absent or wrong polygon row {reference}"
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(failures)
}

impl BrepValue {
    pub(super) const fn table_order(&self) -> u8 {
        match self {
            Self::Location(_) => 0,
            Self::Curve2d(_) => 1,
            Self::Curve(_) => 2,
            Self::Polygon3d(_) => 3,
            Self::PolygonOnTriangulation(_) => 4,
            Self::Surface(_) => 5,
            Self::Triangulation(_) => 6,
            Self::TShape(_) => 7,
            Self::Roots(_) => 8,
        }
    }
}

pub(super) fn with_rows(
    ctx: &DecodeContext<'_>,
    native: &cadmpeg_ir::native::Native,
    mut payload: ShapePayloadRecord,
) -> Result<ShapePayloadRecord, CodecError> {
    let Some(namespace) = native.namespace("fcstd") else {
        return Ok(payload);
    };
    if let ShapePayload::Text { facts, .. } | ShapePayload::Binary { facts, .. } =
        &mut payload.payload
    {
        if facts.polygons3d.as_read().is_some() {
            facts.polygon3d_refs = (1..=facts.polygons3d.len())
                .map(|dense| {
                    let index = facts
                        .indices
                        .get(&3)
                        .and_then(|indices| indices.get(dense - 1))
                        .copied()
                        .unwrap_or(dense);
                    row_id(&payload.id, "Polygon3D", index)
                })
                .collect();
        }
        if facts.polygons_on_triangulations.as_read().is_some() {
            facts.indexed_polygon_refs = (1..=facts.polygons_on_triangulations.len())
                .map(|dense| {
                    let index = facts
                        .indices
                        .get(&4)
                        .and_then(|indices| indices.get(dense - 1))
                        .copied()
                        .unwrap_or(dense);
                    row_id(&payload.id, "PolygonOnTriangulations", index)
                })
                .collect();
        }
    }
    let mut rows = polygon_rows_for_decode(ctx, namespace, std::slice::from_ref(&payload))?;
    rows.retain(|row| row.owner == payload.id);
    rows.sort_by_key(|row| (row.value.table_order(), row.index));
    payload.rows = rows.iter().map(|row| row.id.clone()).collect();
    if let ShapePayload::Text { facts, .. } | ShapePayload::Binary { facts, .. } =
        &mut payload.payload
    {
        if facts.polygon3d_refs.as_read().is_some() {
            facts.polygon3d_refs = rows
                .iter()
                .filter(|row| matches!(row.value, BrepValue::Polygon3d(_)))
                .map(|row| row.id.clone())
                .collect();
        }
        if facts.indexed_polygon_refs.as_read().is_some() {
            facts.indexed_polygon_refs = rows
                .iter()
                .filter(|row| matches!(row.value, BrepValue::PolygonOnTriangulation(_)))
                .map(|row| row.id.clone())
                .collect();
        }
        facts.row_ids = rows
            .iter()
            .map(|row| ((row.value.table_order(), row.index), row.id.clone()))
            .collect();
    }
    Ok(payload)
}

pub(crate) fn payloads(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<Vec<ShapePayloadRecord>, cadmpeg_ir::native::NativeConvertError> {
    let envelopes = namespace.arena_as_for_decode::<ShapePayloadRecord>(ctx, "shape_payloads")?;
    let rows = polygon_rows_for_decode(ctx, namespace, &envelopes)?;
    materialize(envelopes, rows)
}

pub(crate) fn payloads_untracked(
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<Vec<ShapePayloadRecord>, cadmpeg_ir::native::NativeConvertError> {
    let envelopes = namespace.arena_as::<ShapePayloadRecord>("shape_payloads")?;
    let rows = polygon_rows(namespace, &envelopes)?;
    materialize(envelopes, rows)
}

fn materialize(
    mut payloads: Vec<ShapePayloadRecord>,
    rows: Vec<BrepRow>,
) -> Result<Vec<ShapePayloadRecord>, cadmpeg_ir::native::NativeConvertError> {
    for payload in &mut payloads {
        let selected = rows
            .iter()
            .filter(|row| row.owner == payload.id)
            .collect::<Vec<_>>();
        if let ShapePayload::Text { facts, .. } | ShapePayload::Binary { facts, .. } =
            &mut payload.payload
        {
            let mut indices = source::remap::Indices::default();
            for (table, count) in [
                (0, facts.locations.len()),
                (1, facts.curve2ds.len()),
                (2, facts.curves.len()),
                (5, facts.surfaces.len()),
                (6, facts.triangulations.len()),
                (7, facts.tshapes.len()),
            ] {
                for index in 1..=count {
                    indices.bind(table, index, index);
                }
            }
            for (table, refs) in [(3, &facts.polygon3d_refs), (4, &facts.indexed_polygon_refs)] {
                let mut dense = 0;
                for (offset, id) in refs.iter().enumerate() {
                    if selected
                        .iter()
                        .any(|row| row.id == *id && row.value.table_order() == table)
                    {
                        dense += 1;
                        indices.bind(table, offset + 1, dense);
                    }
                }
            }
            // Numeric topology references address the original reference-list positions.
            // Missing polygon rows remove just their representation-list entries.
            if facts.tshapes.0.as_read().is_some() {
                facts.tshapes = TextTShapes(
                    facts
                        .tshapes
                        .iter()
                        .filter_map(|shape| {
                            match indices.resolve(BrepValue::TShape(TextTShapes(
                                vec![shape.clone()].into(),
                            )))? {
                                BrepValue::TShape(mut shapes) => shapes.0.pop(),
                                _ => None,
                            }
                        })
                        .collect(),
                );
            }
            if facts.polygon3d_refs.as_read().is_some() {
                facts.polygons3d = facts
                    .polygon3d_refs
                    .iter()
                    .filter_map(|id| {
                        selected
                            .iter()
                            .find(|row| row.id == *id)
                            .and_then(|row| match &row.value {
                                BrepValue::Polygon3d(value) => Some(value.clone()),
                                _ => None,
                            })
                    })
                    .collect();
            }
            if facts.indexed_polygon_refs.as_read().is_some() {
                facts.polygons_on_triangulations = facts
                    .indexed_polygon_refs
                    .iter()
                    .filter_map(|id| {
                        selected
                            .iter()
                            .find(|row| row.id == *id)
                            .and_then(|row| match &row.value {
                                BrepValue::PolygonOnTriangulation(value) => Some(value.clone()),
                                _ => None,
                            })
                    })
                    .collect();
            }
            facts.row_ids = selected
                .iter()
                .map(|row| ((row.value.table_order(), row.index), row.id.clone()))
                .collect();
            for (table, refs) in [(3, &facts.polygon3d_refs), (4, &facts.indexed_polygon_refs)] {
                facts.indices.insert(
                    table,
                    refs.iter()
                        .filter_map(|id| {
                            selected
                                .iter()
                                .find(|row| row.id == *id)
                                .map(|row| row.index)
                        })
                        .collect(),
                );
            }
            payload.rows = facts
                .polygon3d_refs
                .iter()
                .chain(&facts.indexed_polygon_refs)
                .cloned()
                .collect();
        }
    }
    Ok(payloads)
}

pub(crate) fn row_owners(
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<std::collections::BTreeMap<String, String>, cadmpeg_ir::native::NativeConvertError> {
    let mut owners = std::collections::BTreeMap::new();
    let payloads = namespace.arena_as::<ShapePayloadRecord>("shape_payloads")?;
    for row in polygon_rows(namespace, &payloads)? {
        if !row.owner.is_empty() {
            owners.insert(row.id, row.owner);
        }
    }
    Ok(owners)
}

impl RewriteIdentities for crate::native::CarrierCensusRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.payload)
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(DependencyReference {
            target: &self.payload,
            position: DependencyPosition::Required,
            field: Some("payload"),
        })
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.payload = map.identity(&self.payload)?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        rewrite_fields(value, &["id", "payload"], map)
    }
}
