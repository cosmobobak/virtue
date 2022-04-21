#![allow(dead_code)]
#![allow(
    clippy::collapsible_else_if,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use std::{
    fmt::{Debug, Display, Formatter},
    mem,
};

use crate::{
    attack::{B_DIR, IS_BISHOPQUEEN, IS_KING, IS_KNIGHT, IS_ROOKQUEEN, K_DIR, N_DIR, R_DIR, WHITE_SLIDERS, BLACK_SLIDERS, WHITE_JUMPERS, BLACK_JUMPERS, Q_DIR},
    bitboard::{pop_lsb, write_bb},
    chessmove::Move,
    definitions::{Colour, Square120, BLACK, WHITE, square120_name, WB, BB, BR, WR, WQ, BQ},
    lookups::{
        CASTLE_KEYS, PIECE_BIG, PIECE_COL, PIECE_KEYS, PIECE_MAJ, PIECE_MIN, PIECE_VAL,
        RANKS_BOARD, SIDE_KEY, SQ120_TO_SQ64, PIECE_NAMES,
    },
    movegen::{MoveList, offset_square_offboard},
    validate::{side_valid, square_on_board, piece_valid_empty, piece_valid},
};
use crate::{
    definitions::{Castling, File, Piece, Rank, Undo, BOARD_N_SQUARES, MAX_GAME_MOVES},
    lookups::{filerank_to_square, SQ64_TO_SQ120},
};

#[derive(Eq, PartialEq)]
pub struct Board {
    pieces: [u8; BOARD_N_SQUARES],
    pawns: [u64; 3],
    king_sq: [u8; 2],
    side: u8,
    ep_sq: u8,
    fifty_move_counter: u8,
    ply: usize,
    hist_ply: usize,
    key: u64,
    piece_num: [u8; 13],
    big_piece_counts: [u8; 2],
    major_piece_counts: [u8; 2],
    minor_piece_counts: [u8; 2],
    material: [i32; 2],
    castle_perm: u8,
    history: Vec<Undo>,
    p_list: [[u8; 10]; 13], // p_list[piece][N]
}

impl Board {
    pub fn new() -> Self {
        let mut out = Self {
            pieces: [Square120::NoSquare as u8; BOARD_N_SQUARES],
            pawns: [0; 3],
            king_sq: [Square120::NoSquare as u8; 2],
            side: 0,
            ep_sq: Square120::NoSquare as u8,
            fifty_move_counter: 0,
            ply: 0,
            hist_ply: 0,
            key: 0,
            piece_num: [0; 13],
            big_piece_counts: [0; 2],
            major_piece_counts: [0; 2],
            minor_piece_counts: [0; 2],
            material: [0; 2],
            castle_perm: 0,
            history: Vec::with_capacity(MAX_GAME_MOVES),
            p_list: [[0; 10]; 13],
        };
        out.reset();
        out
    }

    pub fn generate_pos_key(&self) -> u64 {
        let mut key = 0;
        for sq in 0..BOARD_N_SQUARES {
            let piece = self.pieces[sq];
            if piece != Piece::Empty as u8
                && piece != Square120::OffBoard as u8
                && piece != Square120::NoSquare as u8
            {
                debug_assert!(piece >= Piece::WP as u8 && piece <= Piece::BK as u8);
                key ^= PIECE_KEYS[piece as usize][sq];
            }
        }

        if self.side == WHITE {
            key ^= SIDE_KEY;
        }

        if self.ep_sq != 0 {
            debug_assert!((self.ep_sq as usize) < BOARD_N_SQUARES);
            key ^= PIECE_KEYS[Piece::Empty as usize][self.ep_sq as usize];
        }

        debug_assert!(self.castle_perm <= 15);

        key ^= CASTLE_KEYS[self.castle_perm as usize];

        key
    }

    pub fn reset(&mut self) {
        self.pieces.fill(Square120::OffBoard as u8);
        for &i in &SQ64_TO_SQ120 {
            self.pieces[i as usize] = Piece::Empty as u8;
        }
        self.big_piece_counts.fill(0);
        self.major_piece_counts.fill(0);
        self.minor_piece_counts.fill(0);
        self.material.fill(0);
        self.pawns.fill(0);
        self.piece_num.fill(0);
        self.king_sq.fill(Square120::NoSquare as u8);
        self.side = Colour::Both as u8;
        self.ep_sq = Square120::NoSquare as u8;
        self.fifty_move_counter = 0;
        self.ply = 0;
        self.hist_ply = 0;
        self.castle_perm = 0;
        self.key = 0;
    }

    pub fn set_from_fen(&mut self, fen: &str) {
        assert!(fen.is_ascii());

        let mut rank = Rank::Rank8 as u8;
        let mut file = File::FileA as u8;

        self.reset();

        let fen_chars = fen.as_bytes();
        let split_idx = fen_chars.iter().position(|&c| c == b' ').unwrap();
        let (board_part, info_part) = fen_chars.split_at(split_idx);

        for &c in board_part {
            let mut count = 1;
            let piece;
            match c {
                b'P' => piece = Piece::WP as u8,
                b'R' => piece = Piece::WR as u8,
                b'N' => piece = Piece::WN as u8,
                b'B' => piece = Piece::WB as u8,
                b'Q' => piece = Piece::WQ as u8,
                b'K' => piece = Piece::WK as u8,
                b'p' => piece = Piece::BP as u8,
                b'r' => piece = Piece::BR as u8,
                b'n' => piece = Piece::BN as u8,
                b'b' => piece = Piece::BB as u8,
                b'q' => piece = Piece::BQ as u8,
                b'k' => piece = Piece::BK as u8,
                b'1'..=b'8' => {
                    piece = Piece::Empty as u8;
                    count = c - b'0';
                }
                b'/' => {
                    rank -= 1;
                    file = File::FileA as u8;
                    continue;
                }
                c => {
                    panic!(
                        "FEN string is invalid, got unexpected character: \"{}\"",
                        c as char
                    );
                }
            }

            for _ in 0..count {
                let sq64 = rank * 8 + file;
                let sq120 = SQ64_TO_SQ120[sq64 as usize];
                if piece != Piece::Empty as u8 {
                    self.pieces[sq120 as usize] = piece;
                }
                file += 1;
            }
        }

        let mut info_parts = info_part[1..].split(|&c| c == b' ');

        self.set_side(info_parts.next());

        self.set_castling(info_parts.next());

        self.set_ep(info_parts.next());

        self.set_halfmove(info_parts.next());

        self.set_fullmove(info_parts.next());

        self.key = self.generate_pos_key();

        self.update_list_material();
    }

    fn set_side(&mut self, side_part: Option<&[u8]>) {
        self.side = match side_part {
            None => panic!("FEN string is invalid, expected side part."),
            Some([b'w']) => WHITE,
            Some([b'b']) => BLACK,
            Some(other) => panic!(
                "FEN string is invalid, expected side to be 'w' or 'b', got \"{}\"",
                std::str::from_utf8(other).unwrap()
            ),
        };
    }

    fn set_castling(&mut self, castling_part: Option<&[u8]>) {
        match castling_part {
            None => panic!("FEN string is invalid, expected castling part."),
            Some([b'-']) => self.castle_perm = 0,
            Some(castling) => {
                for &c in castling {
                    match c {
                        b'K' => self.castle_perm |= Castling::WK as u8,
                        b'Q' => self.castle_perm |= Castling::WQ as u8,
                        b'k' => self.castle_perm |= Castling::BK as u8,
                        b'q' => self.castle_perm |= Castling::BQ as u8,
                        _ => panic!("FEN string is invalid, expected castling part to be of the form 'KQkq', got \"{}\"", castling.iter().map(|&c| c as char).collect::<String>()),
                    }
                }
            }
        }
    }

    fn set_ep(&mut self, ep_part: Option<&[u8]>) {
        match ep_part {
            None => panic!("FEN string is invalid, expected en passant part."),
            Some([b'-']) => self.ep_sq = Square120::NoSquare as u8,
            Some(ep_sq) => {
                assert!(ep_sq.len() == 2, "FEN string is invalid, expected en passant part to be of the form 'a1', got \"{}\"", ep_sq.iter().map(|&c| c as char).collect::<String>());
                let file = ep_sq[0] as u8 - b'a';
                let rank = ep_sq[1] as u8 - b'1';
                assert!(file >= File::FileA as u8 && file <= File::FileH as u8);
                assert!(rank >= Rank::Rank1 as u8 && rank <= Rank::Rank8 as u8);
                self.ep_sq = filerank_to_square(file, rank);
            }
        }
    }

    fn set_halfmove(&mut self, halfmove_part: Option<&[u8]>) {
        match halfmove_part {
            None => panic!("FEN string is invalid, expected halfmove clock part."),
            Some(halfmove_clock) => {
                self.fifty_move_counter = std::str::from_utf8(halfmove_clock)
                    .expect("FEN string is invalid, expected halfmove clock part to be valid UTF-8")
                    .parse::<u8>()
                    .expect("FEN string is invalid, expected halfmove clock part to be a number");
            }
        }
    }

    fn set_fullmove(&mut self, fullmove_part: Option<&[u8]>) {
        match fullmove_part {
            None => panic!("FEN string is invalid, expected fullmove number part."),
            Some(fullmove_number) => {
                self.ply = std::str::from_utf8(fullmove_number)
                    .expect(
                        "FEN string is invalid, expected fullmove number part to be valid UTF-8",
                    )
                    .parse::<usize>()
                    .expect("FEN string is invalid, expected fullmove number part to be a number")
                    * 2;
                if self.side == BLACK {
                    self.ply += 1;
                }
            }
        }
    }

    fn update_list_material(&mut self) {
        for index in 0..BOARD_N_SQUARES {
            let sq = index;
            let piece = self.pieces[index];
            if piece != Square120::OffBoard as u8 && piece != Piece::Empty as u8 {
                let colour = PIECE_COL[piece as usize];

                if PIECE_BIG[piece as usize] {
                    self.big_piece_counts[colour as usize] += 1;
                }
                if PIECE_MIN[piece as usize] {
                    self.minor_piece_counts[colour as usize] += 1;
                }
                if PIECE_MAJ[piece as usize] {
                    self.major_piece_counts[colour as usize] += 1;
                }

                self.material[colour as usize] += PIECE_VAL[piece as usize];

                self.p_list[piece as usize][self.piece_num[piece as usize] as usize] =
                    sq.try_into().unwrap();
                self.piece_num[piece as usize] += 1;

                if piece == Piece::WK as u8 || piece == Piece::BK as u8 {
                    self.king_sq[colour as usize] = sq.try_into().unwrap();
                }

                if piece == Piece::WP as u8 || piece == Piece::BP as u8 {
                    self.pawns[colour as usize] |= 1 << SQ120_TO_SQ64[sq as usize];
                    self.pawns[Colour::Both as usize] |= 1 << SQ120_TO_SQ64[sq as usize];
                }
            }
        }
    }

    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub fn check_validity(&self) {
        use Colour::{Black, Both, White};
        let mut piece_num = [0; 13];
        let mut big_pce = [0, 0];
        let mut maj_pce = [0, 0];
        let mut min_pce = [0, 0];
        let mut material = [0, 0];

        let mut pawns = self.pawns;

        // check piece lists
        for piece in (Piece::WP as u8)..=(Piece::BK as u8) {
            for p_num in 0..self.piece_num[piece as usize] {
                let sq120 = self.p_list[piece as usize][p_num as usize];
                assert_eq!(self.pieces[sq120 as usize], piece);
            }
        }

        // check piece count and other counters
        for &sq120 in &SQ64_TO_SQ120 {
            let piece = self.pieces[sq120 as usize];
            piece_num[piece as usize] += 1;
            let colour = PIECE_COL[piece as usize];
            if PIECE_BIG[piece as usize] {
                big_pce[colour as usize] += 1;
            }
            if PIECE_MAJ[piece as usize] {
                maj_pce[colour as usize] += 1;
            }
            if PIECE_MIN[piece as usize] {
                min_pce[colour as usize] += 1;
            }
            if colour != Both {
                material[colour as usize] += PIECE_VAL[piece as usize];
            }
        }

        for piece in (Piece::WP as u8)..=(Piece::BK as u8) {
            assert_eq!(piece_num[piece as usize], self.piece_num[piece as usize]);
        }

        // check bitboards count
        assert_eq!(
            pawns[White as usize].count_ones(),
            u32::from(self.piece_num[Piece::WP as usize])
        );
        assert_eq!(
            pawns[Black as usize].count_ones(),
            u32::from(self.piece_num[Piece::BP as usize])
        );
        assert_eq!(
            pawns[Both as usize].count_ones(),
            u32::from(self.piece_num[Piece::WP as usize])
                + u32::from(self.piece_num[Piece::BP as usize])
        );

        // check bitboards' squares
        while pawns[White as usize] > 0 {
            let sq64 = pop_lsb(&mut pawns[White as usize]);
            assert_eq!(
                self.pieces[SQ64_TO_SQ120[sq64 as usize] as usize],
                Piece::WP as u8
            );
        }

        while pawns[Black as usize] > 0 {
            let sq64 = pop_lsb(&mut pawns[Black as usize]);
            assert_eq!(
                self.pieces[SQ64_TO_SQ120[sq64 as usize] as usize],
                Piece::BP as u8
            );
        }

        while pawns[Both as usize] > 0 {
            let sq64 = pop_lsb(&mut pawns[Both as usize]);
            assert!(
                self.pieces[SQ64_TO_SQ120[sq64 as usize] as usize] == Piece::WP as u8
                    || self.pieces[SQ64_TO_SQ120[sq64 as usize] as usize] == Piece::BP as u8
            );
        }

        assert_eq!(material[White as usize], self.material[White as usize]);
        assert_eq!(material[Black as usize], self.material[Black as usize]);
        assert_eq!(
            min_pce[White as usize],
            self.minor_piece_counts[White as usize]
        );
        assert_eq!(
            min_pce[Black as usize],
            self.minor_piece_counts[Black as usize]
        );
        assert_eq!(
            maj_pce[White as usize],
            self.major_piece_counts[White as usize]
        );
        assert_eq!(
            maj_pce[Black as usize],
            self.major_piece_counts[Black as usize]
        );
        assert_eq!(
            big_pce[White as usize],
            self.big_piece_counts[White as usize]
        );
        assert_eq!(
            big_pce[Black as usize],
            self.big_piece_counts[Black as usize]
        );

        assert!(self.side == WHITE || self.side == BLACK);
        assert_eq!(self.generate_pos_key(), self.key);

        assert!(
            self.ep_sq == Square120::NoSquare as u8
                || (RANKS_BOARD[self.ep_sq as usize] == Rank::Rank6 as usize && self.side == WHITE)
                || (RANKS_BOARD[self.ep_sq as usize] == Rank::Rank3 as usize && self.side == BLACK)
        );

        assert!(self.fifty_move_counter < 100);

        assert_eq!(
            self.pieces[self.king_sq[White as usize] as usize],
            Piece::WK as u8
        );
        assert_eq!(
            self.pieces[self.king_sq[Black as usize] as usize],
            Piece::BK as u8
        );
    }

    /// Determines if `sq` is attacked by `side`
    pub fn sq_attacked(&self, sq: usize, side: u8) -> bool {
        use Piece::{Empty, BP, WP};

        debug_assert!(side_valid(side));
        debug_assert!(square_on_board(sq.try_into().unwrap()));
        debug_assert!({
            self.check_validity();
            true
        });

        // pawns
        if side == WHITE {
            if self.pieces[sq - 11] == WP as u8 || self.pieces[sq - 9] == WP as u8 {
                return true;
            }
        } else {
            if self.pieces[sq + 11] == BP as u8 || self.pieces[sq + 9] == BP as u8 {
                return true;
            }
        }

        // knights
        for &dir in &N_DIR {
            let p = self.pieces[(sq as isize + dir) as usize];
            if p != Square120::OffBoard as u8
                && IS_KNIGHT[p as usize]
                && PIECE_COL[p as usize] as u8 == side
            {
                return true;
            }
        }

        // rooks, queens
        for &dir in &R_DIR {
            let mut t_sq = sq as isize + dir;
            let mut piece = self.pieces[t_sq as usize];
            while piece != Square120::OffBoard as u8 {
                if piece != Empty as u8 {
                    if IS_ROOKQUEEN[piece as usize] && PIECE_COL[piece as usize] as u8 == side {
                        return true;
                    }
                    break;
                }
                t_sq += dir;
                piece = self.pieces[t_sq as usize];
            }
        }

        // bishops, queens
        for &dir in &B_DIR {
            let mut t_sq = sq as isize + dir;
            let mut piece = self.pieces[t_sq as usize];
            while piece != Square120::OffBoard as u8 {
                if piece != Empty as u8 {
                    if IS_BISHOPQUEEN[piece as usize] && PIECE_COL[piece as usize] as u8 == side {
                        return true;
                    }
                    break;
                }
                t_sq += dir;
                piece = self.pieces[t_sq as usize];
            }
        }

        // king
        for &dir in &K_DIR {
            let p = self.pieces[(sq as isize + dir) as usize];
            if p != Square120::OffBoard as u8
                && IS_KING[p as usize]
                && PIECE_COL[p as usize] as u8 == side
            {
                return true;
            }
        }

        false
    }

    fn add_quiet_move(&self, m: Move, move_list: &mut MoveList) {
        move_list.push(m, 0);
    }

    fn add_capture_move(&self, m: Move, move_list: &mut MoveList) {
        move_list.push(m, 0);
    }

    fn add_ep_move(&self, m: Move, move_list: &mut MoveList) {
        move_list.push(m, 0);
    }

    fn add_pawn_cap_move<const SIDE: u8>(
        &self,
        from: u8,
        to: u8,
        cap: u8,
        move_list: &mut MoveList,
    ) {
        debug_assert!(piece_valid_empty(cap));
        debug_assert!(square_on_board(from));
        debug_assert!(square_on_board(to));
        let promo_rank = if SIDE == WHITE {
            Rank::Rank7 as usize
        } else {
            Rank::Rank2 as usize
        };
        if RANKS_BOARD[from as usize] == promo_rank {
            if SIDE == WHITE {
                for &promo in &[
                    Piece::WQ as u8,
                    Piece::WN as u8,
                    Piece::WR as u8,
                    Piece::WB as u8,
                ] {
                    self.add_capture_move(Move::new(from, to, cap, promo, 0), move_list);
                }
            } else {
                for &promo in &[
                    Piece::BQ as u8,
                    Piece::BN as u8,
                    Piece::BR as u8,
                    Piece::BB as u8,
                ] {
                    self.add_capture_move(Move::new(from, to, cap, promo, 0), move_list);
                }
            };
        } else {
            self.add_capture_move(Move::new(from, to, cap, Piece::Empty as u8, 0), move_list);
        }
    }

    fn add_pawn_move<const SIDE: u8>(&self, from: u8, to: u8, move_list: &mut MoveList) {
        debug_assert!(square_on_board(from));
        debug_assert!(square_on_board(to));
        let promo_rank = if SIDE == WHITE {
            Rank::Rank7 as usize
        } else {
            Rank::Rank2 as usize
        };
        if RANKS_BOARD[from as usize] == promo_rank {
            if SIDE == WHITE {
                for &promo in &[
                    Piece::WQ as u8,
                    Piece::WN as u8,
                    Piece::WR as u8,
                    Piece::WB as u8,
                ] {
                    self.add_quiet_move(
                        Move::new(from, to, Piece::Empty as u8, promo, 0),
                        move_list,
                    );
                }
            } else {
                for &promo in &[
                    Piece::BQ as u8,
                    Piece::BN as u8,
                    Piece::BR as u8,
                    Piece::BB as u8,
                ] {
                    self.add_quiet_move(
                        Move::new(from, to, Piece::Empty as u8, promo, 0),
                        move_list,
                    );
                }
            };
        } else {
            self.add_quiet_move(
                Move::new(from, to, Piece::Empty as u8, Piece::Empty as u8, 0),
                move_list,
            );
        }
    }

    fn generate_pawn_caps<const SIDE: u8>(&self, sq: u8, move_list: &mut MoveList) {
        let left_sq = if SIDE == WHITE { sq + 9 } else { sq - 9 };
        let right_sq = if SIDE == WHITE { sq + 11 } else { sq - 11 };
        if square_on_board(left_sq)
            && PIECE_COL[self.pieces[left_sq as usize] as usize] == Colour::Black
        {
            self.add_pawn_cap_move::<SIDE>(sq, left_sq, self.pieces[left_sq as usize], move_list);
        }
        if square_on_board(right_sq)
            && PIECE_COL[self.pieces[right_sq as usize] as usize] == Colour::Black
        {
            self.add_pawn_cap_move::<SIDE>(sq, right_sq, self.pieces[right_sq as usize], move_list);
        }
    }

    fn generate_ep<const SIDE: u8>(&self, sq: u8, move_list: &mut MoveList) {
        // this has a bug because epsq can be 99 as a default.
        let left_sq = if SIDE == WHITE { sq + 9 } else { sq - 9 };
        let right_sq = if SIDE == WHITE { sq + 11 } else { sq - 11 };
        if left_sq == self.ep_sq {
            self.add_capture_move(
                Move::new(
                    sq,
                    left_sq,
                    Piece::Empty as u8,
                    Piece::Empty as u8,
                    Move::EP_MASK,
                ),
                move_list,
            );
        }
        if right_sq == self.ep_sq {
            self.add_capture_move(
                Move::new(
                    sq,
                    right_sq,
                    Piece::Empty as u8,
                    Piece::Empty as u8,
                    Move::EP_MASK,
                ),
                move_list,
            );
        }
    }

    fn generate_pawn_forward<const SIDE: u8>(&self, sq: u8, move_list: &mut MoveList) {
        let start_rank: usize = if SIDE == WHITE {
            Rank::Rank2 as usize
        } else {
            Rank::Rank7 as usize
        };
        let offset_sq = if SIDE == WHITE { sq + 10 } else { sq - 10 };
        if self.pieces[sq as usize + 10] == Piece::Empty as u8 {
            self.add_pawn_move::<SIDE>(sq, offset_sq, move_list);
            let double_sq = if SIDE == WHITE { sq + 20 } else { sq - 20 };
            if RANKS_BOARD[sq as usize] == start_rank
                && self.pieces[double_sq as usize] == Piece::Empty as u8
            {
                self.add_quiet_move(
                    Move::new(
                        sq,
                        double_sq,
                        Piece::Empty as u8,
                        Piece::Empty as u8,
                        Move::PAWN_START_MASK,
                    ),
                    move_list,
                );
            }
        }
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    pub fn generate_all_moves(&self, move_list: &mut MoveList) {
        debug_assert!({
            self.check_validity();
            true
        });

        // white pawn moves
        if self.side == WHITE {
            for piece_num in 0..self.piece_num[Piece::WP as usize] {
                let sq = self.p_list[Piece::WP as usize][piece_num as usize];
                debug_assert!(square_on_board(sq));
                self.generate_pawn_forward::<{ WHITE }>(sq, move_list);
                self.generate_pawn_caps::<{ WHITE }>(sq, move_list);
                self.generate_ep::<{ WHITE }>(sq, move_list);
            }
        } else {
            for piece_num in 0..self.piece_num[Piece::BP as usize] {
                let sq = self.p_list[Piece::BP as usize][piece_num as usize];
                debug_assert!(square_on_board(sq));
                self.generate_pawn_forward::<{ BLACK }>(sq, move_list);
                self.generate_pawn_caps::<{ BLACK }>(sq, move_list);
                self.generate_ep::<{ BLACK }>(sq, move_list);
            }
        }

        let jumpers = if self.side == WHITE {
            &WHITE_JUMPERS
        } else {
            &BLACK_JUMPERS
        };
        for &piece in jumpers {
            let dirs = if piece == Piece::WN as u8 || piece == Piece::BN as u8 {
                &N_DIR
            } else {
                &K_DIR
            };
            for piece_num in 0..self.piece_num[piece as usize] {
                let sq = self.p_list[piece as usize][piece_num as usize];
                debug_assert!(square_on_board(sq));
                println!("Piece: {} on {}", PIECE_NAMES[piece as usize], square120_name(sq).unwrap());
                for &offset in dirs {
                    let t_sq = sq as isize + offset;
                    if offset_square_offboard(t_sq) {
                        continue;
                    }

                    // now safe to convert to u8
                    // as offset_square_offboard() is false
                    let t_sq: u8 = unsafe { t_sq.try_into().unwrap_unchecked() };

                    if self.pieces[t_sq as usize] != Piece::Empty as u8 {
                        if PIECE_COL[self.pieces[t_sq as usize] as usize] as u8 == self.side ^ 1 {
                            self.add_capture_move(
                                Move::new(
                                    sq,
                                    t_sq,
                                    self.pieces[t_sq as usize],
                                    Piece::Empty as u8,
                                    0,
                                ),
                                move_list,
                            );
                        }
                    } else {
                        self.add_quiet_move(
                            Move::new(
                                sq,
                                t_sq,
                                Piece::Empty as u8,
                                Piece::Empty as u8,
                                0,
                            ),
                            move_list,
                        );
                    }
                }
            }
        }

        let sliders = if self.side == WHITE {
            &WHITE_SLIDERS
        } else {
            &BLACK_SLIDERS
        };
        for &piece in sliders {
            debug_assert!(piece_valid(piece));
            let dirs: &[isize] = match piece {
                WB | BB => &B_DIR,
                WR | BR => &R_DIR,
                WQ | BQ => &Q_DIR,
                _ => unreachable!(),
            };
            for piece_num in 0..self.piece_num[piece as usize] {
                let sq = self.p_list[piece as usize][piece_num as usize];
                debug_assert!(square_on_board(sq));
                
                for &dir in dirs {
                    let mut slider = sq as isize + dir;
                    while !offset_square_offboard(slider) {
                        // now safe to convert to u8
                        // as offset_square_offboard() is false
                        let t_sq: u8 = unsafe { slider.try_into().unwrap_unchecked() };

                        if self.pieces[t_sq as usize] != Piece::Empty as u8 {
                            if PIECE_COL[self.pieces[t_sq as usize] as usize] as u8 == self.side ^ 1 {
                                self.add_capture_move(
                                    Move::new(
                                        sq,
                                        t_sq,
                                        self.pieces[t_sq as usize],
                                        Piece::Empty as u8,
                                        0,
                                    ),
                                    move_list,
                                );
                            }
                            break;
                        }
                        self.add_quiet_move(
                            Move::new(
                                sq,
                                t_sq,
                                Piece::Empty as u8,
                                Piece::Empty as u8,
                                0,
                            ),
                            move_list,
                        );
                        slider += dir;
                    }
                }
            }
        }

        // castling
        self.generate_castling_moves(move_list);
    }

    fn generate_castling_moves(&self, move_list: &mut MoveList) {
        if self.side == WHITE {
            if (self.castle_perm & Castling::WK as u8) != 0
            && self.pieces[Square120::F1 as usize] == Piece::Empty as u8
            && self.pieces[Square120::G1 as usize] == Piece::Empty as u8 
            && !self.sq_attacked(Square120::E1 as usize, BLACK)
            && !self.sq_attacked(Square120::F1 as usize, BLACK) {
                self.add_quiet_move(
                    Move::new(
                        Square120::E1 as u8,
                        Square120::G1 as u8,
                        Piece::Empty as u8,
                        Piece::Empty as u8,
                        Move::CASTLE_MASK,
                    ),
                    move_list,
                );
            }

            if (self.castle_perm & Castling::WQ as u8) != 0
            && self.pieces[Square120::D1 as usize] == Piece::Empty as u8
            && self.pieces[Square120::C1 as usize] == Piece::Empty as u8
            && self.pieces[Square120::B1 as usize] == Piece::Empty as u8
            && !self.sq_attacked(Square120::E1 as usize, BLACK)
            && !self.sq_attacked(Square120::D1 as usize, BLACK) {
                self.add_quiet_move(
                    Move::new(
                        Square120::E1 as u8,
                        Square120::C1 as u8,
                        Piece::Empty as u8,
                        Piece::Empty as u8,
                        Move::CASTLE_MASK,
                    ),
                    move_list,
                );
            }
        } else {
            if (self.castle_perm & Castling::BK as u8) != 0
            && self.pieces[Square120::F8 as usize] == Piece::Empty as u8
            && self.pieces[Square120::G8 as usize] == Piece::Empty as u8
            && !self.sq_attacked(Square120::E8 as usize, WHITE)
            && !self.sq_attacked(Square120::F8 as usize, WHITE) {
                self.add_quiet_move(
                    Move::new(
                        Square120::E8 as u8,
                        Square120::G8 as u8,
                        Piece::Empty as u8,
                        Piece::Empty as u8,
                        Move::CASTLE_MASK,
                    ),
                    move_list,
                );
            }

            if (self.castle_perm & Castling::BQ as u8) != 0
            && self.pieces[Square120::D8 as usize] == Piece::Empty as u8
            && self.pieces[Square120::C8 as usize] == Piece::Empty as u8
            && self.pieces[Square120::B8 as usize] == Piece::Empty as u8
            && !self.sq_attacked(Square120::E8 as usize, WHITE)
            && !self.sq_attacked(Square120::D8 as usize, WHITE) {
                self.add_quiet_move(
                    Move::new(
                        Square120::E8 as u8,
                        Square120::C8 as u8,
                        Piece::Empty as u8,
                        Piece::Empty as u8,
                        Move::CASTLE_MASK,
                    ),
                    move_list,
                );
            }
        }
    }
}

impl Display for Board {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        static PIECE_CHAR: [u8; 13] = *b".PNBRQKpnbrqk";
        static SIDE_CHAR: [u8; 3] = *b"wb-";
        static RANK_CHAR: [u8; 8] = *b"12345678";
        static FILE_CHAR: [u8; 8] = *b"abcdefgh";

        writeln!(f, "Game Board:")?;

        for rank in ((Rank::Rank1 as u8)..=(Rank::Rank8 as u8)).rev() {
            write!(f, "{} ", rank + 1)?;
            for file in (File::FileA as u8)..=(File::FileH as u8) {
                let sq = filerank_to_square(file, rank);
                let piece = self.pieces[sq as usize];
                write!(f, "{} ", PIECE_CHAR[piece as usize] as char)?;
            }
            writeln!(f)?;
        }

        writeln!(f, "  a b c d e f g h")?;
        writeln!(f, "side: {}", SIDE_CHAR[self.side as usize] as char)?;

        Ok(())
    }
}

impl Debug for Board {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self)?;
        writeln!(f, "ep-square: {}", self.ep_sq)?;
        writeln!(f, "castling: {:b}", self.castle_perm)?;
        writeln!(f, "fifty-move-counter: {}", self.fifty_move_counter)?;
        writeln!(f, "ply: {}", self.ply)?;
        writeln!(f, "hash: {:x}", self.key)?;
        write_bb(self.pawns[Colour::White as usize], f)?;
        writeln!(f)?;
        write_bb(self.pawns[Colour::Black as usize], f)?;
        writeln!(f)?;
        write_bb(self.pawns[Colour::Both as usize], f)?;
        Ok(())
    }
}

mod tests {
    #[test]
    fn read_fen_validity() {
        use super::*;
        let mut b = Board::new();
        b.set_from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        b.check_validity();
    }
}