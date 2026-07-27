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

use crate::protocol::{SQUARE_COUNT, Square};

pub(crate) const PACKED_SQUARE_BYTES: usize = SQUARE_COUNT / 2;
const _: () = assert!(PACKED_SQUARE_BYTES * 2 == SQUARE_COUNT);

/// Two four-bit square values packed into each byte in Chessnut wire order.
///
/// This is how the Move packs its individual squares for messages regarding board positions,
/// like realtime updates, piece status, and auto-moves.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PackedSquares([u8; PACKED_SQUARE_BYTES]);

impl PackedSquares {
  /// Packs canonical A1-to-H8 values into reversed Move wire order.
  ///
  /// The first encoded value occupies the low nibble and the second occupies
  /// the high nibble.
  ///
  /// # Panics
  ///
  /// Panics when a byte doesn't fit within a nibble (greater than `0x0f`).
  pub(crate) fn encode<T: Copy>(
    squares: &[T; SQUARE_COUNT],
    mut encode: impl FnMut(T) -> u8,
  ) -> Self {
    let bytes = core::array::from_fn(|byte_index| {
      let first_wire_index = byte_index * 2;
      let second_wire_index = first_wire_index + 1;
      let first = encode(squares[position_index(first_wire_index)]);
      let second = encode(squares[position_index(second_wire_index)]);

      assert!(first <= 0x0f, "encoded square values must fit in a nibble");
      assert!(second <= 0x0f, "encoded square values must fit in a nibble");

      first | (second << 4)
    });

    Self(bytes)
  }

  /// Decodes packed values into canonical A1-to-H8 order.
  ///
  /// The square passed to `decode` is the canonical square associated with the
  /// four-bit value.
  ///
  /// Early exists on any error from a callback.
  pub(crate) fn decode<T, E>(
    &self,
    mut decode: impl FnMut(Square, u8) -> Result<T, E>,
  ) -> Result<[T; SQUARE_COUNT], E>
  where
    T: Copy + Default,
  {
    let mut squares = [T::default(); SQUARE_COUNT];

    for (byte_index, byte) in self.0.into_iter().enumerate() {
      let first_wire_index = byte_index * 2;
      let second_wire_index = first_wire_index + 1;
      let first_square = square_at_wire_index(first_wire_index);
      let second_square = square_at_wire_index(second_wire_index);

      squares[first_square.index()] = decode(first_square, byte & 0x0f)?;
      squares[second_square.index()] = decode(second_square, byte >> 4)?;
    }

    Ok(squares)
  }

  /// Wraps an already validated fixed-size packed payload.
  pub(crate) const fn from_bytes(bytes: [u8; PACKED_SQUARE_BYTES]) -> Self {
    Self(bytes)
  }

  /// Returns the packed payload in Move wire order.
  pub(crate) const fn into_bytes(self) -> [u8; PACKED_SQUARE_BYTES] {
    self.0
  }
}

/// Converts a Move wire offset into a canonical A1-to-H8 array index.
const fn position_index(wire_index: usize) -> usize {
  SQUARE_COUNT - 1 - wire_index
}

/// Returns the canonical square associated with a Move wire offset.
fn square_at_wire_index(wire_index: usize) -> Square {
  Square::ALL[position_index(wire_index)]
}

#[cfg(test)]
mod tests {
  use core::convert::Infallible;

  use super::*;

  #[test]
  fn packing_uses_chessnut_square_and_nibble_order() {
    let mut squares = [0; SQUARE_COUNT];
    squares[63] = 1;
    squares[62] = 2;
    squares[0] = 3;

    let packed = PackedSquares::encode(&squares, |value| value);
    let bytes = packed.into_bytes();

    assert_eq!(bytes[0], 0x21);
    assert!(bytes[1..31].iter().all(|byte| *byte == 0));
    assert_eq!(bytes[31], 0x30);
  }

  #[test]
  fn packed_squares_round_trip() {
    let squares = core::array::from_fn(|index| (index % 13) as u8);
    let packed = PackedSquares::encode(&squares, |value| value);

    let decoded = packed
      .decode(|_, value| Ok::<u8, Infallible>(value))
      .unwrap();

    assert_eq!(decoded, squares);
  }
}
