# chessnut-move-rs

[![GitHub Repo](https://img.shields.io/badge/github-daymxn/chessnut--move--rs-8da0cb?style=for-the-badge&labelColor=555555&logo=github)](https://crates.io/crates/chessnut-move)
[![Crates.io Package](https://img.shields.io/crates/v/chessnut-move.svg?style=for-the-badge&color=fc8d62&logo=rust)](https://crates.io/crates/chessnut-move)
[![Docs](https://img.shields.io/docsrs/chessnut-move?style=for-the-badge&logo=docs.rs&color=66c2a5)](https://docs.rs/chessnut-move)

A typed, transport-independent Rust SDK for
[Chessnut Move](https://www.chessnutech.com/pages/chessnut-move) boards.

`chessnut-move` turns the board's byte protocol into Rust values such as
`Square`, `Position`, `Command`, and `BoardEvent`. Applications can use the
included `btleplug` and Tokio integrations, provide another Bluetooth backend,
or use the protocol in `no_std` environments without Bluetooth support in the
crate at all.

This is an independent community project and is not affiliated with or
endorsed by Chessnut.

> [!NOTE]
> This library is specifically made for the [Chessnut Move](https://www.chessnutech.com/pages/chessnut-move) boards.
>
> I'm willing to adapt the library for other Chessnut boards, but I would need
> someone else with the boards available for testing purposes.

## Highlights

- Typed commands for LEDs, automatic movement, battery status, tracked-piece
  status, and real-time position updates.
- Chess-specific values for squares, pieces, positions, and LED patterns.
- A runtime-neutral async session that is available with the default features.
- An allocation-free blocking session for embedded and synchronous programs.
- An optional Tokio actor with cloneable handles, request helpers, lifecycle
  reporting, and event streams.
- An optional `btleplug` adapter for using common desktop Bluetooth backends.
- Public transport traits and GATT metadata for integrating other Bluetooth
  libraries.
- Optional structured diagnostics through `tracing`.
- Protocol and core transport support without `std` or `alloc`.

## Table of Contents

- [chessnut-move-rs](#chessnut-move-rs)
  - [Highlights](#highlights)
  - [Table of Contents](#table-of-contents)
  - [Installation](#installation)
  - [Quick start](#quick-start)
  - [Architecture](#architecture)
    - [Runtime-neutral async session](#runtime-neutral-async-session)
  - [Common operations](#common-operations)
    - [Light board squares](#light-board-squares)
    - [Request board information](#request-board-information)
    - [Auto move](#auto-move)
  - [Feature flags](#feature-flags)
  - [Custom transports](#custom-transports)
  - [`no_std` and blocking use](#no_std-and-blocking-use)
  - [Tracing](#tracing)
  - [Examples](#examples)
  - [Additional notes](#additional-notes)
  - [Support](#support)
  - [Contributing](#contributing)
  - [License](#license)

## Installation

The most complete application setup uses `btleplug` for Bluetooth I/O, Tokio
for concurrent access, and `tracing` for diagnostics:

```sh
cargo add chessnut-move --features btleplug,tokio,tracing
cargo add btleplug
cargo add tokio --features macros,rt
cargo add tracing-subscriber --features env-filter
```

Or add it directly to `Cargo.toml`:

```toml
[dependencies]
btleplug = "0.12"
chessnut-move = {
  version = "1",
  features = ["btleplug", "tokio", "tracing"],
}
tokio = { version = "1", features = ["macros", "rt"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

The default features are `std` and `async`. They provide the runtime-neutral
async API but intentionally do not select a Bluetooth implementation or async
runtime:

```toml
[dependencies]
chessnut-move = "1"
```

## Quick start

The Tokio actor is the convenient path when multiple parts of an application
need to send commands or observe the board.

Given a connected `btleplug` peripheral:

```rust
use std::error::Error;

use btleplug::platform::Peripheral;
use chessnut_move::protocol::BoardEvent;
use chessnut_move::transport::btleplug::BtleplugTransport;
use chessnut_move::transport::tokio::{
  ActorConfig, BoardHandle, spawn,
};

type AnyError = Box<dyn Error + Send + Sync>;

async fn use_board(board: &BoardHandle) -> Result<(), AnyError> {
  let mut events = board.subscribe_events().await?;

  let battery = board.battery_status().await?;
  println!("Board battery: {}%", battery.percentage);

  let pieces = board.piece_status().await?;
  println!("Tracking {} physical pieces.", pieces.pieces.len());

  loop {
    let event = events.recv().await?;
    if let BoardEvent::PositionChanged(position) = event {
      println!("Position changed: {position:?}");
      break;
    }
  }

  Ok(())
}

async fn run_connected_board(peripheral: Peripheral) -> Result<(), AnyError> {
  let transport = BtleplugTransport::new(peripheral).await?;
  let (board, task) = spawn(transport, ActorConfig::default())?;

  let operation = use_board(&board).await;
  let shutdown = board.shutdown().await;
  let actor_result = task.await?.into_result();

  operation?;
  shutdown?;
  actor_result?;
  Ok(())
}
```

Discovery, connection timeouts, reconnection, and peripheral selection remain
application policy.

See [`examples/basic.rs`](./examples/basic.rs) for a complete program that
scans for the board, connects, queries its status, waits for a position
update, shuts down the actor, and disconnects.

API documentation for all public types is available on
[docs.rs](https://docs.rs/chessnut-move).

## Architecture

The crate separates the Chessnut Move protocol from Bluetooth and runtime
choices:

| Layer                 | Purpose                                                                                     |
|-----------------------|---------------------------------------------------------------------------------------------|
| `protocol`            | Commands, decoded events, positions, squares, pieces, LED patterns, and protocol errors     |
| `transport`           | Notification decoding, runtime-neutral sessions, and traits for Bluetooth adapters          |
| `transport::btleplug` | Optional adapter from a connected `btleplug` peripheral to the crate's transport traits     |
| `transport::tokio`    | Optional actor task, cloneable handles, request helpers, lifecycle state, and event streams |

This separation allows protocol tests and embedded integrations to avoid
specific Bluetooth dependencies. It also keeps scan duration, device selection,
pairing, reconnection, and application shutdown behavior in the application
where those policies can be chosen deliberately.

### Runtime-neutral async session

The default `Board` alias owns any `AsyncTransport` implementation and performs
the board-session initialization sequence:

```rust
use chessnut_move::protocol::{BoardEvent, Command};
use chessnut_move::transport::{AsyncTransport, Board, BoardError};

async fn run<T: AsyncTransport>(
  transport: T,
) -> Result<(), BoardError<T::Error>> {
  let mut board = Board::new(transport);
  board.initialize().await?;
  board.send(&Command::read_battery_level()).await?;

  loop {
    if let BoardEvent::BatteryStatus(status) = board.next_event().await? {
      println!("Battery: {}%", status.percentage);
      break;
    }
  }

  board.shutdown().await
}
```

Use this API when one task owns the board or when the executor does not require
`Send` futures. Use the Tokio actor when concurrent callers need cloneable
handles and independently subscribed event streams.

## Common operations

Commands are transport-independent values. They can be created before a board
is connected and sent through async, blocking, or Tokio-backed sessions.

### Light board squares

```rust
use chessnut_move::protocol::{
  Command, LedColor, LedPattern, Square,
};

let mut leds = LedPattern::default();
leds.set_color("e2".parse::<Square>()?, LedColor::Green);
leds.set_color("e4".parse::<Square>()?, LedColor::Green);

let command = Command::set_leds(&leds);
```

### Request board information

The Tokio actor correlates status responses with these request helpers:

```rust
use chessnut_move::transport::tokio::{BoardHandle, HandleError};

let battery = board.battery_status().await?;
let pieces = board.piece_status().await?;

println!("Battery: {}%", battery.percentage);
for tracked in pieces.pieces {
  println!("{:?}", tracked);
}
```

Runtime-neutral sessions can send `Command::read_battery_level()` or
`Command::read_piece_status()` and consume the corresponding `BoardEvent`.

### Auto move

`Command::auto_move` accepts the complete target `Position`, not only a source
and destination square:

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

In a real application, derive the target from the board's latest reported
position. Always send `Command::stop_auto_move()` during cancellation or
shutdown.

## Feature flags

| Feature    | Default | Enables                                                                                                                      |
|------------|:-------:|------------------------------------------------------------------------------------------------------------------------------|
| `std`      |   Yes   | Standard-library error integration                                                                                           |
| `async`    |   Yes   | `AsyncTransport`, `AsyncBoard`, and the `Board` alias                                                                        |
| `blocking` |   No    | Allocation-free `BlockingTransport` and `BlockingBoard`                                                                      |
| `btleplug` |   No    | `BtleplugTransport`; also enables `std` and `async`                                                                          |
| `tokio`    |   No    | Actor task, cloneable handles, request helpers, lifecycle state, and event streams; also enables `std`, `alloc`, and `async` |
| `tracing`  |   No    | Structured spans and events; also enables `alloc`                                                                            |
| `alloc`    |   No    | Allocation-dependent integrations without otherwise requiring `std`                                                          |

## Custom transports

Bluetooth libraries integrate through one of three public traits:

- Implement `AsyncTransport` for runtime-neutral async programs.
- Implement `BlockingTransport` for synchronous or embedded programs.
- Implement `TokioTransport` when the transport futures are `Send` and will run
  inside the Tokio actor.

Each trait exposes the same essential operations:

1. Subscribe to a `NotificationSource`.
2. Write a typed `Command`.
3. Copy the next notification into the supplied buffer and return a borrowed
   `Notification`.

`unsubscribe` and `close` hooks have no-op defaults for transports that do not
need explicit cleanup. The board sessions own fixed-size notification buffers,
so a transport does not need to allocate or expose its Bluetooth library's
channel and notification-session types.

The UUIDs and characteristic mapping are public in `transport::gatt`.
Implementations use `Command::bytes()` and `Command::write_kind()` and do not
need access to the actual (private) wire-format types.

## `no_std` and blocking use

Disable the default features to use protocol types and notification decoding
without `std`, allocation, async, or a Bluetooth dependency:

```toml
[dependencies]
chessnut-move = {
  version = "1",
  default-features = false,
}
```

Add the allocation-free blocking session when required:

```toml
[dependencies]
chessnut-move = {
  version = "1",
  default-features = false,
  features = ["blocking"],
}
```

Then implement `BlockingTransport` and use `BlockingBoard`. Enabling `tracing`
in a `no_std` application also enables `alloc`; the crate still does not
install or select a tracing subscriber.

## Tracing

Enable the `tracing` feature to receive structured diagnostics for protocol
decoding, transport I/O, lifecycle transitions, actor queries, timeouts, and
recoverable failures.

The library never installs a global subscriber. Applications decide how events
are formatted, filtered, and collected:

```rust
tracing_subscriber::fmt()
  .with_env_filter("chessnut_move=debug")
  .init();
```

Add `tracing-subscriber` to the application when using this setup:

```toml
[dependencies]
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Use `chessnut_move=trace` to include routine command and notification traffic.
Raw command and notification payloads are not recorded.

## Examples

You can find example implementations under the [/examples](./examples/) directory.

## Additional notes

- Wake the board before scanning and keep it near the Bluetooth adapter.
- Disconnect the official Chessnut application and other nearby clients before
  connecting; the board may only accept one active connection.
- Bluetooth discovery and connection behavior varies by operating system and
  adapter. The `btleplug` adapter inherits the platform behavior of
  `btleplug`.
- Ensure cancellation and shutdown paths send `Command::stop_auto_move()`.

The protocol implementation is based on Chessnut's [published Move API](https://github.com/chessnutech/chess_move_api), and some manual testing.

## Support

Use the repository's
[issue tracker](https://github.com/daymxn/chessnut-move-rs/issues) for
reproducible bugs and feature request.

## Contributing

Contributions are welcome. Read
[`CONTRIBUTING.md`](https://github.com/daymxn/chessnut-move-rs/blob/main/CONTRIBUTING.md)
for the development commands, formatting requirements, licensing process, and
pull request conventions. Contributors using AI-assisted tools must also
follow
[`AI_POLICY.md`](https://github.com/daymxn/chessnut-move-rs/blob/main/AI_POLICY.md).

The usual local checks are:

```sh
cargo make check_format
cargo make build
cargo make test
```

## License

Licensed under the Apache License, Version 2.0. See
[`LICENSE`](https://github.com/daymxn/chessnut-move-rs/blob/main/LICENSE) for
details.
