use crate::config;
use crate::discord_bot::context::Context;
use chrono::{DateTime, Days, NaiveDate, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use twilight_model::channel::Message;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

const TRACKING_DURATION: Days = Days::new(365);

#[derive(Debug)]
pub struct ActivityTracker {
    activity_by_user: DashMap<Id<UserMarker>, Arc<Mutex<UserActivity>>>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct UserActivity {
    message_count_by_date: BTreeMap<NaiveDate, u32>,
    last_message_time: Option<DateTime<Utc>>,
}

impl ActivityTracker {
    pub fn instance() -> &'static ActivityTracker {
        static INSTANCE: OnceLock<ActivityTracker> = OnceLock::new();
        INSTANCE.get_or_init(|| ActivityTracker {
            activity_by_user: DashMap::new(),
        })
    }

    pub async fn on_message(
        &self,
        own_id: Id<UserMarker>,
        guild_id: Id<GuildMarker>,
        ctx: Context,
        message: Message,
    ) -> crate::Result<()> {
        if message.author.id == own_id {
            return Ok(());
        }

        let config = config::get();
        if !ctx
            .has_role(guild_id, message.author.id, config.special_roles.member)
            .await?
            && !ctx
                .has_role(
                    guild_id,
                    message.author.id,
                    config.special_roles.trial_member,
                )
                .await?
        {
            return Ok(());
        }

        if !self.activity_by_user.contains_key(&message.author.id) {
            self.activity_by_user.insert(
                message.author.id,
                Arc::new(Mutex::new(
                    Self::load_tracker_for_user(message.author.id).await?,
                )),
            );
        }

        let activity_for_user = self
            .activity_by_user
            .get(&message.author.id)
            .unwrap()
            .clone();
        let mut activity_for_user = activity_for_user.lock().await;

        let Some(timestamp) = DateTime::<Utc>::from_timestamp_micros(message.timestamp.as_micros())
        else {
            return Err(crate::Error::Other(format!(
                "Invalid message timestamp: {}",
                message.timestamp.iso_8601()
            )));
        };
        let date = timestamp.date_naive();
        let first_date = date - TRACKING_DURATION;

        let old_dates: Vec<_> = activity_for_user
            .message_count_by_date
            .range(..first_date)
            .map(|(&d, _)| d)
            .collect();
        for old_date in old_dates {
            activity_for_user.message_count_by_date.remove(&old_date);
        }

        *activity_for_user
            .message_count_by_date
            .entry(date)
            .or_default() += 1;
        activity_for_user.last_message_time = Some(timestamp);

        tokio::fs::create_dir_all("activity_trackers").await?;

        let json = serde_json::to_string(&*activity_for_user)?;
        crate::utils::write_safe(
            format!("activity_trackers/{}.json", message.author.id),
            format!("activity_trackers/{}.json_new", message.author.id),
            json.as_bytes(),
        )
        .await?;

        Ok(())
    }

    async fn load_tracker_for_user(user_id: Id<UserMarker>) -> crate::Result<UserActivity> {
        let json =
            match tokio::fs::read_to_string(format!("activity_trackers/{user_id}.json")).await {
                Ok(json) => json,
                Err(err) if err.kind() == ErrorKind::NotFound => return Ok(UserActivity::default()),
                Err(err) => return Err(err.into()),
            };
        Ok(serde_json::from_str(&json)?)
    }
}
