// SPDX-License-Identifier: Apache-2.0
//! Optional preparation and member progress notifications for F3Z decoding.
//!
//! A caller installs an observer for the current thread with
//! [`observe_f3z_members`]. Every member decode that runs on that thread,
//! including a repeated decode of the same member, calls
//! [`F3zMemberObserver::member_started`] before and
//! [`F3zMemberObserver::member_finished`] after it. With no observer installed
//! the cost is one thread-local read per phase or member decode.

use std::cell::RefCell;
use std::rc::Rc;

use crate::container::ContainerScan;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Preparation phases before F3Z member decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum F3zPhase {
    /// Read the outer ZIP directory and validate its payloads.
    Scan,
    /// Resolve the model root and classify nested member containers.
    Classify,
    /// Set the member-decode checkpoint and prepare the requested member list.
    SessionOpen,
}

/// Receives preparation progress and one notification pair per member decode.
///
/// `path` is the archive member path. The decoder keeps no clock; an observer
/// that needs durations measures the time between the two calls itself.
pub trait F3zMemberObserver {
    /// Preparation of `phase` starts.
    fn phase_started(&self, _phase: F3zPhase) {}
    /// The preparation phase has ended with `outcome`.
    fn phase_finished(&self, _phase: F3zPhase, _outcome: Result<(), &CodecError>) {}

    /// A decode of member `path` starts.
    fn member_started(&self, path: &str);
    /// The decode of member `path` that started last has ended with `outcome`.
    fn member_finished(&self, path: &str, outcome: Result<(), &CodecError>);
}

thread_local! {
    static OBSERVER: RefCell<Option<Rc<dyn F3zMemberObserver>>> = const { RefCell::new(None) };
}

/// Runs `work` with `observer` installed for preparation and member decodes.
/// The scan phase also occurs for a single-document F3D input.
///
/// The previous observer, if any, is restored when `work` returns. Member
/// decodes on other threads need their own installation on each thread.
pub fn observe_f3z_members<R>(observer: Rc<dyn F3zMemberObserver>, work: impl FnOnce() -> R) -> R {
    struct Restore(Option<Rc<dyn F3zMemberObserver>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            OBSERVER.with(|slot| *slot.borrow_mut() = previous);
        }
    }
    let previous = OBSERVER.with(|slot| slot.borrow_mut().replace(observer));
    let _restore = Restore(previous);
    work()
}

/// Notifies the installed observer around one member decode.
pub(super) fn observed<T>(
    path: &str,
    decode: impl FnOnce() -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let observer = OBSERVER.with(|slot| slot.borrow().clone());
    let Some(observer) = observer else {
        return decode();
    };
    observer.member_started(path);
    let result = decode();
    observer.member_finished(path, result.as_ref().map(|_| ()));
    result
}

/// Notifies preparation progress before the archive kind is known.
/// A single-document F3D input also emits the scan pair.
pub(crate) fn scan<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerScan<'a>, CodecError> {
    phase(F3zPhase::Scan, || crate::container::scan(ctx, root))
}

pub(super) fn phase<T>(
    phase: F3zPhase,
    work: impl FnOnce() -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let observer = OBSERVER.with(|slot| slot.borrow().clone());
    let Some(observer) = observer else {
        return work();
    };
    observer.phase_started(phase);
    let result = work();
    observer.phase_finished(phase, result.as_ref().map(|_| ()));
    result
}

#[cfg(test)]
mod tests;
