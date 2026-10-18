use std::sync::Arc;
use twilight_cache_inmemory::InMemoryCache;
use twilight_http::Client;
use twilight_model::channel::message::{AllowedMentions, Embed, MentionType};
use twilight_model::channel::{Channel, Message};
use twilight_model::guild::{Member, Permissions, Role};
use twilight_model::id::marker::{
    ApplicationMarker, ChannelMarker, GuildMarker, RoleMarker, UserMarker,
};
use twilight_model::id::Id;
use twilight_model::user::User;

/// Shared handles to Discord, passed to everything that needs to talk to Discord.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub http: Arc<Client>,
    pub cache: Arc<InMemoryCache>,
    pub application_id: Id<ApplicationMarker>,
}

impl Context {
    /// Replies to a message without pinging the author of the message.
    pub async fn reply(&self, message: &Message, content: impl AsRef<str>) -> crate::Result<()> {
        self.do_reply(message, content.as_ref(), false).await
    }

    /// Replies to a message and pings the author of the message.
    pub async fn reply_ping(
        &self,
        message: &Message,
        content: impl AsRef<str>,
    ) -> crate::Result<()> {
        self.do_reply(message, content.as_ref(), true).await
    }

    async fn do_reply(&self, message: &Message, content: &str, ping: bool) -> crate::Result<()> {
        // By providing allowed_mentions, Discord disables all pings by default, so we need to
        // re-enable them
        let allowed_mentions = AllowedMentions {
            parse: vec![
                MentionType::Everyone,
                MentionType::Users,
                MentionType::Roles,
            ],
            replied_user: ping,
            ..Default::default()
        };
        self.http
            .create_message(message.channel_id)
            .content(content)
            .allowed_mentions(Some(&allowed_mentions))
            .reply(message.id)
            .await?;
        Ok(())
    }

    /// Replies to a message with an embed.
    pub async fn reply_embed(&self, message: &Message, embed: Embed) -> crate::Result<()> {
        self.http
            .create_message(message.channel_id)
            .embeds(&[embed])
            .reply(message.id)
            .await?;
        Ok(())
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: impl AsRef<str>,
    ) -> crate::Result<Message> {
        Ok(self
            .http
            .create_message(channel_id)
            .content(content.as_ref())
            .await?
            .model()
            .await?)
    }

    pub async fn send_dm(&self, user_id: Id<UserMarker>, content: &str) -> crate::Result<()> {
        let channel = self.create_dm_channel(user_id).await?;
        self.http
            .create_message(channel.id)
            .content(content)
            .await?;
        Ok(())
    }

    pub async fn create_dm_channel(&self, user_id: Id<UserMarker>) -> crate::Result<Channel> {
        Ok(self
            .http
            .create_private_channel(user_id)
            .await?
            .model()
            .await?)
    }

    pub async fn channel(&self, channel_id: Id<ChannelMarker>) -> crate::Result<Channel> {
        if let Some(channel) = self.cache.channel(channel_id) {
            return Ok(channel.clone());
        }
        Ok(self.http.channel(channel_id).await?.model().await?)
    }

    pub async fn guild_channels(&self, guild_id: Id<GuildMarker>) -> crate::Result<Vec<Channel>> {
        Ok(self.http.guild_channels(guild_id).await?.models().await?)
    }

    pub async fn member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> crate::Result<Member> {
        Ok(self
            .http
            .guild_member(guild_id, user_id)
            .await?
            .model()
            .await?)
    }

    pub async fn user(&self, user_id: Id<UserMarker>) -> crate::Result<User> {
        if let Some(user) = self.cache.user(user_id) {
            return Ok(user.clone());
        }
        Ok(self.http.user(user_id).await?.model().await?)
    }

    pub async fn role(
        &self,
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
    ) -> crate::Result<Role> {
        if let Some(role) = self.cache.role(role_id) {
            if role.guild_id() == guild_id {
                return Ok(role.resource().clone());
            }
        }
        Ok(self.http.role(guild_id, role_id).await?.model().await?)
    }

    pub async fn has_role(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        role_id: Id<RoleMarker>,
    ) -> crate::Result<bool> {
        if let Some(member) = self.cache.member(guild_id, user_id) {
            return Ok(member.roles().contains(&role_id));
        }
        Ok(self
            .member(guild_id, user_id)
            .await?
            .roles
            .contains(&role_id))
    }

    pub fn guild_name(&self, guild_id: Id<GuildMarker>) -> Option<String> {
        self.cache
            .guild(guild_id)
            .map(|guild| guild.name().to_owned())
    }

    /// Gets the display name of a member, from the cache if possible.
    pub async fn display_name(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> crate::Result<String> {
        if let Some(name) = cached_display_name(&self.cache, guild_id, user_id) {
            return Ok(name);
        }
        Ok(member_display_name(&self.member(guild_id, user_id).await?).to_owned())
    }

    /// Whether the given member has the administrator permission in the given guild.
    pub fn is_admin(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        roles: &[Id<RoleMarker>],
    ) -> bool {
        let Some(guild) = self.cache.guild(guild_id) else {
            return false;
        };
        if guild.owner_id() == user_id {
            return true;
        }
        drop(guild);

        // the @everyone role has the same ID as the guild
        std::iter::once(guild_id.cast())
            .chain(roles.iter().copied())
            .any(|role_id| {
                self.cache.role(role_id).is_some_and(|role| {
                    role.resource()
                        .permissions
                        .contains(Permissions::ADMINISTRATOR)
                })
            })
    }
}

pub(crate) fn cached_display_name(
    cache: &InMemoryCache,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
) -> Option<String> {
    let member = cache.member(guild_id, user_id)?;
    if let Some(nick) = member.nick() {
        return Some(nick.to_owned());
    }
    let user = cache.user(user_id)?;
    Some(user_display_name(&user).to_owned())
}

pub(crate) fn member_display_name(member: &Member) -> &str {
    member
        .nick
        .as_deref()
        .unwrap_or_else(|| user_display_name(&member.user))
}

pub(crate) fn user_display_name(user: &User) -> &str {
    user.global_name.as_deref().unwrap_or(&user.name)
}

pub(crate) fn avatar_url(user: &User) -> Option<String> {
    user.avatar.map(|avatar| {
        let ext = if avatar.is_animated() { "gif" } else { "webp" };
        format!(
            "https://cdn.discordapp.com/avatars/{}/{}.{}?size=1024",
            user.id, avatar, ext
        )
    })
}

pub(crate) fn default_avatar_url(user: &User) -> String {
    let index = if user.discriminator == 0 {
        (user.id.get() >> 22) % 6
    } else {
        u64::from(user.discriminator) % 5
    };
    format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
}

/// Replaces mentions in the message content with plain text, so that they are readable outside of
/// Discord and don't ping anyone.
pub(crate) fn content_safe(cache: &InMemoryCache, message: &Message) -> String {
    let mut result = String::with_capacity(message.content.len());
    let mut rest = message.content.as_str();
    while let Some(start) = rest.find('<') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let replacement = rest.find('>').and_then(|end| {
            let name = mention_replacement(cache, message, &rest[1..end])?;
            Some((name, end + 1))
        });
        match replacement {
            Some((name, len)) => {
                result.push_str(&name);
                rest = &rest[len..];
            }
            None => {
                result.push('<');
                rest = &rest[1..];
            }
        }
    }
    result.push_str(rest);
    result
        .replace("@everyone", "@\u{200B}everyone")
        .replace("@here", "@\u{200B}here")
}

fn mention_replacement(cache: &InMemoryCache, message: &Message, mention: &str) -> Option<String> {
    if let Some(role_id) = mention.strip_prefix("@&") {
        let role_id: Id<RoleMarker> = role_id.parse().ok()?;
        let name = cache
            .role(role_id)
            .map(|role| role.resource().name.clone())
            .unwrap_or_else(|| "deleted-role".to_owned());
        Some(format!("@{name}"))
    } else if let Some(user_id) = mention.strip_prefix('@') {
        let user_id: Id<UserMarker> = user_id.strip_prefix('!').unwrap_or(user_id).parse().ok()?;
        let name = message
            .mentions
            .iter()
            .find(|mention| mention.id == user_id)
            .map(|mention| {
                mention
                    .member
                    .as_ref()
                    .and_then(|member| member.nick.clone())
                    .unwrap_or_else(|| mention.name.clone())
            })
            .unwrap_or_else(|| "invalid-user".to_owned());
        Some(format!("@{name}"))
    } else if let Some(channel_id) = mention.strip_prefix('#') {
        let channel_id: Id<ChannelMarker> = channel_id.parse().ok()?;
        let name = cache
            .channel(channel_id)
            .and_then(|channel| channel.name.clone())
            .unwrap_or_else(|| "deleted-channel".to_owned());
        Some(format!("#{name}"))
    } else {
        None
    }
}
