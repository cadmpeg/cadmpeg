// SPDX-License-Identifier: Apache-2.0
//! Actual shape geometry and required root-use admission feed topology producers.
use super::*;

impl PayloadSource<'_> {
    fn raw_shape(
        &self,
        payload: &ShapePayloadRecord,
        dense: usize,
    ) -> Result<TextTShape, CodecError> {
        let original = payload.source_index(7, dense);
        let view = self.row(7, original)?;
        let count =
            self.marks.counts.get(&7).copied().ok_or_else(|| {
                CodecError::malformed("shape source has no accepted topology count")
            })?;
        let version = payload
            .payload
            .topology_version()
            .ok_or_else(|| CodecError::malformed("shape source has no topology version"))?;
        let facts = payload
            .payload
            .shape_set()
            .ok_or_else(|| CodecError::malformed("shape source has no typed tables"))?;
        let value =
            if self.binary {
                let mut cursor = BinaryCursor {
                    view,
                    census_marks: vec![],
                    representation_marks: vec![],
                };
                parse_binary_tshape(
                    &mut cursor,
                    version,
                    original,
                    count,
                    BinaryGeometryCounts {
                        curves: *self.marks.counts.get(&2).ok_or_else(|| {
                            CodecError::malformed("shape source has no curve count")
                        })?,
                        curves2d: *self.marks.counts.get(&1).ok_or_else(|| {
                            CodecError::malformed("shape source has no parameter curve count")
                        })?,
                        surfaces: *self.marks.counts.get(&5).ok_or_else(|| {
                            CodecError::malformed("shape source has no surface count")
                        })?,
                        locations: *self.marks.counts.get(&0).ok_or_else(|| {
                            CodecError::malformed("shape source has no location count")
                        })?,
                        polygons3d: *self.marks.counts.get(&3).ok_or_else(|| {
                            CodecError::malformed("shape source has no polygon count")
                        })?,
                        indexed_polygons: *self.marks.counts.get(&4).ok_or_else(|| {
                            CodecError::malformed("shape source has no indexed polygon count")
                        })?,
                        triangulations: *self.marks.counts.get(&6).ok_or_else(|| {
                            CodecError::malformed("shape source has no triangulation count")
                        })?,
                    },
                )?
            } else {
                let counts = self
                    .marks
                    .counts
                    .iter()
                    .map(|(table, count)| (TABLES[usize::from(*table)].to_owned(), *count))
                    .collect();
                let shapes = (1..=facts.tshapes.len())
                    .filter_map(|dense| {
                        let index = payload.source_index(7, dense);
                        (index < original).then_some((index, dense))
                    })
                    .collect();
                let (tokens, ranges) = text_tokens(view);
                let mut cursor = cursor(&tokens, &ranges, view);
                source_rows::text_tshape(&mut cursor, original, count, &counts, version, &shapes)?
            };
        Ok(value)
    }
    pub(super) fn shape(
        &self,
        payload: &ShapePayloadRecord,
        dense: usize,
    ) -> Result<TextTShape, CodecError> {
        self.normalize_shape(payload, self.raw_shape(payload, dense)?)
    }
    fn normalize_shape(
        &self,
        payload: &ShapePayloadRecord,
        mut value: TextTShape,
    ) -> Result<TextTShape, CodecError> {
        let facts = payload
            .payload
            .shape_set()
            .ok_or_else(|| CodecError::malformed("shape source has no typed tables"))?;
        let mut indices = remap::Indices::default();
        for (table, length) in [
            (0, facts.locations.len()),
            (1, facts.curve2ds.len()),
            (2, facts.curves.len()),
            (3, facts.polygons3d.len()),
            (4, facts.polygons_on_triangulations.len()),
            (5, facts.surfaces.len()),
            (6, facts.triangulations.len()),
            (7, facts.tshapes.len()),
        ] {
            for dense in 1..=length {
                indices.bind(table, payload.source_index(table, dense), dense);
            }
        }
        let resolved = indices
            .resolve(BrepValue::TShape(vec![value].into()))
            .ok_or_else(|| {
                CodecError::malformed("shape producer has a missing required source target")
            })?;
        let BrepValue::TShape(mut shapes) = resolved else {
            return Err(CodecError::malformed("shape source changed its value type"));
        };
        value = shapes
            .0
            .pop()
            .ok_or_else(|| CodecError::malformed("shape source has no value"))?;
        value.children = value
            .children
            .into_iter()
            .filter_map(|mut child| {
                child.shape = (1..=facts.tshapes.len())
                    .find(|dense| payload.source_index(7, *dense) == child.shape)?;
                Some(child)
            })
            .collect();
        Ok(value)
    }
    fn edge_values(
        &self,
        payload: &ShapePayloadRecord,
        original: TextTShape,
    ) -> Result<(TextTShape, Option<TextCurve>, bool), CodecError> {
        let supported = |representation: &TextEdgeRepresentation| {
            matches!(
                representation,
                TextEdgeRepresentation::Curve3d { .. }
                    | TextEdgeRepresentation::Polygon3d { .. }
                    | TextEdgeRepresentation::PolygonOnTriangulation { .. }
                    | TextEdgeRepresentation::PolygonPair { .. }
            )
        };
        let required = match &original.geometry {
            TextTShapeGeometry::Edge {
                degenerated,
                representations,
                ..
            } => !degenerated && representations.iter().any(supported),
            _ => return Err(CodecError::malformed("edge source is not an edge")),
        };
        let value = self.normalize_shape(payload, original)?;
        let TextTShapeGeometry::Edge {
            representations, ..
        } = &value.geometry
        else {
            return Err(CodecError::malformed(
                "normalized edge source changed its kind",
            ));
        };
        let available = !required || representations.iter().any(supported);
        let curve = representations
            .iter()
            .filter(|_| required)
            .find_map(|representation| {
                if let TextEdgeRepresentation::Curve3d { curve, .. } = representation {
                    Some(*curve)
                } else {
                    None
                }
            })
            .map(|dense| {
                let original = payload.source_index(2, dense);
                let view = self.row(2, original)?;
                if self.binary {
                    parse_binary_curve(
                        &mut BinaryCursor {
                            view,
                            census_marks: vec![],
                            representation_marks: vec![],
                        },
                        0,
                    )
                } else {
                    let (tokens, ranges) = text_tokens(view);
                    parse_curve(&mut cursor(&tokens, &ranges, view), 0, original)
                }
            })
            .transpose()?;
        Ok((value, curve, available))
    }
    pub(super) fn edge_admitted(
        &self,
        payload: &ShapePayloadRecord,
        shape: usize,
    ) -> Result<bool, CodecError> {
        let original = self.raw_shape(payload, shape)?;
        self.edge_values(payload, original)
            .map(|(_, _, admitted)| admitted)
    }
    fn edge_inputs(
        &self,
        payload: &ShapePayloadRecord,
        shape: usize,
        expected: Transform,
        ordinal: Option<usize>,
    ) -> Result<EdgeInputs, CodecError> {
        let original = self.raw_shape(payload, shape)?;
        let (value, curve, available) = self.edge_values(payload, original)?;
        let transform = self.placement(payload, shape, expected, ordinal)?;
        Ok(EdgeInputs {
            shape: value,
            transform,
            curve,
            carrier_available: available,
        })
    }
    fn component_values(
        &self,
        payload: &ShapePayloadRecord,
        shape: usize,
    ) -> Result<(TextShapeKind, bool), CodecError> {
        let original = self.raw_shape(payload, shape)?;
        let kind = original.kind();
        let mut pending = vec![(shape, Some(original))];
        let mut seen = std::collections::BTreeSet::new();
        while let Some((index, original)) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            let original = match original {
                Some(value) => value,
                None => self.raw_shape(payload, index)?,
            };
            if matches!(&original.geometry, TextTShapeGeometry::Edge { .. }) {
                let (edge, _, available) = self.edge_values(payload, original)?;
                if available && !edge.children.is_empty() {
                    return Ok((kind, true));
                }
                continue;
            }
            let current = self.normalize_shape(payload, original)?;
            match current.geometry {
                TextTShapeGeometry::Vertex { .. } => return Ok((kind, true)),
                TextTShapeGeometry::Face {
                    surface: Some(surface),
                    ..
                } => {
                    let row = self.row(5, payload.source_index(5, surface.index()))?;
                    self.surface(row)?;
                    return Ok((kind, true));
                }
                TextTShapeGeometry::Face {
                    triangulation: Some(index),
                    ..
                } => {
                    let row = self.row(6, payload.source_index(6, index.index()))?;
                    let version = payload.payload.topology_version().ok_or_else(|| {
                        CodecError::malformed("component has no topology version")
                    })?;
                    let triangulation = if self.binary {
                        source_rows::binary_triangulation(
                            &mut BinaryCursor {
                                view: row,
                                census_marks: vec![],
                                representation_marks: vec![],
                            },
                            version,
                        )?
                    } else {
                        let (tokens, ranges) = text_tokens(row);
                        source_rows::text_triangulation(
                            &mut cursor(&tokens, &ranges, row),
                            version,
                        )?
                    };
                    if !triangulation.nodes().is_empty() {
                        return Ok((kind, true));
                    }
                }
                _ => pending.extend(
                    current
                        .children
                        .into_iter()
                        .map(|child| (child.shape, None)),
                ),
            }
        }
        Ok((kind, false))
    }
}
impl ProjectionSources<'_> {
    pub(crate) fn edge_carrier_available(
        &self,
        payload: &ShapePayloadRecord,
        shape: usize,
        expected: Transform,
        ordinal: Option<usize>,
    ) -> Result<bool, CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("edge admission has no original source authority")
        })?;
        Ok(source
            .edge_inputs(payload, shape, expected, ordinal)?
            .carrier_available)
    }
    pub(crate) fn edge<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        shape: usize,
        expected: Transform,
        ordinal: Option<usize>,
        build: impl FnOnce(EdgeInputs) -> Result<Option<T>, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("edge payload has no original source authority")
        })?;
        let extent = source.row(7, payload.source_index(7, shape))?;
        let (value, owner) = ctx.record_owner(id, extent, || {
            build(source.edge_inputs(payload, shape, expected, ordinal)?)
        })?;
        if value.is_none() {
            ctx.retire_record_owner(&owner)?;
        }
        Ok(value)
    }
    pub(crate) fn component<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        shape: usize,
        build: impl FnOnce(TextShapeKind) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let source = self
            .payloads
            .get(&payload.id)
            .ok_or_else(|| CodecError::malformed("component has no original source authority"))?;
        let (value, owner) =
            ctx.record_owner(id, source.row(7, payload.source_index(7, shape))?, || {
                let (value, admitted) = source.component_values(payload, shape)?;
                if admitted {
                    build(value).map(Some)
                } else {
                    Ok(None)
                }
            })?;
        if value.is_none() {
            ctx.retire_record_owner(&owner)?;
        }
        Ok(value)
    }
    pub(crate) fn body<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        shape: usize,
        ordinal: Option<usize>,
        build: impl FnOnce(TextShapeKind, Transform) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let source = self
            .payloads
            .get(&payload.id)
            .ok_or_else(|| CodecError::malformed("body has no original source authority"))?;
        let (value, owner) =
            ctx.record_owner(id, source.row(7, payload.source_index(7, shape))?, || {
                let (value, admitted) = source.component_values(payload, shape)?;
                let roots = source.root_values(payload)?;
                let index = ordinal
                    .unwrap_or(1)
                    .checked_sub(1)
                    .ok_or_else(|| CodecError::malformed("body root ordinal is zero"))?;
                let (root, matrix) = roots
                    .get(index)
                    .ok_or_else(|| CodecError::malformed("body source root is absent"))?;
                if root.shape != shape {
                    return Err(CodecError::malformed(
                        "body source root differs from its topology",
                    ));
                }
                if admitted {
                    build(value, *matrix).map(Some)
                } else {
                    Ok(None)
                }
            })?;
        if value.is_none() {
            ctx.retire_record_owner(&owner)?;
        }
        Ok(value)
    }
    pub(crate) fn topology_placed<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        shape: usize,
        expected: Transform,
        ordinal: Option<usize>,
        build: impl FnOnce(TextTShape, Transform) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let source = self
            .payloads
            .get(&payload.id)
            .ok_or_else(|| CodecError::malformed("topology has no original source authority"))?;
        ctx.record(id, source.row(7, payload.source_index(7, shape))?, || {
            let value = source.shape(payload, shape)?;
            let matrix = source.placement(payload, shape, expected, ordinal)?;
            build(value, matrix)
        })
    }
    pub(crate) fn topology<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        shape: usize,
        build: impl FnOnce(TextTShape) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("topology payload has no original source authority")
        })?;
        let extent = source.row(7, payload.source_index(7, shape))?;
        ctx.record(id, extent, || {
            let value = source.shape(payload, shape)?;
            if !source.reaches(payload, shape)? {
                return Err(CodecError::malformed(
                    "source root does not admit this topology record",
                ));
            }
            build(value)
        })
    }
}
