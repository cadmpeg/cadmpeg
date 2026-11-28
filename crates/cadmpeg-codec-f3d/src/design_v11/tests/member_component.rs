// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn a_member_design_without_a_scope_is_held_by_the_nearest_member_that_places_it() {
    const OUTER: &str = "outer.f3d@";
    let definitions = [
        definition(
            "f3d:xref:product-definition#outer.f3d",
            Storage::Embedded,
            &[],
        ),
        definition(
            "f3d:outer.f3d@xref:product-definition#inner.f3d",
            Storage::Embedded,
            &[],
        ),
        definition(
            "f3d:inner.f3d@xref:product-definition#deep.f3d",
            Storage::Embedded,
            &[],
        ),
        definition(
            "f3d:deep.f3d@model:component#c",
            Storage::Embedded,
            &["f3d:deep.f3d@brep:entity#1"],
        ),
    ];
    let placers = crate::design_v11::Placers::new(&definitions);
    let scopes = BTreeMap::from([(String::new(), 0_u8), (OUTER.to_owned(), 0)]);
    assert_eq!(crate::design_v11::host(&scopes, &placers, OUTER), OUTER);
    assert_eq!(
        crate::design_v11::host(&scopes, &placers, "inner.f3d@"),
        OUTER
    );
    assert_eq!(
        crate::design_v11::host(&scopes, &placers, "deep.f3d@"),
        OUTER
    );
    assert_eq!(crate::design_v11::host(&scopes, &placers, "stray.f3d@"), "");
    assert_eq!(
        crate::design_v11::host(&BTreeMap::<String, u8>::new(), &placers, "deep.f3d@"),
        ""
    );

    let scopes = BTreeMap::from([
        (String::new(), (crate::tree::file_root_id(), ())),
        (
            OUTER.to_owned(),
            (
                ProductDefinitionId::mint("f3d:xref:product-definition#outer.f3d").unwrap(),
                (),
            ),
        ),
    ]);
    let spans = crate::design_v11::span(&definitions, &placers, &scopes);
    assert!(spans[OUTER]
        .bodies
        .contains(&BodyId::mint("f3d:deep.f3d@brep:entity#1").unwrap()));
}
