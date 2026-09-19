// SPDX-License-Identifier: Apache-2.0
//! External-reference designs, their references and the placement transform they state.

use super::identity::DesignAffineTransform;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_transform, XrefPlacementTransform, "transform");
const EPS_XREF_PLACEMENT_RIGID_E8: f64 = 1.0e-8;

fn valid_xref_placement_transform(transform: &[[f64; 4]; 4]) -> bool {
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return false;
    }
    let columns = [
        [transform[0][0], transform[1][0], transform[2][0]],
        [transform[0][1], transform[1][1], transform[2][1]],
        [transform[0][2], transform[1][2], transform[2][2]],
    ];
    for (ordinal, column) in columns.iter().enumerate() {
        let norm = column.iter().map(|value| value * value).sum::<f64>();
        if (norm - 1.0).abs() > EPS_XREF_PLACEMENT_RIGID_E8 {
            return false;
        }
        for other in &columns[..ordinal] {
            let dot = column
                .iter()
                .zip(other)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > EPS_XREF_PLACEMENT_RIGID_E8 {
                return false;
            }
        }
    }
    let determinant = transform[0][0]
        * (transform[1][1] * transform[2][2] - transform[1][2] * transform[2][1])
        - transform[0][1] * (transform[1][0] * transform[2][2] - transform[1][2] * transform[2][0])
        + transform[0][2] * (transform[1][0] * transform[2][1] - transform[1][1] * transform[2][0]);
    (determinant - 1.0).abs() <= EPS_XREF_PLACEMENT_RIGID_E8
}

/// A finite proper rigid transform from an F3D XREF placement record.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct XrefPlacementTransform(DesignAffineTransform);

impl XrefPlacementTransform {
    /// Four row-major rows.
    pub(crate) fn rows(self) -> [[f64; 4]; 4] {
        self.0.rows()
    }
}

impl TryFrom<[[f64; 4]; 4]> for XrefPlacementTransform {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if valid_xref_placement_transform(&rows) {
            Ok(Self(DesignAffineTransform(rows)))
        } else {
            Err("transform must be a finite proper rigid affine matrix".into())
        }
    }
}

impl From<XrefPlacementTransform> for [[f64; 4]; 4] {
    fn from(value: XrefPlacementTransform) -> Self {
        value.rows()
    }
}

/// One design entry of the top-level `RedirectionsStream.dat` table
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
/// The first source entry describes the document itself; each further entry
/// describes one referenced document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefDesign {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Position of this entry in the source `designs` array; entry 0 is the
    /// document itself.
    pub(crate) ordinal: u32,
    /// Source `file-version` integer.
    pub(crate) file_version: i64,
    /// The document's `.f3d` file name.
    pub(crate) target_file_name: String,
    /// The document's display name.
    pub(crate) display_name: String,
    /// `urn:adsk.wipprod:dm.lineage:<key>` lineage identity.
    pub(crate) lineage_urn: String,
    /// `urn:adsk.wipprod:fs.file:vf.<key>?version=N` version identity.
    pub(crate) version_urn: String,
}

/// One outgoing XREF placement of the top-level `RedirectionsStream.dat` table
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct XrefReference {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Position of this reference in the source `references` array.
    pub(crate) ordinal: u32,
    /// Zero-based occurrence position among Design records carrying this
    /// container reference's occurrence role.
    #[serde(default)]
    pub(crate) occurrence_ordinal: u32,
    /// The referencing document's own file name.
    pub(crate) from: String,
    /// The target design entry's `target_file_name`.
    pub(crate) relative_path: String,
    /// Occurrence-role GUID joining this reference to the Design-segment
    /// `DcXRefPCIFeature` record and the ACT GUID pool.
    /// The role also accepts a GUID prefix followed by an underscore and URN, beyond relaxed GUID text.
    pub(crate) neutron_role: String,
    /// The independent `neutronData` property value. It is retained exactly
    /// and is never inferred from or aliased to `neutron_role`.
    pub(crate) neutron_data: String,
    /// Source Design occurrence transform in centimetres. `None` is the
    /// serialized identity-placement form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<XrefPlacementTransform>,
}
