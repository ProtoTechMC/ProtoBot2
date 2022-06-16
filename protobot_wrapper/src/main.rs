use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn main() {
    loop {
        if let Err(err) = std::fs::set_permissions("./protobot", Permissions::from_mode(0o777)) {
            eprintln!("Failed to set permissions: {}", err);
            break;
        }
        let result = Command::new("./protobot")
            .status()
            .expect("Failed to execute protobot process");
        if !result.success() {
            eprintln!("protobot exited with exit code {}", result);
            break;
        }
        if let Err(err) = std::fs::remove_file("protobot") {
            eprintln!("Failed to remove old executable: {}", err);
            break;
        }
        if let Err(err) = std::fs::rename("protobot_updated", "protobot") {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Error moving protobot_updated over protobot: {}", err);
            }
            break;
        }
    }
}
