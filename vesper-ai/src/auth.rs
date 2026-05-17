use reqwest::RequestBuilder;

/// Authentication mode for Anthropic API calls.
#[derive(Clone)]
pub enum Auth {
    /// Classic API key — header: `x-api-key: sk-ant-api03-...`
    ApiKey(String),
    /// Claude Code OAuth token — header: `Authorization: Bearer sk-ant-oat01-...`
    Bearer(String),
}

impl Auth {
    /// Attach the correct auth header to a request builder.
    pub fn apply(&self, req: RequestBuilder) -> RequestBuilder {
        match self {
            Auth::ApiKey(k) => req.header("x-api-key", k),
            Auth::Bearer(t) => req.header("Authorization", format!("Bearer {t}")),
        }
    }

    /// Resolve auth from environment, then config file, then Claude Code keychain.
    ///
    /// Priority:
    ///   1. `ANTHROPIC_API_KEY` env var      → `Auth::ApiKey`
    ///   2. `~/.config/vesper/api_key` file  → `Auth::ApiKey`
    ///   3. macOS keychain `Claude Code-credentials` → `Auth::Bearer`
    pub fn resolve() -> Option<Self> {
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            return Some(Auth::ApiKey(key));
        }
        if let Ok(home) = std::env::var("HOME") {
            let p = std::path::PathBuf::from(home).join(".config/vesper/api_key");
            if let Ok(raw) = std::fs::read_to_string(p) {
                let key = raw.trim().to_string();
                if !key.is_empty() {
                    return Some(Auth::ApiKey(key));
                }
            }
        }
        read_claude_keychain().map(Auth::Bearer)
    }
}

/// Read the Claude Code OAuth token from the macOS keychain.
/// Returns `None` on any failure (non-macOS, keychain missing, parse error).
fn read_claude_keychain() -> Option<String> {
    // `security` is macOS-only; silently skip on other platforms.
    #[cfg(not(target_os = "macos"))]
    return None;

    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let json = String::from_utf8(output.stdout).ok()?;
        let v: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
        v["claudeAiOauth"]["accessToken"]
            .as_str()
            .map(String::from)
    }
}
