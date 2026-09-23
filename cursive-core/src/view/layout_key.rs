//! Keys summarizing everything that affects a view's size.
//!
//! See [`View::layout_key`](crate::view::View::layout_key).

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Returns a key that was never returned before.
///
/// Use it for content versions (bump it on every change), or as the key of
/// a view whose size may have changed in unknown ways.
pub fn fresh_layout_key() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    // The top bit keeps fresh keys apart from address-based ones.
    NEXT.fetch_add(1, Ordering::Relaxed) | (1 << 63)
}

/// Returns a key identifying the type `V`, to start a view's key from.
///
/// Views that hash their settings should start from it: two views of
/// different types can have equal settings but different sizes, and a
/// parent must notice when one replaces the other.
pub fn layout_key_seed<V: ?Sized + 'static>() -> u64 {
    combine_layout_key(0, &std::any::TypeId::of::<V>())
}

/// Combines a key with any hashable value, typically a view's own settings.
///
/// Order matters: `combine_layout_key(combine_layout_key(k, a), b)` differs
/// from `combine_layout_key(combine_layout_key(k, b), a)`.
pub fn combine_layout_key<T: Hash + ?Sized>(key: u64, value: &T) -> u64 {
    let mut hasher = KeyHasher(key);
    value.hash(&mut hasher);
    hasher.finish()
}

/// Returns the key for a view with no information about its content:
/// always changed if it needs a relayout, stable otherwise.
///
/// This is the default implementation of `View::layout_key`.
pub(crate) fn default_layout_key<T: ?Sized>(view: &T, needs_relayout: bool) -> u64 {
    if needs_relayout {
        fresh_layout_key()
    } else {
        // Stable as long as the view stays clean and doesn't move.
        view as *const T as *const () as usize as u64
    }
}

// FxHash-style mixing: fast, and good enough for comparing a view's key
// with its own previous value.
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(byte as u64);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
}
