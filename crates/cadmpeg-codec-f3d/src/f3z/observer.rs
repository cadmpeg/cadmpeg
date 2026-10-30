// SPDX-License-Identifier: Apache-2.0
//! Optional per-member progress notifications for F3Z decoding.
//!
//! A caller installs an observer for the current thread with
//! [`observe_f3z_members`]. Every member decode that runs on that thread,
//! including a repeated decode of the same member, calls
//! [`F3zMemberObserver::member_started`] before and
//! [`F3zMemberObserver::member_finished`] after it. With no observer installed
//! the cost is one thread-local read per member decode.

use std::cell::RefCell;
use std::rc::Rc;

use cadmpeg_core::CodecError;

/// Receives one start and one finish notification per F3Z member decode.
///
/// `path` is the archive member path. The decoder keeps no clock; an observer
/// that needs durations measures the time between the two calls itself.
pub trait F3zMemberObserver {
    /// A decode of member `path` starts.
    fn member_started(&self, path: &str);
    /// The decode of member `path` that started last has ended with `outcome`.
    fn member_finished(&self, path: &str, outcome: Result<(), &CodecError>);
}

thread_local! {
    static OBSERVER: RefCell<Option<Rc<dyn F3zMemberObserver>>> = const { RefCell::new(None) };
}

/// Runs `work` with `observer` installed for F3Z member decodes on this thread.
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

#[cfg(test)]
mod tests;
