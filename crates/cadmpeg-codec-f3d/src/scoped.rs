//! Scoped threads that still work on `wasm32-unknown-unknown`.
//!
//! That target has no threads: `std::thread::scope`'s `spawn` panics there (a trap, in a
//! WebAssembly host), so the decoder would stop on every design with design history. Natively
//! this is `std::thread::scope`. On that target each spawned closure runs to completion when it
//! is spawned and `join` returns its result, so the same code gives the same answer on one
//! thread.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use std::thread::scope;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use sequential::scope;

// Built natively only for its test.
#[cfg(any(test, all(target_arch = "wasm32", target_os = "unknown")))]
mod sequential {
    use std::marker::PhantomData;

    pub(crate) struct Scope<'scope, 'env: 'scope> {
        scope: PhantomData<&'scope mut &'scope ()>,
        env: PhantomData<&'env mut &'env ()>,
    }

    pub(crate) struct ScopedJoinHandle<'scope, T> {
        result: std::thread::Result<T>,
        scope: PhantomData<&'scope ()>,
    }

    impl<'scope> Scope<'scope, '_> {
        pub(crate) fn spawn<F, T>(&'scope self, work: F) -> ScopedJoinHandle<'scope, T>
        where
            F: FnOnce() -> T + Send + 'scope,
            T: Send + 'scope,
        {
            // `&self` is `std::thread::scope`'s own signature.
            let Self { scope: _, env: _ } = self;
            ScopedJoinHandle {
                result: Ok(work()),
                scope: PhantomData,
            }
        }
    }

    impl<T> ScopedJoinHandle<'_, T> {
        pub(crate) fn join(self) -> std::thread::Result<T> {
            self.result
        }
    }

    pub(crate) fn scope<'env, F, T>(body: F) -> T
    where
        F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> T,
    {
        body(&Scope {
            scope: PhantomData,
            env: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::sequential;

    /// The shim gives what `std::thread::scope` gives: spawned closures borrow from the caller,
    /// mutate disjoint data, and `join` returns each closure's value.
    #[test]
    fn spawned_work_runs_to_completion_and_joins_with_its_value() {
        let words = ["a".to_owned(), "bc".to_owned()];
        let mut total = 0_usize;
        let (lengths, doubled) = sequential::scope(|scope| {
            let lengths = scope.spawn(|| words.iter().map(String::len).collect::<Vec<_>>());
            let doubled = scope.spawn(|| {
                total += 21;
                total * 2
            });
            (lengths.join().ok(), doubled.join().ok())
        });
        assert_eq!(lengths, Some(vec![1, 2]));
        assert_eq!(doubled, Some(42));
        assert_eq!(total, 21);
    }
}
