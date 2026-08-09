//! Buffers that zero themselves on drop.
//!
//! [`kamiroh_domain::NodeSecret`] protects key material once it is inside the
//! type, but reading a key from disk and writing one back both need a plaintext
//! buffer on the way through. Those buffers get the same treatment, so no copy
//! of a secret outlives the function that made it.

use core::ops::{Deref, DerefMut};

/// Overwrites `bytes` with zeroes in a way the compiler may not elide.
///
/// A plain `bytes.fill(0)` before a drop is a dead store the optimiser is free
/// to remove, which is exactly the store that matters here.
#[allow(unsafe_code)]
fn wipe(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusively borrowed `u8`.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// A heap buffer wiped on drop — used for bytes read off disk.
pub(crate) struct WipedVec(Vec<u8>);

impl WipedVec {
    /// Takes ownership of `bytes`, which will be wiped when this value drops.
    ///
    /// Note this cannot wipe earlier copies: pass a buffer that has not been
    /// cloned, reallocated, or otherwise duplicated since it was filled.
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Deref for WipedVec {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for WipedVec {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// A fixed-size buffer wiped on drop — used for the hex form written to disk.
pub(crate) struct WipedArray<const N: usize>([u8; N]);

impl<const N: usize> WipedArray<N> {
    /// Creates a zeroed buffer.
    pub(crate) fn zeroed() -> Self {
        Self([0u8; N])
    }
}

impl<const N: usize> Deref for WipedArray<N> {
    type Target = [u8; N];

    fn deref(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> DerefMut for WipedArray<N> {
    fn deref_mut(&mut self) -> &mut [u8; N] {
        &mut self.0
    }
}

impl<const N: usize> Drop for WipedArray<N> {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}
