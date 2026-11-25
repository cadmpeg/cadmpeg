// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::products::Storage;
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::assembly_test::{
    f3d_with_xref_placements, f3d_without_brep, f3z_archive, XREF_ROLE,
};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;

fn decode(bytes: Vec<u8>) -> EditableDecodeResult {
    EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(bytes),
            &DecodeOptions::default(),
        )
        .unwrap(),
    )
}

/// The ids of a member's bodies as the member states them alone, each with
/// its scope qualified by `key`.
fn qualified_bodies(alone: &EditableDecodeResult, key: &str) -> BTreeSet<String> {
    alone
        .ir()
        .geometry
        .bodies
        .iter()
        .map(|body| {
            format!(
                "f3d:{key}@{}",
                body.id.as_str().strip_prefix("f3d:").unwrap()
            )
        })
        .collect()
}

fn member_definitions(decoded: &EditableDecodeResult) -> Vec<&str> {
    decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .filter(|definition| {
            matches!(
                definition.storage,
                Storage::File {
                    source: Some(_),
                    ..
                }
            )
        })
        .map(|definition| definition.id.as_str())
        .collect()
}

fn scope_findings(decoded: &EditableDecodeResult) -> Vec<String> {
    cadmpeg_ir::validate::validate_neutral_with_source_fidelity(
        decoded.ir(),
        decoded.source_fidelity(),
        Vec::new(),
    )
    .unwrap()
    .findings
    .into_iter()
    .filter(|finding| {
        matches!(
            finding.rule.as_str(),
            "products.component-record-scope-unqualified"
                | "products.entity-id-scope-key-names-no-file"
                | "identity_order.entity-id-does-not-match-format"
                | "identity_order.entity-id-not-globally-unique"
        ) || finding.rule.as_str().starts_with("products.file-digest")
    })
    .map(|finding| finding.message)
    .collect()
}

#[test]
fn records_of_each_member_take_its_definition_key() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let alone = decode(component.clone());
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", XREF_ROLE), ("second.f3d", XREF_ROLE)],
    );
    let decoded = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("first.f3d", component.as_slice()),
            ("second.f3d", component.as_slice()),
        ],
    ));

    let bodies = decoded
        .ir()
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mut expected = qualified_bodies(&alone, "first.f3d");
    expected.extend(qualified_bodies(&alone, "second.f3d"));
    assert_eq!(bodies, expected);

    assert_eq!(
        member_definitions(&decoded),
        [
            "f3d:xref:product-definition#first.f3d",
            "f3d:xref:product-definition#second.f3d"
        ]
    );
    let digest = Sha256Digest::digest(&component);
    assert_eq!(
        decoded
            .source_fidelity()
            .file_digests
            .iter()
            .map(|(definition, digest)| (definition.as_str(), digest))
            .collect::<Vec<_>>(),
        [
            ("f3d:xref:product-definition#first.f3d", &digest),
            ("f3d:xref:product-definition#second.f3d", &digest)
        ]
    );
    for key in ["first.f3d", "second.f3d"] {
        let image = decoded
            .source_fidelity()
            .retained_record(&format!("f3d:{key}@file:source-image#0"))
            .expect("each member retains its source image under its key");
        assert_eq!(image.data(), Some(component.as_slice()));
        assert!(image.stream().starts_with(&format!("f3d:{key}@")));
    }
    assert!(decoded.ir().model.occurrences.iter().all(|occurrence| {
        crate::ids::member_key(occurrence.id.as_str())
            .is_none_or(|key| ["first.f3d", "second.f3d"].contains(&key))
    }));
    assert_eq!(scope_findings(&decoded), Vec::<String>::new());
}

#[test]
fn a_member_placed_twice_is_read_once() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let alone = decode(component.clone());
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("comp.f3d", XREF_ROLE), ("comp.f3d", XREF_ROLE)],
    );
    let decoded = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    ));

    let definition = "f3d:xref:product-definition#comp.f3d";
    assert_eq!(member_definitions(&decoded), [definition]);
    let placements = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.prototype.as_str() == definition)
        .count();
    assert_eq!(placements, 2);
    assert_eq!(
        decoded
            .ir()
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str().to_owned())
            .collect::<BTreeSet<_>>(),
        qualified_bodies(&alone, "comp.f3d")
    );
    assert_eq!(
        decoded
            .source_fidelity()
            .file_digests
            .keys()
            .map(cadmpeg_ir::ids::ProductDefinitionId::as_str)
            .collect::<Vec<_>>(),
        [definition]
    );
    assert_eq!(scope_findings(&decoded), Vec::<String>::new());
}

#[test]
fn a_nested_member_definition_is_a_record_of_its_placing_member() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let middle = f3d_without_brep(
        "assembly-design",
        "middle.f3d",
        &[("component.f3d", XREF_ROLE)],
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let decoded = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    ));

    assert_eq!(
        member_definitions(&decoded),
        [
            "f3d:middle.f3d@xref:product-definition#component.f3d",
            "f3d:xref:product-definition#middle.f3d"
        ]
    );
    assert!(decoded
        .ir()
        .geometry
        .bodies
        .iter()
        .all(|body| body.id.as_str().starts_with("f3d:component.f3d@")));
    assert_eq!(scope_findings(&decoded), Vec::<String>::new());
}

fn placements_of<'a>(
    decoded: &'a EditableDecodeResult,
    definition: &'a str,
) -> impl Iterator<Item = &'a cadmpeg_ir::products::Occurrence> {
    decoded
        .ir()
        .model
        .occurrences
        .iter()
        .filter(move |occurrence| occurrence.prototype.as_str() == definition)
}

#[test]
fn each_placement_of_a_member_keeps_its_own_pose() {
    const SECOND_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let shifted = |x: f64| {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    };
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_with_xref_placements(
        "assembly-design",
        "root.f3d",
        &[
            ("comp.f3d", XREF_ROLE, shifted(1.0)),
            ("comp.f3d", SECOND_ROLE, shifted(3.0)),
        ],
        None,
    );
    let decoded = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    ));

    let definition = "f3d:xref:product-definition#comp.f3d";
    assert_eq!(member_definitions(&decoded), [definition]);
    let mut translations = placements_of(&decoded, definition)
        .map(|occurrence| occurrence.transform.unwrap().rows()[0][3])
        .collect::<Vec<_>>();
    translations.sort_by(f64::total_cmp);
    assert_eq!(translations, [10.0, 30.0]);
    assert_eq!(scope_findings(&decoded), Vec::<String>::new());
}

#[test]
fn a_path_key_escapes_each_byte_outside_the_kept_set() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let alone = decode(component.clone());
    let path = "sub dir/p@rt%.f3d";
    let key = "sub%20dir/p%40rt%25.f3d";
    let root = f3d_without_brep("assembly-design", "root.f3d", &[(path, XREF_ROLE)]);
    let decoded = decode(f3z_archive(
        "root.f3d",
        &[("root.f3d", root.as_slice()), (path, component.as_slice())],
    ));

    assert_eq!(
        member_definitions(&decoded),
        [format!("f3d:xref:product-definition#{key}")]
    );
    assert_eq!(
        decoded
            .ir()
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str().to_owned())
            .collect::<BTreeSet<_>>(),
        qualified_bodies(&alone, key)
    );
    assert_eq!(scope_findings(&decoded), Vec::<String>::new());
}

#[test]
fn a_repeated_middle_member_and_its_member_are_read_once() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let alone = decode(component.clone());
    let middle = f3d_without_brep("assembly-design", "middle.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("middle.f3d", XREF_ROLE), ("middle.f3d", XREF_ROLE)],
    );
    let decoded = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("leaf.f3d", component.as_slice()),
        ],
    ));

    let middle_definition = "f3d:xref:product-definition#middle.f3d";
    let leaf_definition = "f3d:middle.f3d@xref:product-definition#leaf.f3d";
    assert_eq!(
        member_definitions(&decoded),
        [leaf_definition, middle_definition]
    );
    assert_eq!(placements_of(&decoded, middle_definition).count(), 2);
    assert_eq!(placements_of(&decoded, leaf_definition).count(), 1);
    assert_eq!(
        decoded
            .ir()
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str().to_owned())
            .collect::<BTreeSet<_>>(),
        qualified_bodies(&alone, "leaf.f3d")
    );
    assert_eq!(scope_findings(&decoded), Vec::<String>::new());
}

#[test]
fn the_placer_of_a_member_does_not_depend_on_reference_order() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let first = f3d_without_brep("assembly-design", "a.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let second = f3d_without_brep("assembly-design", "b.f3d", &[("leaf.f3d", XREF_ROLE)]);
    for order in [["a.f3d", "b.f3d"], ["b.f3d", "a.f3d"]] {
        let root = f3d_without_brep(
            "assembly-design",
            "root.f3d",
            &[(order[0], XREF_ROLE), (order[1], XREF_ROLE)],
        );
        let decoded = decode(f3z_archive(
            "root.f3d",
            &[
                ("root.f3d", root.as_slice()),
                ("a.f3d", first.as_slice()),
                ("b.f3d", second.as_slice()),
                ("leaf.f3d", component.as_slice()),
            ],
        ));

        assert_eq!(
            member_definitions(&decoded),
            [
                "f3d:a.f3d@xref:product-definition#leaf.f3d",
                "f3d:xref:product-definition#a.f3d",
                "f3d:xref:product-definition#b.f3d"
            ],
            "reference order {order:?}"
        );
        assert_eq!(scope_findings(&decoded), Vec::<String>::new());
    }
}
