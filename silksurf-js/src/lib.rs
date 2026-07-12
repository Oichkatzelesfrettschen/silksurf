//! `SilkSurfJS` -- the `SilkSurf` JavaScript runtime.
//!
//! Production execution delegates to `boa_engine` (ECMA-262 2024+)
//! through [`SilkContext`], which installs the browser host layer:
//! DOM bridge, document/location/navigator, storage, crypto, fetch,
//! timers, and console. The hand-written VM this crate once carried is
//! removed per AD-025; git history preserves it and
//! `silksurf-specification/SILKSURF-JS-DESIGN.md` records its design.
#![allow(
    clippy::collapsible_if,
    clippy::doc_markdown,
    clippy::map_unwrap_or,
    clippy::redundant_closure,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::needless_lifetimes,
    clippy::type_complexity,
    clippy::needless_pass_by_value,
    clippy::collapsible_else_if
)]
#![allow(unknown_lints)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
// Host-object installation code favors explicit match arms and long
// builder chains; these documentation and shape lints stay deferred
// until the embedding API stabilizes.
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::too_many_lines)]
// boa native-function callbacks require Result-returning signatures, so
// host installers keep Result wrappers even where the body cannot fail.
#![allow(clippy::unnecessary_wraps)]
// DOM node ids and typed-array/crypto sizes cross u32/f64/usize
// boundaries at the JS value interface; ranges are checked at the
// conversion sites.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

#[cfg(all(feature = "fast-alloc", not(target_arch = "wasm32")))]
#[global_allocator]
static GLOBAL_ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "tracing-full")]
pub mod tracing_support;

// Production JS runtime backed by boa_engine (ECMA-262 2024+).
pub mod boa_backend;

// Re-export the crate-level entry point.
pub use boa_backend::{AsyncCompletion, SilkContext};
