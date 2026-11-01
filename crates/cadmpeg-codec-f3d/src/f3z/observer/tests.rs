// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;

use super::*;
use crate::test_support::assembly_test::{
    f3d_with_text_brep_stream, f3d_without_brep, f3z_archive, XREF_ROLE,
};
use crate::F3dCodec;
use cadmpeg_ir::{Codec, DecodeOptions};

#[derive(Default)]
struct Events(RefCell<Vec<String>>);

impl F3zMemberObserver for Events {
    fn member_started(&self, path: &str) {
        self.0.borrow_mut().push(format!("start {path}"));
    }

    fn member_finished(&self, path: &str, outcome: Result<(), &CodecError>) {
        let outcome = if outcome.is_ok() { "ok" } else { "err" };
        self.0.borrow_mut().push(format!("finish {path} {outcome}"));
    }
}

/// A root placing two parents, each of which places one failing member.
fn archive_with_shared_failing_member() -> Vec<u8> {
    let component = f3d_with_text_brep_stream(
        &["FusionAssetName[Active]/Breps.BlobParts/BREP0.sat"],
        b"invalid SAT stream",
    );
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", XREF_ROLE), ("second.f3d", XREF_ROLE)],
    );
    let first = f3d_without_brep("design", "first.f3d", &[("component.f3d", XREF_ROLE)]);
    let second = f3d_without_brep("design", "second.f3d", &[("component.f3d", XREF_ROLE)]);
    f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("first.f3d", &first),
            ("second.f3d", &second),
            ("component.f3d", &component),
        ],
    )
}

#[test]
fn observer_pairs_each_member_decode_with_its_outcome() {
    let archive = archive_with_shared_failing_member();
    let events = Rc::new(Events::default());
    let decoded = observe_f3z_members(events.clone(), || {
        F3dCodec.decode(&mut Cursor::new(&archive), &DecodeOptions::default())
    });
    assert!(decoded.is_ok());
    let events = events.0.borrow();
    for path in ["root.f3d", "first.f3d", "second.f3d"] {
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == format!("start {path}"))
                .count(),
            1,
            "{path}: {events:?}"
        );
        assert!(events.contains(&format!("finish {path} ok")), "{events:?}");
    }
    let starts = events
        .iter()
        .filter(|event| *event == "start component.f3d")
        .count();
    let failures = events
        .iter()
        .filter(|event| *event == "finish component.f3d err")
        .count();
    assert!(starts >= 1, "{events:?}");
    assert_eq!(starts, failures, "{events:?}");
}

#[test]
fn observer_is_scoped_to_its_call() {
    let archive = archive_with_shared_failing_member();
    let events = Rc::new(Events::default());
    observe_f3z_members(events.clone(), || ());
    F3dCodec
        .decode(&mut Cursor::new(&archive), &DecodeOptions::default())
        .unwrap();
    assert!(events.0.borrow().is_empty());
}
