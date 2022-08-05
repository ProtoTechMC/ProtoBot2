use crate::discord_bot::guild_storage::GuildStorage;
use serde::{Deserialize, Serialize};
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::{GuildId, UserId};
use std::collections::HashMap;
use std::ops::{Add, AddAssign, Sub, SubAssign};

async fn print_usage(
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let prefix = GuildStorage::get(guild_id).await.command_prefix.clone();
    message
        .reply(
            ctx,
            format!(
                r#"```\n
                {prefix}chess start <@opponent>\n
                {prefix}chess [move] <theMove>\n
                {prefix}chess resign\n
                {prefix}chess board\n
                {prefix}chess help\n
                {prefix}chess option <flip> <value>\n
                ```"#
            ),
        )
        .await?;
    Ok(())
}

async fn display_name(guild_id: GuildId, ctx: &Context, user: UserId) -> String {
    guild_id
        .member(ctx, user)
        .await
        .ok()
        .map(|member| member.display_name().into_owned())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

async fn print_board(
    state: &mut ChessState,
    game: &ChessGame,
    include_to_move_message: bool,
    user: UserId,
    ctx: &Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let fen = game
        .board
        .iter()
        .rev()
        .map(|row| {
            let mut row_fen = String::new();
            let mut empty_squares_count = 0;
            for piece in row {
                match piece {
                    None => empty_squares_count += 1,
                    Some(piece) => {
                        if empty_squares_count != 0 {
                            row_fen.push_str(&empty_squares_count.to_string());
                            empty_squares_count = 0;
                        }
                        #[rustfmt::skip]
                        let piece_char = match piece {
                            Piece { typ: PieceType::Pawn, black: false } => 'P',
                            Piece { typ: PieceType::Rook, black: false } => 'R',
                            Piece { typ: PieceType::Knight, black: false } => 'N',
                            Piece { typ: PieceType::Bishop, black: false } => 'B',
                            Piece { typ: PieceType::Queen, black: false } => 'Q',
                            Piece { typ: PieceType::King, black: false } => 'K',
                            Piece { typ: PieceType::Pawn, black: true } => 'p',
                            Piece { typ: PieceType::Rook, black: true } => 'r',
                            Piece { typ: PieceType::Knight, black: true } => 'n',
                            Piece { typ: PieceType::Bishop, black: true } => 'b',
                            Piece { typ: PieceType::Queen, black: true } => 'q',
                            Piece { typ: PieceType::King, black: true } => 'k',
                        };
                        row_fen.push(piece_char);
                    }
                }
            }
            if empty_squares_count != 0 {
                row_fen.push_str(&empty_squares_count.to_string());
            }
            row_fen
        })
        .collect::<Vec<_>>()
        .join("/");

    let mut url = format!(
        "https://backscattering.de/web-boardimage/board.png?fen={}",
        fen
    );
    if state.options.entry(user).or_default().flip {
        if user == game.user_white {
            url.push_str("&orientation=white");
        } else {
            url.push_str("&orientation=black");
        }
    }
    if game.last_move.0.is_valid() {
        let (from, to) = game.last_move;
        url.push_str("&last_move=");
        url.push(char::from_u32('a' as u32 + from.x() as u32).unwrap());
        url.push(char::from_u32('1' as u32 + from.y() as u32).unwrap());
        url.push(char::from_u32('a' as u32 + to.x() as u32).unwrap());
        url.push(char::from_u32('1' as u32 + to.y() as u32).unwrap());
    }
    // TODO: checked king

    message
        .channel_id
        .send_message(ctx, move |new_message| {
            new_message
                .reference_message(message)
                .embed(move |embed| embed.image(url))
        })
        .await?;

    if include_to_move_message {
        if game.black_to_move {
            message
                .channel_id
                .send_message(ctx, |new_message| {
                    new_message.content(format!("Black to move <@{}>", game.user_black))
                })
                .await?;
        } else {
            message
                .channel_id
                .send_message(ctx, |new_message| {
                    new_message.content(format!("White to move <@{}>", game.user_white))
                })
                .await?;
        }
    }

    Ok(())
}

async fn start_game(
    user_a: UserId,
    user_b: UserId,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    if user_a == user_b {
        message.reply(ctx, "You cannot play yourself!").await?;
        return Ok(());
    }

    let mut storage = GuildStorage::get_mut(guild_id).await;
    if storage.chess_state.get_game(user_a).is_some() {
        message
            .reply(
                &ctx,
                format!(
                    "{} is already in a game",
                    display_name(guild_id, &ctx, user_a).await
                ),
            )
            .await?;
        storage.discard();
        return Ok(());
    }
    if storage.chess_state.get_game(user_b).is_some() {
        message
            .reply(
                &ctx,
                format!(
                    "{} is already in a game",
                    display_name(guild_id, &ctx, user_b).await
                ),
            )
            .await?;
        storage.discard();
        return Ok(());
    }

    let (white, black) = if rand::random::<bool>() {
        (user_a, user_b)
    } else {
        (user_b, user_a)
    };

    let mut game = ChessGame {
        user_white: white,
        user_black: black,
        black_to_move: false,
        castle_state: (
            CastleState {
                queenside: true,
                kingside: true,
            },
            CastleState {
                queenside: true,
                kingside: true,
            },
        ),
        en_passant_square: Square::invalid(),
        board: Default::default(),
        promote_piece: None,
        last_move: (Square::invalid(), Square::invalid()),
    };

    let majors = [
        PieceType::Rook,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Queen,
        PieceType::King,
        PieceType::Bishop,
        PieceType::Knight,
        PieceType::Rook,
    ];
    for x in 0..8 {
        game.board[0][x] = Some(Piece {
            typ: majors[x],
            black: false,
        });
        game.board[1][x] = Some(Piece {
            typ: PieceType::Pawn,
            black: false,
        });
        game.board[6][x] = Some(Piece {
            typ: PieceType::Pawn,
            black: true,
        });
        game.board[7][x] = Some(Piece {
            typ: majors[x],
            black: true,
        });
    }

    print_board(
        &mut storage.chess_state,
        &game,
        true,
        game.user_white,
        &ctx,
        message,
    )
    .await?;

    storage.chess_state.games.push(game);

    storage.save().await;

    Ok(())
}

pub(crate) async fn run(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let args: Vec<&str> = args.split(' ').collect();
    if args.is_empty() {
        return print_usage(guild_id, ctx, message).await;
    }

    match args[0] {
        "start" => {
            if args.len() != 2 {
                return print_usage(guild_id, ctx, message).await;
            }
            let opponent = match args[1]
                .strip_prefix("<@")
                .and_then(|arg| arg.strip_suffix('>'))
                .and_then(|arg| arg.parse().ok())
            {
                Some(id) => UserId(id),
                None => return print_usage(guild_id, ctx, message).await,
            };
            return start_game(message.author.id, opponent, guild_id, ctx, message).await;
        }
        _ => {}
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(transparent)]
struct Square(u8);

impl Square {
    fn new(x: u8, y: u8) -> Self {
        Square(x | (y << 4))
    }

    fn invalid() -> Self {
        Square(0x88)
    }

    fn is_valid(&self) -> bool {
        (self.0 & 0x88) == 0
    }

    fn is_invalid(&self) -> bool {
        !self.is_valid()
    }

    fn x(&self) -> u8 {
        self.0 & 3
    }

    fn y(&self) -> u8 {
        (self.0 >> 4) & 3
    }

    fn get<'a>(&self, board: &'a Board) -> &'a Option<Piece> {
        &board[self.y() as usize][self.x() as usize]
    }

    fn get_mut<'a>(&self, board: &'a mut Board) -> &'a mut Option<Piece> {
        &mut board[self.y() as usize][self.x() as usize]
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
enum PieceType {
    #[serde(rename = "R")]
    Rook,
    #[serde(rename = "N")]
    Knight,
    #[serde(rename = "B")]
    Bishop,
    #[serde(rename = "Q")]
    Queen,
    #[serde(rename = "K")]
    King,
    #[serde(rename = "P")]
    Pawn,
}

#[derive(Debug, Deserialize, Serialize)]
struct Piece {
    typ: PieceType,
    black: bool,
}

type Board = [[Option<Piece>; 8]; 8];

impl Add for Square {
    type Output = Square;

    fn add(self, rhs: Square) -> Square {
        Square(self.0.wrapping_add(rhs.0))
    }
}

impl AddAssign for Square {
    fn add_assign(&mut self, rhs: Square) {
        *self = *self + rhs;
    }
}

impl Sub for Square {
    type Output = Square;

    fn sub(self, rhs: Square) -> Square {
        Square(self.0.wrapping_sub(rhs.0))
    }
}

impl SubAssign for Square {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ChessState {
    games: Vec<ChessGame>,
    options: HashMap<UserId, ChessOptions>,
}

impl ChessState {
    fn get_game(&self, user_id: UserId) -> Option<&ChessGame> {
        self.games
            .iter()
            .find(|game| game.user_white == user_id || game.user_black == user_id)
    }

    fn get_game_mut(&mut self, user_id: UserId) -> Option<&mut ChessGame> {
        self.games
            .iter_mut()
            .find(|game| game.user_white == user_id || game.user_black == user_id)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ChessGame {
    user_white: UserId,
    user_black: UserId,
    black_to_move: bool,
    castle_state: (CastleState, CastleState),
    #[serde(
        skip_serializing_if = "Square::is_invalid",
        default = "Square::invalid"
    )]
    en_passant_square: Square,
    board: Board,
    #[serde(skip)]
    promote_piece: Option<PieceType>,
    last_move: (Square, Square),
}

#[derive(Debug, Deserialize, Serialize)]
struct CastleState {
    queenside: bool,
    kingside: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChessOptions {
    flip: bool,
}

impl Default for ChessOptions {
    fn default() -> Self {
        Self { flip: true }
    }
}
