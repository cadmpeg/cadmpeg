// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::products::{ExternalDocument, OccurrenceParent, PrototypeReference};

use crate::test_support::test_container::{external_reference_segment, outer_body_catpart};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalReferenceContent {
    id: String,
    target: String,
    segment: String,
}

#[test]
fn external_document_references_project_as_occurrences_without_invented_placements() {
    let mut body = external_reference_segment("Support.CATPart");
    body.extend(external_reference_segment("../Assembly/Support.CATProduct"));
    let bytes = outer_body_catpart(&body);
    let decoded = super::decode(bytes);
    let namespace = decoded.ir().native.namespace("catia").unwrap();
    let external_references = crate::test_support::with_service_context(|ctx| {
        namespace
            .arena_iter_as_for_decode::<ExternalReferenceContent>(ctx, "external_references")
            .collect::<Result<Vec<_>, _>>()
    })
    .unwrap();
    assert_eq!(external_references.len(), 2);
    assert_eq!(decoded.ir().model.occurrences.len(), 2);
    let expected = ["Support.CATPart", "../Assembly/Support.CATProduct"];
    for (ordinal, ((reference, occurrence), target)) in external_references
        .iter()
        .zip(&decoded.ir().model.occurrences)
        .zip(expected)
        .enumerate()
    {
        assert_eq!(reference.target, target);
        assert_eq!(
            occurrence.native_ref.as_deref(),
            Some(reference.id.as_str())
        );
        assert_eq!(occurrence.ordinal, u32::try_from(ordinal).unwrap());
        assert_eq!(occurrence.parent, OccurrenceParent::Root {});
        let PrototypeReference::External {
            document: ExternalDocument::Path { path },
            object,
        } = &occurrence.prototype
        else {
            panic!("persisted CATIA path is an external prototype");
        };
        assert_eq!(path.as_str(), target);
        assert!(object.is_none());
        assert!(occurrence.transform.is_none());
        assert!(occurrence.linked_prototype.is_none());
        assert!(occurrence.name.is_none());
        assert!(occurrence.visible.is_none());
        assert!(occurrence.link.is_none());
        let wire = serde_json::to_value(occurrence).unwrap();
        assert!(wire.get("transform").is_none());
        assert!(wire.get("linked_prototype").is_none());
        assert!(wire.get("visible").is_none());
        assert!(namespace.arenas()["finjpl_segments"]
            .iter()
            .any(|segment| segment.id() == reference.segment));
    }
    assert_ne!(
        decoded.ir().model.occurrences[0].id,
        decoded.ir().model.occurrences[1].id
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.starts_with("External-reference record ")));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}
