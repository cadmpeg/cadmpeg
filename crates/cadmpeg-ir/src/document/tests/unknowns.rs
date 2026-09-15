// SPDX-License-Identifier: Apache-2.0
use crate::{CadIr, NativeUnknownRecord};

fn record(key: &str, links: &[&str]) -> NativeUnknownRecord {
    NativeUnknownRecord {
        id: crate::ids::UnknownId::mint(format!("test:native:unknown#{key}")).unwrap(),
        links: links
            .iter()
            .map(|link| (*link).try_into().unwrap())
            .collect(),
    }
}

#[test]
fn reserved_unknown_arena_accepts_owned_and_borrowed_records() {
    let records = [record("b", &[]), record("a", &["test:model:body#0"])];
    let mut owned = CadIr::empty();
    owned
        .set_native_unknowns_from("test", records.clone())
        .unwrap();
    let mut borrowed = CadIr::empty();
    borrowed
        .set_native_unknowns_from("test", records.iter())
        .unwrap();
    assert_eq!(owned, borrowed);
    assert_eq!(
        owned.native_unknowns("test").unwrap(),
        [records[1].clone(), records[0].clone()]
    );
    let wire = owned.to_canonical_json().unwrap();
    assert_eq!(CadIr::from_json(&wire).unwrap(), owned);
    let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(
        value["native"]["test"]["unknowns"],
        serde_json::json!([
            {"id": "test:native:unknown#a", "links": ["test:model:body#0"]},
            {"id": "test:native:unknown#b"}
        ])
    );
}

#[test]
fn duplicate_unknown_replacement_leaves_existing_document_unchanged() {
    for existing in [false, true] {
        let mut document = CadIr::empty();
        if existing {
            document
                .set_native_unknowns("test", &[record("original", &[])])
                .unwrap();
        }
        let before = document.clone();
        let error = document
            .set_native_unknowns_from(
                "test",
                [
                    record("duplicate", &["test:model:body#first"]),
                    record("duplicate", &["test:model:body#second"]),
                ],
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("test:native:unknown#duplicate"),
            "{error}"
        );
        assert_eq!(document, before);
    }
}

#[test]
fn complete_document_keeps_raw_native_fields_but_product_reader_checks_links() {
    let mut document = CadIr::empty();
    document
        .set_native_unknowns("test", &[record("a", &["test:model:body#unresolved"])])
        .unwrap();
    let valid = serde_json::to_value(&document).unwrap();
    let parsed = CadIr::from_json(&valid.to_string()).unwrap();
    assert_eq!(
        parsed.native_unknowns("test").unwrap(),
        document.native_unknowns("test").unwrap()
    );
    for links in [
        serde_json::json!(null),
        serde_json::json!(true),
        serde_json::json!(1),
        serde_json::json!("test:model:body#unresolved"),
        serde_json::json!({}),
        serde_json::json!([null]),
        serde_json::json!([false]),
        serde_json::json!([3]),
        serde_json::json!([{}]),
        serde_json::json!([[]]),
        serde_json::json!([""]),
        serde_json::json!(["test:body#missing-scope"]),
    ] {
        let mut wire = valid.clone();
        wire["native"]["test"]["unknowns"][0]["links"] = links;
        // Generic native fields remain raw evidence through document parsing.
        for parsed in [
            CadIr::from_json(&wire.to_string()).unwrap(),
            serde_json::from_value::<CadIr>(wire.clone()).unwrap(),
        ] {
            assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
            assert!(parsed.native_unknowns("test").is_err());
            assert!(parsed.native_unknowns_iter("test").next().unwrap().is_err());
        }
    }
}
