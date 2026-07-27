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

//! Structured validation errors returned by notification decoders.

use thiserror::Error;

#[cfg(doc)]
use crate::protocol;
use crate::protocol::Square;

/// Reports that a notification does not contain its complete required payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("notification is too short: expected at least {expected} bytes, got {actual}")]
pub struct NotificationTooShortError {
  /// Minimum number of bytes required for this notification type.
  pub expected: usize,

  /// Number of bytes present in the received notification.
  pub actual: usize,
}

/// Reports an invalid field in a [board battery] response.
///
/// [board battery]: protocol::Command::read_battery_level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BatteryStatusError {
  /// The charging flag was neither zero nor one.
  ///
  /// The standard Move board's response for battery status has a third byte which represents the
  /// state of charging; `0` for not charging, and `1` for charging.
  #[error("invalid charging flag {0:#04x}: expected 0 or 1")]
  InvalidChargingFlag(u8),

  /// The reported board battery percentage exceeded 100.
  #[error("invalid battery percentage {0}: expected a value from 0 through 100")]
  InvalidBatteryPercentage(u8),
}

/// Reports an invalid field in a [piece status] response.
///
/// [piece status]: protocol::Command::read_piece_status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PieceStatusError {
  /// A piece record contained an unknown identity code.
  #[error("invalid tracked-piece identity {value:#04x} at index {index}")]
  InvalidTrackedPiece {
    /// Zero-based record index in the tracked-piece response.
    index: usize,

    /// Unrecognized identity byte.
    value: u8,
  },

  /// A battery value was neither a percentage nor the unavailable sentinel.
  #[error(
    "invalid tracked-piece battery percentage {value} at index {index}: expected 0 through 100 or 0xff for unavailable"
  )]
  InvalidTrackedPieceBattery {
    /// Zero-based record index in the tracked-piece response.
    index: usize,

    /// Invalid battery byte.
    value: u8,
  },
}

/// Reports why a command-response notification could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DecodeResponseNotificationError {
  /// The response ended before its required fields were available.
  #[error(transparent)]
  NotificationTooShort(#[from] NotificationTooShortError),

  /// A tracked-piece response contained an invalid field.
  #[error(transparent)]
  PieceStatus(#[from] PieceStatusError),

  /// A board battery response contained an invalid field.
  #[error(transparent)]
  BatteryStatus(#[from] BatteryStatusError),

  /// The response type byte is not supported by this crate.
  #[error("unexpected notification type {0:#04x}")]
  UnexpectedNotification(u8),

  /// A fixed response-header byte did not match the protocol.
  #[error(
    "invalid notification header byte at offset {offset}: expected {expected:#04x}, got {actual:#04x}"
  )]
  InvalidNotificationHeader {
    /// Zero-based byte offset of the invalid header field.
    offset: usize,

    /// Header byte required by the protocol.
    expected: u8,

    /// Header byte found in the notification.
    actual: u8,
  },
}

/// Reports why a [position notification] could not be decoded.
///
/// [position notification]: protocol::BoardEvent::PositionChanged
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DecodePositionNotificationError {
  /// The notification ended before all 64 squares were available.
  #[error(transparent)]
  NotificationTooShort(#[from] NotificationTooShortError),

  /// A square contained an unknown four-bit piece code.
  #[error("invalid piece value {value:#04x} at square {square}")]
  InvalidPiece {
    /// Square containing the invalid value.
    square: Square,

    /// Unrecognized four-bit piece code.
    value: u8,
  },
}
