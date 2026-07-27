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

//! Typed values and codecs for the Chessnut Move byte protocol.
//!
//! Applications normally construct a [`Command`][protocol::Command] and
//! consume [`BoardEvent`][protocol::BoardEvent] values through a session in the
//! [`transport`] module. Transport implementers can use
//! [`decode_position_notification`][protocol::decode_position_notification] and
//! [`decode_response_notification`][protocol::decode_response_notification]
//! when they need direct access to the protocol codecs.
//!
//! Board positions use canonical A1-to-H8 indexing through
//! [`Square`][protocol::Square], while encoding and decoding preserve the Move
//! board's reversed wire order.
//!
//! # Examples
//!
//! Consumers can handle decoded events without depending on a particular
//! Bluetooth library:
//!
//! ```
//! use chessnut_move::protocol::{
//!     BoardEvent, File, Rank, Square,
//! };
//!
//! fn occupied(event: BoardEvent, square: Square) -> Option<bool> {
//!     match event {
//!         BoardEvent::PositionChanged(position) => {
//!             Some(position.piece_at(square).is_some())
//!         }
//!         BoardEvent::BatteryStatus(_) | BoardEvent::PieceStatus(_) => None,
//!     }
//! }
//!
//! let e4 = Square::new(File::E, Rank::Four);
//! # let _ = (occupied, e4);
//! ```

use crate::protocol::wire::{PackedSquares, decode_piece};
#[cfg(doc)]
use crate::transport;

mod command;
mod errors;
mod event;
mod led;
mod position;
mod square;
mod wire;

pub use command::*;
pub use errors::*;
pub use event::*;
pub use led::*;
pub use position::*;
pub use square::*;

const BOARD_STATE_OFFSET: usize = 2;
const RESPONSE_HEADER: u8 = 0x41;
const RESPONSE_HEADER_LENGTH: usize = 3;
const BATTERY_RESPONSE_LENGTH: usize = 5;
const BATTERY_RESPONSE_PAYLOAD_LENGTH: u8 = 0x03;
const BATTERY_RESPONSE_TYPE: u8 = 0x0c;
const PIECE_STATUS_RESPONSE_PAYLOAD_LENGTH: u8 = 0x89;
const PIECE_STATUS_RESPONSE_TYPE: u8 = 0x0b;
const TRACKED_PIECE_BYTES: usize = 4;
const PIECE_STATUS_RESPONSE_LENGTH: usize =
  RESPONSE_HEADER_LENGTH + TRACKED_PIECE_COUNT * TRACKED_PIECE_BYTES;

/// Decodes a [position notification] emitted by the [position characteristic].
///
/// Bytes two through thirty-three contain the packed state of all 64 squares.
/// Any trailing bytes are transport metadata and are ignored.
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{
///     Color, File, Piece, PieceKind, Rank, Square,
///     decode_position_notification,
/// };
///
/// let mut notification = [0_u8; 34];
/// notification[2] = 0x21; // h8 = black queen, g8 = black king
/// let position = decode_position_notification(&notification)?;
///
/// assert_eq!(
///     position.piece_at(Square::new(File::H, Rank::Eight)),
///     Some(Piece {
///         color: Color::Black,
///         kind: PieceKind::Queen,
///     }),
/// );
/// # Ok::<(), chessnut_move::protocol::DecodePositionNotificationError>(())
/// ```
///
/// # Errors
///
/// Returns [`DecodePositionNotificationError::NotificationTooShort`] when
/// fewer than 34 bytes are supplied.
///
/// Returns
/// [`DecodePositionNotificationError::InvalidPiece`] when a square contains an
/// unknown piece code.
///
/// [position notification]: BoardEvent::PositionChanged
/// [position characteristic]: transport::gatt::Characteristic::PositionNotification
pub fn decode_position_notification(
  bytes: &[u8],
) -> Result<Position, DecodePositionNotificationError> {
  let _span = trace_span!(
    "decode_position_notification",
    notification_len = bytes.len()
  );
  let expected_minimum = BOARD_STATE_OFFSET + BOARD_STATE_LENGTH;
  let payload = bytes
    .get(BOARD_STATE_OFFSET..)
    .and_then(|payload| payload.first_chunk::<BOARD_STATE_LENGTH>())
    .ok_or(NotificationTooShortError {
      expected: expected_minimum,
      actual: bytes.len(),
    })?;

  let packed = PackedSquares::from_bytes(*payload);
  let squares = packed.decode(decode_piece)?;
  trace_event!("decoded position notification");

  Ok(Position::new(squares))
}

/// Decodes a battery or tracked-piece response notification.
///
/// The notification type byte determines whether the returned event is
/// [`BoardEvent::BatteryStatus`] or [`BoardEvent::PieceStatus`].
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{
///     BatteryStatus, BoardEvent, decode_response_notification,
/// };
///
/// let event = decode_response_notification(&[0x41, 0x03, 0x0c, 0x01, 72])?;
/// assert_eq!(
///     event,
///     BoardEvent::BatteryStatus(BatteryStatus {
///         charging: true,
///         percentage: 72,
///     }),
/// );
/// # Ok::<(), chessnut_move::protocol::DecodeResponseNotificationError>(())
/// ```
///
/// # Errors
///
/// Returns [`DecodeResponseNotificationError::NotificationTooShort`] for a
/// truncated frame, [`DecodeResponseNotificationError::InvalidNotificationHeader`]
/// for invalid header bytes, or
/// [`DecodeResponseNotificationError::UnexpectedNotification`] for an
/// unsupported response type.
///
/// Battery and piece payload validation errors are
/// returned through [`DecodeResponseNotificationError::BatteryStatus`] and
/// [`DecodeResponseNotificationError::PieceStatus`].
pub fn decode_response_notification(
  bytes: &[u8],
) -> Result<BoardEvent, DecodeResponseNotificationError> {
  let _span = trace_span!(
    "decode_response_notification",
    notification_len = bytes.len()
  );
  require_length(bytes, RESPONSE_HEADER_LENGTH)?;
  require_header_byte(bytes, 0, RESPONSE_HEADER)?;

  let event = match bytes[2] {
    BATTERY_RESPONSE_TYPE => decode_battery_status(bytes).map(BoardEvent::BatteryStatus),
    PIECE_STATUS_RESPONSE_TYPE => decode_piece_status(bytes).map(BoardEvent::PieceStatus),
    response_type => Err(DecodeResponseNotificationError::UnexpectedNotification(
      response_type,
    )),
  }?;

  match event {
    BoardEvent::BatteryStatus(_status) => trace_event!(
      response = "battery_status",
      charging = _status.charging,
      percentage = _status.percentage,
      "decoded command response"
    ),
    BoardEvent::PieceStatus(_) => trace_event!(
      response = "piece_status",
      piece_count = TRACKED_PIECE_COUNT,
      "decoded command response"
    ),
    BoardEvent::PositionChanged(_) => {}
  }

  Ok(event)
}

fn decode_battery_status(bytes: &[u8]) -> Result<BatteryStatus, DecodeResponseNotificationError> {
  require_length(bytes, BATTERY_RESPONSE_LENGTH)?;
  require_header_byte(bytes, 1, BATTERY_RESPONSE_PAYLOAD_LENGTH)?;

  let charging = match bytes[3] {
    0 => false,
    1 => true,
    value => return Err(BatteryStatusError::InvalidChargingFlag(value).into()),
  };
  let percentage = validate_battery_percentage(bytes[4])?;

  Ok(BatteryStatus {
    charging,
    percentage,
  })
}

fn decode_piece_status(bytes: &[u8]) -> Result<PieceStatus, DecodeResponseNotificationError> {
  require_length(bytes, PIECE_STATUS_RESPONSE_LENGTH)?;
  require_header_byte(bytes, 1, PIECE_STATUS_RESPONSE_PAYLOAD_LENGTH)?;

  let mut pieces = [TrackedPieceStatus::default(); TRACKED_PIECE_COUNT];

  for (index, status) in pieces.iter_mut().enumerate() {
    let offset = RESPONSE_HEADER_LENGTH + index * TRACKED_PIECE_BYTES;
    let identity = bytes[offset];
    let battery_percentage = bytes[offset + 3];

    *status = TrackedPieceStatus {
      piece: decode_tracked_piece(index, identity)?,
      x: bytes[offset + 1],
      y: bytes[offset + 2],
      battery_percentage: validate_tracked_piece_battery(index, battery_percentage)?,
    };
  }

  Ok(PieceStatus { pieces })
}

// The response carries an identity byte for every physical tracked piece.
// Index order is stable, but the identity is still validated so firmware
// changes or corrupt frames cannot silently relabel a piece.
fn decode_tracked_piece(index: usize, value: u8) -> Result<Piece, PieceStatusError> {
  use Color::{Black, White};
  use PieceKind::{Bishop, King, Knight, Pawn, Queen, Rook};

  let piece = match value {
    0x01 => Piece {
      color: White,
      kind: Pawn,
    },
    0x02 => Piece {
      color: White,
      kind: Rook,
    },
    0x03 => Piece {
      color: White,
      kind: Knight,
    },
    0x04 => Piece {
      color: White,
      kind: Bishop,
    },
    0x05 => Piece {
      color: White,
      kind: Queen,
    },
    0x06 => Piece {
      color: White,
      kind: King,
    },
    0x07 => Piece {
      color: Black,
      kind: Pawn,
    },
    0x08 => Piece {
      color: Black,
      kind: Rook,
    },
    0x09 => Piece {
      color: Black,
      kind: Knight,
    },
    0x0a => Piece {
      color: Black,
      kind: Bishop,
    },
    0x0b => Piece {
      color: Black,
      kind: Queen,
    },
    0x0c => Piece {
      color: Black,
      kind: King,
    },
    value => {
      return Err(PieceStatusError::InvalidTrackedPiece { index, value });
    }
  };

  Ok(piece)
}

fn validate_battery_percentage(value: u8) -> Result<u8, BatteryStatusError> {
  if value <= 100 {
    Ok(value)
  } else {
    Err(BatteryStatusError::InvalidBatteryPercentage(value))
  }
}

fn validate_tracked_piece_battery(index: usize, value: u8) -> Result<Option<u8>, PieceStatusError> {
  // Hardware reports 0xff when a physical piece's battery telemetry is
  // unavailable. Values between 101 and 254 are not defined by the protocol.
  match value {
    0..=100 => Ok(Some(value)),
    0xff => Ok(None),
    _ => Err(PieceStatusError::InvalidTrackedPieceBattery { index, value }),
  }
}

fn require_length(bytes: &[u8], expected: usize) -> Result<(), NotificationTooShortError> {
  if bytes.len() < expected {
    Err(NotificationTooShortError {
      expected,
      actual: bytes.len(),
    })
  } else {
    Ok(())
  }
}

fn require_header_byte(
  bytes: &[u8],
  offset: usize,
  expected: u8,
) -> Result<(), DecodeResponseNotificationError> {
  let actual = bytes[offset];
  if actual == expected {
    Ok(())
  } else {
    Err(DecodeResponseNotificationError::InvalidNotificationHeader {
      offset,
      expected,
      actual,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const TRACKED_PIECE_IDENTITIES: [u8; TRACKED_PIECE_COUNT] = [
    1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8, 9, 9, 10, 10,
    11, 11, 12,
  ];

  #[test]
  fn decodes_battery_status_response() {
    let event = decode_response_notification(&[0x41, 0x03, 0x0c, 0x01, 87]);

    assert_eq!(
      event,
      Ok(BoardEvent::BatteryStatus(BatteryStatus {
        charging: true,
        percentage: 87,
      }))
    );
  }

  #[test]
  fn rejects_invalid_battery_status_fields() {
    assert_eq!(
      decode_response_notification(&[0x41, 0x03, 0x0c, 0x02, 87]),
      Err(BatteryStatusError::InvalidChargingFlag(0x02).into())
    );
    assert_eq!(
      decode_response_notification(&[0x41, 0x03, 0x0c, 0x00, 101]),
      Err(BatteryStatusError::InvalidBatteryPercentage(101).into())
    );
  }

  #[test]
  fn decodes_all_tracked_piece_statuses() {
    let response = piece_status_response();
    let BoardEvent::PieceStatus(status) = decode_response_notification(&response).unwrap() else {
      panic!("expected a piece-status event");
    };

    assert_eq!(
      status.pieces[0],
      TrackedPieceStatus {
        piece: Piece {
          color: Color::White,
          kind: PieceKind::Pawn,
        },
        x: 0,
        y: 255,
        battery_percentage: Some(50),
      }
    );
    assert_eq!(
      status.pieces[16].piece,
      Piece {
        color: Color::White,
        kind: PieceKind::King,
      }
    );
    assert_eq!(
      status.pieces[17].piece,
      Piece {
        color: Color::Black,
        kind: PieceKind::Pawn,
      }
    );
    assert_eq!(
      status.pieces[33].piece,
      Piece {
        color: Color::Black,
        kind: PieceKind::King,
      }
    );
    assert_eq!(status.pieces[33].x, 33);
    assert_eq!(status.pieces[33].y, 222);
    assert_eq!(status.pieces[33].battery_percentage, Some(83));
  }

  #[test]
  fn decodes_unavailable_tracked_piece_battery() {
    let mut response = piece_status_response();
    response[RESPONSE_HEADER_LENGTH + 3] = 0xff;

    let BoardEvent::PieceStatus(status) = decode_response_notification(&response).unwrap() else {
      panic!("expected a piece-status event");
    };

    assert_eq!(status.pieces[0].battery_percentage, None);
  }

  #[test]
  fn rejects_invalid_tracked_piece_fields() {
    let mut invalid_identity = piece_status_response();
    invalid_identity[RESPONSE_HEADER_LENGTH + 5 * TRACKED_PIECE_BYTES] = 0xff;
    assert_eq!(
      decode_response_notification(&invalid_identity),
      Err(
        PieceStatusError::InvalidTrackedPiece {
          index: 5,
          value: 0xff,
        }
        .into()
      )
    );

    let mut invalid_battery = piece_status_response();
    invalid_battery[RESPONSE_HEADER_LENGTH + 7 * TRACKED_PIECE_BYTES + 3] = 101;
    assert_eq!(
      decode_response_notification(&invalid_battery),
      Err(
        PieceStatusError::InvalidTrackedPieceBattery {
          index: 7,
          value: 101,
        }
        .into()
      )
    );
  }

  #[test]
  fn validates_response_length_and_header() {
    assert_eq!(
      decode_response_notification(&[0x41, 0x03]),
      Err(
        NotificationTooShortError {
          expected: RESPONSE_HEADER_LENGTH,
          actual: 2,
        }
        .into()
      )
    );
    assert_eq!(
      decode_response_notification(&[0x40, 0x03, 0x0c, 0, 50]),
      Err(DecodeResponseNotificationError::InvalidNotificationHeader {
        offset: 0,
        expected: 0x41,
        actual: 0x40,
      })
    );
    assert_eq!(
      decode_response_notification(&[0x41, 0x04, 0x0c, 0, 50]),
      Err(DecodeResponseNotificationError::InvalidNotificationHeader {
        offset: 1,
        expected: 0x03,
        actual: 0x04,
      })
    );
    assert_eq!(
      decode_response_notification(&[0x41, 0x01, 0xff]),
      Err(DecodeResponseNotificationError::UnexpectedNotification(
        0xff
      ))
    );
  }

  #[test]
  fn rejects_short_piece_status_response() {
    let response = piece_status_response();

    assert_eq!(
      decode_response_notification(&response[..response.len() - 1]),
      Err(
        NotificationTooShortError {
          expected: PIECE_STATUS_RESPONSE_LENGTH,
          actual: PIECE_STATUS_RESPONSE_LENGTH - 1,
        }
        .into()
      )
    );
  }

  fn piece_status_response() -> [u8; PIECE_STATUS_RESPONSE_LENGTH] {
    let mut response = [0; PIECE_STATUS_RESPONSE_LENGTH];
    response[..RESPONSE_HEADER_LENGTH].copy_from_slice(&[
      RESPONSE_HEADER,
      PIECE_STATUS_RESPONSE_PAYLOAD_LENGTH,
      PIECE_STATUS_RESPONSE_TYPE,
    ]);

    for (index, identity) in TRACKED_PIECE_IDENTITIES.iter().copied().enumerate() {
      let offset = RESPONSE_HEADER_LENGTH + index * TRACKED_PIECE_BYTES;
      response[offset] = identity;
      response[offset + 1] = index as u8;
      response[offset + 2] = u8::MAX - index as u8;
      response[offset + 3] = 50 + index as u8;
    }

    response
  }
}
