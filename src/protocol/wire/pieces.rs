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

use crate::protocol::{Color, DecodePositionNotificationError, Piece, PieceKind, Square};

/// Encodes one optional piece as the Move protocol's four-bit piece code.
pub(crate) fn encode_piece(piece: Option<Piece>) -> u8 {
  use Color::{Black, White};
  use PieceKind::{Bishop, King, Knight, Pawn, Queen, Rook};

  match piece {
    None => 0x0,
    Some(Piece {
      color: Black,
      kind: Queen,
    }) => 0x1,
    Some(Piece {
      color: Black,
      kind: King,
    }) => 0x2,
    Some(Piece {
      color: Black,
      kind: Bishop,
    }) => 0x3,
    Some(Piece {
      color: Black,
      kind: Pawn,
    }) => 0x4,
    Some(Piece {
      color: Black,
      kind: Knight,
    }) => 0x5,
    Some(Piece {
      color: White,
      kind: Rook,
    }) => 0x6,
    Some(Piece {
      color: White,
      kind: Pawn,
    }) => 0x7,
    Some(Piece {
      color: Black,
      kind: Rook,
    }) => 0x8,
    Some(Piece {
      color: White,
      kind: Bishop,
    }) => 0x9,
    Some(Piece {
      color: White,
      kind: Knight,
    }) => 0xa,
    Some(Piece {
      color: White,
      kind: Queen,
    }) => 0xb,
    Some(Piece {
      color: White,
      kind: King,
    }) => 0xc,
  }
}

/// Decodes one four-bit piece code and attaches its square to any error.
pub(crate) fn decode_piece(
  square: Square,
  value: u8,
) -> Result<Option<Piece>, DecodePositionNotificationError> {
  use Color::{Black, White};
  use PieceKind::{Bishop, King, Knight, Pawn, Queen, Rook};

  match value {
    0x0 => Ok(None),
    0x1 => Ok(Some(Piece {
      color: Black,
      kind: Queen,
    })),
    0x2 => Ok(Some(Piece {
      color: Black,
      kind: King,
    })),
    0x3 => Ok(Some(Piece {
      color: Black,
      kind: Bishop,
    })),
    0x4 => Ok(Some(Piece {
      color: Black,
      kind: Pawn,
    })),
    0x5 => Ok(Some(Piece {
      color: Black,
      kind: Knight,
    })),
    0x6 => Ok(Some(Piece {
      color: White,
      kind: Rook,
    })),
    0x7 => Ok(Some(Piece {
      color: White,
      kind: Pawn,
    })),
    0x8 => Ok(Some(Piece {
      color: Black,
      kind: Rook,
    })),
    0x9 => Ok(Some(Piece {
      color: White,
      kind: Bishop,
    })),
    0xa => Ok(Some(Piece {
      color: White,
      kind: Knight,
    })),
    0xb => Ok(Some(Piece {
      color: White,
      kind: Queen,
    })),
    0xc => Ok(Some(Piece {
      color: White,
      kind: King,
    })),
    value => Err(DecodePositionNotificationError::InvalidPiece { square, value }),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::protocol::square::{File, Rank};

  const PIECE_CODES: [(u8, Option<Piece>); 13] = [
    (0x0, None),
    (
      0x1,
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::Queen,
      }),
    ),
    (
      0x2,
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::King,
      }),
    ),
    (
      0x3,
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::Bishop,
      }),
    ),
    (
      0x4,
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::Pawn,
      }),
    ),
    (
      0x5,
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::Knight,
      }),
    ),
    (
      0x6,
      Some(Piece {
        color: Color::White,
        kind: PieceKind::Rook,
      }),
    ),
    (
      0x7,
      Some(Piece {
        color: Color::White,
        kind: PieceKind::Pawn,
      }),
    ),
    (
      0x8,
      Some(Piece {
        color: Color::Black,
        kind: PieceKind::Rook,
      }),
    ),
    (
      0x9,
      Some(Piece {
        color: Color::White,
        kind: PieceKind::Bishop,
      }),
    ),
    (
      0xa,
      Some(Piece {
        color: Color::White,
        kind: PieceKind::Knight,
      }),
    ),
    (
      0xb,
      Some(Piece {
        color: Color::White,
        kind: PieceKind::Queen,
      }),
    ),
    (
      0xc,
      Some(Piece {
        color: Color::White,
        kind: PieceKind::King,
      }),
    ),
  ];

  #[test]
  fn encodes_and_decodes_every_piece_code() {
    let square = Square::new(File::E, Rank::Four);

    for (code, piece) in PIECE_CODES {
      assert_eq!(encode_piece(piece), code);
      assert_eq!(decode_piece(square, code), Ok(piece));
    }
  }

  #[test]
  fn rejects_unknown_piece_codes_with_the_square() {
    let square = Square::new(File::B, Rank::Seven);

    for value in 0xd..=u8::MAX {
      assert_eq!(
        decode_piece(square, value),
        Err(DecodePositionNotificationError::InvalidPiece { square, value })
      );
    }
  }
}
