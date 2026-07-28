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

//! A runtime-neutral async board session without Tokio.
//!
//! BLE APIs and executors vary by platform, so this example exposes a single
//! async entry point that an application can run on smol, async-std, Embassy,
//! a local executor, or another runtime. The connector returns an
//! [`AsyncTransport`], and its [`AsyncTransport::close`] method must disconnect
//! the board.

use chessnut_move::protocol::{BatteryStatus, BoardEvent, Command, PieceStatus, Position};
use chessnut_move::transport::gatt::DEVICE_NAME;
use chessnut_move::transport::{AsyncBoard, AsyncTransport, BoardError};

/// Asynchronously connects a BLE implementation to a Chessnut Move board.
#[allow(async_fn_in_trait)]
pub trait AsyncConnector {
  /// Error reported by the BLE implementation.
  type Error;

  /// Connected transport returned after discovery and connection.
  ///
  /// Its `close` implementation must disconnect the peripheral.
  type Transport: AsyncTransport<Error = Self::Error>;

  /// Scans for and connects to a peripheral with `advertised_name`.
  async fn connect(&mut self, advertised_name: &str) -> Result<Self::Transport, Self::Error>;
}

/// Receives the values reported during the example session.
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
/// `AsyncBoard::initialize` subscribes to notifications and sends the
/// real-time update command. This function then queries the board battery and
/// tracked-piece status before waiting for one position change.
///
/// The transport controls receive timeouts through
/// [`AsyncTransport::next_notification`]. Shutdown is attempted even when the
/// session operation fails.
pub async fn run<C, O>(connector: &mut C, observer: &mut O) -> Result<(), ExampleError<C::Error>>
where
  C: AsyncConnector,
  O: Observer,
{
  let transport = connector
    .connect(DEVICE_NAME)
    .await
    .map_err(ExampleError::Connect)?;
  let mut board = AsyncBoard::new(transport);

  let operation = use_board(&mut board, observer)
    .await
    .map_err(ExampleError::Session);
  let shutdown = board.shutdown().await.map_err(ExampleError::Session);

  operation?;
  shutdown
}

async fn use_board<T, O>(
  board: &mut AsyncBoard<T>,
  observer: &mut O,
) -> Result<(), BoardError<T::Error>>
where
  T: AsyncTransport,
  O: Observer,
{
  board.initialize().await?;

  board.send(&Command::read_battery_level()).await?;
  loop {
    if let BoardEvent::BatteryStatus(status) = board.next_event().await? {
      observer.board_battery(status);
      break;
    }
  }

  board.send(&Command::read_piece_status()).await?;
  loop {
    if let BoardEvent::PieceStatus(status) = board.next_event().await? {
      observer.piece_status(&status);
      break;
    }
  }

  loop {
    if let BoardEvent::PositionChanged(position) = board.next_event().await? {
      observer.position_changed(&position);
      return Ok(());
    }
  }
}
