// SPDX-License-Identifier: Apache-2.0
//! Canonical flat polygon arena rows; payload lists supply their ownership.

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PolygonRecord<T> {
    pub(crate) id: String,
    #[serde(flatten)]
    pub(crate) value: T,
}

pub(crate) type Polygon3dRecord = PolygonRecord<TextPolygon3d>;
pub(crate) type PolygonOnTriangulationRecord = PolygonRecord<TextPolygonOnTriangulation>;

impl<T> RewriteIdentities for PolygonRecord<T> {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        rewrite_fields(value, &["id"], map)
    }
}

fn bind(mut rows: Vec<BrepRow>, payloads: &[ShapePayloadRecord]) -> Vec<BrepRow> {
    for row in &mut rows {
        let mut claims = payloads.iter().filter_map(|payload| {
            let facts = payload.payload.shape_set()?;
            let references = match row.value {
                BrepValue::Polygon3d(_) => &facts.polygon3d_refs,
                BrepValue::PolygonOnTriangulation(_) => &facts.indexed_polygon_refs,
                _ => return None,
            };
            references
                .iter()
                .position(|id| *id == row.id)
                .map(|index| (&payload.id, index + 1))
        });
        if let Some((owner, index)) = claims.next() {
            if claims.next().is_none() {
                row.owner.clone_from(owner);
                row.index = index;
            }
        }
    }
    rows
}

pub(crate) fn polygon_rows(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<BrepRow>, cadmpeg_ir::native::NativeConvertError> {
    let mut rows = namespace
        .arena_as::<Polygon3dRecord>("polygons3d")?
        .into_iter()
        .map(|row| BrepRow {
            id: row.id,
            owner: String::new(),
            index: 0,
            value: BrepValue::Polygon3d(row.value),
        })
        .collect::<Vec<_>>();
    rows.extend(
        namespace
            .arena_as::<PolygonOnTriangulationRecord>("polygons_on_triangulations")?
            .into_iter()
            .map(|row| BrepRow {
                id: row.id,
                owner: String::new(),
                index: 0,
                value: BrepValue::PolygonOnTriangulation(row.value),
            }),
    );
    Ok(bind(rows, payloads))
}

pub(crate) fn polygon_rows_for_decode(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<BrepRow>, cadmpeg_ir::native::NativeConvertError> {
    let mut rows = namespace
        .arena_as_for_decode::<Polygon3dRecord>(ctx, "polygons3d")?
        .into_iter()
        .map(|row| BrepRow {
            id: row.id,
            owner: String::new(),
            index: 0,
            value: BrepValue::Polygon3d(row.value),
        })
        .collect::<Vec<_>>();
    rows.extend(
        namespace
            .arena_as_for_decode::<PolygonOnTriangulationRecord>(ctx, "polygons_on_triangulations")?
            .into_iter()
            .map(|row| BrepRow {
                id: row.id,
                owner: String::new(),
                index: 0,
                value: BrepValue::PolygonOnTriangulation(row.value),
            }),
    );
    Ok(bind(rows, payloads))
}
