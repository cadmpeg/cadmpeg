// SPDX-License-Identifier: Apache-2.0
//! The recursive process types: step folders and program groups. A cycle's
//! expansion holds no cycle, so motions do not nest. The document's depth
//! bound (§2.5) bounds their reading, and every operation at the deepest
//! depth the reader admits runs on a 256 KiB stack.

use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::fixtures::{cycles, operation_id};
use super::read_desktop;
use crate::bounds::MAX_DOCUMENT_DEPTH;
use crate::process::cycle::Cycle;
use crate::process::setup::{ProgramStep, Step};
use crate::process::toolpath::Motion;

fn folders(depth: usize) -> Step {
    (0..depth).fold(Step::Operation(operation_id(1)), |step, _| Step::Folder {
        name: None,
        steps: vec![step],
    })
}

fn groups(depth: usize) -> ProgramStep {
    (0..depth).fold(ProgramStep::Operation(operation_id(1)), |step, _| {
        ProgramStep::Group {
            name: None,
            steps: vec![step],
        }
    })
}

/// Runs `work` on a thread whose stack is 2 MiB, the default for a spawned
/// thread.
fn on_a_default_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}

/// Finds the deepest nesting the reader admits, requires one level more to
/// be refused by the document's depth bound, and at the deepest admitted depth
/// reads, prints, clones, compares, and drops on a default thread.
fn nests_to_the_document_depth_bound<T>(nested: impl Fn(usize) -> T)
where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + 'static,
{
    let text = |depth| cadmpeg_core::serde_erase::json::to_string(&nested(depth)).unwrap();
    let past = (1..=MAX_DOCUMENT_DEPTH + 1)
        .find(|&depth| read_desktop::<T>(&text(depth)).is_err())
        .expect("the reader refuses a depth past the document bound");
    assert!(past > 1, "a single level is refused");
    let refusal = on_a_default_thread({
        let text = text(past);
        move || read_desktop::<T>(&text).err()
    });
    assert!(
        matches!(&refusal, Some(CodecError::Malformed(message)) if message.contains("levels of a CADIR document")),
        "depth {past}: {refusal:?}"
    );
    let deepest = text(past - 1);
    let printed = on_a_default_thread({
        let deepest = deepest.clone();
        move || {
            let read = read_desktop::<T>(&deepest).unwrap();
            let printed = cadmpeg_core::serde_erase::json::to_string(&read).unwrap();
            let cloned = read.clone();
            assert!(cloned == read);
            drop(cloned);
            drop(read);
            printed
        }
    });
    assert_eq!(printed, deepest);
}

#[test]
fn step_folders_nest_to_the_document_depth_bound_on_a_default_thread() {
    nests_to_the_document_depth_bound(folders);
}

#[test]
fn program_groups_nest_to_the_document_depth_bound_on_a_default_thread() {
    nests_to_the_document_depth_bound(groups);
}

#[test]
fn a_cycle_expansion_holds_no_cycle() {
    let inner =
        cadmpeg_core::serde_erase::json::to_value(Motion::Cycle(Box::new(cycles().remove(1))))
            .unwrap();
    let mut outer = cadmpeg_core::serde_erase::json::to_value(cycles().remove(1)).unwrap();
    outer["expansion"] = serde_json::json!([inner]);
    assert!(read_desktop::<Cycle>(&outer.to_string()).is_err());
}
