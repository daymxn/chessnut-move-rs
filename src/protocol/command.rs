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

//! Typed construction of every command currently defined by the Move API.

use crate::protocol::wire::{PackedSquares, encode_piece};
use crate::protocol::{BOARD_STATE_LENGTH, LedPattern, Position};
#[cfg(doc)]
use crate::{protocol, transport};

const AUTO_MOVE_PREFIX: [u8; 2] = [0x42, 0x21];
const SET_LED_PREFIX: [u8; 2] = [0x43, 0x20];
const AUTO_MOVE_COMMAND_LENGTH: usize = 35;
const SET_LED_COMMAND_LENGTH: usize = SET_LED_PREFIX.len() + BOARD_STATE_LENGTH;

/// Maximum number of bytes in a command, defined by the Move's public API surface.
///
/// Transport implementations can use this value when allocating fixed command
/// buffers.
pub const MAX_COMMAND_LEN: usize = const_max(AUTO_MOVE_COMMAND_LENGTH, SET_LED_COMMAND_LENGTH);

/// Returns the larger of two compile-time command lengths.
const fn const_max(left: usize, right: usize) -> usize {
  if left > right { left } else { right }
}

/// An encoded command ready to write to the board's command characteristic.
///
/// Construct commands with methods such as [`Command::set_leds`] and
/// [`Command::read_battery_level`].
///
/// Transport implementations use [`Command::bytes`] and [`Command::write_kind`] to perform
/// the write.
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{
///     Command, File, LedColor, LedPattern, Rank, Square, WriteKind,
/// };
///
/// let mut leds = LedPattern::default();
/// leds.set_color(Square::new(File::E, Rank::Four), LedColor::Green);
///
/// let command = Command::set_leds(&leds);
/// assert_eq!(command.write_kind(), WriteKind::WithoutResponse);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
  bytes: [u8; MAX_COMMAND_LEN],
  len: u8,
  write_kind: WriteKind,
}

/// Selects the GATT write operation required by a [`Command`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WriteKind {
  /// Requests a GATT-level response from the peripheral.
  WithResponse,

  /// Submits the value without requesting a GATT-level response.
  WithoutResponse,
}

/// Selects how an [automatic movement] reacts to user interaction.
///
/// [automatic movement]: Command::auto_move
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoMoveMode {
  /// Continues toward the target position despite user piece movement.
  Force,

  /// Stops automatic movement when the user moves a piece.
  Normal,
}

impl AutoMoveMode {
  /// Returns the byte value used for this mode in an [auto-move command].
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::AutoMoveMode;
  ///
  /// assert_eq!(AutoMoveMode::Force.flag(), 0);
  /// assert_eq!(AutoMoveMode::Normal.flag(), 1);
  /// ```
  ///
  /// [auto-move command]: Command::auto_move
  pub const fn flag(self) -> u8 {
    match self {
      Self::Force => 0x00,
      Self::Normal => 0x01,
    }
  }
}

impl Command {
  // All public constructors pass statically sized arrays through this helper.
  // Its const assertions make adding a command larger than MAX_COMMAND_LEN a
  // compile-time error instead of truncating the command.
  pub(crate) const fn from_bytes<const N: usize>(input: &[u8; N], write_kind: WriteKind) -> Self {
    const {
      assert!(N <= MAX_COMMAND_LEN);
      assert!(MAX_COMMAND_LEN <= u8::MAX as usize);
    }

    let mut bytes = [0; MAX_COMMAND_LEN];
    let mut index = 0;
    while index < N {
      bytes[index] = input[index];
      index += 1;
    }

    Self {
      bytes,
      len: N as u8,
      write_kind,
    }
  }

  /// Creates a command that moves the physical pieces to a target position.
  ///
  /// The target describes all 64 squares, not only the pieces that changed.
  /// [Position notifications] are unavailable while the board is performing an
  /// automatic movement.
  ///
  /// Use [`Command::stop_auto_move`] to cancel movement.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{
  ///     AutoMoveMode, Color, Command, File, Piece, PieceKind, Position, Rank,
  ///     SQUARE_COUNT, Square,
  /// };
  ///
  /// let mut target = Position::new([None; SQUARE_COUNT]);
  /// target.set_piece(
  ///     Square::new(File::E, Rank::Four),
  ///     Some(Piece {
  ///         color: Color::White,
  ///         kind: PieceKind::Pawn,
  ///     }),
  /// );
  ///
  /// let command = Command::auto_move(target, AutoMoveMode::Normal);
  /// assert_eq!(command.bytes().len(), 35);
  /// ```
  ///
  /// [Position notifications]: protocol::BoardEvent::PositionChanged
  pub fn auto_move(position: Position, mode: AutoMoveMode) -> Self {
    trace_event!(
      mode = ?mode,
      occupied_squares = position
        .squares()
        .iter()
        .filter(|piece| piece.is_some())
        .count(),
      "encoding auto-move command"
    );
    let mut command = [0; AUTO_MOVE_COMMAND_LENGTH];
    let bytes = PackedSquares::encode(position.squares(), encode_piece).into_bytes();

    command[..AUTO_MOVE_PREFIX.len()].copy_from_slice(&AUTO_MOVE_PREFIX);
    command[AUTO_MOVE_PREFIX.len()..AUTO_MOVE_PREFIX.len() + BOARD_STATE_LENGTH]
      .copy_from_slice(&bytes);
    command[AUTO_MOVE_COMMAND_LENGTH - 1] = mode.flag();

    Self::from_bytes(&command, WriteKind::WithoutResponse)
  }

  /// Creates a command that stops the current [automatic movement].
  ///
  /// The command is safe to send when no automatic movement is active.
  ///
  /// [automatic movement]: Command::auto_move
  #[cfg_attr(
    feature = "async",
    doc = r#"
# Examples

Send the stop command through an initialized async session:

```no_run
use chessnut_move::protocol::Command;
use chessnut_move::transport::{AsyncBoard, AsyncTransport, BoardError};

async fn stop<T: AsyncTransport>(
    board: &mut AsyncBoard<T>,
) -> Result<(), BoardError<T::Error>> {
    board.send(&Command::stop_auto_move()).await
}
```
"#
  )]
  pub const fn stop_auto_move() -> Self {
    let mut command = [0; AUTO_MOVE_COMMAND_LENGTH];
    command[0] = AUTO_MOVE_PREFIX[0];
    command[1] = AUTO_MOVE_PREFIX[1];

    Self::from_bytes(&command, WriteKind::WithoutResponse)
  }

  /// Creates a command that enables realtime position notifications.
  ///
  /// Low-level transport users must subscribe to
  /// [`NotificationSource::Position`] before writing this command so the
  /// initial position is not missed. Board sessions perform this sequence as
  /// part of their initialization procedure.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{Command, WriteKind};
  ///
  /// let command = Command::enable_realtime_updates();
  /// assert_eq!(command.bytes(), [0x21, 0x01, 0x00]);
  /// assert_eq!(command.write_kind(), WriteKind::WithResponse);
  /// ```
  ///
  /// [`NotificationSource::Position`]: transport::NotificationSource::Position
  #[cfg_attr(
    feature = "async",
    doc = "See [`AsyncBoard::initialize`](transport::AsyncBoard::initialize) for the runtime-neutral async initialization procedure."
  )]
  #[cfg_attr(
    feature = "blocking",
    doc = "See [`BlockingBoard::initialize`](transport::BlockingBoard::initialize) for the blocking initialization procedure."
  )]
  #[cfg_attr(
    feature = "tokio",
    doc = "The [Tokio actor](transport::tokio::spawn) performs initialization in its spawned task."
  )]
  pub const fn enable_realtime_updates() -> Self {
    Self::from_bytes(&[0x21, 0x01, 0x00], WriteKind::WithResponse)
  }

  /// Creates a query for the board's battery status.
  ///
  /// The corresponding command-response notification decodes to
  /// [`BoardEvent::BatteryStatus`].
  ///
  /// [`BoardEvent::BatteryStatus`]: protocol::BoardEvent::BatteryStatus
  #[cfg_attr(
    feature = "async",
    doc = r#"
# Examples

Send the query through an initialized async session and wait for its response:

```no_run
use chessnut_move::protocol::{BatteryStatus, BoardEvent, Command};
use chessnut_move::transport::{AsyncBoard, AsyncTransport, BoardError};

async fn read_battery<T: AsyncTransport>(
    board: &mut AsyncBoard<T>,
) -> Result<BatteryStatus, BoardError<T::Error>> {
    board.send(&Command::read_battery_level()).await?;

    loop {
        if let BoardEvent::BatteryStatus(status) = board.next_event().await? {
            return Ok(status);
        }
    }
}
```
"#
  )]
  #[cfg_attr(
    feature = "tokio",
    doc = "Tokio actor consumers can use [`BoardHandle::battery_status`](transport::tokio::BoardHandle::battery_status) to send and correlate this query."
  )]
  pub const fn read_battery_level() -> Self {
    query_command(0x0c)
  }

  /// Creates a query for the status of all tracked physical pieces.
  ///
  /// The corresponding command-response notification decodes to
  /// [`BoardEvent::PieceStatus`].
  ///
  /// [`BoardEvent::PieceStatus`]: protocol::BoardEvent::PieceStatus
  #[cfg_attr(
    feature = "async",
    doc = r#"
# Examples

Send the query through an initialized async session and wait for its response:

```no_run
use chessnut_move::protocol::{BoardEvent, Command, PieceStatus};
use chessnut_move::transport::{AsyncBoard, AsyncTransport, BoardError};

async fn read_pieces<T: AsyncTransport>(
    board: &mut AsyncBoard<T>,
) -> Result<PieceStatus, BoardError<T::Error>> {
    board.send(&Command::read_piece_status()).await?;

    loop {
        if let BoardEvent::PieceStatus(status) = board.next_event().await? {
            return Ok(status);
        }
    }
}
```
"#
  )]
  #[cfg_attr(
    feature = "tokio",
    doc = "Tokio actor consumers can use [`BoardHandle::piece_status`](transport::tokio::BoardHandle::piece_status) to send and correlate this query."
  )]
  pub const fn read_piece_status() -> Self {
    query_command(0x0b)
  }

  /// Creates a command that replaces the LED color of every square.
  ///
  /// Squares not selected in `pattern` are turned off. Use
  /// [`LedPattern::default`] to turn off all square LEDs.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{
  ///     Command, File, LedColor, LedPattern, Rank, Square,
  /// };
  ///
  /// let mut pattern = LedPattern::default();
  /// pattern.set_color(Square::new(File::E, Rank::Four), LedColor::Blue);
  ///
  /// let command = Command::set_leds(&pattern);
  /// assert_eq!(command.bytes().len(), 34);
  /// ```
  pub fn set_leds(pattern: &LedPattern) -> Self {
    trace_event!("encoding LED command");
    let packed = pattern.encode();

    let mut command = [0; SET_LED_COMMAND_LENGTH];

    command[..SET_LED_PREFIX.len()].copy_from_slice(&SET_LED_PREFIX);
    command[SET_LED_PREFIX.len()..].copy_from_slice(&packed);

    Self::from_bytes(&command, WriteKind::WithoutResponse)
  }

  /// Returns the complete byte sequence to write to the board.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::Command;
  ///
  /// assert_eq!(Command::enable_realtime_updates().bytes(), [0x21, 0x01, 0x00]);
  /// ```
  pub fn bytes(&self) -> &[u8] {
    &self.bytes[..self.len as usize]
  }

  /// Returns the GATT write operation required by this command.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{Command, WriteKind};
  ///
  /// assert_eq!(
  ///     Command::stop_auto_move().write_kind(),
  ///     WriteKind::WithoutResponse,
  /// );
  /// ```
  pub const fn write_kind(&self) -> WriteKind {
    self.write_kind
  }
}

/// Creates a three-byte register query using a GATT write with response.
const fn query_command(register: u8) -> Command {
  Command::from_bytes(&[0x41, 0x01, register], WriteKind::WithResponse)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::protocol::{Color, File, LedColor, Piece, PieceKind, Rank, SQUARE_COUNT, Square};

  #[test]
  fn fixed_commands_can_be_constructed_at_compile_time() {
    const STOP: Command = Command::stop_auto_move();
    const ENABLE_UPDATES: Command = Command::enable_realtime_updates();
    const READ_BATTERY: Command = Command::read_battery_level();
    const READ_PIECES: Command = Command::read_piece_status();

    assert_eq!(&STOP.bytes()[..2], AUTO_MOVE_PREFIX);
    assert_eq!(ENABLE_UPDATES.bytes(), [0x21, 0x01, 0x00]);
    assert_eq!(READ_BATTERY.bytes(), [0x41, 0x01, 0x0c]);
    assert_eq!(READ_PIECES.bytes(), [0x41, 0x01, 0x0b]);
  }

  #[test]
  fn auto_move_command_encodes_target_position_and_mode() {
    let mut position = Position::new([None; SQUARE_COUNT]);
    position.set_piece(
      Square::new(File::H, Rank::Eight),
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::Queen,
      }),
    );
    position.set_piece(
      Square::new(File::G, Rank::Eight),
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::King,
      }),
    );

    let command = Command::auto_move(position, AutoMoveMode::Normal);

    assert_eq!(&command.bytes()[..2], AUTO_MOVE_PREFIX);
    assert_eq!(command.bytes()[2], 0x21);
    assert!(command.bytes()[3..34].iter().all(|byte| *byte == 0));
    assert_eq!(command.bytes()[34], AutoMoveMode::Normal.flag());
    assert_eq!(command.write_kind(), WriteKind::WithoutResponse);
  }

  #[test]
  fn stop_auto_move_command_zeroes_the_target_and_force_flag() {
    let command = Command::stop_auto_move();

    assert_eq!(&command.bytes()[..2], AUTO_MOVE_PREFIX);
    assert!(command.bytes()[2..].iter().all(|byte| *byte == 0));
    assert_eq!(command.write_kind(), WriteKind::WithoutResponse);
  }

  #[test]
  fn battery_level_command_matches_move_api() {
    let command = Command::read_battery_level();

    assert_eq!(command.bytes(), [0x41, 0x01, 0x0c]);
    assert_eq!(command.write_kind(), WriteKind::WithResponse);
  }

  #[test]
  fn piece_status_command_matches_move_api() {
    let command = Command::read_piece_status();

    assert_eq!(command.bytes(), [0x41, 0x01, 0x0b]);
    assert_eq!(command.write_kind(), WriteKind::WithResponse);
  }

  #[test]
  fn led_command_uses_chessnut_square_and_nibble_order() {
    let mut pattern = LedPattern::default();
    pattern.set_color(Square::new(File::H, Rank::Eight), LedColor::Red);
    pattern.set_color(Square::new(File::G, Rank::Eight), LedColor::Green);
    pattern.set_color(Square::new(File::A, Rank::One), LedColor::Blue);

    let command = Command::set_leds(&pattern);

    assert_eq!(command.bytes().len(), SET_LED_COMMAND_LENGTH);
    assert_eq!(&command.bytes()[..2], SET_LED_PREFIX);
    assert_eq!(command.bytes()[2], 0x21);
    assert!(command.bytes()[3..33].iter().all(|byte| *byte == 0));
    assert_eq!(command.bytes()[33], 0x30);
    assert_eq!(command.write_kind(), WriteKind::WithoutResponse);
  }
}
