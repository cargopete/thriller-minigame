use std::sync::Arc;

use anyhow::{Context, Result};
use vesper_ai::client::AnthropicClient;
use vesper_ui::app::App;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .context("ANTHROPIC_API_KEY environment variable not set")?;

    let client = Arc::new(AnthropicClient::new(api_key));

    let mut terminal = ratatui::init();
    let result = App::new().run(&mut terminal, client).await;
    ratatui::restore();
    result
}
