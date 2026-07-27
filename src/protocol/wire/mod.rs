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

//! Private conversions between public protocol values and packed wire bytes.
//!
//! The Move board reverses canonical square order and packs two four-bit values
//! into each byte. Keeping that representation private prevents downstream
//! code from constructing partially validated wire values.

mod pieces;
mod squares;

pub(crate) use pieces::{decode_piece, encode_piece};
pub(crate) use squares::PackedSquares;
