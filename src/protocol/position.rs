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

//! Canonical chess positions and piece identities.

#[cfg(doc)]
use crate::protocol;
use crate::protocol::{SQUARE_COUNT, Square};

/// Number of packed bytes used to encode all 64 board squares.
pub const BOARD_STATE_LENGTH: usize = 32;

/// Piece occupancy for all 64 squares.
///
/// The internal array uses canonical A1-to-H8 indexing. Use
/// [`Position::piece_at`] and [`Position::set_piece`] when the square is known
/// by file and rank.
///
/// Realtime board updates carry positions through
/// [`BoardEvent::PositionChanged`][protocol::BoardEvent::PositionChanged].
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{
///     Color, File, Piece, PieceKind, Position, Rank, SQUARE_COUNT, Square,
/// };
///
/// let mut position = Position::new([None; SQUARE_COUNT]);
/// let e4 = Square::new(File::E, Rank::Four);
/// position.set_piece(
///     e4,
///     Some(Piece {
///         color: Color::White,
///         kind: PieceKind::Pawn,
///     }),
/// );
/// assert!(position.piece_at(e4).is_some());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Position {
  squares: [Option<Piece>; SQUARE_COUNT],
}

impl Position {
  /// Creates a position from squares in canonical A1-to-H8 order.
  ///
  /// Index zero represents A1 and index 63 represents H8.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{
  ///     Color, File, Piece, PieceKind, Position, Rank, SQUARE_COUNT, Square,
  /// };
  ///
  /// let white_king = Piece {
  ///     color: Color::White,
  ///     kind: PieceKind::King,
  /// };
  /// let e1 = Square::new(File::E, Rank::One);
  /// let mut squares = [None; SQUARE_COUNT];
  /// squares[e1.index()] = Some(white_king);
  ///
  /// let position = Position::new(squares);
  /// assert_eq!(position.piece_at(e1), Some(white_king));
  /// ```
  pub fn new(squares: [Option<Piece>; SQUARE_COUNT]) -> Self {
    Self { squares }
  }

  /// Returns the piece occupying a square, or `None` when it is empty.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{
  ///     BoardEvent, File, PieceKind, Rank, Square,
  /// };
  ///
  /// fn pawn_reached_e4(event: BoardEvent) -> bool {
  ///     let BoardEvent::PositionChanged(position) = event else {
  ///         return false;
  ///     };
  ///
  ///     position
  ///         .piece_at(Square::new(File::E, Rank::Four))
  ///         .is_some_and(|piece| piece.kind == PieceKind::Pawn)
  /// }
  /// # let _ = pawn_reached_e4;
  /// ```
  pub const fn piece_at(&self, square: Square) -> Option<Piece> {
    self.squares[square.index()]
  }

  /// Replaces the piece occupying a square.
  ///
  /// Pass `None` to empty the square.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::protocol::{
  ///     AutoMoveMode, Command, File, Position, Rank, Square,
  /// };
  ///
  /// fn move_e2_to_e4(mut observed: Position) -> Command {
  ///     let e2 = Square::new(File::E, Rank::Two);
  ///     let e4 = Square::new(File::E, Rank::Four);
  ///     let pawn = observed.piece_at(e2);
  ///
  ///     observed.set_piece(e2, None);
  ///     observed.set_piece(e4, pawn);
  ///     Command::auto_move(observed, AutoMoveMode::Normal)
  /// }
  /// # let _ = move_e2_to_e4;
  /// ```
  pub fn set_piece(&mut self, square: Square, piece: Option<Piece>) {
    self.squares[square.index()] = piece;
  }

  /// Returns canonical square storage for private wire encoders.
  pub(crate) const fn squares(&self) -> &[Option<Piece>; SQUARE_COUNT] {
    &self.squares
  }
}

/// A chess piece described by its [color] and [kind].
///
/// # Examples
///
/// ```
/// use chessnut_move::protocol::{Color, Piece, PieceKind};
///
/// let queen = Piece {
///     color: Color::Black,
///     kind: PieceKind::Queen,
/// };
/// ```
///
/// [color]: Color
/// [kind]: PieceKind
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
  /// Side to which the piece belongs.
  pub color: Color,

  /// Chess role of the piece.
  pub kind: PieceKind,
}

/// Side to which a chess piece belongs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
  /// White side.
  White,

  /// Black side.
  Black,
}

/// Chess role of a piece.
#[derive(PartialEq, Eq, Ord, PartialOrd, Copy, Clone, Debug, Hash)]
pub enum PieceKind {
  /// The Pawn piece on a Chess board.
  Pawn,

  /// The Knight piece on a Chess board.
  Knight,

  /// The Bishop piece on a Chess board.
  Bishop,

  /// The Rook piece on a Chess board.
  Rook,

  /// The Queen piece on a Chess board.
  Queen,

  /// The King piece on a Chess board.
  King,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::protocol::{File, Rank};

  #[test]
  fn pieces_are_accessed_by_square() {
    let square = Square::new(File::E, Rank::Four);
    let piece = Piece {
      color: Color::White,
      kind: PieceKind::Pawn,
    };
    let mut position = Position::new([None; SQUARE_COUNT]);

    position.set_piece(square, Some(piece));

    assert_eq!(position.piece_at(square), Some(piece));
  }
}
