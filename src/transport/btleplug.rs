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

//! Optional [`btleplug`](https://docs.rs/btleplug/0.12/btleplug/) adapter for
//! connected Chessnut Move peripherals.
//!
//! This module does not scan, select adapters, connect, or reconnect. After an
//! application connects a peripheral, [`BtleplugTransport::new`] discovers and
//! validates the characteristics needed by the runtime-neutral sessions or the
//! Tokio actor.
//!
//! Use the
//! [`btleplug::api::Manager`](https://docs.rs/btleplug/0.12/btleplug/api/trait.Manager.html)
//! and
//! [`btleplug::api::Central`](https://docs.rs/btleplug/0.12/btleplug/api/trait.Central.html)
//! traits to select an adapter and scan. Use
//! [`btleplug::api::Peripheral::connect`](https://docs.rs/btleplug/0.12/btleplug/api/trait.Peripheral.html#tymethod.connect)
//! before constructing this adapter.
//!
//! # Examples
//!
//! Find the advertised board, connect it, and create a transport:
//!
//! ```no_run
//! use std::error::Error;
//! use std::io;
//! use btleplug::api::{
//!     Central, Manager as _, Peripheral as _, ScanFilter,
//! };
//! use btleplug::platform::{Manager, Peripheral};
//! use chessnut_move::transport::btleplug::BtleplugTransport;
//! use chessnut_move::transport::gatt::DEVICE_NAME;
//!
//! async fn connect() -> Result<BtleplugTransport<Peripheral>, Box<dyn Error>> {
//!     let manager = Manager::new().await?;
//!     let adapter = manager
//!         .adapters()
//!         .await?
//!         .into_iter()
//!         .next()
//!         .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no BLE adapter"))?;
//!
//!     adapter.start_scan(ScanFilter::default()).await?;
//!     let mut board = None;
//!     for peripheral in adapter.peripherals().await? {
//!         let is_move = peripheral
//!             .properties()
//!             .await?
//!             .and_then(|properties| properties.local_name)
//!             .is_some_and(|name| name == DEVICE_NAME);
//!         if is_move {
//!             board = Some(peripheral);
//!             break;
//!         }
//!     }
//!
//!     let board = board
//!         .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "board not found"))?;
//!     board.connect().await?;
//!     Ok(BtleplugTransport::new(board).await?)
//! }
//! ```

use core::pin::Pin;

use ::btleplug::api::{
  Characteristic as BtleplugCharacteristic, Peripheral, ValueNotification, WriteType,
};
use futures_util::{Stream, StreamExt};
use thiserror::Error;
use uuid::Uuid;

use crate::protocol::{Command, WriteKind};
#[cfg(doc)]
use crate::transport;
#[cfg(feature = "tokio")]
use crate::transport::TokioTransport;
use crate::transport::gatt::Characteristic;
use crate::transport::{AsyncTransport, Notification, NotificationSource};

type NotificationStream = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

/// Errors from the optional `btleplug` transport adapter.
#[derive(Debug, Error)]
pub enum Error {
  /// The underlying `btleplug` backend returned an error.
  #[error(transparent)]
  Backend(#[from] ::btleplug::Error),

  /// Service discovery did not expose a required Move characteristic.
  #[error("required Chessnut Move characteristic is missing: {0:?}")]
  MissingCharacteristic(Characteristic),

  /// The peripheral-wide notification stream ended.
  #[error("the btleplug notification stream ended")]
  NotificationStreamEnded,

  /// A received notification cannot fit in the session buffer.
  #[error("notification has {actual} bytes but the provided buffer holds {capacity}")]
  NotificationTooLong {
    /// Number of bytes in the received notification.
    actual: usize,

    /// Number of bytes available in the supplied buffer.
    capacity: usize,
  },

  /// A notification came from a characteristic not used by this protocol.
  #[error("received a notification from unexpected characteristic {0}")]
  UnexpectedNotificationCharacteristic(Uuid),
}

/// An [`AsyncTransport`] adapter around a connected `btleplug` peripheral.
///
/// Device discovery and connection remain with the application. Construction
/// discovers the peripheral's services, validates the three required
/// characteristics, and opens its notification stream.
///
/// The adapter implements [`AsyncTransport`] and, with the `tokio` feature,
/// `TokioTransport`.
#[cfg_attr(
  feature = "tokio",
  doc = "See [`TokioTransport`](transport::tokio::TokioTransport) for the actor-compatible transport contract."
)]
pub struct BtleplugTransport<P> {
  peripheral: P,
  command_write: BtleplugCharacteristic,
  position_notification: BtleplugCharacteristic,
  command_response: BtleplugCharacteristic,
  notifications: NotificationStream,
}

impl<P: Peripheral> BtleplugTransport<P> {
  /// Creates an adapter around an already connected peripheral.
  ///
  /// Call
  /// [`Peripheral::connect`](https://docs.rs/btleplug/0.12/btleplug/api/trait.Peripheral.html#tymethod.connect)
  /// before this method. This method performs service discovery but does not
  /// establish the Bluetooth connection.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use btleplug::api::Peripheral;
  /// use chessnut_move::transport::btleplug::{BtleplugTransport, Error};
  ///
  /// # async fn create<P: Peripheral>(
  /// #     peripheral: P,
  /// # ) -> Result<BtleplugTransport<P>, Error> {
  /// BtleplugTransport::new(peripheral).await
  /// # }
  /// ```
  ///
  /// # Errors
  ///
  /// Returns [`Error::Backend`] when service discovery or notification-stream
  /// creation fails. Returns [`Error::MissingCharacteristic`] when the
  /// connected peripheral does not expose a required Move characteristic.
  pub async fn new(peripheral: P) -> Result<Self, Error> {
    debug_event!("discovering Chessnut Move GATT services");
    peripheral.discover_services().await?;

    let characteristics = peripheral.characteristics();
    trace_event!(
      characteristic_count = characteristics.len(),
      "discovered peripheral characteristics"
    );
    let find = |characteristic: Characteristic| {
      let uuid = Uuid::from_u128(characteristic.uuid_u128());
      characteristics
        .iter()
        .find(|candidate| candidate.uuid == uuid)
        .cloned()
        .ok_or_else(|| {
          warn_event!(
            characteristic = ?characteristic,
            "required Chessnut Move characteristic is missing"
          );
          Error::MissingCharacteristic(characteristic)
        })
    };

    let command_write = find(Characteristic::CommandWrite)?;
    let position_notification = find(Characteristic::PositionNotification)?;
    let command_response = find(Characteristic::CommandResponse)?;
    let notifications = peripheral.notifications().await?;
    debug_event!("Chessnut Move GATT transport is ready");

    Ok(Self {
      peripheral,
      command_write,
      position_notification,
      command_response,
      notifications,
    })
  }

  /// Returns a shared reference to the wrapped peripheral.
  pub const fn peripheral(&self) -> &P {
    &self.peripheral
  }

  /// Returns a mutable reference to the wrapped peripheral.
  ///
  /// Direct operations can change connection or subscription state expected by
  /// the adapter.
  pub fn peripheral_mut(&mut self) -> &mut P {
    &mut self.peripheral
  }

  /// Consumes the adapter and returns the wrapped peripheral.
  ///
  /// This method does not unsubscribe or disconnect the peripheral.
  pub fn into_peripheral(self) -> P {
    self.peripheral
  }

  /// Maps a protocol notification source to its discovered btleplug value.
  fn notification_characteristic(&self, source: NotificationSource) -> &BtleplugCharacteristic {
    match source {
      NotificationSource::Position => &self.position_notification,
      NotificationSource::CommandResponse => &self.command_response,
    }
  }

  /// Enables a discovered notification characteristic.
  async fn subscribe_source(&mut self, source: NotificationSource) -> Result<(), Error> {
    trace_event!(source = ?source, "subscribing to BLE notifications");
    self
      .peripheral
      .subscribe(self.notification_characteristic(source))
      .await?;
    Ok(())
  }

  /// Disables a discovered notification characteristic.
  async fn unsubscribe_source(&mut self, source: NotificationSource) -> Result<(), Error> {
    trace_event!(source = ?source, "unsubscribing from BLE notifications");
    self
      .peripheral
      .unsubscribe(self.notification_characteristic(source))
      .await?;
    Ok(())
  }

  /// Maps the protocol write kind and submits a command to the write characteristic.
  async fn write(&mut self, command: &Command) -> Result<(), Error> {
    trace_event!(
      command_len = command.bytes().len(),
      write_kind = ?command.write_kind(),
      "writing command to BLE characteristic"
    );
    let write_type = match command.write_kind() {
      WriteKind::WithResponse => WriteType::WithResponse,
      WriteKind::WithoutResponse => WriteType::WithoutResponse,
    };

    self
      .peripheral
      .write(&self.command_write, command.bytes(), write_type)
      .await?;
    Ok(())
  }

  /// Copies and tags one value from the peripheral-wide notification stream.
  async fn receive<'a>(&'a mut self, buffer: &'a mut [u8]) -> Result<Notification<'a>, Error> {
    // btleplug exposes one stream for the peripheral. Tagging each value by
    // UUID keeps the protocol decoder independent of btleplug types.
    let notification = self
      .notifications
      .next()
      .await
      .ok_or(Error::NotificationStreamEnded)?;

    let source =
      if notification.uuid == Uuid::from_u128(Characteristic::PositionNotification.uuid_u128()) {
        NotificationSource::Position
      } else if notification.uuid == Uuid::from_u128(Characteristic::CommandResponse.uuid_u128()) {
        NotificationSource::CommandResponse
      } else {
        warn_event!(
          characteristic_uuid = %notification.uuid,
          "received notification from an unexpected characteristic"
        );
        return Err(Error::UnexpectedNotificationCharacteristic(
          notification.uuid,
        ));
      };

    let actual = notification.value.len();
    if actual > buffer.len() {
      warn_event!(
        notification_len = actual,
        buffer_capacity = buffer.len(),
        "notification exceeds receive buffer"
      );
      return Err(Error::NotificationTooLong {
        actual,
        capacity: buffer.len(),
      });
    }

    buffer[..actual].copy_from_slice(&notification.value);
    trace_event!(
      source = ?source,
      notification_len = actual,
      "received BLE notification"
    );
    Ok(Notification::new(source, &buffer[..actual]))
  }
}

impl<P: Peripheral> AsyncTransport for BtleplugTransport<P> {
  type Error = Error;

  async fn subscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
    self.subscribe_source(source).await
  }

  async fn unsubscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
    self.unsubscribe_source(source).await
  }

  async fn write_command(&mut self, command: &Command) -> Result<(), Self::Error> {
    self.write(command).await
  }

  async fn next_notification<'a>(
    &'a mut self,
    buffer: &'a mut [u8],
  ) -> Result<Notification<'a>, Self::Error> {
    self.receive(buffer).await
  }
}

#[cfg(feature = "tokio")]
impl<P: Peripheral + 'static> TokioTransport for BtleplugTransport<P> {
  type Error = Error;

  async fn subscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
    self.subscribe_source(source).await
  }

  async fn unsubscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
    self.unsubscribe_source(source).await
  }

  async fn write_command(&mut self, command: &Command) -> Result<(), Self::Error> {
    self.write(command).await
  }

  async fn next_notification<'a>(
    &'a mut self,
    buffer: &'a mut [u8],
  ) -> Result<Notification<'a>, Self::Error> {
    self.receive(buffer).await
  }
}
