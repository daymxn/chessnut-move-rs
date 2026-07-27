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

//! Runtime-neutral sessions and notification routing for Chessnut Move boards.
//!
//! A transport implementation maps [`Command`][protocol::Command] values and
//! [`NotificationSource`][transport::NotificationSource] values to a Bluetooth library. The
//! session types own protocol buffers and perform command encoding,
//! notification decoding, [initialization][protocol::Command::enable_realtime_updates],
//! and shutdown.
//!
//! # Examples
//!
//! A custom adapter tags bytes with their originating characteristic before
//! passing them to the shared decoder:
//!
//! ```
//! use chessnut_move::protocol::{BatteryStatus, BoardEvent};
//! use chessnut_move::transport::{
//!     DecodedNotification, Notification, NotificationSource,
//!     decode_notification,
//! };
//!
//! let bytes = [0x41, 0x03, 0x0c, 0x00, 64];
//! let notification =
//!     Notification::new(NotificationSource::CommandResponse, &bytes);
//! let decoded = decode_notification(notification)?;
//!
//! assert_eq!(
//!     decoded,
//!     DecodedNotification::Event(BoardEvent::BatteryStatus(BatteryStatus {
//!         charging: false,
//!         percentage: 64,
//!     })),
//! );
//! # Ok::<(), chessnut_move::transport::DecodeNotificationError>(())
//! ```
#![doc = "# Session APIs"]
#![cfg_attr(
  feature = "async",
  doc = "- [`Board`][transport::Board] is the default runtime-neutral async session."
)]
#![cfg_attr(
  feature = "blocking",
  doc = "- [`BlockingBoard`][transport::BlockingBoard] provides the synchronous session."
)]
#![cfg_attr(
  feature = "tokio",
  doc = "- The [`tokio`][transport::tokio] module provides a cloneable actor handle and event streams."
)]

use thiserror::Error;

#[cfg(doc)]
use crate::protocol;
use crate::protocol::{
  BoardEvent, DecodePositionNotificationError, DecodeResponseNotificationError,
  decode_position_notification, decode_response_notification,
};

#[cfg(feature = "async")]
mod asynchronous;
#[cfg(feature = "blocking")]
mod blocking;
#[cfg(feature = "btleplug")]
pub mod btleplug;
pub mod gatt;
#[cfg(feature = "tokio")]
pub mod tokio;

#[cfg(feature = "async")]
pub use asynchronous::{AsyncBoard, AsyncTransport};
#[cfg(feature = "blocking")]
pub use blocking::{BlockingBoard, BlockingTransport};
#[cfg(feature = "tokio")]
pub use tokio::TokioTransport;

/// Header observed on the board's undocumented realtime-enable acknowledgement.
///
/// The acknowledgement can arrive after command-response subscription even
/// when that subscription follows the enable write, so every session decoder
/// recognizes and consumes it.
const REALTIME_UPDATES_ACKNOWLEDGEMENT: u8 = 0x23;

/// Default runtime-neutral asynchronous board session.
///
/// This is an alias for [`AsyncBoard`].
#[cfg(feature = "async")]
pub type Board<T> = AsyncBoard<T>;

/// Size of the largest notification currently defined by the Move protocol.
///
/// Keeping this bounded lets board sessions receive notifications without
/// requiring `alloc`.
pub const MAX_NOTIFICATION_LEN: usize = 3 + 34 * 4;

/// Identifies which Chessnut Move characteristic produced a notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotificationSource {
  /// Realtime packed position notifications.
  ///
  /// This maps to [`gatt::Characteristic::PositionNotification`].
  Position,

  /// Battery, tracked-piece, and session-control responses.
  ///
  /// This maps to [`gatt::Characteristic::CommandResponse`].
  CommandResponse,
}

impl NotificationSource {
  /// Returns the GATT characteristic associated with this notification source.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::transport::gatt::Characteristic;
  /// use chessnut_move::transport::NotificationSource;
  ///
  /// assert_eq!(
  ///     NotificationSource::Position.characteristic(),
  ///     Characteristic::PositionNotification,
  /// );
  /// ```
  pub const fn characteristic(self) -> gatt::Characteristic {
    match self {
      Self::Position => gatt::Characteristic::PositionNotification,
      Self::CommandResponse => gatt::Characteristic::CommandResponse,
    }
  }
}

/// A borrowed notification returned by a transport implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notification<'a> {
  source: NotificationSource,
  bytes: &'a [u8],
}

impl<'a> Notification<'a> {
  /// Creates a notification tagged with its originating characteristic.
  ///
  /// The returned value borrows `bytes`; transports normally borrow from the
  /// receive buffer supplied to their notification method.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::transport::{Notification, NotificationSource};
  ///
  /// let bytes = [0x23, 0x01, 0x00];
  /// let notification =
  ///     Notification::new(NotificationSource::CommandResponse, &bytes);
  /// assert_eq!(notification.bytes(), &bytes);
  /// ```
  pub const fn new(source: NotificationSource, bytes: &'a [u8]) -> Self {
    Self { source, bytes }
  }

  /// Returns the characteristic category that produced this notification.
  pub const fn source(&self) -> NotificationSource {
    self.source
  }

  /// Returns the complete borrowed notification payload.
  pub const fn bytes(&self) -> &'a [u8] {
    self.bytes
  }
}

/// Reports why a tagged transport notification could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum DecodeNotificationError {
  /// A position notification was malformed.
  #[error(transparent)]
  Position(#[from] DecodePositionNotificationError),

  /// A command-response notification was malformed or unsupported.
  #[error(transparent)]
  CommandResponse(#[from] DecodeResponseNotificationError),
}

/// A protocol notification decoded by a board session.
///
/// Control acknowledgements are represented separately because they are part
/// of session management rather than observable board state.
///
/// The event variant is intentionally stored inline to keep notification
/// decoding available without `alloc`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodedNotification {
  /// A notification that represents observable board data.
  Event(BoardEvent),

  /// The board acknowledged the realtime-update command.
  ///
  /// Sessions consume this acknowledgement after sending
  /// [`Command::enable_realtime_updates`][protocol::Command::enable_realtime_updates].
  RealtimeUpdatesAcknowledged,
}

/// An error produced while operating a board session.
#[derive(Debug, Error)]
pub enum BoardError<E> {
  /// The transport failed while subscribing, writing, receiving, or closing.
  #[error("transport error: {0}")]
  Transport(E),

  /// A received notification could not be decoded.
  #[error(transparent)]
  Decode(#[from] DecodeNotificationError),
}

/// Decodes a tagged transport notification into an event or session-control
/// acknowledgement.
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{BatteryStatus, BoardEvent};
/// use chessnut_move::transport::{
///     DecodedNotification, Notification, NotificationSource,
///     decode_notification,
/// };
///
/// let bytes = [0x41, 0x03, 0x0c, 0x00, 64];
/// let decoded = decode_notification(Notification::new(
///     NotificationSource::CommandResponse,
///     &bytes,
/// ))?;
/// assert_eq!(
///     decoded,
///     DecodedNotification::Event(BoardEvent::BatteryStatus(BatteryStatus {
///         charging: false,
///         percentage: 64,
///     })),
/// );
/// # Ok::<(), chessnut_move::transport::DecodeNotificationError>(())
/// ```
///
/// # Errors
///
/// Returns [`DecodeNotificationError::Position`] when position bytes are
/// malformed, or [`DecodeNotificationError::CommandResponse`] when response
/// bytes are malformed or unsupported.
pub fn decode_notification(
  notification: Notification<'_>,
) -> Result<DecodedNotification, DecodeNotificationError> {
  let _span = trace_span!(
    "decode_notification",
    source = ?notification.source,
    notification_len = notification.bytes.len()
  );
  let decoded = match notification.source {
    NotificationSource::Position => decode_position_notification(notification.bytes)
      .map(BoardEvent::PositionChanged)
      .map(DecodedNotification::Event)
      .map_err(Into::into),
    NotificationSource::CommandResponse
      if notification.bytes.first().copied() == Some(REALTIME_UPDATES_ACKNOWLEDGEMENT) =>
    {
      Ok(DecodedNotification::RealtimeUpdatesAcknowledged)
    }
    NotificationSource::CommandResponse => decode_response_notification(notification.bytes)
      .map(DecodedNotification::Event)
      .map_err(Into::into),
  };

  match &decoded {
    Ok(DecodedNotification::RealtimeUpdatesAcknowledged) => {
      debug_event!("received realtime-update acknowledgement");
    }
    Ok(DecodedNotification::Event(BoardEvent::PositionChanged(_))) => {
      trace_event!(event = "position_changed", "decoded board event");
    }
    Ok(DecodedNotification::Event(BoardEvent::BatteryStatus(_))) => {
      trace_event!(event = "battery_status", "decoded board event");
    }
    Ok(DecodedNotification::Event(BoardEvent::PieceStatus(_))) => {
      trace_event!(event = "piece_status", "decoded board event");
    }
    Err(_error) => {
      trace_event!(error = ?_error, "notification decoding failed");
    }
  }

  decoded
}

#[cfg(any(feature = "async", feature = "blocking"))]
/// Shared storage for runtime-neutral owning sessions.
///
/// The fixed notification buffer keeps polling allocation-free and is reused
/// for every call to the transport.
pub(crate) struct BoardState<T> {
  pub(crate) transport: T,
  pub(crate) notification_buffer: [u8; MAX_NOTIFICATION_LEN],
}

#[cfg(any(feature = "async", feature = "blocking"))]
impl<T> BoardState<T> {
  /// Creates session storage around a transport.
  pub(crate) const fn new(transport: T) -> Self {
    Self {
      transport,
      notification_buffer: [0; MAX_NOTIFICATION_LEN],
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::protocol::BatteryStatus;

  #[test]
  fn decodes_notifications_according_to_their_source() {
    let response = [0x41, 0x03, 0x0c, 0x01, 74];
    let event = decode_notification(Notification::new(
      NotificationSource::CommandResponse,
      &response,
    ));

    assert_eq!(
      event,
      Ok(DecodedNotification::Event(BoardEvent::BatteryStatus(
        BatteryStatus {
          charging: true,
          percentage: 74,
        },
      )))
    );
  }

  #[test]
  fn recognizes_realtime_update_acknowledgement() {
    let notification = Notification::new(NotificationSource::CommandResponse, &[0x23, 0x01, 0x00]);

    assert_eq!(
      decode_notification(notification),
      Ok(DecodedNotification::RealtimeUpdatesAcknowledged)
    );
  }
}
