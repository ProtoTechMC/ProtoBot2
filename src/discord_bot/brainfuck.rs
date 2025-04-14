use crate::discord_bot::guild_storage::GuildStorage;
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::GuildId;
use std::time::{Duration, Instant};

const MEM_LIMIT: usize = 65536;
const TIME_LIMIT: Duration = Duration::from_secs(5);

enum BrainfuckError {
    UnbalancedBrackets,
    OutOfMemory,
    TimeOut,
}

pub(crate) async fn run(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    if args.is_empty() {
        message
            .reply(
                ctx,
                format!(
                    "{}brainfuck <code> [ \"|\" <input> ]",
                    GuildStorage::get(guild_id).await.command_prefix
                ),
            )
            .await?;
        return Ok(());
    }

    let (brainfuck, input) = match args.find('|') {
        Some(index) => {
            let (brainfuck, input) = args.split_at(index);
            (brainfuck, input[1..].trim())
        }
        None => (args, ""),
    };

    let brainfuck: Vec<_> = brainfuck.chars().collect();

    let is_ascii = input
        .split(' ')
        .any(|int| int.trim().parse::<u8>().is_err());
    let input: Vec<_> = if is_ascii {
        input
            .chars()
            .map(|char| ((char as u32) % 256) as u8)
            .collect()
    } else {
        input
            .split(' ')
            .map(|int| int.trim().parse().unwrap())
            .collect()
    };

    let output = tokio::task::spawn_blocking(move || run_brainfuck(brainfuck, input)).await?;

    match output {
        Ok(output) => {
            if output.is_empty() {
                message
                    .reply(ctx, "Program terminated with no output")
                    .await?;
            } else {
                message
                    .reply(
                        &ctx,
                        output
                            .iter()
                            .map(|out| out.to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                    )
                    .await?;
                let output = output
                    .into_iter()
                    .map(|out| {
                        let char = char::from_u32(out as u32).unwrap();
                        if char.is_control() {
                            " ".to_owned()
                        } else {
                            char.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !output.chars().all(|char| char.is_whitespace()) {
                    message.reply(ctx, output.trim()).await?;
                }
            }
        }
        Err(BrainfuckError::UnbalancedBrackets) => {
            message.reply(ctx, "Unbalanced [] square brackets").await?;
        }
        Err(BrainfuckError::OutOfMemory) => {
            message.reply(ctx, "Out of memory").await?;
        }
        Err(BrainfuckError::TimeOut) => {
            message.reply(ctx, "Timed out").await?;
        }
    }

    Ok(())
}

fn run_brainfuck(brainfuck: Vec<char>, input: Vec<u8>) -> Result<Vec<u8>, BrainfuckError> {
    let mut pc = 0;
    let mut ptr = 0;
    let mut input_index = 0;
    let mut mem: Vec<u8> = Vec::new();
    let mut stack = Vec::new();
    let mut output = Vec::new();

    let start_time = Instant::now();
    let mut iter_count: u8 = 0;

    fn adjust_index(idx: isize, mem: &mut Vec<u8>) -> Result<isize, BrainfuckError> {
        if idx < 0 {
            let old_len = mem.len();
            let new_len = old_len + (-idx) as usize;
            if new_len > MEM_LIMIT {
                return Err(BrainfuckError::OutOfMemory);
            }
            mem.extend(std::iter::repeat_n(0, (-idx) as usize));
            mem.copy_within(0..old_len, (-idx) as usize);
            mem[0..(-idx) as usize].fill(0);
            Ok(0)
        } else {
            let additional_len = idx - mem.len() as isize + 1;
            if additional_len > 0 {
                let new_len = idx as usize + 1;
                if new_len > MEM_LIMIT {
                    return Err(BrainfuckError::OutOfMemory);
                }
                mem.extend(std::iter::repeat_n(0, additional_len as usize));
            }
            Ok(idx)
        }
    }

    while pc < brainfuck.len() {
        iter_count = iter_count.wrapping_add(1);
        if iter_count == 0 {
            let elapsed = start_time.elapsed();
            if elapsed > TIME_LIMIT {
                return Err(BrainfuckError::TimeOut);
            }
        }

        let mut increment_pc = true;
        let ch = brainfuck[pc];
        match ch {
            '>' => ptr += 1,
            '<' => ptr -= 1,
            '+' => {
                ptr = adjust_index(ptr, &mut mem)?;
                mem[ptr as usize] = mem[ptr as usize].wrapping_add(1);
            }
            '-' => {
                ptr = adjust_index(ptr, &mut mem)?;
                mem[ptr as usize] = mem[ptr as usize].wrapping_sub(1);
            }
            '.' => {
                ptr = adjust_index(ptr, &mut mem)?;
                output.push(mem[ptr as usize]);
            }
            ',' => {
                let val = if input_index >= input.len() {
                    0
                } else {
                    let val = input[input_index];
                    input_index += 1;
                    val
                };
                ptr = adjust_index(ptr, &mut mem)?;
                mem[ptr as usize] = val;
            }
            '[' => {
                ptr = adjust_index(ptr, &mut mem)?;
                if mem[ptr as usize] != 0 {
                    stack.push(pc);
                } else {
                    let mut bracket_count: usize = 1;
                    while bracket_count > 0 {
                        pc += 1;
                        if pc == brainfuck.len() {
                            return Err(BrainfuckError::UnbalancedBrackets);
                        }
                        if brainfuck[pc] == '[' {
                            bracket_count += 1;
                        } else if brainfuck[pc] == ']' {
                            bracket_count -= 1;
                        }
                    }
                }
            }
            ']' => {
                pc = match stack.pop() {
                    Some(pc) => pc,
                    None => return Err(BrainfuckError::UnbalancedBrackets),
                };
                increment_pc = false;
            }
            _ => {}
        }

        if increment_pc {
            pc += 1;
        }
    }

    if !stack.is_empty() {
        return Err(BrainfuckError::UnbalancedBrackets);
    }

    Ok(output)
}
