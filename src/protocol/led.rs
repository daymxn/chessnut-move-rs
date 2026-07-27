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

//! Square-addressed LED patterns and their private wire encoding.

#[cfg(doc)]
use crate::protocol;
use crate::protocol::wire::PackedSquares;
use crate::protocol::{BOARD_STATE_LENGTH, SQUARE_COUNT, Square};

/// A color supported by a Chessnut Move square LED.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LedColor {
  /// Turns the square LED off.
  #[default]
  Off,

  /// Illuminates the square in red.
  Red,

  /// Illuminates the square in green.
  Green,

  /// Illuminates the square in blue.
  Blue,
}

impl LedColor {
  /// Returns the four-bit value used in packed LED commands.
  const fn encoded(self) -> u8 {
    match self {
      Self::Off => 0,
      Self::Red => 1,
      Self::Green => 2,
      Self::Blue => 3,
    }
  }
}

/// LED colors for all 64 squares in canonical A1-to-H8 order.
///
/// Pass a pattern to [`Command::set_leds`][protocol::Command::set_leds]
/// to create the command written to the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LedPattern {
  squares: [LedColor; SQUARE_COUNT],
}

impl LedPattern {
  /// Creates a pattern from [`LedColor`]s in canonical A1-to-H8 order.
  ///
  /// Index zero controls A1 and index 63 controls H8.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{LedColor, LedPattern, SQUARE_COUNT};
  ///
  /// let pattern = LedPattern::new([LedColor::Red; SQUARE_COUNT]);
  /// assert_eq!(pattern, LedPattern::all(LedColor::Red));
  /// ```
  pub const fn new(squares: [LedColor; SQUARE_COUNT]) -> Self {
    Self { squares }
  }

  /// Creates a pattern with the same [`LedColor`] on every square.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, LedColor, LedPattern, Rank, Square};
  ///
  /// let pattern = LedPattern::all(LedColor::Green);
  /// assert_eq!(
  ///     pattern.color_at(Square::new(File::H, Rank::Eight)),
  ///     LedColor::Green,
  /// );
  /// ```
  pub const fn all(color: LedColor) -> Self {
    Self {
      squares: [color; SQUARE_COUNT],
    }
  }

  /// Returns the selected [`LedColor`] for a square.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, LedColor, LedPattern, Rank, Square};
  ///
  /// let pattern = LedPattern::all(LedColor::Blue);
  /// assert_eq!(
  ///     pattern.color_at(Square::new(File::A, Rank::One)),
  ///     LedColor::Blue,
  /// );
  /// ```
  pub const fn color_at(&self, square: Square) -> LedColor {
    self.squares[square.index()]
  }

  /// Replaces the selected [`LedColor`] for one square.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, LedColor, LedPattern, Rank, Square};
  ///
  /// let square = Square::new(File::D, Rank::Five);
  /// let mut pattern = LedPattern::default();
  /// pattern.set_color(square, LedColor::Red);
  /// assert_eq!(pattern.color_at(square), LedColor::Red);
  /// ```
  pub fn set_color(&mut self, square: Square, color: LedColor) {
    self.squares[square.index()] = color;
  }

  pub(crate) fn encode(&self) -> [u8; BOARD_STATE_LENGTH] {
    PackedSquares::encode(&self.squares, LedColor::encoded).into_bytes()
  }
}

impl Default for LedPattern {
  fn default() -> Self {
    Self::all(LedColor::Off)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::protocol::{File, Rank};

  #[test]
  fn colors_are_accessed_by_square() {
    let square = Square::new(File::C, Rank::Six);
    let mut pattern = LedPattern::default();

    pattern.set_color(square, LedColor::Blue);

    assert_eq!(pattern.color_at(square), LedColor::Blue);
  }
}
