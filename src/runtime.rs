//! Sync-to-async bridge for synchronous paths that must call async network work.

use std::future::Future;

/// Run an async future to completion from a synchronous context.
///
/// When an active runtime is present this uses `block_in_place` on the current
/// handle, which **requires the multi-thread runtime flavor** (`rt-multi-thread`,
/// the `#[tokio::main]` default). The exec hot path reaches this through nested
/// `block_in_place` (dispatch wraps `pass_through::run`, which reaches
/// `quota::get`); nested `block_in_place` is supported only under the
/// multi-thread runtime — do not switch the runtime flavor or move these sync
/// paths onto a current-thread context.
///
/// The fallback arm runs only when there is **no** active runtime (e.g. a sync
/// unit test that never entered Tokio). It does **not** cover in-process calls
/// from inside a current-thread `#[tokio::test]`: there `try_current()` returns
/// `Ok`, and `block_in_place` would panic. Any future in-process test that
/// exercises these sync bridges must use `#[tokio::test(flavor = "multi_thread")]`.
pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        #[expect(
            clippy::expect_used,
            reason = "fallback runtime build is infallible in practice"
        )]
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build fallback current-thread runtime");
        runtime.block_on(fut)
    }
}
