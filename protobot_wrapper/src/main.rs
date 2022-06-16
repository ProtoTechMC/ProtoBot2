use std::process::Command;

fn main() {
    loop {
        let result = Command::new("protobot")
            .status()
            .expect("Failed to execute protobot process");
        if !result.success() {
            eprintln!("protobot exited with exit code {}", result);
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
