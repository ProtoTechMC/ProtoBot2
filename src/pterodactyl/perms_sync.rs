use crate::pterodactyl::PterodactylServer;
use crate::{config, ProtobotData};
use log::{error, info};
use pterodactyl_api::client::Server;
use std::collections::HashSet;
use uuid::Uuid;

pub(crate) async fn run(
    data: &ProtobotData,
    mut args: impl Iterator<Item = &str>,
) -> crate::Result<()> {
    let Some(server_name) = args.next() else {
        error!("Missing server argument");
        return Ok(());
    };

    let config = config::get();

    if server_name == "all" {
        for server in &config.pterodactyl_servers {
            if server.category.is_proto() {
                run_on_server(data, server).await?;
            }
        }
    } else {
        let Some(server) = config
            .pterodactyl_servers
            .iter()
            .find(|s| s.name == server_name)
        else {
            error!("Unknown server: {}", server_name);
            return Ok(());
        };
        if !server.category.is_proto() {
            error!("Cannot run perms sync on non-proto server: {}", server_name);
            return Ok(());
        }
        run_on_server(data, server).await?;
    }

    info!("Successfully synced perms");

    Ok(())
}

async fn run_on_server(data: &ProtobotData, server: &PterodactylServer) -> crate::Result<()> {
    let config = config::get();

    let mut remaining_superadmins: HashSet<_> =
        config.pterodactyl_emails.superadmin.iter().collect();
    let mut remaining_admins: HashSet<_> = config.pterodactyl_emails.admin.iter().collect();
    let mut remaining_panel_access: HashSet<_> = config.pterodactyl_emails.normal.iter().collect();
    let ignored_emails: HashSet<_> = config.pterodactyl_emails.ignore.iter().collect();

    let superadmin_perms: HashSet<_> = config
        .pterodactyl_perms
        .superadmin
        .get_perms(server.category)
        .iter()
        .collect();
    let admin_perms: HashSet<_> = config
        .pterodactyl_perms
        .admin
        .get_perms(server.category)
        .iter()
        .collect();
    let panel_access_perms: HashSet<_> = config
        .pterodactyl_perms
        .normal
        .get_perms(server.category)
        .iter()
        .collect();

    let server = data.pterodactyl.get_server(&server.id);
    let existing_users = server.list_users().await?;
    for existing_user in existing_users {
        if remaining_superadmins.remove(&existing_user.email) {
            if existing_user.permissions.iter().collect::<HashSet<_>>() != superadmin_perms {
                set_user_perms(&server, existing_user.uuid, &superadmin_perms).await?;
            }
        } else if remaining_admins.remove(&existing_user.email) {
            if existing_user.permissions.iter().collect::<HashSet<_>>() != admin_perms {
                set_user_perms(&server, existing_user.uuid, &admin_perms).await?;
            }
        } else if remaining_panel_access.remove(&existing_user.email) {
            if existing_user.permissions.iter().collect::<HashSet<_>>() != panel_access_perms {
                set_user_perms(&server, existing_user.uuid, &panel_access_perms).await?;
            }
        } else if !ignored_emails.contains(&existing_user.email) {
            server.delete_user(existing_user.uuid).await?;
        }
    }

    for superadmin in remaining_superadmins {
        if !superadmin_perms.is_empty() {
            server
                .add_user(
                    superadmin,
                    superadmin_perms.iter().map(|str| (*str).clone()).collect(),
                )
                .await?;
        }
    }
    for admin in remaining_admins {
        if !admin_perms.is_empty() {
            server
                .add_user(
                    admin,
                    admin_perms.iter().map(|str| (*str).clone()).collect(),
                )
                .await?;
        }
    }
    for user in remaining_panel_access {
        if !panel_access_perms.is_empty() {
            server
                .add_user(
                    user,
                    panel_access_perms
                        .iter()
                        .map(|str| (*str).clone())
                        .collect(),
                )
                .await?;
        }
    }

    Ok(())
}

async fn set_user_perms(
    server: &Server<'_>,
    user_uuid: Uuid,
    perms: &HashSet<&String>,
) -> crate::Result<()> {
    if perms.is_empty() {
        server.delete_user(user_uuid).await?;
    } else {
        server
            .set_user_permissions(user_uuid, perms.iter().map(|str| (*str).clone()).collect())
            .await?;
    }
    Ok(())
}
