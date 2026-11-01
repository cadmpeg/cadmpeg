// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::UsdLossCode;

/// Value-level golden: the stable string form of every code, pinned.
#[test]
fn code_strings_are_pinned() {
    let codes: Vec<&str> = UsdLossCode::ALL.iter().map(|code| code.code()).collect();
    assert_eq!(
        codes,
        [
            "source.dialect-unverified",
            "composition.sublayers-not-composed",
            "composition.external-arc",
            "composition.layer-unavailable",
            "composition.target-missing",
            "composition.variant-not-selected",
            "composition.over-undefined",
            "composition.cycle",
            "composition.override-not-applied",
            "prim.type-not-transferred",
            "prim.intrinsic-not-transferred",
            "prim.api-schema-not-transferred",
            "attribute.property-not-transferred",
            "attribute.value-malformed",
            "attribute.animation-not-transferred",
            "metadata.not-transferred",
            "crate.value-not-read",
            "xform.malformed",
            "mesh.subdivision-not-applied",
            "mesh.face-rejected",
            "mesh.rejected",
            "mesh.primvar-not-transferred",
            "mesh.normals-not-transferred",
            "mesh.subset-not-transferred",
            "curves.rejected",
            "nurbs.patch-rejected",
            "nurbs.trim-not-transferred",
            "nurbs.range-not-transferred",
            "appearance.display-color-not-transferred",
            "appearance.material-not-transferred",
            "appearance.texture-unlocated",
            "physics.joint-rejected",
            "physics.joint-property-not-transferred",
            "physics.solver-property-not-transferred",
            "physics.property-not-transferred",
            "export.vertex-precision",
            "export.face-not-written",
            "export.curve-not-written",
            "export.channel-not-written",
            "export.appearance-reduced",
            "export.texture-external",
            "export.topology-tessellated",
            "export.carrier-not-written",
            "export.joint-not-written",
            "export.solver-property-not-written",
        ]
    );
}

/// Codes are unique and use the stable `family.detail` kebab shape.
#[test]
fn codes_are_unique_and_well_formed() {
    let mut seen = BTreeSet::new();
    for code in UsdLossCode::ALL {
        let text = code.code();
        assert!(seen.insert(text), "duplicate code {text}");
        let (family, detail) = text.split_once('.').expect("family.detail shape");
        assert!(!family.is_empty() && !detail.is_empty());
        assert!(
            text.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'.' || byte == b'-'),
            "code {text} is not lowercase kebab"
        );
    }
}

/// The note builder fixes severity from the code.
#[test]
fn note_takes_severity_from_the_code() {
    for code in UsdLossCode::ALL {
        let note = code.note("x");
        assert_eq!(note.severity, code.spec().1);
        assert_eq!(note.message, "x");
        assert_eq!(note.code, code.kind());
    }
}
