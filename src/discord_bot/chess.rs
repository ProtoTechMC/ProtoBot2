use crate::discord_bot::guild_storage::GuildStorage;
use log::warn;
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::{char, one_of};
use nom::combinator::{eof, map, not, opt, value};
use nom::sequence::{pair, preceded, terminated, tuple};
use nom::{Finish, IResult};
use serde::{Deserialize, Serialize};
use serenity::builder::{CreateEmbed, CreateMessage};
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::{GuildId, UserId};
use std::collections::HashMap;
use std::ops::{Add, AddAssign, Sub, SubAssign};
// ===== USER INTERACTION ===== //

async fn print_usage(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let prefix = GuildStorage::get(guild_id).await.command_prefix.clone();
    message
        .reply(
            ctx,
            format!(
                "```\n\
                {prefix}chess start <@opponent>\n\
                {prefix}chess [move] <theMove>\n\
                {prefix}chess resign\n\
                {prefix}chess board\n\
                {prefix}chess help\n\
                {prefix}chess option <flip> <value>\n\
                ```"
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
        .map(|member| member.display_name().to_owned())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

async fn print_board(
    options: &ChessOptions,
    game: &mut ChessGame,
    include_to_move_message: bool,
    user: UserId,
    ctx: &Context,
    message: &Message,
) -> crate::Result<()> {
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
    if options.flip {
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

    let checked_king = get_checked_king(game);
    if checked_king.is_valid() {
        url.push_str("&check=");
        url.push(char::from_u32('a' as u32 + checked_king.x() as u32).unwrap());
        url.push(char::from_u32('1' as u32 + checked_king.y() as u32).unwrap());
    }

    message
        .channel_id
        .send_message(
            ctx,
            CreateMessage::new()
                .reference_message(message)
                .embed(CreateEmbed::new().image(url)),
        )
        .await?;

    if include_to_move_message {
        if game.black_to_move {
            message
                .channel_id
                .send_message(
                    ctx,
                    CreateMessage::new().content(format!("Black to move <@{}>", game.user_black)),
                )
                .await?;
        } else {
            message
                .channel_id
                .send_message(
                    ctx,
                    CreateMessage::new().content(format!("White to move <@{}>", game.user_white)),
                )
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
) -> crate::Result<()> {
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
    for (x, &major) in majors.iter().enumerate() {
        game.board[0][x] = Some(Piece {
            typ: major,
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
            typ: major,
            black: true,
        });
    }

    let user = game.user_white;
    print_board(
        storage.chess_state.options.entry(user).or_default(),
        &mut game,
        true,
        user,
        &ctx,
        message,
    )
    .await?;

    storage.chess_state.games.push(game);

    storage.save().await;

    Ok(())
}

async fn resign_game(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let mut storage = GuildStorage::get_mut(guild_id).await;
    let resigner = message.author.id;
    let resigned_game = match storage
        .chess_state
        .games
        .iter()
        .enumerate()
        .find(|(_, game)| game.user_white == resigner || game.user_black == resigner)
    {
        Some((index, _)) => storage.chess_state.games.remove(index),
        None => {
            message.reply(ctx, "You aren't in a game").await?;
            storage.discard();
            return Ok(());
        }
    };

    let opponent = if resigner == resigned_game.user_white {
        resigned_game.user_black
    } else {
        resigned_game.user_white
    };

    message
        .reply(
            &ctx,
            format!(
                "{} resigned, <@{}> wins!",
                display_name(guild_id, &ctx, resigner).await,
                opponent
            ),
        )
        .await?;

    storage.save().await;

    Ok(())
}

async fn set_option(
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
    key: &str,
    value: &str,
) -> crate::Result<()> {
    let mut storage = GuildStorage::get_mut(guild_id).await;
    let options = storage
        .chess_state
        .options
        .entry(message.author.id)
        .or_default();
    match (key, value) {
        ("flip", "false") => options.flip = false,
        ("flip", "true") => options.flip = true,
        ("flip", _) => {
            message
                .reply(ctx, "Invalid option value for \"flip\"")
                .await?;
            storage.discard();
            return Ok(());
        }
        _ => {
            message
                .reply(
                    ctx,
                    format!(
                        "Invalid option name. Type {}chess help for a list of options.",
                        storage.command_prefix
                    ),
                )
                .await?;
            storage.discard();
            return Ok(());
        }
    }
    message
        .reply(
            &ctx,
            format!(
                "Option \"{}\" set to {} for {}",
                key,
                value,
                display_name(guild_id, &ctx, message.author.id).await
            ),
        )
        .await?;
    storage.save().await;
    Ok(())
}

async fn do_move(
    mov: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let mut storage = GuildStorage::get_mut(guild_id).await;
    let (game, options) = match storage
        .chess_state
        .get_game_and_options_mut(message.author.id)
    {
        Some(game_and_options) => game_and_options,
        None => {
            storage.discard();
            message.reply(ctx, "You aren't in a game").await?;
            return Ok(());
        }
    };

    let (black, opponent) = if message.author.id == game.user_white {
        (false, game.user_black)
    } else {
        (true, game.user_white)
    };
    if black != game.black_to_move {
        storage.discard();
        message.reply(ctx, "It's not your turn").await?;
        return Ok(());
    }

    let mov = match parse_move_with_context(mov, game) {
        Ok(mov) => mov,
        Err(msg) => {
            storage.discard();
            message.reply(ctx, msg).await?;
            return Ok(());
        }
    };

    let piece = match mov
        .src
        .get(&game.board)
        .filter(|piece| piece.black == black)
    {
        Some(piece) => piece,
        None => {
            storage.discard();
            message.reply(ctx, "Invalid piece").await?;
            return Ok(());
        }
    };

    let mut new_state = game.clone();

    new_state.promote_piece = mov.promote_piece;

    if !piece.move_piece(mov.src, mov.dst, &mut new_state, false) {
        storage.discard();
        message.reply(ctx, "Invalid move").await?;
        return Ok(());
    }

    if get_checked_king(&mut new_state).is_valid() {
        storage.discard();
        message.reply(ctx, "Illegal move").await?;
        return Ok(());
    }

    new_state.last_move = (mov.src, mov.dst);
    new_state.black_to_move = !game.black_to_move;

    *game = new_state;

    let win_state = detect_win_state(game);

    if let Err(err) = print_board(
        options,
        game,
        win_state == WinState::None,
        opponent,
        &ctx,
        message,
    )
    .await
    {
        storage.discard();
        return Err(err);
    }

    if win_state != WinState::None {
        if let Some((index, _)) = storage
            .chess_state
            .games
            .iter()
            .enumerate()
            .find(|(_, game)| {
                game.user_white == message.author.id || game.user_black == message.author.id
            })
        {
            storage.chess_state.games.remove(index);
        }
    }

    storage.save().await;

    match win_state {
        WinState::Checkmate => {
            message
                .reply(
                    &ctx,
                    format!(
                        "Checkmate! {} wins! <@{}> lost.",
                        display_name(guild_id, &ctx, message.author.id).await,
                        opponent
                    ),
                )
                .await?;
        }
        WinState::Stalemate => {
            message
                .reply(
                    &ctx,
                    format!(
                        "Stalemate. Draw between {} and <@{}>.",
                        display_name(guild_id, &ctx, message.author.id).await,
                        opponent
                    ),
                )
                .await?;
        }
        _ => {}
    }

    Ok(())
}

pub(crate) async fn run(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if args.is_empty() {
        return print_usage(guild_id, ctx, message).await;
    }
    let args: Vec<&str> = args.split(' ').collect();

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
                Some(id) => UserId::new(id),
                None => return print_usage(guild_id, ctx, message).await,
            };
            return start_game(message.author.id, opponent, guild_id, ctx, message).await;
        }
        "resign" => return resign_game(guild_id, ctx, message).await,
        "help" | "?" => return print_usage(guild_id, ctx, message).await,
        "board" => {
            let mut storage = GuildStorage::get_mut(guild_id).await;
            if let Some((game, options)) = storage
                .chess_state
                .get_game_and_options_mut(message.author.id)
            {
                print_board(options, game, true, message.author.id, &ctx, message).await?;
                storage.save().await;
            } else {
                message.reply(ctx, "You aren't in a game").await?;
                storage.discard();
            }
        }
        "option" => {
            if args.len() != 3 {
                return print_usage(guild_id, ctx, message).await;
            }
            return set_option(guild_id, ctx, message, args[1], args[2]).await;
        }
        "move" => {
            if args.len() == 1 {
                return print_usage(guild_id, ctx, message).await;
            }
            return do_move(&args[1..].join(" "), guild_id, ctx, message).await;
        }
        _ => {
            return do_move(&args.join(" "), guild_id, ctx, message).await;
        }
    }

    Ok(())
}

// ===== PIECE MOVEMENT ===== //

fn move_pawn(src: Square, dst: Square, black: bool, game: &mut ChessGame, simulate: bool) -> bool {
    let board = &mut game.board;
    let dir = if black { -1 } else { 1 };
    let add = |pos: Square, dx: u8, dy: u8| {
        if black {
            Square::new(pos.x().wrapping_add(dx), pos.y().wrapping_sub(dy))
        } else {
            Square::new(pos.x().wrapping_add(dx), pos.y().wrapping_add(dy))
        }
    };

    if dst.y() as i8 != src.y() as i8 + dir {
        if dst.x() != src.x() {
            return false;
        }
        if dst.y() as i8 != src.y() as i8 + dir * 2 {
            return false;
        }
        if src.y() != (if black { 6 } else { 1 }) {
            return false;
        }
        if add(src, 0, 1).get(board).is_some() || add(src, 0, 2).get(board).is_some() {
            return false;
        }
        if !simulate {
            game.en_passant_square = dst;
        }
    } else if dst.x() == src.x() {
        if dst.get(board).is_some() {
            return false;
        }
        if !simulate {
            game.en_passant_square = Square::invalid();
        }
    } else {
        if src.x().abs_diff(dst.x()) != 1 {
            return false;
        }
        match dst.get(board) {
            None => {
                let eps = Square::new(dst.x(), src.y());
                if game.en_passant_square != eps {
                    return false;
                }
                if !simulate {
                    *eps.get_mut(board) = None;
                }
            }
            Some(dst_piece) => {
                if dst_piece.black == black {
                    return false;
                }
            }
        }
        if !simulate {
            game.en_passant_square = Square::invalid();
        }
    }

    if !simulate {
        let piece = if dst.y() == 0 || dst.y() == 7 {
            Piece {
                typ: game.promote_piece.unwrap(),
                black,
            }
        } else {
            Piece {
                typ: PieceType::Pawn,
                black,
            }
        };
        *dst.get_mut(board) = Some(piece);
        *src.get_mut(board) = None;
    }

    true
}

fn move_rook(src: Square, dst: Square, black: bool, game: &mut ChessGame, simulate: bool) -> bool {
    let board = &mut game.board;
    if src.y() == dst.y() {
        let min_x = src.x().min(dst.x());
        let max_x = src.x().max(dst.x());
        for x in min_x + 1..max_x {
            if Square::new(x, src.y()).get(board).is_some() {
                return false;
            }
        }
    } else if src.x() == dst.x() {
        let min_y = src.y().min(dst.y());
        let max_y = src.y().max(dst.y());
        for y in min_y + 1..max_y {
            if Square::new(src.x(), y).get(board).is_some() {
                return false;
            }
        }
    } else {
        return false;
    }
    let dst_piece = dst.get_mut(board);
    if let Some(dst_piece) = dst_piece {
        if dst_piece.black == black {
            return false;
        }
    }

    if !simulate {
        *dst_piece = Some(Piece {
            typ: PieceType::Rook,
            black,
        });
        *src.get_mut(board) = None;

        match (src.x(), src.y()) {
            (0, 0) => game.castle_state.0.queenside = false,
            (0, 7) => game.castle_state.1.queenside = false,
            (7, 0) => game.castle_state.0.kingside = false,
            (7, 7) => game.castle_state.1.kingside = false,
            _ => {}
        }

        game.en_passant_square = Square::invalid();
    }

    true
}

fn move_knight(
    src: Square,
    dst: Square,
    black: bool,
    game: &mut ChessGame,
    simulate: bool,
) -> bool {
    let board = &mut game.board;
    if src.x() == dst.x() || src.y() == dst.y() {
        return false;
    }
    if src.x().abs_diff(dst.x()) + src.y().abs_diff(dst.y()) != 3 {
        return false;
    }
    let dst_piece = dst.get_mut(board);
    if let Some(dst_piece) = dst_piece {
        if dst_piece.black == black {
            return false;
        }
    }

    if !simulate {
        *dst_piece = Some(Piece {
            typ: PieceType::Knight,
            black,
        });
        *src.get_mut(board) = None;
        game.en_passant_square = Square::invalid();
    }

    true
}

fn move_bishop_or_queen(
    src: Square,
    dst: Square,
    black: bool,
    game: &mut ChessGame,
    simulate: bool,
    queen: bool,
) -> bool {
    let board = &mut game.board;
    let dx = dst.x() as i8 - src.x() as i8;
    let dy = dst.y() as i8 - src.y() as i8;
    if dx.abs() != dy.abs() && (!queen || (dx != 0 && dy != 0)) {
        return false;
    }

    let dir_x = dx.signum();
    let dir_y = dy.signum();
    for delta in 1..dx.abs().max(dy.abs()) {
        if Square::new(
            (src.x() as i8 + dir_x * delta) as u8,
            (src.y() as i8 + dir_y * delta) as u8,
        )
        .get(board)
        .is_some()
        {
            return false;
        }
    }

    let dst_piece = dst.get_mut(board);
    if let Some(dst_piece) = dst_piece {
        if dst_piece.black == black {
            return false;
        }
    }

    if !simulate {
        *dst_piece = Some(Piece {
            typ: if queen {
                PieceType::Queen
            } else {
                PieceType::Bishop
            },
            black,
        });
        *src.get_mut(board) = None;
        game.en_passant_square = Square::invalid();
    }

    true
}

fn move_bishop(
    src: Square,
    dst: Square,
    black: bool,
    game: &mut ChessGame,
    simulate: bool,
) -> bool {
    move_bishop_or_queen(src, dst, black, game, simulate, false)
}

fn move_queen(src: Square, dst: Square, black: bool, game: &mut ChessGame, simulate: bool) -> bool {
    move_bishop_or_queen(src, dst, black, game, simulate, true)
}

fn move_king(src: Square, dst: Square, black: bool, game: &mut ChessGame, simulate: bool) -> bool {
    if src.y().abs_diff(dst.y()) > 1 {
        return false;
    }
    let dx = dst.x() as i8 - src.x() as i8;
    if dx.abs() > 2 {
        return false;
    }

    if let Some(dst_piece) = dst.get(&game.board) {
        if dst_piece.black == black {
            return false;
        }
    }

    if dx.abs() == 2 {
        if src.y() != dst.y() {
            return false;
        }
        let castle_state = if black {
            game.castle_state.1
        } else {
            game.castle_state.0
        };
        if dx < 0 {
            if !castle_state.queenside {
                return false;
            }
            if Square::new(1, src.y()).get(&game.board).is_some() {
                return false;
            }
        } else if !castle_state.kingside {
            return false;
        }

        if dst.get(&game.board).is_some() {
            return false;
        }
        let middle_square = Square::new((src.x() as i8 + dx / 2) as u8, src.y());
        if middle_square.get(&game.board).is_some() {
            return false;
        }
        if is_piece_attacked(game, src) {
            return false;
        }
        *middle_square.get_mut(&mut game.board) = Some(Piece {
            typ: PieceType::King,
            black,
        });
        *src.get_mut(&mut game.board) = None;
        let attacked = is_piece_attacked(game, src);
        *middle_square.get_mut(&mut game.board) = None;
        *src.get_mut(&mut game.board) = Some(Piece {
            typ: PieceType::King,
            black,
        });
        if attacked {
            return false;
        }

        if !simulate {
            *middle_square.get_mut(&mut game.board) = Some(Piece {
                typ: PieceType::Rook,
                black,
            });
            *Square::new(if dx < 0 { 0 } else { 7 }, src.y()).get_mut(&mut game.board) = None;
        }
    }

    if !simulate {
        *dst.get_mut(&mut game.board) = Some(Piece {
            typ: PieceType::King,
            black,
        });
        *src.get_mut(&mut game.board) = None;
        if black {
            game.castle_state.1.kingside = false;
            game.castle_state.1.queenside = false;
        } else {
            game.castle_state.0.kingside = false;
            game.castle_state.0.queenside = false;
        }
        game.en_passant_square = Square::invalid();
    }

    true
}

// ===== GAME LOGIC ===== //

fn is_piece_attacked(game: &mut ChessGame, pos: Square) -> bool {
    let black = match pos.get(&game.board) {
        Some(piece) => piece.black,
        None => return false,
    };
    for x in 0..8 {
        for y in 0..8 {
            let other_pos = Square::new(x, y);
            if let Some(piece) = other_pos.get(&game.board) {
                if piece.black != black && piece.move_piece(other_pos, pos, game, true) {
                    return true;
                }
            }
        }
    }
    false
}

fn get_checked_king(game: &mut ChessGame) -> Square {
    for x in 0..8 {
        for y in 0..8 {
            let pos = Square::new(x, y);
            if let Some(piece) = pos.get(&game.board) {
                if piece.typ == PieceType::King && piece.black == game.black_to_move {
                    return if is_piece_attacked(game, pos) {
                        pos
                    } else {
                        Square::invalid()
                    };
                }
            }
        }
    }
    Square::invalid()
}

#[derive(Debug, Eq, PartialEq)]
enum WinState {
    None,
    Checkmate,
    Stalemate,
}

fn detect_win_state(game: &mut ChessGame) -> WinState {
    for from_x in 0..8 {
        for from_y in 0..8 {
            let src = Square::new(from_x, from_y);
            if let Some(piece) = src
                .get(&game.board)
                .filter(|piece| piece.black == game.black_to_move)
            {
                for to_x in 0..8 {
                    for to_y in 0..8 {
                        let dst = Square::new(to_x, to_y);
                        if src != dst && piece.move_piece(src, dst, game, true) {
                            let mut game_copy = game.clone();
                            piece.move_piece(src, dst, &mut game_copy, false);
                            if get_checked_king(&mut game_copy).is_invalid() {
                                return WinState::None;
                            }
                        }
                    }
                }
            }
        }
    }

    if get_checked_king(game).is_valid() {
        WinState::Checkmate
    } else {
        WinState::Stalemate
    }
}

// ===== STRUCTS ===== //

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
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
        self.0 & 7
    }

    fn y(&self) -> u8 {
        (self.0 >> 4) & 7
    }

    fn get(&self, board: &Board) -> Option<Piece> {
        board[self.y() as usize][self.x() as usize]
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
struct Piece {
    typ: PieceType,
    black: bool,
}

impl Piece {
    fn move_piece(self, src: Square, dst: Square, game: &mut ChessGame, simulate: bool) -> bool {
        match self.typ {
            PieceType::Pawn => move_pawn(src, dst, self.black, game, simulate),
            PieceType::Rook => move_rook(src, dst, self.black, game, simulate),
            PieceType::Knight => move_knight(src, dst, self.black, game, simulate),
            PieceType::Bishop => move_bishop(src, dst, self.black, game, simulate),
            PieceType::Queen => move_queen(src, dst, self.black, game, simulate),
            PieceType::King => move_king(src, dst, self.black, game, simulate),
        }
    }
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

    fn get_game_and_options_mut(
        &mut self,
        user_id: UserId,
    ) -> Option<(&mut ChessGame, &mut ChessOptions)> {
        self.games
            .iter_mut()
            .find(|game| game.user_white == user_id || game.user_black == user_id)
            .map(|game| (game, self.options.entry(user_id).or_default()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
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

// ===== MOVE PARSING ===== //

struct ResolvedMove {
    src: Square,
    dst: Square,
    promote_piece: Option<PieceType>,
}

fn parse_move_with_context(mov: &str, game: &mut ChessGame) -> Result<ResolvedMove, &'static str> {
    let mov = parse_move(mov).map_err(|_| "Invalid syntax")?;
    let (src, dst) = match mov.pos {
        MovePos::QueensideCastle => {
            let legal = if game.black_to_move {
                game.castle_state.1.queenside
            } else {
                game.castle_state.0.queenside
            };
            if !legal {
                return Err("Illegal move");
            }
            let y = if game.black_to_move { 7 } else { 0 };
            (Square::new(4, y), Square::new(2, y))
        }
        MovePos::KingsideCastle => {
            let legal = if game.black_to_move {
                game.castle_state.1.kingside
            } else {
                game.castle_state.0.kingside
            };
            if !legal {
                return Err("Illegal move");
            }
            let y = if game.black_to_move { 7 } else { 0 };
            (Square::new(4, y), Square::new(6, y))
        }
        MovePos::FromTo { src, dst } => (src, dst),
        MovePos::WithPiece {
            typ: piece_type,
            dst,
            src_x,
            src_y,
        } => {
            let mut found_src = Square::invalid();
            for x in 0..8 {
                if src_x.map(|x1| x1 == x) != Some(false) {
                    for y in 0..8 {
                        if src_y.map(|y1| y1 == y) != Some(false) {
                            let src = Square::new(x, y);
                            if let Some(piece) = src.get(&game.board) {
                                if piece
                                    == (Piece {
                                        typ: piece_type,
                                        black: game.black_to_move,
                                    })
                                    && piece.move_piece(src, dst, game, true)
                                {
                                    if found_src.is_valid() {
                                        return Err("Ambiguous move");
                                    }
                                    found_src = src;
                                }
                            }
                        }
                    }
                }
            }
            if found_src.is_invalid() {
                return Err("Invalid move");
            }
            (found_src, dst)
        }
    };

    if src == dst {
        return Err("Invalid move");
    }

    let mut needs_promotion = false;
    if let Some(piece) = src.get(&game.board) {
        if piece.typ == PieceType::Pawn && (dst.y() == 0 || dst.y() == 7) {
            needs_promotion = true;
        }
    }

    if needs_promotion && mov.promote_piece.is_none() {
        return Err("Don't know what to promote to");
    } else if !needs_promotion && mov.promote_piece.is_some() {
        return Err("Unexpected promote piece");
    }

    Ok(ResolvedMove {
        src,
        dst,
        promote_piece: mov.promote_piece,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MovePos {
    WithPiece {
        typ: PieceType,
        dst: Square,
        src_x: Option<u8>,
        src_y: Option<u8>,
    },
    FromTo {
        src: Square,
        dst: Square,
    },
    KingsideCastle,
    QueensideCastle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Move {
    pos: MovePos,
    promote_piece: Option<PieceType>,
}

fn parse_move(mov: &str) -> Result<Move, ()> {
    alt((parse_standard_move, parse_simple_move))(mov)
        .finish()
        .map(|(_, mov)| mov)
        .map_err(|err| {
            warn!("{:?}", err);
        })
}

fn parse_standard_move(mov: &str) -> IResult<&str, Move> {
    fn parse_dst(mov: &str) -> IResult<&str, (u8, u8)> {
        preceded(opt(one_of("xX")), pair(parse_file, parse_rank))(mov)
    }

    map(
        tuple((
            alt((
                value(MovePos::QueensideCastle, tag("0-0-0")),
                value(MovePos::KingsideCastle, tag("0-0")),
                map(pair(parse_file, parse_rank), |(x, y)| MovePos::WithPiece {
                    typ: PieceType::Pawn,
                    dst: Square::new(x, y),
                    src_x: None,
                    src_y: None,
                }),
                map(
                    tuple((
                        not(char('B')),
                        parse_file,
                        one_of("xX"),
                        parse_file,
                        parse_rank,
                    )),
                    |(_, from_x, _, to_x, to_y)| MovePos::WithPiece {
                        typ: PieceType::Pawn,
                        dst: Square::new(to_x, to_y),
                        src_x: Some(from_x),
                        src_y: None,
                    },
                ),
                map(
                    tuple((
                        one_of("pPrRnNbBqQkK"),
                        opt(preceded(
                            not(parse_dst),
                            alt((
                                map(parse_file, |file| (false, file)),
                                map(parse_rank, |rank| (true, rank)),
                            )),
                        )),
                        parse_dst,
                    )),
                    |(piece, from_xy, (x, y))| {
                        let piece_type = match piece {
                            'p' | 'P' => PieceType::Pawn,
                            'r' | 'R' => PieceType::Rook,
                            'n' | 'N' => PieceType::Knight,
                            'b' | 'B' => PieceType::Bishop,
                            'q' | 'Q' => PieceType::Queen,
                            'k' | 'K' => PieceType::King,
                            _ => unreachable!(),
                        };
                        let (from_x, from_y) = match from_xy {
                            Some((false, file)) => (Some(file), None),
                            Some((true, rank)) => (None, Some(rank)),
                            None => (None, None),
                        };
                        MovePos::WithPiece {
                            typ: piece_type,
                            dst: Square::new(x, y),
                            src_x: from_x,
                            src_y: from_y,
                        }
                    },
                ),
            )),
            parse_suffix,
            eof,
        )),
        |(pos, promote_piece, _)| Move { pos, promote_piece },
    )(mov)
}

fn parse_simple_move(mov: &str) -> IResult<&str, Move> {
    map(
        tuple((
            parse_file,
            parse_rank,
            opt(one_of(" -xX")),
            parse_file,
            parse_rank,
            parse_suffix,
            eof,
        )),
        |(from_x, from_y, _, to_x, to_y, promote_piece, _)| Move {
            pos: MovePos::FromTo {
                src: Square::new(from_x, from_y),
                dst: Square::new(to_x, to_y),
            },
            promote_piece,
        },
    )(mov)
}

fn parse_suffix(mov: &str) -> IResult<&str, Option<PieceType>> {
    terminated(
        opt(preceded(
            char('='),
            map(one_of("qQnNbBrR"), |char| match char {
                'q' | 'Q' => PieceType::Queen,
                'n' | 'N' => PieceType::Knight,
                'b' | 'B' => PieceType::Bishop,
                'r' | 'R' => PieceType::Rook,
                _ => unreachable!(),
            }),
        )),
        opt(alt((
            value((), tag("++")),
            value((), char('+')),
            value((), char('#')),
        ))),
    )(mov)
}

fn parse_file(mov: &str) -> IResult<&str, u8> {
    map(one_of("aAbBcCdDeEfFgGhH"), |char| {
        if ('a'..='h').contains(&char) {
            char as u8 - b'a'
        } else {
            char as u8 - b'A'
        }
    })(mov)
}

fn parse_rank(mov: &str) -> IResult<&str, u8> {
    map(one_of("12345678"), |char| (char as u8) - b'1')(mov)
}
