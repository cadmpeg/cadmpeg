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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TextTriangulationWire", into = "TextTriangulationWire")]
pub struct TextTriangulation {
    /// Chordal deflection.
    pub deflection: f64,
    nodes: Vec<Point3>,
    uv_nodes: Option<PerNode<Point2>>,
    triangles: Vec<[u32; 3]>,
    normals: Option<PerNode<Vector3>>,
}

impl TextTriangulation {
    /// Admits attributes with exactly one value per node.
    pub fn try_new(
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
    pub fn nodes(&self) -> &[Point3] {
        &self.nodes
    }

    /// Returns zero-based triangle indices.
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    #[cfg(test)]
    /// Returns optional UV coordinates in node order.
    pub fn uv_nodes(&self) -> Option<&[Point2]> {
        self.uv_nodes.as_ref().map(|values| values.0.as_slice())
    }

    /// Returns optional normals in node order.
    pub fn normals(&self) -> Option<&[Vector3]> {
        self.normals.as_ref().map(|values| values.0.as_slice())
    }
}

#[derive(Serialize, Deserialize)]
struct TextTriangulationWire {
    deflection: f64,
    nodes: Vec<Point3>,
    uv_nodes: Option<Vec<Point2>>,
    triangles: Vec<[u32; 3]>,
    normals: Option<Vec<Vector3>>,
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

impl From<TextTriangulation> for TextTriangulationWire {
    fn from(value: TextTriangulation) -> Self {
        Self {
            deflection: value.deflection,
            nodes: value.nodes,
            uv_nodes: value.uv_nodes.map(|values| values.0),
            triangles: value
                .triangles
                .into_iter()
                .map(|triangle| triangle.map(|index| index + 1))
                .collect(),
            normals: value.normals.map(|values| values.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
