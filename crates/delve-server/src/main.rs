#![forbid(unsafe_code)]

use delve_providers::{CompletionProvider, EchoProvider, ProviderRequest};

fn main() -> Result<(), delve_providers::ProviderError> {
    let provider = EchoProvider;
    let response = provider.generate(&ProviderRequest {
        prompt: String::from("server-health-check"),
        thread_id: None,
    })?;

    println!("Delve server scaffold");
    println!("Provider response: {}", response.output);

    Ok(())
}
