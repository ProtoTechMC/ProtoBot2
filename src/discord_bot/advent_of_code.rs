use crate::config;
use crate::discord_bot::context::Context;
use chrono::{DateTime, Datelike, Days, NaiveTime, Utc};
use tracing::{error, info};
use twilight_model::channel::ChannelType;

const POST_TIME: NaiveTime = NaiveTime::from_hms_opt(4, 45, 0).unwrap();

/// Every day at the post time, creates the advent of code thread for that day, if there is one.
pub(crate) async fn run(ctx: Context) {
    let mut last_post_time = Utc::now();
    loop {
        // the sleep could finish slightly early if the system clock is adjusted, so never schedule
        // the same post time twice
        let post_time = next_post_time(Utc::now().max(last_post_time));
        last_post_time = post_time;
        let sleep_duration = (post_time - Utc::now()).to_std().unwrap_or_default();
        tokio::time::sleep(sleep_duration).await;

        if post_time.month() != 12 {
            continue;
        }
        let day = post_time.day();
        let config = config::get();
        let Some(message) = config.advent_of_code_messages.get(day as usize - 1) else {
            continue;
        };

        if let Err(err) = create_thread(&ctx, day, message).await {
            error!("Failed to create advent of code thread for day {day}: {err}");
        }
    }
}

/// Gets the first post time strictly after `now`.
fn next_post_time(now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.date_naive().and_time(POST_TIME).and_utc();
    if today > now {
        today
    } else {
        today + Days::new(1)
    }
}

async fn create_thread(ctx: &Context, day: u32, message: &str) -> crate::Result<()> {
    let channel_id = config::get().special_channels.advent_of_code;
    let name = format!("Day {day}");

    let thread = ctx
        .http
        .create_thread(channel_id, &name, ChannelType::PublicThread)
        .await?
        .model()
        .await?;
    ctx.send_message(thread.id, message).await?;

    info!("Created advent of code thread for day {day}");
    Ok(())
}
