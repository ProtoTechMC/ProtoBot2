use ed25519_dalek::{Keypair, Signer};
use std::env;
use std::process::abort;

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() <= 3 {
        eprintln!("protobot_sender <url> <file> <keypair>");
        abort();
    }

    let keypair = Keypair::from_bytes(&base64::decode(&args[3]).expect("Invalid base 64"))
        .expect("Invalid keypair");

    let file = std::fs::read(&args[2]).unwrap();
    let signature = keypair.sign(&file);

    match reqwest::blocking::Client::new()
        .put(&args[1])
        .header("Signature", signature.to_string())
        .body(file)
        .send()
    {
        Ok(response) => {
            if response.status().is_success() {
                println!("{}", response.text().unwrap_or_else(|_| "OK".to_owned()));
            } else {
                eprintln!(
                    "HTTP {}: {}",
                    response.status(),
                    response.text().unwrap_or_else(|_| "Error".to_owned())
                );
                abort();
            }
        }
        Err(err) => {
            eprintln!("{}", err);
            abort();
        }
    }
}
