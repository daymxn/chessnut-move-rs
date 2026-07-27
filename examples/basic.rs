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

use std::error::Error;
use std::io;
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Manager, Peripheral};
use chessnut_move::protocol::BoardEvent;
use chessnut_move::transport::btleplug::BtleplugTransport;
use chessnut_move::transport::gatt::DEVICE_NAME;
use chessnut_move::transport::tokio::{ActorConfig, BoardHandle, EventStreamError, spawn};
use tokio::time::{sleep, timeout};

type AnyError = Box<dyn Error + Send + Sync>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), AnyError> {
  tracing_subscriber::fmt()
    .with_env_filter(
      tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "chessnut_move=debug".into()),
    )
    .init();

  let peripheral = connect_board().await?;
  let operation = run_session(peripheral.clone()).await;
  let disconnect = peripheral.disconnect().await;

  operation?;
  disconnect?;
  Ok(())
}

async fn run_session(peripheral: Peripheral) -> Result<(), AnyError> {
  let transport = BtleplugTransport::new(peripheral).await?;
  let (board, task) = spawn(transport, ActorConfig::default())?;

  // The actor subscribes and enables realtime updates during initialization.
  let operation = use_board(&board).await;
  let shutdown = board.shutdown().await;
  let actor_result = task.await?.into_result();

  actor_result?;
  operation?;
  shutdown?;
  Ok(())
}

async fn use_board(board: &BoardHandle) -> Result<(), AnyError> {
  let mut events = board.subscribe_events().await?;

  let battery = board.battery_status().await?;
  println!("Board battery: {}%", battery.percentage);

  let pieces = board.piece_status().await?;
  let unavailable = pieces
    .pieces
    .iter()
    .filter(|piece| piece.battery_percentage.is_none())
    .count();
  println!(
    "Tracked {} pieces ({unavailable} without a battery reading).",
    pieces.pieces.len()
  );

  println!("Move a piece to produce a position update...");
  let position = timeout(Duration::from_secs(30), async {
    loop {
      if let BoardEvent::PositionChanged(position) = events.recv().await? {
        break Ok::<_, EventStreamError>(position);
      }
    }
  })
  .await??;
  println!("Position update: {position:?}");
  Ok(())
}

async fn connect_board() -> Result<Peripheral, AnyError> {
  let adapter = Manager::new()
    .await?
    .adapters()
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no BLE adapter found"))?;

  adapter.start_scan(ScanFilter::default()).await?;
  sleep(Duration::from_secs(5)).await;
  let peripherals = adapter.peripherals().await?;
  adapter.stop_scan().await?;

  for peripheral in peripherals {
    let is_move = peripheral
      .properties()
      .await?
      .and_then(|properties| properties.local_name)
      .as_deref()
      == Some(DEVICE_NAME);

    if is_move {
      peripheral
        .connect_with_timeout(Duration::from_secs(10))
        .await?;
      return Ok(peripheral);
    }
  }

  Err(io::Error::new(io::ErrorKind::NotFound, "Chessnut Move not found").into())
}
