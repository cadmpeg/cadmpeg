// SPDX-License-Identifier: Apache-2.0
//! Registry-driven validation of every typed entity reference.

use crate::document::CadIr;
use crate::index::ModelIndex;
use crate::report::{Check, Finding, Severity};
use crate::schema::EntitySchema;

pub(super) fn check_typed_references(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    macro_rules! check_arenas {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
            $(for entity in &ir.model.$field {
                let owner = entity.identity();
                entity.visit_references(&mut |reference| {
                    if !index.contains(&reference.target)
                        && !findings.iter().any(|finding| {
                            finding.check == Check::ReferentialIntegrity
                                && finding.entity.as_deref() == Some(owner)
                                && finding.message.contains(&reference.target)
                        })
                    {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!("unresolved typed reference {}", reference.target),
                            entity: Some(owner.to_owned()),
                        });
                    }
                });
            })*
        };
    }

    crate::document::arena_registry!(check_arenas);
}

#[cfg(test)]
mod tests {
    use super::check_typed_references;
    use crate::assets::AssetId;
    use crate::examples::unit_cube;
    use crate::index::ModelIndex;
    use crate::math::Point3;
    use crate::report::{Check, Severity};
    use crate::tessellation::{
        Tessellation, TessellationNormals, TessellationTextureAssignment, TessellationTopology,
    };

    #[test]
    fn unresolved_asset_reference_is_reported() {
        let missing = AssetId::mint("synthetic:test:asset#missing").expect("valid identity");
        let tessellation = Tessellation::new(
            "synthetic:test:tessellation#textured",
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2]],
            TessellationTopology::List,
            TessellationNormals::None,
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_texture_assignments(vec![TessellationTextureAssignment {
            source_id: None,
            texture: missing.clone(),
            triangles: vec![0],
        }])
        .expect("valid local texture assignment");
        let owner = tessellation.id.clone();
        let mut ir = unit_cube();
        ir.model.tessellations.push(tessellation);
        let mut findings = Vec::new();
        check_typed_references(&ir, &ModelIndex::new(&ir), &mut findings);
        assert!(findings.iter().any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.severity == Severity::Error
                && finding.entity.as_deref() == Some(owner.as_str())
                && finding.message == format!("unresolved typed reference {missing}")
        }));
    }
}
