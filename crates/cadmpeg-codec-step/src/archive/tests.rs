// SPDX-License-Identifier: Apache-2.0
use super::{resolve_uri, ReferenceTarget, ROOT_NAME};

#[test]
fn resolves_archive_relative_uris_and_fragments() {
    assert_eq!(
        resolve_uri(ROOT_NAME, "parts/child.p21#target").unwrap(),
        ReferenceTarget::Internal {
            member: "parts/child.p21".into(),
            fragment: Some("target".into()),
        }
    );
    assert_eq!(
        resolve_uri("parts/child.p21", "../shared.p21#value").unwrap(),
        ReferenceTarget::Internal {
            member: "shared.p21".into(),
            fragment: Some("value".into()),
        }
    );
    assert_eq!(
        resolve_uri(ROOT_NAME, "https://example.invalid/part.p21#root").unwrap(),
        ReferenceTarget::External
    );
}

#[test]
fn rejects_archive_relative_traversal() {
    assert!(resolve_uri(ROOT_NAME, "../outside.p21").is_err());
    assert!(resolve_uri(ROOT_NAME, "parts//child.p21").is_err());
}
