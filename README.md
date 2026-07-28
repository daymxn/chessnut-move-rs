<h1 align="center">
chessnut-move-rs
</h1>

> A typed, transport-independent Rust SDK for Chessnut Move boards.

[![GitHub Repo](https://img.shields.io/badge/github-daymxn/chessnut--move--rs-8da0cb?style=for-the-badge&labelColor=555555&logo=github)](https://github.com/daymxn/chessnut-move-rs)
[![Crates.io Package](https://img.shields.io/badge/crates.io-chessnut--move-fc8d62?style=for-the-badge&logo=rust)](https://crates.io/crates/chessnut-move)
[![Docs](https://img.shields.io/docsrs/chessnut-move?style=for-the-badge&logo=docs.rs&color=66c2a5)](https://docs.rs/chessnut-move)


---

<br>

## Demo

```rust
use std::error::Error;
use std::io;
use std::time::Duration;

use btleplug::platform::Peripheral;
use chessnut_move::protocol::BoardEvent;
use chessnut_move::transport::btleplug::BtleplugTransport;
use chessnut_move::transport::tokio::{ActorConfig, BoardHandle, EventStreamError, spawn};
use tokio::time::{sleep, timeout};

type AnyError = Box<dyn Error + Send + Sync>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), AnyError> {
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

  let piece_status = board.piece_status().await?;
  for piece in piece_status.pieces.iter() {
    match piece.battery_percentage {
      Some(battery_percentage) => {
        println!("{:?} ({:?}%)", piece.piece, battery_percentage);
      }
      None => {
        println!("{:?} (unavailable)", piece.piece);
      }
    }
  }

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
```

## Installation

The SDK is published on crates.io under `chessnut-move`.

```sh
cargo add chessnut-move
```

## Overview

chessnut-move is an SDK for interfacing with [Chessnut Move](https://www.chessnutech.com/pages/chessnut-move) boards,
allowing you to interact with your boards via a type-safe Rust interface.

I was looking for a way to interact with my board via Rust, but none of the existing libraries offered support for the
Move boards (only the Evo + other chessnut offerings).

This SDK also offers [`no_std`](#no-std-usage) support, and is [runtime agnostic](#transports); so you can use whatever bluetooth
library you'd like.

> [!Note]
> This is an independent community project and is not affiliated with or
> endorsed by Chessnut.

## Usage

### Architecture

The crate separates the Chessnut Move protocol from Bluetooth and runtime
choices. This results in the crate being seperated into two layers:

| Layer       | Purpose                                                                                 |
|-------------|-----------------------------------------------------------------------------------------|
| `protocol`  | Commands, decoded events, positions, squares, pieces, LED patterns, and protocol errors |
| `transport` | Notification decoding, runtime-neutral sessions, and traits for Bluetooth adapters      |

The `protocol` layer is just a raw interface of the wire protcol for the Chessnut Move board. It makes no assumptions
about what you'll be using to talk with the board; it just provides the necessary types to do so.

The `transport` layer offers the actual transport mechanisim for interacting with the Chessnut Move board. It uses the
protocol layer under the hood, and facilitates the Bluetooth connections.

This separation allows protocol tests and embedded integrations to avoid
specific Bluetooth dependencies, and keeps things extensible. It also keeps scan duration, device selection, pairing and
reconnection in your application; where those policies can be chosen deliberately.

### Transports

Transports are delibrated defined in a way that you can provide your own (eg; for your own embedded hardware or a custom
Bluetooth library).

But the crate _does_ provide a few common transports out of the box.

#### Tokio

> [!TIP]
> You can find a more comprehensive example of this in the [basic.rs](./examples/basic.rs) example.

If you're using the popular async library [tokio](https://docs.rs/tokio/latest/tokio/), you can take advantage of the [`tokio` feature flag](https://docs.rs/chessnut-move/latest/chessnut_move/transport/tokio/index.html) and the transports
it provides.

```rust
use std::error::Error;
use std::io;
use std::time::Duration;

use chessnut_move::protocol::BoardEvent;
use chessnut_move::transport::tokio::{BoardHandle, EventStreamError};
use tokio::time::timeout;

type AnyError = Box<dyn Error + Send + Sync>;

async fn use_board(board: &BoardHandle) -> Result<(), AnyError> {
  let mut events = board.subscribe_events().await?;

  let battery = board.battery_status().await?;
  println!("Board battery: {}%", battery.percentage);

  let piece_status = board.piece_status().await?;
  for piece in piece_status.pieces.iter() {
    match piece.battery_percentage {
      Some(battery_percentage) => {
        println!("{:?} ({:?}%)", piece.piece, battery_percentage);
      }
      None => {
        println!("{:?} (unavailable)", piece.piece);
      }
    }
  }

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
```

#### Async

> [!TIP]
> You can find a more comprehensive example of this in the [async_without_tokio.rs](./examples/async_without_tokio.rs) example.

If you're using native `async`, but don't want to use tokio, you can use the `async` feature flag to access a
runtime-netural async transport.

```rust
use chessnut_move::protocol::{BoardEvent, Command, LedPattern};
use chessnut_move::transport::{AsyncBoard, AsyncTransport, BoardError};

async fn run<T: AsyncTransport>(
  transport: T,
) -> Result<(), BoardError<T::Error>> {
  let mut board = AsyncBoard::new(transport);
  board.initialize().await?;

  board.send(&Command::set_leds(&LedPattern::default())).await?;
  match board.next_event().await? {
    BoardEvent::PositionChanged(position) => {
      println!("position: {position:?}");
    }
    BoardEvent::BatteryStatus(_) | BoardEvent::PieceStatus(_) => {}
  }

  board.shutdown().await
}
```

#### btleplug

> [!TIP]
> You can find a more comprehensive example of this in the [basic.rs](./examples/basic.rs) example.

If you're using the popular Bluetooth library [blteplug](https://docs.rs/btleplug/latest/btleplug/), we offer additional
adapters for using it as a transport via the [`blteplug` feature flag](https://docs.rs/chessnut-move/latest/chessnut_move/transport/btleplug/index.html).

```rust
use std::error::Error;
use std::io;
use btleplug::api::{
  Central, Manager as _, Peripheral as _, ScanFilter,
};
use btleplug::platform::{Manager, Peripheral};
use chessnut_move::transport::btleplug::BtleplugTransport;
use chessnut_move::transport::gatt::DEVICE_NAME;

async fn connect() -> Result<BtleplugTransport<Peripheral>, Box<dyn Error>> {
  let manager = Manager::new().await?;
  let adapter = manager
    .adapters()
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no BLE adapter"))?;  

  adapter.start_scan(ScanFilter::default()).await?;
  let mut board = None;
  for peripheral in adapter.peripherals().await? {
    let is_move = peripheral
      .properties()
      .await?
      .and_then(|properties| properties.local_name)
      .is_some_and(|name| name == DEVICE_NAME);
    if is_move {
      board = Some(peripheral);
      break;
    }
  }

  let board = board
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "board not found"))?;

  board.connect().await?;
  Ok(BtleplugTransport::new(board).await?)
}
```

#### Blocking

> [!TIP]
> You can find a more comprehensive example of this in the [blocking_no_std.rs](./examples/blocking_no_std.rs) example.

For `no_std` environvments, you can use the `blocking` feature flag to access the Allocation-free [`BlockingTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/trait.BlockingTransport.html) and [`BlockingBoard`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/struct.BlockingBoard.html).

```rust
use chessnut_move::protocol::{BoardEvent, Command, LedPattern};
use chessnut_move::transport::{
  BlockingBoard, BlockingTransport, BoardError,
};

fn run<T: BlockingTransport>(
  transport: T,
) -> Result<(), BoardError<T::Error>> {
  let mut board = BlockingBoard::new(transport);
  board.initialize()?;

  board.send(&Command::set_leds(&LedPattern::default()))?;
  match board.next_event()? {
    BoardEvent::PositionChanged(position) => {
      println!("position: {position:?}");
    }
    BoardEvent::BatteryStatus(_) | BoardEvent::PieceStatus(_) => {}
  }

  board.shutdown()
}
```

#### Custom Bluetooth Transports

If you want to add support for a custom Bluetooth library, you'll need to intregrate through one of three public traits:

- Implement [`AsyncTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/trait.AsyncTransport.html)
  for runtime-neutral async programs.
- Implement [`BlockingTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/trait.BlockingTransport.html) for
  synchronous or embedded programs.
- Implement [`TokioTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/tokio/trait.TokioTransport.html) when
  the transport futures are `Send` and will run
  inside the Tokio actor.

Each trait exposes the same essential operations:

1. Subscribe to a [`NotificationSource`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/enum.NotificationSource.html).
2. Write a typed [`Command`](https://docs.rs/chessnut-move/latest/chessnut_move/protocol/struct.Command.html).
3. Copy the next notification into the supplied buffer and return a borrowed
   [`Notification`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/struct.Notification.html).

Additionally, it's worth noting that [`unsubscribe`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/tokio/trait.TokioTransport.html#method.unsubscribe)
and [`close`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/tokio/trait.TokioTransport.html#method.close)
hooks have no-op defaults for transports that do not
need explicit cleanup. The board sessions also own fixed-size notification buffers,
so a transport does not need to allocate or expose its Bluetooth library's
channel and notification-session types.

The UUIDs and characteristic mapping are public in [`transport::gatt`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/gatt/index.html).
Implementations use [`Command::bytes()`](https://docs.rs/chessnut-move/latest/chessnut_move/protocol/struct.Command.html#method.bytes) and [`Command::write_kind()`](https://docs.rs/chessnut-move/latest/chessnut_move/protocol/struct.Command.html#method.write_kind)
and do not
need access to the actual (private) wire-format types.

To learn more, you can look at how the [`AsyncTransport` is implemented](./src/transport/asynchronous.rs).

### Commands

Commands are transport-independent values; they can be created before a board
is connected and sent through async, blocking, or Tokio-backed sessions.

These represent the actual "commands" you'll be sending to the Chessnut Move board.

#### Square lights

```rust
use chessnut_move::protocol::{
  Command, LedColor, LedPattern, Square,
};

let mut leds = LedPattern::default ();
leds.set_color("e2".parse::<Square>() ?, LedColor::Green);
leds.set_color("e4".parse::<Square>() ?, LedColor::Green);

let command = Command::set_leds( & leds);

board.send(command).await?;
```

#### Board information

```rust
let battery = board.battery_status().await?;
let pieces = board.piece_status().await?;

println!("Battery: {}%", battery.percentage);
for tracked in pieces.pieces {
println!("{:?}", tracked);
}
```

#### Auto Move

> [!NOTE]
> The Chessnut Move baord requires the **full** board FEN for auto-moves.
>
> This means you have to provide the full [`Position`](https://docs.rs/chessnut-move/latest/chessnut_move/protocol/struct.Position.html) when executing auto-moves.
>
> In a real application, derive the target from the board's latest reported
> position.

```rust
use chessnut_move::protocol::{
  AutoMoveMode, Command, Position, Square,
};

fn move_e2_to_e4(mut target: Position) -> Result<Command, chessnut_move::protocol::ParseSquareError> {
  let e2 = "e2".parse::<Square>()?;
  let e4 = "e4".parse::<Square>()?;

  let pawn = target.piece_at(e2);
  target.set_piece(e2, None);
  target.set_piece(e4, pawn);

  Ok(Command::auto_move(target, AutoMoveMode::Normal))
}

let stop = Command::stop_auto_move();
```

### Tracing

For [tracing](https://docs.rs/tracing/latest/tracing/) support, we provide structured diagnostics for protocol decoding, transport I/O, lifecycle transitions, actor queries, timeouts, and
recoverable failures via the `tracing` feature.

The SDK never installs a global subscriber; applications decide how events
are formatted, filtered, and collected.

```rust
tracing_subscriber::fmt()
  .with_env_filter("chessnut_move=debug")
  .init();
```

Use `chessnut_move=trace` to include routine command and notification traffic.

> [!NOTE]
> Raw command and notification payloads are not recorded.

### `no-std` usage

Disable the default features to use protocol types and notification decoding
without `std`, allocation, async, or a Bluetooth dependency:

```toml
[dependencies]
chessnut-move = {
  version = "*",
  default-features = false,
}
```

If you don't want to implement your own transport, you can use the allocation-free blocking session:

```toml
[dependencies]
chessnut-move = {
  version = "*",
  default-features = false,
  features = ["blocking"],
}
```

Then implement [`BlockingTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/trait.BlockingTransport.html) and use [`BlockingBoard`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/struct.BlockingBoard.html).

Note that enabling [`tracing`](#tracing) in a `no_std` application also enables `alloc`.

## Feature flags

| Feature    | Default | Enables                                                                                                                      |
|------------|:-------:|------------------------------------------------------------------------------------------------------------------------------|
| `std`      |   Yes   | Standard-library error integration                                                                                           |
| `async`    |   Yes   | [`AsyncTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/trait.AsyncTransport.html), [`AsyncBoard`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/struct.AsyncBoard.html), and the [`Board`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/type.Board.html) alias                                                                        |
| `blocking` |   No    | Allocation-free [`BlockingTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/trait.BlockingTransport.html) and [`BlockingBoard`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/struct.BlockingBoard.html)                                                                      |
| `btleplug` |   No    | [`BtleplugTransport`](https://docs.rs/chessnut-move/latest/chessnut_move/transport/btleplug/struct.BtleplugTransport.html); also enables `std` and `async`                                                                          |
| `tokio`    |   No    | Actor task, cloneable handles, request helpers, lifecycle state, and event streams; also enables `std`, `alloc`, and `async` |
| `tracing`  |   No    | Structured spans and events; also enables `alloc`                                                                            |
| `alloc`    |   No    | Allocation-dependent integrations without otherwise requiring `std`                                                          |

## Examples

You can find example implementations under the
[`examples`](./examples/) directory:

| Example | Configuration | Purpose |
|---------|---------------|---------|
| [`basic.rs`](./examples/basic.rs) | `btleplug,tokio,tracing` | Complete desktop BLE application with scanning, status queries, events, tracing, and graceful shutdown |
| [`blocking_no_std.rs`](./examples/blocking_no_std.rs) | `--no-default-features --features blocking` | Allocation-free firmware integration using a platform-provided blocking BLE transport |
| [`async_without_tokio.rs`](./examples/async_without_tokio.rs) | `--no-default-features --features async` | Runtime-neutral async integration for a non-Tokio executor and BLE transport |

The blocking and runtime-neutral async examples are compiled as libraries
because the crate cannot select an embedded platform entry point, BLE stack, or
executor for the application. Each exposes a `run` function that accepts a
platform connector and demonstrates connection, initialization, battery and
tracked-piece queries, position updates, shutdown, and disconnection.

## Additional Notes

- The Chessnut Move boards only allow a single bluetooth connection; so ensure you're not connecting with the native app when using the SDK.
- Bluetooth discovery and connection behavior varies by operating system and
  adapter. The `btleplug` adapter inherits the platform behavior of
  `btleplug`. Add support for your [own bluetooth adapter](#custom-bluetooth-transports) if `blteplug` doesn't fit your requirements.
- The protocol implementation is based on Chessnut's [published Move API](https://github.com/chessnutech/chess_move_api), and some manual testing.
- This library is specifically made for the [Chessnut Move](https://www.chessnutech.com/pages/chessnut-move) boards. I'm willing to adapt the library for other Chessnut boards, but I would need someone else with the boards available for testing purposes.

## Support

Use the GitHub's [issue tracker](https://github.com/daymxn/chessnut-move-rs/issues) for
reproducible bugs and feature request.

Issue templates are provided for both.

## Contributing

If you're interested in contributing to the SDK, give the [CONTRIBUTING](CONTRIBUTING.md) doc a read.

Contributors using AI-assisted tools must also follow our
[AI policy](https://github.com/daymxn/chessnut-move-rs/blob/main/AI_POLICY.md).

## License

[Apache 2.0](/LICENSE)
