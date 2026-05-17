use std::sync::Arc;

use anyhow::{Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, MultiSelect, Select};
use directories::ProjectDirs;

use vesper_ai::client::AnthropicClient;
use vesper_db::{Db, Player};
use vesper_ui::app::{App, NpcBrief};

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .context("ANTHROPIC_API_KEY not set")?;

    let db = open_db()?;
    let player = resolve_player(&db)?;

    let nearby = db
        .nearby_npcs(&player.location, 6)?
        .into_iter()
        .map(|n| NpcBrief { name: n.name, role: n.role })
        .collect::<Vec<_>>();
    let alive = db.alive_count()?;

    let client = Arc::new(AnthropicClient::new(api_key));
    let mut terminal = ratatui::init();
    let result = App::new(player.name, nearby, alive)
        .run(&mut terminal, client)
        .await;
    ratatui::restore();
    result
}

fn open_db() -> Result<Db> {
    let dirs = ProjectDirs::from("", "", "vesper")
        .context("cannot determine data directory")?;
    let data = dirs.data_dir();
    std::fs::create_dir_all(data)?;
    Db::open(&data.join("vesper.db"))
}

fn resolve_player(db: &Db) -> Result<Player> {
    if db.has_save()? {
        let player = db.load_player()?;
        println!("\n VESPER\n");
        let resume = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Resume as {}?", player.name))
            .default(true)
            .interact()?;
        if resume {
            return Ok(player);
        }
        db.wipe()?;
    }
    run_wizard(db)
}

fn run_wizard(db: &Db) -> Result<Player> {
    println!();
    println!("  A R R I V A L");
    println!();
    println!("  The road brought you here.");
    println!("  It will not take you back.");
    println!();
    println!("  Before you enter Ash Hollow, tell us who you are.");
    println!();

    let theme = ColorfulTheme::default();

    let name: String = Input::with_theme(&theme)
        .with_prompt("Name")
        .interact_text()?;

    let gender_opts = ["Man", "Woman", "Non-binary", "Prefer not to say"];
    let gender_idx = Select::with_theme(&theme)
        .with_prompt("Gender")
        .items(&gender_opts)
        .default(3)
        .interact()?;
    let gender = (gender_idx < 3).then(|| gender_opts[gender_idx].to_string());

    let age_str: String = Input::with_theme(&theme)
        .with_prompt("Age (leave blank to skip)")
        .allow_empty(true)
        .interact_text()?;
    let age: Option<i32> = age_str.trim().parse().ok().filter(|&n: &i32| n > 0);

    let interest_opts = [
        "Looking after someone",
        "Getting somewhere",
        "Finding answers",
        "Keeping people safe",
        "Making sense of things",
        "Fixing things",
        "Faith",
        "Medicine",
        "Music",
        "Keeping records",
    ];
    let picks = MultiSelect::with_theme(&theme)
        .with_prompt("What matters to you? (space to select, enter to confirm)")
        .items(&interest_opts)
        .interact()?;
    let interests: Vec<String> = picks.iter().map(|&i| interest_opts[i].to_string()).collect();

    let backstory_raw: String = Input::with_theme(&theme)
        .with_prompt("Last thing you remember before the road (leave blank to skip)")
        .allow_empty(true)
        .interact_text()?;
    let backstory = (!backstory_raw.trim().is_empty())
        .then(|| backstory_raw.trim().to_string());

    let player = Player {
        name,
        gender,
        age,
        interests,
        backstory,
        sanity: 80,
        location: "town".into(),
    };

    db.create_save(&player)?;
    Ok(player)
}
