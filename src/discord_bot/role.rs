use crate::discord_bot::commands::check_admin;
use crate::discord_bot::guild_storage::GuildStorage;
use serde::{Deserialize, Serialize};
use serenity::all::CreateMessage;
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::{GuildId, RoleId, UserId};
use std::collections::HashMap;

async fn print_usage(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    let prefix = &storage.command_prefix;
    message
        .reply(
            ctx,
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
    guild_id: GuildId,
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
                if !message
                    .author
                    .has_role(&ctx, guild_id, assigning_role)
                    .await?
                {
                    message
                        .reply(ctx, "Insufficient permissions to perform this command")
                        .await?;
                    return Ok(());
                }
            }

            // try to assign the role
            let user = match args[1].parse() {
                Ok(user) => user,
                Err(_) => {
                    message.reply(ctx, "Invalid user id").await?;
                    return Ok(());
                }
            };
            let user = UserId::new(user);
            let role = match storage.role_data.roles.get(&args[2].to_lowercase()) {
                Some(&role) => role,
                None => {
                    message
                        .reply(ctx, "That role is not listed as allowed")
                        .await?;
                    return Ok(());
                }
            };

            let member = match guild_id.member(&ctx, user).await {
                Ok(member) => member,
                Err(_) => {
                    message
                        .reply(ctx, "A user with that ID does not exist in this server.")
                        .await?;
                    return Ok(());
                }
            };
            if member.add_role(&ctx, role).await.is_err() {
                message
                    .reply(ctx, "Failed to add role. Check the role permissions.")
                    .await?;
                return Ok(());
            }

            // log that the role was added
            let message_result = message.reply(&ctx, "Added role successfully.").await;
            if let Some(log_channel) = storage.log_channel {
                log_channel
                    .send_message(
                        &ctx,
                        CreateMessage::new().content(format!(
                            "{} (ID {}) gave role {} (ID {}) to {} (ID {})",
                            message.author.name,
                            message.author.id,
                            guild_id
                                .role(&ctx, role)
                                .await
                                .map(|role| role.name)
                                .unwrap_or_else(|_| "<unknown>".to_string()),
                            role,
                            member.user.name,
                            user,
                        )),
                    )
                    .await?;
            }
            message_result?;
        }
        "list-allowed" => {
            let storage = GuildStorage::get(guild_id).await;
            let mut sorted_roles: Vec<_> = storage.role_data.roles.iter().collect();
            sorted_roles.sort_by_key(|(name, _)| *name);
            message
                .reply(
                    ctx,
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
                message
                    .reply(
                        ctx,
                        "That role name is already assigned to something that is allowed.",
                    )
                    .await?;
                return Ok(());
            }

            let role_id = match args[2].parse() {
                Ok(role_id) => role_id,
                Err(_) => {
                    storage.discard();
                    message.reply(ctx, "Invalid role id").await?;
                    return Ok(());
                }
            };
            let role_id = RoleId::new(role_id);
            if guild_id.role(&ctx, role_id).await.is_err() {
                storage.discard();
                message.reply(ctx, "No role found with that id").await?;
                return Ok(());
            }

            storage.role_data.roles.insert(role_name, role_id);
            storage.save().await;

            message
                .reply(ctx, "Successfully added allowed role.")
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
                    message
                        .reply(ctx, "Successfully removed allowed role.")
                        .await?;
                }
                None => {
                    storage.discard();
                    message
                        .reply(ctx, "That role is not in the list of allowed roles.")
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
                    message.reply(ctx, "Invalid role id").await?;
                    return Ok(());
                }
            };
            let role_id = RoleId::new(role_id);
            if guild_id.role(&ctx, role_id).await.is_err() {
                storage.discard();
                message.reply(ctx, "No role found with that id").await?;
                return Ok(());
            }

            storage.role_data.assigning_role = Some(role_id);
            storage.save().await;
            message
                .reply(ctx, "Successfully changed assigning role")
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
    assigning_role: Option<RoleId>,
    roles: HashMap<String, RoleId>,
}
