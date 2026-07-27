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

//! Decoded board and tracked-piece status values.

use crate::protocol::{Color, Piece, PieceKind, Position};
#[cfg(doc)]
use crate::{protocol, transport};

/// A decoded board-state or status notification.
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{BoardEvent, File, Rank, Square};
///
/// fn report(event: BoardEvent) {
///     match event {
///         BoardEvent::PositionChanged(position) => {
///             let e4 = Square::new(File::E, Rank::Four);
///             println!("e4 contains {:?}", position.piece_at(e4));
///         }
///         BoardEvent::BatteryStatus(status) => {
///             println!("board battery: {}%", status.percentage);
///         }
///         BoardEvent::PieceStatus(status) => {
///             let unavailable = status
///                 .pieces
///                 .iter()
///                 .filter(|piece| piece.battery_percentage.is_none())
///                 .count();
///             println!("{unavailable} piece batteries are unavailable");
///         }
///     }
/// }
/// # let _ = report;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoardEvent {
  /// The occupancy or piece arrangement of the 64 squares changed.
  ///
  /// This event is only fired when [realtime updates] is enabled, and there's not an active
  /// [auto-move] occurring.
  ///
  /// [realtime updates]: protocol::Command::enable_realtime_updates
  /// [auto-move]: protocol::Command::auto_move
  PositionChanged(Position),

  /// The board responded to a [battery-status query].
  ///
  /// [battery-status query]: protocol::Command::read_battery_level
  BatteryStatus(BatteryStatus),

  /// The board responded to a [piece-status query].
  ///
  /// [piece-status query]: protocol::Command::read_piece_status
  PieceStatus(PieceStatus),
}

/// Charging state and battery percentage reported by the board.
///
/// This value is carried by [`BoardEvent::BatteryStatus`] after a
/// [`Command::read_battery_level`][protocol::Command::read_battery_level]
/// response is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BatteryStatus {
  /// Whether the board is actively plugged in and charging.
  pub charging: bool,

  /// The battery percentage of the board.
  ///
  /// The decoder guarantees a value from `0` through `100`.
  pub percentage: u8,
}

/// Status reported for one tracked physical chess piece.
///
/// Coordinates use the board firmware's normalized `0..=255` coordinate
/// system and are independent of [`Square`] indexes.
///
/// [`Default`] produces an unavailable white-pawn placeholder used to
/// initialize fixed response arrays. A default value is not an observation
/// received from a board.
///
/// [`Square`]: protocol::Square
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackedPieceStatus {
  /// The chess piece that this status report represents.
  pub piece: Piece,

  /// The normalized X coordinate of the piece on the board.
  ///
  /// Values range from `0` through `255`.
  pub x: u8,

  /// The normalized Y coordinate of the piece on the board.
  ///
  /// Values range from `0` through `255`.
  pub y: u8,

  /// The piece's reported battery percentage.
  ///
  /// This is `None` when the board reports `0xff`, which indicates that the
  /// piece's battery level is currently unavailable.
  pub battery_percentage: Option<u8>,
}

impl Default for TrackedPieceStatus {
  fn default() -> Self {
    Self {
      piece: Piece {
        color: Color::White,
        kind: PieceKind::Pawn,
      },
      x: 0,
      y: 0,
      battery_percentage: None,
    }
  }
}

/// Number of physical piece records in a tracked-piece response.
///
/// The Move protocol reports eight pawns, two rooks, two knights, two bishops,
/// two queens, and one king for each color.
pub const TRACKED_PIECE_COUNT: usize = 34;

/// Status records for every physical piece tracked by the board.
///
/// Records are ordered as white pawns, rooks, knights, bishops, queens, king,
/// followed by the corresponding black pieces.
///
/// This value is carried by [`BoardEvent::PieceStatus`] after
/// [`Command::read_piece_status`][protocol::Command::read_piece_status] is sent.
/// Low-level transport implementations can obtain it through
/// [`decode_response_notification`][protocol::decode_response_notification].
#[cfg_attr(
  feature = "tokio",
  doc = r#"
# Examples

Query the running board actor with
[`BoardHandle::piece_status`](transport::tokio::BoardHandle::piece_status),
then inspect the records returned by the board:

```no_run
use chessnut_move::transport::tokio::{BoardHandle, HandleError};

async fn report_piece_batteries(board: &BoardHandle) -> Result<(), HandleError> {
    let status = board.piece_status().await?;

    for tracked in &status.pieces {
        match tracked.battery_percentage {
            Some(percentage) => {
                println!("{:?}: {percentage}%", tracked.piece);
            }
            None => println!("{:?}: unavailable", tracked.piece),
        }
    }

    Ok(())
}
```
"#
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PieceStatus {
  /// Piece records in the order defined by the Move protocol.
  pub pieces: [TrackedPieceStatus; TRACKED_PIECE_COUNT],
}
