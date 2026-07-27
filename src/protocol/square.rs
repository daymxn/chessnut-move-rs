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

//! Canonical A1-to-H8 square coordinates and notation parsing.

use core::fmt;
use core::str::FromStr;

use thiserror::Error;

const BOARD_WIDTH: u8 = 8;

/// Number of squares on a chessboard.
pub const SQUARE_COUNT: usize = (BOARD_WIDTH as usize) * (BOARD_WIDTH as usize);

/// A chessboard file from A through H.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum File {
  /// A-file.
  A,

  /// B-file.
  B,

  /// C-file.
  C,

  /// D-file.
  D,

  /// E-file.
  E,

  /// F-file.
  F,

  /// G-file.
  G,

  /// H-file.
  H,
}

impl File {
  /// Returns the zero-based file index from A through H.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::File;
  ///
  /// assert_eq!(File::A.index(), 0);
  /// assert_eq!(File::H.index(), 7);
  /// ```
  pub const fn index(self) -> u8 {
    self as u8
  }

  const fn from_ascii(byte: u8) -> Option<Self> {
    match byte {
      b'a' | b'A' => Some(Self::A),
      b'b' | b'B' => Some(Self::B),
      b'c' | b'C' => Some(Self::C),
      b'd' | b'D' => Some(Self::D),
      b'e' | b'E' => Some(Self::E),
      b'f' | b'F' => Some(Self::F),
      b'g' | b'G' => Some(Self::G),
      b'h' | b'H' => Some(Self::H),
      _ => None,
    }
  }
}

impl fmt::Display for File {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(match self {
      Self::A => "a",
      Self::B => "b",
      Self::C => "c",
      Self::D => "d",
      Self::E => "e",
      Self::F => "f",
      Self::G => "g",
      Self::H => "h",
    })
  }
}

/// A chessboard rank from one through eight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Rank {
  /// First rank.
  One,

  /// Second rank.
  Two,

  /// Third rank.
  Three,

  /// Fourth rank.
  Four,

  /// Fifth rank.
  Five,

  /// Sixth rank.
  Six,

  /// Seventh rank.
  Seven,

  /// Eighth rank.
  Eight,
}

impl Rank {
  /// Returns the zero-based rank index from one through eight.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::Rank;
  ///
  /// assert_eq!(Rank::One.index(), 0);
  /// assert_eq!(Rank::Eight.index(), 7);
  /// ```
  pub const fn index(self) -> u8 {
    self as u8
  }

  const fn from_ascii(byte: u8) -> Option<Self> {
    match byte {
      b'1' => Some(Self::One),
      b'2' => Some(Self::Two),
      b'3' => Some(Self::Three),
      b'4' => Some(Self::Four),
      b'5' => Some(Self::Five),
      b'6' => Some(Self::Six),
      b'7' => Some(Self::Seven),
      b'8' => Some(Self::Eight),
      _ => None,
    }
  }
}

impl fmt::Display for Rank {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(match self {
      Self::One => "1",
      Self::Two => "2",
      Self::Three => "3",
      Self::Four => "4",
      Self::Five => "5",
      Self::Six => "6",
      Self::Seven => "7",
      Self::Eight => "8",
    })
  }
}

/// A chessboard square stored in canonical A1-to-H8 order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Square(u8);

impl Square {
  /// This table gives internal wire codecs an infallible conversion from a
  /// canonical index. Its length is tied to [SQUARE_COUNT] at compile time.
  pub(crate) const ALL: [Self; SQUARE_COUNT] = {
    let mut squares = [Self(0); SQUARE_COUNT];
    let mut index = 0;
    while index < SQUARE_COUNT {
      squares[index] = Self(index as u8);
      index += 1;
    }
    squares
  };

  /// Creates a square from its file and rank.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, Rank, Square};
  ///
  /// let square = Square::new(File::E, Rank::Four);
  /// assert_eq!(square.to_string(), "e4");
  /// ```
  pub const fn new(file: File, rank: Rank) -> Self {
    Self(rank.index() * BOARD_WIDTH + file.index())
  }

  /// Creates a square from a canonical zero-based index.
  ///
  /// Returns `None` when `index` is not less than [`SQUARE_COUNT`].
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, Rank, Square};
  ///
  /// assert_eq!(Square::from_index(0), Some(Square::new(File::A, Rank::One)));
  /// assert_eq!(Square::from_index(64), None);
  /// ```
  pub const fn from_index(index: usize) -> Option<Self> {
    if index < SQUARE_COUNT {
      Some(Self(index as u8))
    } else {
      None
    }
  }

  /// Returns the canonical zero-based A1-to-H8 index.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, Rank, Square};
  ///
  /// assert_eq!(Square::new(File::A, Rank::One).index(), 0);
  /// assert_eq!(Square::new(File::H, Rank::Eight).index(), 63);
  /// ```
  pub const fn index(self) -> usize {
    self.0 as usize
  }

  /// Returns the square's file.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, Rank, Square};
  ///
  /// assert_eq!(Square::new(File::C, Rank::Six).file(), File::C);
  /// ```
  pub const fn file(self) -> File {
    match self.0 % BOARD_WIDTH {
      0 => File::A,
      1 => File::B,
      2 => File::C,
      3 => File::D,
      4 => File::E,
      5 => File::F,
      6 => File::G,
      7 => File::H,
      _ => unreachable!(),
    }
  }

  /// Returns the square's rank.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{File, Rank, Square};
  ///
  /// assert_eq!(Square::new(File::C, Rank::Six).rank(), Rank::Six);
  /// ```
  pub const fn rank(self) -> Rank {
    match self.0 / BOARD_WIDTH {
      0 => Rank::One,
      1 => Rank::Two,
      2 => Rank::Three,
      3 => Rank::Four,
      4 => Rank::Five,
      5 => Rank::Six,
      6 => Rank::Seven,
      7 => Rank::Eight,
      _ => unreachable!(),
    }
  }
}

impl fmt::Display for Square {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "{}{}", self.file(), self.rank())
  }
}

impl From<Square> for usize {
  fn from(square: Square) -> Self {
    square.index()
  }
}

impl FromStr for Square {
  type Err = ParseSquareError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let [file, rank] = value.as_bytes() else {
      return Err(ParseSquareError);
    };
    let file = File::from_ascii(*file).ok_or(ParseSquareError)?;
    let rank = Rank::from_ascii(*rank).ok_or(ParseSquareError)?;

    Ok(Self::new(file, rank))
  }
}

/// Reports that a string is not exactly one valid file and one valid rank.
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{File, Rank, Square};
///
/// assert_eq!("e4".parse::<Square>(), Ok(Square::new(File::E, Rank::Four)));
/// assert!("e9".parse::<Square>().is_err());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("invalid square notation")]
pub struct ParseSquareError;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn squares_use_canonical_a1_to_h8_indexes() {
    assert_eq!(Square::ALL.len(), SQUARE_COUNT);
    assert_eq!(Square::ALL[0], Square::new(File::A, Rank::One));
    assert_eq!(
      Square::ALL[SQUARE_COUNT - 1],
      Square::new(File::H, Rank::Eight)
    );
    assert_eq!(Square::new(File::A, Rank::One).index(), 0);
    assert_eq!(Square::new(File::H, Rank::Eight).index(), 63);
    assert_eq!(Square::from_index(64), None);
  }

  #[test]
  fn square_notation_parses_case_insensitively() {
    assert_eq!(
      "H8".parse::<Square>(),
      Ok(Square::new(File::H, Rank::Eight))
    );
    assert_eq!("a1".parse::<Square>(), Ok(Square::new(File::A, Rank::One)));
  }

  #[test]
  fn invalid_square_notation_is_rejected() {
    assert_eq!("i1".parse::<Square>(), Err(ParseSquareError));
    assert_eq!("a9".parse::<Square>(), Err(ParseSquareError));
    assert_eq!("a10".parse::<Square>(), Err(ParseSquareError));
  }
}
