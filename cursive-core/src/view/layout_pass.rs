//! Scoping for memoized size computations.
//!
//! Views can't be modified while a layout (or a size computation) is running:
//! the whole tree is borrowed for it. Any answer computed during that time
//! can be reused until the outermost call returns, even for views that
//! otherwise need a relayout. Containers use this to avoid re-measuring the
//! same child with the same constraint several times.

use std::cell::Cell;

thread_local! {
    /// (id of the current pass, number of nested `in_layout_pass` calls).
    static PASS: Cell<(u64, usize)> = const { Cell::new((0, 0)) };
}

/// Runs `f` inside a layout pass, starting a new one if none is running.
pub(crate) fn in_layout_pass<R>(f: impl FnOnce() -> R) -> R {
    struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            PASS.with(|pass| {
                let (id, depth) = pass.get();
                pass.set((id, depth - 1));
            });
        }
    }

    PASS.with(|pass| {
        let (id, depth) = pass.get();
        pass.set(if depth == 0 {
            (id + 1, 1)
        } else {
            (id, depth + 1)
        });
    });
    let _guard = Guard;
    f()
}

/// Returns the id of the current layout pass, if one is running.
pub(crate) fn current_pass() -> Option<u64> {
    PASS.with(|pass| match pass.get() {
        (_, 0) => None,
        (id, _) => Some(id),
    })
}
