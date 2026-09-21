// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::math::{Point2, Point3, Vector3};
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
    pub(crate) deflection: f64,
    nodes: Vec<Point3>,
    uv_nodes: Option<PerNode<Point2>>,
    triangles: Vec<[u32; 3]>,
    normals: Option<PerNode<Vector3>>,
}

impl TextTriangulation {
    /// Admits attributes with exactly one value per node.
    pub(super) fn try_new(
        deflection: f64,
        nodes: Vec<Point3>,
        uv_nodes: Option<Vec<Point2>>,
        triangles: Vec<[u32; 3]>,
        normals: Option<Vec<Vector3>>,
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
    pub(crate) fn nodes(&self) -> &[Point3] {
        &self.nodes
    }

    /// Returns zero-based triangle indices.
    pub(crate) fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    #[cfg(test)]
    /// Returns optional UV coordinates in node order.
    pub(super) fn uv_nodes(&self) -> Option<&[Point2]> {
        self.uv_nodes.as_ref().map(|values| values.0.as_slice())
    }

    /// Returns optional normals in node order.
    pub(crate) fn normals(&self) -> Option<&[Vector3]> {
        self.normals.as_ref().map(|values| values.0.as_slice())
    }
}

#[derive(Deserialize)]
struct TextTriangulationWire {
    deflection: f64,
    nodes: Vec<Point3>,
    uv_nodes: Option<Vec<Point2>>,
    triangles: Vec<[u32; 3]>,
    normals: Option<Vec<Vector3>>,
}

/// The retained wire shape, borrowed from the triangulation it states.
///
/// Reading owns the node, attribute and triangle tables it builds; writing
/// states them once and copies nothing.
#[derive(Serialize)]
struct TextTriangulationOut<'a> {
    deflection: f64,
    nodes: &'a [Point3],
    uv_nodes: Option<&'a [Point2]>,
    triangles: OneBasedTriangles<'a>,
    normals: Option<&'a [Vector3]>,
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
            deflection: self.deflection,
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
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    use super::{TextTriangulation, TextTriangulationWire};

    #[test]
    fn rejects_invalid_wire_triangle_indices() {
        for triangle in [[0, 1, 1], [1, 2, 1]] {
            let wire = TextTriangulationWire {
                deflection: 0.0,
                nodes: vec![Point3::new(0.0, 0.0, 0.0)],
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
            0.5,
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            Some(vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
            ]),
            vec![[1, 2, 3], [3, 2, 1]],
            Some(vec![
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
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
        let nodes = vec![Point3::new(0.0, 0.0, 0.0)];
        assert!(
            TextTriangulation::try_new(0.0, nodes.clone(), Some(vec![]), vec![], None)
                .unwrap_err()
                .contains("uv_nodes")
        );
        assert!(
            TextTriangulation::try_new(0.0, nodes.clone(), None, vec![], Some(vec![]))
                .unwrap_err()
                .contains("normals")
        );
        let value = TextTriangulation::try_new(
            0.0,
            nodes,
            Some(vec![Point2::new(0.0, 0.0)]),
            vec![],
            Some(vec![Vector3::new(0.0, 0.0, 1.0)]),
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
