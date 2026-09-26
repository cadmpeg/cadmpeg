// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FinitePoint2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
struct PerNode<T>(Vec<T>);

impl<T> PerNode<T> {
    fn try_new(values: Vec<T>, count: usize, field: &str) -> Result<Self, String> {
        if values.len() != count {
            return Err(format!("{field} length must equal nodes length"));
        }
        Ok(Self(values))
    }
}

/// One indexed display triangulation with aligned optional node attributes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "TextTriangulationWire")]
pub(crate) struct TextTriangulation {
    /// Chordal deflection.
    pub(crate) deflection: FiniteReal,
    nodes: Vec<FinitePoint3>,
    uv_nodes: Option<PerNode<FinitePoint2>>,
    triangles: Vec<[u32; 3]>,
    normals: Option<PerNode<FiniteVector3>>,
}

impl TextTriangulation {
    /// Admits attributes with exactly one value per node.
    pub(super) fn try_new(
        deflection: FiniteReal,
        nodes: Vec<FinitePoint3>,
        uv_nodes: Option<Vec<FinitePoint2>>,
        triangles: Vec<[u32; 3]>,
        normals: Option<Vec<FiniteVector3>>,
    ) -> Result<Self, String> {
        let triangles = triangles
            .into_iter()
            .map(|triangle| {
                if triangle.iter().any(|&index| {
                    index == 0 || usize::try_from(index).map_or(true, |index| index > nodes.len())
                }) {
                    return Err("triangles node index is out of bounds".to_owned());
                }
                Ok(triangle.map(|index| index - 1))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let uv_nodes = uv_nodes
            .map(|values| PerNode::try_new(values, nodes.len(), "uv_nodes"))
            .transpose()?;
        let normals = normals
            .map(|values| PerNode::try_new(values, nodes.len(), "normals"))
            .transpose()?;
        Ok(Self {
            deflection,
            nodes,
            uv_nodes,
            triangles,
            normals,
        })
    }

    /// Returns ordered model-space vertices.
    pub(crate) fn nodes(&self) -> &[FinitePoint3] {
        &self.nodes
    }

    /// Returns zero-based triangle indices.
    pub(crate) fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    #[cfg(test)]
    /// Returns optional UV coordinates in node order.
    pub(super) fn uv_nodes(&self) -> Option<&[FinitePoint2]> {
        self.uv_nodes.as_ref().map(|values| values.0.as_slice())
    }

    /// Returns optional normals in node order.
    pub(crate) fn normals(&self) -> Option<&[FiniteVector3]> {
        self.normals.as_ref().map(|values| values.0.as_slice())
    }
}

#[derive(Deserialize)]
struct TextTriangulationWire {
    deflection: FiniteReal,
    nodes: Vec<FinitePoint3>,
    uv_nodes: Option<Vec<FinitePoint2>>,
    triangles: Vec<[u32; 3]>,
    normals: Option<Vec<FiniteVector3>>,
}

/// The retained wire shape, borrowed from the triangulation it states.
///
/// Reading owns the node, attribute and triangle tables it builds; writing
/// states them once and copies nothing.
#[derive(Serialize)]
struct TextTriangulationOut<'a> {
    deflection: f64,
    nodes: &'a [FinitePoint3],
    uv_nodes: Option<&'a [FinitePoint2]>,
    triangles: OneBasedTriangles<'a>,
    normals: Option<&'a [FiniteVector3]>,
}

/// Triangle node indices written one-based, as the wire states them.
struct OneBasedTriangles<'a>(&'a [[u32; 3]]);

impl Serialize for OneBasedTriangles<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // `try_new` refuses a zero index and stores `index - 1`, so a stored
        // index is at most `u32::MAX - 1` and the restated index fits.
        serializer.collect_seq(
            self.0
                .iter()
                .map(|triangle| triangle.map(|index| index + 1)),
        )
    }
}

impl Serialize for TextTriangulation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TextTriangulationOut {
            deflection: self.deflection.get(),
            nodes: &self.nodes,
            uv_nodes: self.uv_nodes.as_ref().map(|values| values.0.as_slice()),
            triangles: OneBasedTriangles(&self.triangles),
            normals: self.normals.as_ref().map(|values| values.0.as_slice()),
        }
        .serialize(serializer)
    }
}

impl TryFrom<TextTriangulationWire> for TextTriangulation {
    type Error = String;
    fn try_from(wire: TextTriangulationWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.deflection,
            wire.nodes,
            wire.uv_nodes,
            wire.triangles,
            wire.normals,
        )
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_ir::units::FinitePoint2;

    use super::{TextTriangulation, TextTriangulationWire};

    fn point3(x: f64, y: f64, z: f64) -> FinitePoint3 {
        FinitePoint3::new(Point3::new(x, y, z)).expect("finite test point")
    }

    fn point2(u: f64, v: f64) -> FinitePoint2 {
        FinitePoint2::new(Point2::new(u, v)).expect("finite test UV point")
    }

    fn vector3(x: f64, y: f64, z: f64) -> FiniteVector3 {
        FiniteVector3::new(Vector3::new(x, y, z)).expect("finite test normal")
    }

    #[test]
    fn rejects_invalid_wire_triangle_indices() {
        for triangle in [[0, 1, 1], [1, 2, 1]] {
            let wire = TextTriangulationWire {
                deflection: FiniteReal::ZERO,
                nodes: vec![point3(0.0, 0.0, 0.0)],
                uv_nodes: None,
                triangles: vec![triangle],
                normals: None,
            };
            assert!(TextTriangulation::try_from(wire).is_err());
        }
    }

    #[test]
    fn writes_one_based_triangles_beside_the_aligned_node_attributes() {
        let value = TextTriangulation::try_new(
            FiniteReal::HALF,
            vec![
                point3(0.0, 0.0, 0.0),
                point3(1.0, 0.0, 0.0),
                point3(0.0, 1.0, 0.0),
            ],
            Some(vec![point2(0.0, 0.0), point2(1.0, 0.0), point2(0.0, 1.0)]),
            vec![[1, 2, 3], [3, 2, 1]],
            Some(vec![
                vector3(0.0, 0.0, 1.0),
                vector3(1.0, 0.0, 0.0),
                vector3(0.0, 1.0, 0.0),
            ]),
        )
        .unwrap();
        assert_eq!(value.triangles(), [[0, 1, 2], [2, 1, 0]]);
        let wire = serde_json::json!({
            "deflection": 0.5,
            "nodes": [
                {"x": 0.0, "y": 0.0, "z": 0.0},
                {"x": 1.0, "y": 0.0, "z": 0.0},
                {"x": 0.0, "y": 1.0, "z": 0.0}
            ],
            "uv_nodes": [{"u": 0.0, "v": 0.0}, {"u": 1.0, "v": 0.0}, {"u": 0.0, "v": 1.0}],
            "triangles": [[1, 2, 3], [3, 2, 1]],
            "normals": [
                {"x": 0.0, "y": 0.0, "z": 1.0},
                {"x": 1.0, "y": 0.0, "z": 0.0},
                {"x": 0.0, "y": 1.0, "z": 0.0}
            ]
        });
        assert_eq!(serde_json::to_value(&value).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<TextTriangulation>(wire).unwrap(),
            value
        );
    }

    #[test]
    fn rejects_misaligned_attributes_at_constructor_and_serde() {
        let nodes = vec![point3(0.0, 0.0, 0.0)];
        assert!(TextTriangulation::try_new(
            FiniteReal::ZERO,
            nodes.clone(),
            Some(vec![]),
            vec![],
            None
        )
        .unwrap_err()
        .contains("uv_nodes"));
        assert!(TextTriangulation::try_new(
            FiniteReal::ZERO,
            nodes.clone(),
            None,
            vec![],
            Some(vec![])
        )
        .unwrap_err()
        .contains("normals"));
        let value = TextTriangulation::try_new(
            FiniteReal::ZERO,
            nodes,
            Some(vec![point2(0.0, 0.0)]),
            vec![],
            Some(vec![vector3(0.0, 0.0, 1.0)]),
        )
        .unwrap();
        assert_eq!(value.uv_nodes().unwrap().len(), value.nodes().len());
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(
            serde_json::from_value::<TextTriangulation>(wire.clone()).unwrap(),
            value
        );
        for field in ["uv_nodes", "normals"] {
            let mut invalid = wire.clone();
            invalid[field] = serde_json::json!([]);
            assert!(serde_json::from_value::<TextTriangulation>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
}
