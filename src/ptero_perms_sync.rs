use crate::{config, ProtobotData};
use log::{error, info};
use pterodactyl_api::client::Server;
use std::collections::HashSet;
use uuid::Uuid;

pub(crate) async fn run(
    data: &ProtobotData,
    mut args: impl Iterator<Item = &str>,
) -> Result<(), crate::Error> {
    let Some(server) = args.next() else {
        error!("Missing server argument");
        return Ok(());
    };

    let config = config::get();

    if server == "all" {
        for server in &config.pterodactyl_server_ids {
            run_on_server(data, server).await?;
        }
    } else {
        run_on_server(data, server).await?;
    }

    info!("Successfully synced perms");

    Ok(())
}

async fn run_on_server(data: &ProtobotData, server: &str) -> Result<(), crate::Error> {
    let config = config::get();

    let mut remaining_superadmins: HashSet<_> = config.panel_superadmin_emails.iter().collect();
    let mut remaining_admins: HashSet<_> = config.panel_admin_emails.iter().collect();
    let mut remaining_panel_access: HashSet<_> = config.panel_access_emails.iter().collect();
    let ignored_emails: HashSet<_> = config.panel_ignore_emails.iter().collect();

    let superadmin_perms: HashSet<_> = config.panel_superadmin_perms.iter().collect();
    let admin_perms: HashSet<_> = if server == config.pterodactyl_self {
        config.panel_admin_self_perms.iter().collect()
    } else {
        config.panel_admin_perms.iter().collect()
    };
    let panel_access_perms: HashSet<_> = if server == config.pterodactyl_smp {
        config.panel_access_smp_perms.iter().collect()
    } else if server == config.pterodactyl_self {
        HashSet::new()
    } else {
        config.panel_access_ptero_perms.iter().collect()
    };

    let server = data.pterodactyl.get_server(server);
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
) -> Result<(), crate::Error> {
    if perms.is_empty() {
        server.delete_user(user_uuid).await?;
    } else {
        server
            .set_user_permissions(user_uuid, perms.iter().map(|str| (*str).clone()).collect())
            .await?;
    }
    Ok(())
}
