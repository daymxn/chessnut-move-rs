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

//! A `no_std`, allocation-free blocking board session.
//!
//! BLE APIs vary between embedded platforms, so this example accepts a
//! platform-provided connector. The returned transport must implement
//! [`BlockingTransport`] and its [`BlockingTransport::close`] method must
//! disconnect the board.

#![no_std]

use chessnut_move::protocol::{BatteryStatus, BoardEvent, Command, PieceStatus, Position};
use chessnut_move::transport::gatt::DEVICE_NAME;
use chessnut_move::transport::{BlockingBoard, BlockingTransport, BoardError};

/// Connects an embedded BLE implementation to a Chessnut Move board.
pub trait BlockingConnector {
  /// Error reported by the BLE implementation.
  type Error;

  /// Connected transport returned after discovery and connection.
  ///
  /// Its `close` implementation must disconnect the peripheral.
  type Transport: BlockingTransport<Error = Self::Error>;

  /// Scans for and connects to a peripheral with `advertised_name`.
  fn connect(&mut self, advertised_name: &str) -> Result<Self::Transport, Self::Error>;
}

/// Receives the values reported during the example session.
///
/// A firmware application can implement this with fixed-capacity queues,
/// serial output, a display, or direct state updates without allocating.
pub trait Observer {
  /// Handles the board battery response.
  fn board_battery(&mut self, status: BatteryStatus);

  /// Handles the tracked-piece response.
  fn piece_status(&mut self, status: &PieceStatus);

  /// Handles the next real-time position update.
  fn position_changed(&mut self, position: &Position);
}

/// Error returned while connecting or interacting with the board.
#[derive(Debug)]
pub enum ExampleError<E> {
  /// Discovery or connection failed before the session was created.
  Connect(E),

  /// Session initialization, command I/O, notification decoding, or shutdown
  /// failed.
  Session(BoardError<E>),
}

/// Connects, queries board status, observes one move, and disconnects.
///
/// `BlockingBoard::initialize` subscribes to notifications and sends the
/// real-time update command. This function then queries the board battery and
/// tracked-piece status before waiting for one position change.
///
/// The transport controls receive timeouts through
/// [`BlockingTransport::next_notification`]. Shutdown is attempted even when
/// the session operation fails.
pub fn run<C, O>(connector: &mut C, observer: &mut O) -> Result<(), ExampleError<C::Error>>
where
  C: BlockingConnector,
  O: Observer,
{
  let transport = connector
    .connect(DEVICE_NAME)
    .map_err(ExampleError::Connect)?;
  let mut board = BlockingBoard::new(transport);

  let operation = use_board(&mut board, observer).map_err(ExampleError::Session);
  let shutdown = board.shutdown().map_err(ExampleError::Session);

  operation?;
  shutdown
}

fn use_board<T, O>(
  board: &mut BlockingBoard<T>,
  observer: &mut O,
) -> Result<(), BoardError<T::Error>>
where
  T: BlockingTransport,
  O: Observer,
{
  board.initialize()?;

  board.send(&Command::read_battery_level())?;
  loop {
    if let BoardEvent::BatteryStatus(status) = board.next_event()? {
      observer.board_battery(status);
      break;
    }
  }

  board.send(&Command::read_piece_status())?;
  loop {
    if let BoardEvent::PieceStatus(status) = board.next_event()? {
      observer.piece_status(&status);
      break;
    }
  }

  loop {
    if let BoardEvent::PositionChanged(position) = board.next_event()? {
      observer.position_changed(&position);
      return Ok(());
    }
  }
}
