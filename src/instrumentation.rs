// Copyright 2026 Daymon Littrell-Reyes
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Internal feature-gated tracing macros.
//!
//! Keeping the feature gate here lets call sites retain structured fields
//! without evaluating those fields or depending on `tracing` when
//! instrumentation is disabled.
#![allow(unused_macros)] // Some levels are unused in protocol-only feature sets.

/// Non-unit guard used to keep span call sites lint-clean without tracing.
#[cfg(not(feature = "tracing"))]
pub(crate) struct DisabledSpanGuard;

macro_rules! trace_event {
  ($($fields:tt)*) => {{
    #[cfg(feature = "tracing")]
    ::tracing::trace!($($fields)*);
  }};
}

macro_rules! debug_event {
  ($($fields:tt)*) => {{
    #[cfg(feature = "tracing")]
    ::tracing::debug!($($fields)*);
  }};
}

macro_rules! info_event {
  ($($fields:tt)*) => {{
    #[cfg(feature = "tracing")]
    ::tracing::info!($($fields)*);
  }};
}

macro_rules! warn_event {
  ($($fields:tt)*) => {{
    #[cfg(feature = "tracing")]
    ::tracing::warn!($($fields)*);
  }};
}

macro_rules! error_event {
  ($($fields:tt)*) => {{
    #[cfg(feature = "tracing")]
    ::tracing::error!($($fields)*);
  }};
}

macro_rules! trace_span {
  ($($fields:tt)*) => {{
    #[cfg(feature = "tracing")]
    let guard = ::tracing::trace_span!($($fields)*).entered();
    #[cfg(not(feature = "tracing"))]
    let guard = $crate::instrumentation::DisabledSpanGuard;
    guard
  }};
}
