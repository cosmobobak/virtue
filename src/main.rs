#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    dead_code,
    unused_imports,
    clippy::missing_const_for_fn,
    clippy::if_not_else
)]
use board::Board;
use chessmove::Move;
use definitions::Piece;
use rand::Rng;

use crate::lookups::SIDE_KEY;
/// Rust chess engine built using the VICE video series.
///
///
use crate::{
    definitions::Square64,
    lookups::{filerank_to_square, PIECE_KEYS},
    movegen::MoveList,
};
// #[macro_use]
// extern crate lazy_static;

mod attack;
mod bitboard;
mod board;
mod chessmove;
mod definitions;
mod lookups;
mod movegen;
mod validate;

fn sq_attack_by_side(side: u8, board: &Board) {
    println!("Squares attacked by side {}:", side);
    for rank in (0..8).rev() {
        for file in 0..8 {
            let sq = filerank_to_square(file, rank);
            if board.sq_attacked(sq as usize, side) {
                print!("X ");
            } else {
                print!("- ");
            }
        }
        println!();
    }
    println!();
}

const LEGALMOVES48: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

fn main() {
    let mut b = Board::new();
    b.set_from_fen(LEGALMOVES48);

    let mut move_list = MoveList::new();
    b.generate_all_moves(&mut move_list);
    println!("{}", move_list);
}