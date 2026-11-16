// SPDX-License-Identifier: Apache-2.0
//! The recursive process types the spec gives no ceiling: step folders,
//! program groups, and motions nested through a cycle's expansion. The JSON
//! depth limit bounds their reading, and every operation at the deepest
//! depth the reader admits runs on a 256 KiB stack.

use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::fixtures::{cycles, operation_id};
use super::read_desktop;
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

fn expansions(depth: usize) -> Motion {
    let leaf = || -> Cycle { cycles().remove(1) };
    (0..depth).fold(Motion::Cycle(Box::new(leaf())), |motion, _| {
        let mut cycle = leaf();
        cycle.expansion = Some(vec![motion]);
        Motion::Cycle(Box::new(cycle))
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
/// be refused by the JSON depth limit, and at the deepest admitted depth
/// reads, prints, clones, compares, and drops on a default thread.
fn nests_to_the_json_depth_limit<T>(nested: impl Fn(usize) -> T)
where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + 'static,
{
    let text = |depth| serde_json::to_string(&nested(depth)).unwrap();
    let past = (1..128)
        .find(|&depth| read_desktop::<T>(&text(depth)).is_err())
        .expect("the reader refuses some depth below the JSON limit");
    assert!(past > 1, "a single level is refused");
    let refusal = on_a_default_thread({
        let text = text(past);
        move || read_desktop::<T>(&text).err()
    });
    assert!(
        matches!(&refusal, Some(CodecError::Malformed(message)) if message.contains("recursion limit exceeded")),
        "depth {past}: {refusal:?}"
    );
    let deepest = text(past - 1);
    let printed = on_a_default_thread({
        let deepest = deepest.clone();
        move || {
            let read = read_desktop::<T>(&deepest).unwrap();
            let printed = serde_json::to_string(&read).unwrap();
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
fn step_folders_nest_to_the_json_depth_limit_on_a_default_thread() {
    nests_to_the_json_depth_limit(folders);
}

#[test]
fn program_groups_nest_to_the_json_depth_limit_on_a_default_thread() {
    nests_to_the_json_depth_limit(groups);
}

#[test]
fn cycle_expansions_nest_to_the_json_depth_limit_on_a_default_thread() {
    nests_to_the_json_depth_limit(expansions);
}
