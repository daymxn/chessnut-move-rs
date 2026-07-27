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

//! Device name and [GATT] UUIDs used by the Chessnut Move protocol.
//!
//! Custom Bluetooth adapters can match discovered characteristics with these
//! constants or with the typed [`Characteristic`] values.
//!
//! [GATT]: https://www.bluetooth.com/bluetooth-resources/intro-to-bluetooth-gap-gatt/

/// Default Bluetooth advertising name of a Chessnut Move board.
pub const DEVICE_NAME: &str = "Chessnut Move";

/// UUID of the service containing realtime position notifications.
pub const POSITION_SERVICE_UUID: &str = "1b7e8261-2877-41c3-b46e-cf057c562023";

/// UUID of the characteristic that emits realtime packed positions.
pub const POSITION_NOTIFICATION_UUID: &str = "1b7e8262-2877-41c3-b46e-cf057c562023";

/// UUID of the service containing commands and command responses.
pub const COMMAND_SERVICE_UUID: &str = "1b7e8271-2877-41c3-b46e-cf057c562023";

/// UUID of the characteristic to which commands are written.
pub const COMMAND_WRITE_UUID: &str = "1b7e8272-2877-41c3-b46e-cf057c562023";

/// UUID of the characteristic that emits command and control responses.
pub const COMMAND_RESPONSE_UUID: &str = "1b7e8273-2877-41c3-b46e-cf057c562023";

/// A GATT characteristic used by the Chessnut Move protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Characteristic {
  /// Characteristic that emits realtime packed positions.
  PositionNotification,

  /// Characteristic to which encoded commands are written.
  CommandWrite,

  /// Characteristic that emits query and session-control responses.
  CommandResponse,
}

impl Characteristic {
  /// Returns the canonical UUID string for this characteristic.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::transport::gatt::{
  ///     Characteristic, COMMAND_WRITE_UUID,
  /// };
  ///
  /// assert_eq!(Characteristic::CommandWrite.uuid(), COMMAND_WRITE_UUID);
  /// ```
  pub const fn uuid(self) -> &'static str {
    match self {
      Self::PositionNotification => POSITION_NOTIFICATION_UUID,
      Self::CommandWrite => COMMAND_WRITE_UUID,
      Self::CommandResponse => COMMAND_RESPONSE_UUID,
    }
  }

  /// Returns the UUID as a `u128`, useful for BLE libraries with typed UUIDs.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::transport::gatt::Characteristic;
  ///
  /// assert_eq!(
  ///     Characteristic::CommandResponse.uuid_u128(),
  ///     0x1b7e8273_2877_41c3_b46e_cf057c562023_u128,
  /// );
  /// ```
  pub const fn uuid_u128(self) -> u128 {
    match self {
      Self::PositionNotification => 0x1b7e8262_2877_41c3_b46e_cf057c562023,
      Self::CommandWrite => 0x1b7e8272_2877_41c3_b46e_cf057c562023,
      Self::CommandResponse => 0x1b7e8273_2877_41c3_b46e_cf057c562023,
    }
  }
}
