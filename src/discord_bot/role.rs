use crate::discord_bot::commands::check_admin;
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use twilight_model::channel::Message;
use twilight_model::id::marker::{GuildMarker, RoleMarker};
use twilight_model::id::Id;

async fn print_usage(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    let prefix = &storage.command_prefix;
    ctx.reply(
        message,
        format!(
            "```\n\
                {prefix}role add <user> <role-name>\n\
                {prefix}role list-allowed\n\
                Admin only:\n\
                {prefix}role add-allowed <role-name> <role-id>\n\
                {prefix}role remove-allowed <role-name>\n\
                {prefix}role assigning-role [role-id]\n\
                The role-name in this command is arbitrary and is set by add-allowed.\n\
                The assigning role is the role that has permission to run these commands.\n\
                ```"
        ),
    )
    .await?;
    Ok(())
}

pub(crate) async fn run(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let args: Vec<_> = args.split_whitespace().collect();
    match args[0] {
        "add" => {
            if args.len() != 3 {
                return print_usage(guild_id, ctx, message).await;
            }
            let storage = GuildStorage::get(guild_id).await;

            // check permission
            if let Some(assigning_role) = storage.role_data.assigning_role {
                if !ctx
                    .has_role(guild_id, message.author.id, assigning_role)
                    .await?
                {
                    ctx.reply(message, "Insufficient permissions to perform this command")
                        .await?;
                    return Ok(());
                }
            }

            // try to assign the role
            let user = match args[1].parse() {
                Ok(user) => user,
                Err(_) => {
                    ctx.reply(message, "Invalid user id").await?;
                    return Ok(());
                }
            };
            let role = match storage.role_data.roles.get(&args[2].to_lowercase()) {
                Some(&role) => role,
                None => {
                    ctx.reply(message, "That role is not listed as allowed")
                        .await?;
                    return Ok(());
                }
            };

            let member = match ctx.member(guild_id, user).await {
                Ok(member) => member,
                Err(_) => {
                    ctx.reply(
                        message,
                        "A user with that ID does not exist in this server.",
                    )
                    .await?;
                    return Ok(());
                }
            };
            if ctx
                .http
                .add_guild_member_role(guild_id, user, role)
                .await
                .is_err()
            {
                ctx.reply(message, "Failed to add role. Check the role permissions.")
                    .await?;
                return Ok(());
            }

            // log that the role was added
            let message_result = ctx.reply(message, "Added role successfully.").await;
            if let Some(log_channel) = storage.log_channel {
                ctx.send_message(
                    log_channel,
                    format!(
                        "{} (ID {}) gave role {} (ID {}) to {} (ID {})",
                        message.author.name,
                        message.author.id,
                        ctx.role(guild_id, role)
                            .await
                            .map(|role| role.name)
                            .unwrap_or_else(|_| "<unknown>".to_string()),
                        role,
                        member.user.name,
                        user,
                    ),
                )
                .await?;
            }
            message_result?;
        }
        "list-allowed" => {
            let storage = GuildStorage::get(guild_id).await;
            let mut sorted_roles: Vec<_> = storage.role_data.roles.iter().collect();
            sorted_roles.sort_by_key(|(name, _)| *name);
            ctx.reply(
                message,
                sorted_roles
                    .into_iter()
                    .map(|(name, id)| format!("• {name}: {id}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .await?;
        }
        "add-allowed" => {
            if !check_admin(&ctx, message).await? {
                return Ok(());
            }
            if args.len() != 3 {
                return print_usage(guild_id, ctx, message).await;
            }
            let mut storage = GuildStorage::get_mut(guild_id).await;

            let role_name = args[1].to_lowercase();
            if storage.role_data.roles.contains_key(&role_name) {
                storage.discard();
                ctx.reply(
                    message,
                    "That role name is already assigned to something that is allowed.",
                )
                .await?;
                return Ok(());
            }

            let role_id = match args[2].parse() {
                Ok(role_id) => role_id,
                Err(_) => {
                    storage.discard();
                    ctx.reply(message, "Invalid role id").await?;
                    return Ok(());
                }
            };
            if ctx.role(guild_id, role_id).await.is_err() {
                storage.discard();
                ctx.reply(message, "No role found with that id").await?;
                return Ok(());
            }

            storage.role_data.roles.insert(role_name, role_id);
            storage.save().await;

            ctx.reply(message, "Successfully added allowed role.")
                .await?;
        }
        "remove-allowed" => {
            if !check_admin(&ctx, message).await? {
                return Ok(());
            }
            if args.len() != 2 {
                return print_usage(guild_id, ctx, message).await;
            }
            let role_name = args[1].to_lowercase();
            let mut storage = GuildStorage::get_mut(guild_id).await;
            match storage.role_data.roles.remove(&role_name) {
                Some(_) => {
                    storage.save().await;
                    ctx.reply(message, "Successfully removed allowed role.")
                        .await?;
                }
                None => {
                    storage.discard();
                    ctx.reply(message, "That role is not in the list of allowed roles.")
                        .await?;
                }
            }
        }
        "assigning-role" => {
            if !check_admin(&ctx, message).await? {
                return Ok(());
            }
            if args.len() != 2 {
                return print_usage(guild_id, ctx, message).await;
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;

            let role_id = match args[1].parse() {
                Ok(role_id) => role_id,
                Err(_) => {
                    storage.discard();
                    ctx.reply(message, "Invalid role id").await?;
                    return Ok(());
                }
            };
            if ctx.role(guild_id, role_id).await.is_err() {
                storage.discard();
                ctx.reply(message, "No role found with that id").await?;
                return Ok(());
            }

            storage.role_data.assigning_role = Some(role_id);
            storage.save().await;
            ctx.reply(message, "Successfully changed assigning role")
                .await?;
        }
        _ => {
            return print_usage(guild_id, ctx, message).await;
        }
    }

    Ok(())
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct RoleData {
    assigning_role: Option<Id<RoleMarker>>,
    roles: HashMap<String, Id<RoleMarker>>,
}
