// The granularity of evaluation in this engine is going to be thousandths of a pawn.

use crate::{lookups::{init_eval_masks, init_passed_isolated_bb}, board::Board, movegen::MoveConsumer, chessmove::Move};
use crate::definitions::Piece;

pub const PAWN_VALUE: i32   =   1_000;
pub const KNIGHT_VALUE: i32 =   3_250;
pub const BISHOP_VALUE: i32 =   3_330;
pub const ROOK_VALUE: i32   =   5_500;
pub const QUEEN_VALUE: i32  =  10_000;
pub const KING_VALUE: i32   = 500_000;

/// The value of checkmate.
/// To recover depth-to-mate, we subtract depth (ply) from this value.
/// e.g. if white has a mate in two ply, the output from a depth-5 search will be
/// `3_000_000 - 2 = 2_999_998`.
pub const MATE_SCORE: i32 = 3_000_000;

/// A threshold over which scores must be mate.
pub const IS_MATE_SCORE: i32 = MATE_SCORE - 300;

/// The value of a draw.
pub const DRAW_SCORE: i32 = 0;

#[rustfmt::skip]
pub static PIECE_VALUES: [i32; 13] = [
    0,
    PAWN_VALUE, KNIGHT_VALUE, BISHOP_VALUE, ROOK_VALUE, QUEEN_VALUE, KING_VALUE,
    PAWN_VALUE, KNIGHT_VALUE, BISHOP_VALUE, ROOK_VALUE, QUEEN_VALUE, KING_VALUE,
];

/// The malus applied when a pawn has no pawns of its own colour to the left or right.
pub const ISOLATED_PAWN_MALUS: i32 = PAWN_VALUE / 3;

/// The malus applied when two (or more) pawns of a colour are on the same file.
pub const DOUBLED_PAWN_MALUS: i32 = PAWN_VALUE / 2 + 50;

/// The bonus granted for having two bishops.
pub const BISHOP_PAIR_BONUS: i32 = PAWN_VALUE / 4;

/// The bonus granted for having more pawns when you have knights on the board.
// pub const KNIGHT_PAWN_BONUS: i32 = PAWN_VALUE / 15;

// The multipliers applied to mobility scores.
pub const PAWN_MOBILITY_MULTIPLIER: i32 = 10;
pub const KNIGHT_MOBILITY_MULTIPLIER: i32 = 15;
pub const BISHOP_MOBILITY_MULTIPLIER: i32 = 10;
pub const ROOK_MOBILITY_MULTIPLIER: i32 = 10;
pub const QUEEN_MOBILITY_MULTIPLIER: i32 = 10;
pub const KING_MOBILITY_MULTIPLIER: i32 = 10;

/// The multiplier applied to the pst scores.
pub const PST_MULTIPLIER: i32 = 3;

const PAWN_DANGER: i32   = 200;
const KNIGHT_DANGER: i32 = 300;
const BISHOP_DANGER: i32 = 100;
const ROOK_DANGER: i32   = 400;
const QUEEN_DANGER: i32  = 500;

#[rustfmt::skip]
pub static PIECE_DANGER_VALUES: [i32; 13] = [
    0,
    PAWN_DANGER, KNIGHT_DANGER, BISHOP_DANGER, ROOK_DANGER, QUEEN_DANGER, 0,
    PAWN_DANGER, KNIGHT_DANGER, BISHOP_DANGER, ROOK_DANGER, QUEEN_DANGER, 0,
];

/// The bonus for having IDX pawns in front of the king.
pub static SHIELD_BONUS: [i32; 4] = [0, 50, 200, 300];

/// A threshold over which we will not bother evaluating more than material and PSTs.
pub const LAZY_THRESHOLD_1: i32 = 14_000;
/// A threshold over which we will not bother evaluating more than pawns and mobility.
pub const LAZY_THRESHOLD_2: i32 = 8_000;

const PAWN_PHASE: f32 = 0.1;
const KNIGHT_PHASE: f32 = 1.0;
const BISHOP_PHASE: f32 = 1.0;
const ROOK_PHASE: f32 = 2.0;
const QUEEN_PHASE: f32 = 4.0;
const TOTAL_PHASE: f32 = 16.0 * PAWN_PHASE
    + 4.0 * KNIGHT_PHASE
    + 4.0 * BISHOP_PHASE
    + 4.0 * ROOK_PHASE
    + 2.0 * QUEEN_PHASE;

pub static RANK_BB: [u64; 8] = init_eval_masks().0;
pub static FILE_BB: [u64; 8] = init_eval_masks().1;

pub static WHITE_PASSED_BB: [u64; 64] = init_passed_isolated_bb().0;
pub static BLACK_PASSED_BB: [u64; 64] = init_passed_isolated_bb().1;

pub static ISOLATED_BB: [u64; 64] = init_passed_isolated_bb().2;

/// The bonus applied when a pawn has no pawns of the opposite colour ahead of it, or to the left or right, scaled by the rank that the pawn is on.
pub static PASSED_PAWN_BONUS: [i32; 8] = [
    0,                               // illegal
    PAWN_VALUE / 10,                 // tenth of a pawn
    PAWN_VALUE / 8,                  // eighth of a pawn
    PAWN_VALUE / 5,                  // fifth of a pawn
    (2 * PAWN_VALUE) / 5,            // two fifths of a pawn
    PAWN_VALUE / 2 + PAWN_VALUE / 4, // three quarters of a pawn
    PAWN_VALUE + PAWN_VALUE / 2,     // one and a half pawns
    0,                               // illegal
];

/// `game_phase` computes a number between 0.0 and 1.0, which is the phase of the game.
/// 0.0 is the opening, 1.0 is the endgame.
#[allow(clippy::cast_precision_loss, clippy::many_single_char_names)]
pub fn game_phase(p: usize, n: usize, b: usize, r: usize, q: usize) -> f32 {
    let mut phase = TOTAL_PHASE;
    phase -= PAWN_PHASE * p as f32;
    phase -= KNIGHT_PHASE * n as f32;
    phase -= BISHOP_PHASE * b as f32;
    phase -= ROOK_PHASE * r as f32;
    phase -= QUEEN_PHASE * q as f32;
    phase / TOTAL_PHASE
}

/// A struct that holds all the terms in the evaluation function, intended to be used by the
/// tuner for optimising the evaluation function.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalVector {
    /// Whether this position is valid to use for tuning (positions should be quiescent, amongst other considerations).
    pub valid: bool,
    /// The relative pawn count
    pub pawns: i32,
    /// The relative knight count
    pub knights: i32,
    /// The relative bishop count
    pub bishops: i32,
    /// The relative rook count
    pub rooks: i32,
    /// The relative queen count
    pub queens: i32,
    /// The bishop pair score. (can only be -1, 0 or 1)
    pub bishop_pair: i32,
    /// The relative number of passed pawns by rank.
    pub passed_pawns_by_rank: [i32; 8],
    /// The relative number of isolated pawns.
    pub isolated_pawns: i32,
    /// The relative number of doubled pawns.
    pub doubled_pawns: i32,
    /// The relative pst score, before scaling.
    pub pst: i32,
    /// The relative pawn mobility count.
    pub pawn_mobility: i32,
    /// The relative knight mobility count.
    pub knight_mobility: i32,
    /// The relative bishop mobility count.
    pub bishop_mobility: i32,
    /// The relative rook mobility count.
    pub rook_mobility: i32,
    /// The relative queen mobility count.
    pub queen_mobility: i32,
    /// The relative king mobility count.
    pub king_mobility: i32,
    /// The relative shield count.
    pub pawn_shield: i32,
    /// The turn (1 or -1)
    pub turn: i32,
}

impl EvalVector {
    pub const fn new() -> Self {
        Self {
            valid: true,
            pawns: 0,
            knights: 0,
            bishops: 0,
            rooks: 0,
            queens: 0,
            bishop_pair: 0,
            passed_pawns_by_rank: [0; 8],
            isolated_pawns: 0,
            doubled_pawns: 0,
            pst: 0,
            pawn_mobility: 0,
            knight_mobility: 0,
            bishop_mobility: 0,
            rook_mobility: 0,
            queen_mobility: 0,
            king_mobility: 0,
            pawn_shield: 0,
            turn: 0,
        }
    }

    pub fn csvify(&self) -> String {
        let csv = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.pawns, self.knights, self.bishops, self.rooks, self.queens,
            self.bishop_pair, self.passed_pawns_by_rank[0], self.passed_pawns_by_rank[1],
            self.passed_pawns_by_rank[2], self.passed_pawns_by_rank[3], self.passed_pawns_by_rank[4],
            self.passed_pawns_by_rank[5], self.passed_pawns_by_rank[6], self.passed_pawns_by_rank[7],
            self.isolated_pawns, self.doubled_pawns, self.pst, self.pawn_mobility,
            self.knight_mobility, self.bishop_mobility, self.rook_mobility, self.queen_mobility,
            self.king_mobility, self.pawn_shield, self.turn
        );
        assert!(csv.chars().filter(|&c| c == ',').count() == Self::header().chars().filter(|&c| c == ',').count());
        csv
    }

    pub const fn header() -> &'static str {
        "p,n,b,r,q,bpair,ppr0,ppr1,ppr2,ppr3,ppr4,ppr5,ppr6,ppr7,isolated,doubled,pst,p_mob,n_mob,b_mob,r_mob,q_mob,k_mob,p_shield,turn"
    }
}

pub struct MoveCounter<'a> {
    counters: [i32; 6],
    board: &'a Board,
}

impl<'a> MoveCounter<'a> {
    pub const fn new(board: &'a Board) -> Self {
        Self { counters: [0; 6], board }
    }

    pub const fn score(&self) -> i32 {
        let pawns = self.counters[0] * PAWN_MOBILITY_MULTIPLIER;
        let knights = self.counters[1] * KNIGHT_MOBILITY_MULTIPLIER;
        let bishops = self.counters[2] * BISHOP_MOBILITY_MULTIPLIER;
        let rooks = self.counters[3] * ROOK_MOBILITY_MULTIPLIER;
        let queens = self.counters[4] * QUEEN_MOBILITY_MULTIPLIER;
        let kings = self.counters[5] * KING_MOBILITY_MULTIPLIER;
        pawns + knights + bishops + rooks + queens + kings
    }

    pub fn get_mobility_of(&self, piece: Piece) -> i32 {
        match piece {
            Piece::WP | Piece::BP => self.counters[0],
            Piece::WN | Piece::BN => self.counters[1],
            Piece::WB | Piece::BB => self.counters[2],
            Piece::WR | Piece::BR => self.counters[3],
            Piece::WQ | Piece::BQ => self.counters[4],
            Piece::WK | Piece::BK => self.counters[5],
            Piece::Empty => panic!("Tried to get mobility of empty piece"),
        }
    }
}

impl<'a> MoveConsumer for MoveCounter<'a> {
    fn push(&mut self, m: Move, _score: i32) {
        let moved_piece = self.board.moved_piece(m);
        let idx = (moved_piece - 1) % 6;
        unsafe {
            *self.counters.get_unchecked_mut(idx as usize) += 1;
        }
    }
}