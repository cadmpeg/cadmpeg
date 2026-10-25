// SPDX-License-Identifier: Apache-2.0
//! Polygon samples use their original lanes and the required topology and mesh inputs.
use super::*;

pub(crate) struct PolygonUse {
    pub(crate) shape: usize,
    pub(crate) representation: usize,
    pub(crate) table: u8,
    pub(crate) polygon: usize,
    pub(crate) secondary: bool,
    pub(crate) expected: Transform,
    pub(crate) root: Option<usize>,
}
pub(crate) struct PolygonInputs {
    pub(crate) nodes: Vec<FinitePoint3>,
    pub(crate) parameters: Option<Vec<FiniteReal>>,
    pub(crate) deflection: NonNegativeReal,
    pub(crate) transform: Transform,
}
impl ProjectionSources<'_> {
    pub(crate) fn polygon<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        usage: PolygonUse,
        build: impl FnOnce(PolygonInputs) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let source = self
            .payloads
            .get(&payload.id)
            .ok_or_else(|| CodecError::malformed("polygon has no original source authority"))?;
        let primary = source.row(
            usage.table,
            payload.source_index(usage.table, usage.polygon),
        )?;
        ctx.record(id, primary, || {
            let shape = source.shape(payload, usage.shape)?;
            let TextTShapeGeometry::Edge {
                representations, ..
            } = shape.geometry
            else {
                return Err(CodecError::malformed("polygon use requires an edge source"));
            };
            let representation = representations
                .get(usage.representation)
                .ok_or_else(|| CodecError::malformed("polygon source representation is absent"))?;
            let (table, polygon, triangulation, location) = match representation {
                TextEdgeRepresentation::Polygon3d { polygon, location } if !usage.secondary => {
                    (3, *polygon, None, *location)
                }
                TextEdgeRepresentation::PolygonOnTriangulation {
                    polygon,
                    triangulation,
                    location,
                } if !usage.secondary => (4, *polygon, Some(*triangulation), *location),
                TextEdgeRepresentation::PolygonPair {
                    polygons,
                    triangulation,
                    location,
                } => (
                    4,
                    polygons[usize::from(usage.secondary)],
                    Some(*triangulation),
                    *location,
                ),
                _ => {
                    return Err(CodecError::malformed(
                        "edge source does not admit this polygon use",
                    ))
                }
            };
            if table != usage.table || polygon != usage.polygon {
                return Err(CodecError::malformed(
                    "polygon source use differs from its actual home",
                ));
            }
            let (nodes, parameters, deflection) = if table == 3 {
                let value = if source.binary {
                    source_rows::binary_polygon3d(&mut BinaryCursor {
                        view: primary,
                        census_marks: vec![],
                        representation_marks: vec![],
                    })?
                } else {
                    let (tokens, ranges) = text_tokens(primary);
                    source_rows::text_polygon3d(&mut cursor(&tokens, &ranges, primary))?
                };
                (value.nodes, value.parameters, value.deflection)
            } else {
                let value = if source.binary {
                    source_rows::binary_indexed_polygon(&mut BinaryCursor {
                        view: primary,
                        census_marks: vec![],
                        representation_marks: vec![],
                    })?
                } else {
                    let (tokens, ranges) = text_tokens(primary);
                    source_rows::text_indexed_polygon(&mut cursor(&tokens, &ranges, primary))?
                };
                let index = triangulation
                    .ok_or_else(|| CodecError::malformed("indexed polygon has no triangulation"))?;
                let view = source.row(6, payload.source_index(6, index))?;
                let version = payload.payload.topology_version().ok_or_else(|| {
                    CodecError::malformed("indexed polygon has no topology version")
                })?;
                let mesh = if source.binary {
                    source_rows::binary_triangulation(
                        &mut BinaryCursor {
                            view,
                            census_marks: vec![],
                            representation_marks: vec![],
                        },
                        version,
                    )?
                } else {
                    let (tokens, ranges) = text_tokens(view);
                    source_rows::text_triangulation(&mut cursor(&tokens, &ranges, view), version)?
                };
                let nodes = value
                    .nodes
                    .iter()
                    .map(|index| {
                        let offset = usize::try_from(*index)
                            .ok()
                            .and_then(|index| index.checked_sub(1))
                            .ok_or_else(|| {
                                CodecError::malformed("indexed polygon node is invalid")
                            })?;
                        mesh.nodes().get(offset).copied().ok_or_else(|| {
                            CodecError::malformed(
                                "indexed polygon node is outside its triangulation",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (nodes, value.parameters, value.deflection)
            };
            let transform = source
                .placement(payload, usage.shape, usage.expected, usage.root)?
                .compose(source.location(payload, location)?)
                .map_err(location_transform_error)?;
            build(PolygonInputs {
                nodes,
                parameters,
                deflection,
                transform,
            })
        })
    }
}
