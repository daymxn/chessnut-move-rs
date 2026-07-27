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

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
//! Typed, transport-independent SDK for Chessnut Move boards.
//!
//! The crate separates the board's byte protocol from Bluetooth I/O. The
//! [`protocol`] module provides typed commands and decoded board data, while
//! [`transport`] provides runtime-neutral session traits and optional adapters.
//!
//! Includes support for `no_std` environments, as well as fully featured [Tokio] and `async` based
//! projects.
//!
//! # Features
//!
//! - `std` enables standard-library support and is enabled by default.
//! - `async` enables `transport::AsyncBoard` and is enabled by default.
//! - `blocking` enables `transport::BlockingBoard`.
//! - `btleplug` enables `transport::btleplug::BtleplugTransport`, providing support for [btleplug].
//! - `tokio` enables the actor API in `transport::tokio`.
//! - `tracing` emits structured spans and events through [tracing]. This
//!   feature also enables `alloc`, but does not install a subscriber.
//!
//! Disable default features to use the protocol types in `no_std`
//! environments.
//!
//! # Tracing
//!
//! Enable the `tracing` feature to emit structured spans and events from
//! protocol decoding, transport I/O, session lifecycle operations, actor
//! queries, and recoverable failures. The crate does not install a
//! [subscriber], so applications retain control over formatting, filtering,
//! and collection.
//!
//! Event targets follow the Rust module path, such as
//! `chessnut_move::transport::tokio`. Routine command and notification traffic
//! is emitted at `trace`, lifecycle and query state at `debug` or `info`, and
//! failures at `warn` or `error`. Raw command and notification payloads are not
//! recorded.
//!
//! In a standard application, a [`tracing-subscriber`] formatter can be
//! installed before connecting:
//!
//! ```no_run
//! tracing_subscriber::fmt()
//!     .with_env_filter("chessnut_move=debug")
//!     .init();
//! ```
//!
//! In `no_std` environments, the `tracing` feature requires `alloc` and can
//! emit to any compatible collector supplied by the application.
//!
//! # Examples
//!
//! Commands are typed values that can be prepared before a board is connected.
//! This example creates an LED pattern that can be sent through any supported
//! board session:
//!
//! ```
//! use chessnut_move::protocol::{
//!     Command, File, LedColor, LedPattern, Rank, Square,
//! };
//!
//! let mut leds = LedPattern::default();
//! leds.set_color(Square::new(File::E, Rank::Four), LedColor::Green);
//! leds.set_color(Square::new(File::E, Rank::Five), LedColor::Green);
//!
//! let show_move = Command::set_leds(&leds);
//! assert_eq!(show_move.bytes().len(), 34);
//! ```
//!
//! [Tokio]: https://docs.rs/tokio/latest/tokio/
//! [btleplug]: https://docs.rs/btleplug/latest/btleplug/
//! [subscriber]: https://docs.rs/tracing/latest/tracing/trait.Subscriber.html
//! [tracing]: https://docs.rs/tracing/latest/tracing/
//! [`tracing-subscriber`]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/
#![cfg_attr(
  feature = "async",
  doc = r#"
# Async session

The default async API owns a connected transport for the complete board
session:

```no_run
use chessnut_move::protocol::{BoardEvent, Command, LedColor, LedPattern};
use chessnut_move::transport::{AsyncTransport, Board, BoardError};

async fn run<T: AsyncTransport>(
    transport: T,
) -> Result<(), BoardError<T::Error>> {
    let mut board = Board::new(transport);
    board.initialize().await?;

    board.send(&Command::set_leds(&LedPattern::all(LedColor::Off))).await?;
    if let BoardEvent::PositionChanged(position) = board.next_event().await? {
        println!("position: {position:?}");
    }

    board.shutdown().await
}
```
"#
)]

#[macro_use]
mod instrumentation;

/// Typed commands, decoded notifications, and chessboard values.
pub mod protocol;

/// Runtime-neutral board sessions and optional Bluetooth adapters.
pub mod transport;
