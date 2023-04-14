use crate::{config, ProtobotData};
use log::{error, info};
use std::collections::HashSet;

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
    let admin_perms: HashSet<_> = config.panel_admin_perms.iter().collect();
    let panel_access_perms: HashSet<_> = if server == config.pterodactyl_smp {
        config.panel_access_smp_perms.iter().collect()
    } else {
        config.panel_access_ptero_perms.iter().collect()
    };

    let server = data.pterodactyl.get_server(server);
    let existing_users = server.list_users().await?;
    for existing_user in existing_users {
        if remaining_superadmins.remove(&existing_user.email) {
            if existing_user.permissions.iter().collect::<HashSet<_>>() != superadmin_perms {
                server
                    .set_user_permissions(existing_user.uuid, config.panel_superadmin_perms.clone())
                    .await?;
            }
        } else if remaining_admins.remove(&existing_user.email) {
            if existing_user.permissions.iter().collect::<HashSet<_>>() != admin_perms {
                server
                    .set_user_permissions(existing_user.uuid, config.panel_admin_perms.clone())
                    .await?;
            }
        } else if remaining_panel_access.remove(&existing_user.email) {
            if existing_user.permissions.iter().collect::<HashSet<_>>() != panel_access_perms {
                server
                    .set_user_permissions(
                        existing_user.uuid,
                        config.panel_access_ptero_perms.clone(),
                    )
                    .await?;
            }
        } else if !ignored_emails.contains(&existing_user.email) {
            server.delete_user(existing_user.uuid).await?;
        }
    }

    for superadmin in remaining_superadmins {
        server
            .add_user(superadmin, config.panel_superadmin_perms.clone())
            .await?;
    }
    for admin in remaining_admins {
        server
            .add_user(admin, config.panel_admin_perms.clone())
            .await?;
    }
    for user in remaining_panel_access {
        server
            .add_user(user, config.panel_access_ptero_perms.clone())
            .await?;
    }

    Ok(())
}
