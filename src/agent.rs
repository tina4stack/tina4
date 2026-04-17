//! Tina4 Agent — LLM-powered coding assistant with multi-agent orchestration.
//!
//! Reads agent configs from `.tina4/agents/*/config.json` + `system.md`.
//! Serves an HTTP+SSE endpoint for the dev admin frontend.
//! Handles supervisor routing, plan creation, code generation, and tool execution.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::console::{icon_info, icon_ok, icon_play, icon_warn};

// ── Agent config structures ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub model: String,          // "thinking", "vision", "image-gen" — maps to user settings
    pub temperature: f32,
    pub max_tokens: u32,
    pub tools: Vec<String>,
    pub max_iterations: u32,
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub config: AgentConfig,
    pub system_prompt: String,
}

// ── Model settings (from dev admin) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    pub provider: String,
    pub model: String,
    pub url: String,
    #[serde(alias = "apiKey", default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSettings {
    pub thinking: ModelSettings,
    pub vision: ModelSettings,
    #[serde(rename = "imageGen")]
    pub image_gen: ModelSettings,
}

// ── Chat messages ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,           // "user", "assistant", "system"
    pub content: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,  // which agent generated this
}

// ── Escalation tracking ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Escalation {
    pub id: String,
    pub category: String,       // "uncommitted", "untested", "security", "convention"
    pub level: u8,              // 0=silent, 1=gentle, 2=concerned, 3=action
    pub message: String,
    pub first_seen: String,
    pub last_prompted: String,
    pub dismissed: bool,
    pub acted_on: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    pub id: String,
    pub timestamp: String,
    pub message: String,
    pub category: String,
    pub actions: Vec<ThoughtAction>,
    pub dismissed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtAction {
    pub label: String,
    pub action: String,         // "create_branch", "scaffold_tests", "show_fix", etc.
}

// ── Supervisor action (parsed from LLM JSON response) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorAction {
    pub action: String,         // "plan", "code", "respond", "analyze_image", "generate_image", "debug"
    #[serde(default)]
    pub delegate_to: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<String>>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

// ── LLM API types (OpenAI-compatible) ──

#[derive(Debug, Serialize)]
struct LlmRequest {
    model: String,
    messages: Vec<LlmMessage>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<LlmOptions>,
}

#[derive(Debug, Serialize)]
struct LlmOptions {
    num_ctx: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    choices: Vec<LlmChoice>,
}

#[derive(Debug, Deserialize)]
struct LlmChoice {
    message: LlmMessage,
}

// ── Default agent configs ──

const DEFAULT_AGENTS: &[(&str, &str, &str)] = &[
    ("supervisor", r#"{"model":"thinking","temperature":0.3,"max_tokens":2048,"tools":["list_routes","list_tables","project_info","file_list"],"max_iterations":1}"#,
     r#"You are Tina4, the AI coding assistant built into the Tina4 dev admin.

You are the supervisor. The developer chats with you directly. You understand their request, gather requirements, coordinate specialist agents, and steer the project from start to finish.

## Your Personality
You are direct, practical, and efficient. You ask only what matters. You never explain framework internals or list modules. You talk like a colleague who just gets things done.

## Communication Style
- Ask SHORT questions about what the USER needs, not technology choices
- Never list framework features or module names
- Focus on WHAT the user wants, not HOW you'll build it
- When executing a plan, give clear progress updates: "Step 2 of 5 done. Moving to the login page..."
- After completing work, summarize what was built in plain English

## CRITICAL: Gather Requirements First

When a developer says they want to build something, DO NOT immediately create a plan. Instead:
1. Ask clarifying questions to understand what they need
2. Keep asking until you have enough detail OR the developer says "just build it", "go ahead", "you decide"

## When to Stop Asking

Stop asking and act when:
- The developer says "go ahead", "build it", "just do it", "you decide"
- You have enough detail after 2-3 rounds of questions
- The request is simple enough (e.g. "add a health check endpoint")

## Steering the Project

You keep the big picture in mind:
- Remember what has been built so far in this conversation
- When executing a plan, work through it step by step — one task at a time
- After each task, briefly confirm what was done and what's next
- If something fails, handle it before moving on
- At the end of the plan, give a summary of everything that was built

## Rules
1. Gather requirements before planning
2. Always plan before coding — create plans in .tina4/plans/
3. Never reinvent what the framework provides
4. Keep questions concise — max 3-4 per round
5. If the developer provides a detailed spec upfront, skip questions and plan directly
6. NEVER show file paths, code, or technical jargon to the user

## Actions
Only respond with JSON when ready to delegate:
{"action": "plan", "delegate_to": "planner", "context": "detailed description with all gathered requirements"}
{"action": "code", "delegate_to": "coder", "context": "what to write", "files": ["path1", "path2"]}
{"action": "execute_plan", "delegate_to": "coder", "context": "plan file path to execute step by step"}
{"action": "analyze_image", "delegate_to": "vision"}
{"action": "generate_image", "delegate_to": "image-gen", "prompt": "what to generate"}
{"action": "debug", "delegate_to": "debug", "error": "the error message"}
{"action": "respond", "message": "your conversational response or questions"}

For questions and conversation, ALWAYS use:
{"action": "respond", "message": "your message here"}
"#),

    ("planner", r#"{"model":"thinking","temperature":0.2,"max_tokens":4096,"tools":["file_read","file_list","list_routes","list_tables"],"max_iterations":3}"#,
     r#"You are the Planner agent. You create simple plans that a non-technical person can understand.

## How to write a plan

Write a short numbered list of what will be built. Use plain English. No technical jargon.

Example:
1. Set up the database for storing contacts
2. Create a page where visitors fill in their name, email, and message
3. Save the submission to the database
4. Send an email notification to the site owner
5. Show a thank you message after submission

## RULES — follow these exactly

- NEVER mention file paths, file names, or directories
- NEVER mention code, classes, functions, methods, or APIs
- NEVER use tables or technical formatting
- NEVER say "Create migration", "Create ORM model", "Create route" — say what it DOES, not what it IS
- NEVER mention the framework by name
- NEVER say "ORM", "AutoCrud", "middleware", "endpoint", "schema", "migration"
- Write like you're explaining to someone who doesn't code
- Maximum 10 steps
- Each step is ONE simple sentence
- Start with an objective sentence before the numbered list
"#),

    ("coder", r#"{"model":"thinking","temperature":0.1,"max_tokens":4096,"tools":["file_read","file_write"],"max_iterations":10}"#,
     r#"You are the Coder agent for Tina4 projects. Write code that follows the plan exactly.

## CRITICAL: File Structure

All Tina4 projects use this structure — NEVER use Laravel, Django, Rails, or Express patterns:

```
project/
  app.py
  migrations/        ← SQL migration files (at project ROOT)
  src/
    routes/          ← route files (one per file)
    orm/             ← ORM model files (one per file)
    templates/       ← Frond HTML templates (.twig)
    seeds/           ← database seed files
```

NEVER create: app/, Controllers/, Models/, Views/, Database/, database/ folders.

## Python Route Example (src/routes/contact.py)

```python
from tina4_python import get, post
from tina4_python.core import response

@get("/contact")
async def get_contact(request, response):
    return response.html(template("contact.twig"))

@post("/contact")
async def post_contact(request, response):
    name = request.body.get("name", "")
    email = request.body.get("email", "")
    message = request.body.get("message", "")
    # save to database, send email, etc.
    return response.redirect("/contact?success=1")
```

## Python ORM Example (src/orm/Contact.py)

```python
from tina4_python.orm import fields, model

class Contact(model.Model):
    __table_name__ = "contacts"
    id = fields.AutoField(primary_key=True)
    name = fields.CharField(max_length=255)
    email = fields.CharField(max_length=255)
    message = fields.TextField()
    created_at = fields.DateTimeField(auto_now_add=True)
```

## Migration Example (migrations/001_create_contacts.sql)  ← at project ROOT

```sql
CREATE TABLE IF NOT EXISTS contacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name VARCHAR(255),
    email VARCHAR(255),
    message TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

## Template Example (src/templates/contact.twig)

```html
<form method="post" action="/contact">
    <input name="name" placeholder="Name" required>
    <input name="email" type="email" placeholder="Email" required>
    <textarea name="message" placeholder="Message" required></textarea>
    <button type="submit">Send</button>
</form>
```

## Rules
- ALWAYS use the src/ structure shown above
- NEVER create app/, Controllers/, Models/, Views/, Database/ folders
- One route per file, one model per file
- Return each file as: ## FILE: path/to/file
"#),

    ("vision", r#"{"model":"vision","temperature":0.3,"max_tokens":2048,"tools":[],"max_iterations":1}"#,
     r#"You are the Vision agent for Tina4 projects.

Your job: analyze images (screenshots, mockups, diagrams) and describe what you see in detail.

Describe:
- UI elements (buttons, forms, tables, navigation)
- Layout and structure
- Colors and styling
- Text content
- Suggested Tina4 implementation approach
"#),

    ("image-gen", r#"{"model":"image-gen","temperature":0.7,"max_tokens":256,"tools":[],"max_iterations":1}"#,
     r#"Generate images based on user descriptions."#),

    ("debug", r#"{"model":"thinking","temperature":0.2,"max_tokens":4096,"tools":["file_read","database_query"],"max_iterations":5}"#,
     r#"You are the Debug agent for Tina4 projects.

Your job: analyze errors, read the relevant source files, and suggest fixes.

## Process
1. Parse the error type and traceback
2. Read the file where the error occurred
3. Identify the root cause
4. Suggest a specific fix with code
5. If the fix requires file changes, describe them precisely
"#),
];

// ── Public API ──

/// Scaffold default agent configs into `.tina4/agents/`.
pub fn scaffold_agents(project_dir: &Path) {
    let agents_dir = project_dir.join(".tina4").join("agents");

    for (name, config_json, system_prompt) in DEFAULT_AGENTS {
        let agent_dir = agents_dir.join(name);
        let config_path = agent_dir.join("config.json");
        let prompt_path = agent_dir.join("system.md");

        if config_path.exists() && prompt_path.exists() {
            continue; // Don't overwrite existing configs
        }

        if let Err(e) = fs::create_dir_all(&agent_dir) {
            eprintln!("  {} Failed to create {}: {}", icon_warn(), agent_dir.display(), e);
            continue;
        }

        if !config_path.exists() {
            if let Err(e) = fs::write(&config_path, config_json) {
                eprintln!("  {} Failed to write {}: {}", icon_warn(), config_path.display(), e);
            }
        }

        if !prompt_path.exists() {
            if let Err(e) = fs::write(&prompt_path, system_prompt) {
                eprintln!("  {} Failed to write {}: {}", icon_warn(), prompt_path.display(), e);
            }
        }
    }

    // Create plans and chat directories
    let _ = fs::create_dir_all(project_dir.join(".tina4").join("plans"));
    let _ = fs::create_dir_all(project_dir.join(".tina4").join("chat").join("threads"));

    println!("  {} Agent configs scaffolded in .tina4/agents/", icon_ok());
}

/// Load all agents from `.tina4/agents/`.
pub fn load_agents(project_dir: &Path) -> Vec<Agent> {
    let agents_dir = project_dir.join(".tina4").join("agents");
    let mut agents = Vec::new();

    if !agents_dir.exists() {
        return agents;
    }

    if let Ok(entries) = fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }

            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let config_path = path.join("config.json");
            let prompt_path = path.join("system.md");

            let config: AgentConfig = match fs::read_to_string(&config_path) {
                Ok(s) => match serde_json::from_str(&s) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("  {} Bad config for agent '{}': {}", icon_warn(), name, e);
                        continue;
                    }
                },
                Err(_) => continue,
            };

            let system_prompt = fs::read_to_string(&prompt_path).unwrap_or_default();

            agents.push(Agent { name, config, system_prompt });
        }
    }

    agents
}

/// Load chat settings from `.tina4/chat/settings.json` or use defaults.
pub fn load_chat_settings(project_dir: &Path) -> ChatSettings {
    let path = project_dir.join(".tina4").join("chat").join("settings.json");
    if let Ok(s) = fs::read_to_string(&path) {
        if let Ok(settings) = serde_json::from_str(&s) {
            return settings;
        }
    }
    // Defaults — Tina4 Cloud (per-model-type endpoints, models fetched at runtime)
    ChatSettings {
        thinking: ModelSettings {
            provider: "tina4".into(),
            model: String::new(),
            url: "http://41.71.84.173:11437".into(),
            api_key: String::new(),
        },
        vision: ModelSettings {
            provider: "tina4".into(),
            model: String::new(),
            url: "http://41.71.84.173:11434".into(),
            api_key: String::new(),
        },
        image_gen: ModelSettings {
            provider: "tina4".into(),
            model: String::new(),
            url: "http://41.71.84.173:11436".into(),
            api_key: String::new(),
        },
    }
}

/// Save chat message to `.tina4/chat/history.json`.
pub fn save_message(project_dir: &Path, message: &ChatMessage) {
    let history_path = project_dir.join(".tina4").join("chat").join("history.json");
    let mut messages: Vec<ChatMessage> = if let Ok(s) = fs::read_to_string(&history_path) {
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        Vec::new()
    };
    messages.push(message.clone());
    let _ = fs::write(&history_path, serde_json::to_string_pretty(&messages).unwrap_or_default());
}

/// Load chat history from `.tina4/chat/history.json`.
pub fn load_history(project_dir: &Path) -> Vec<ChatMessage> {
    let path = project_dir.join(".tina4").join("chat").join("history.json");
    if let Ok(s) = fs::read_to_string(&path) {
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Fetch the first available model from an Ollama-compatible server.
async fn fetch_first_model(base_url: &str) -> Option<String> {
    let client = reqwest::Client::new();
    // Try Ollama /api/tags first
    if let Ok(resp) = client.get(format!("{}/api/tags", base_url)).send().await {
        if let Ok(text) = resp.text().await {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(models) = data["models"].as_array() {
                    if let Some(first) = models.first() {
                        let name = first["name"].as_str()
                            .or_else(|| first["model"].as_str())
                            .unwrap_or("");
                        if !name.is_empty() {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
    }
    // Try OpenAI /v1/models
    if let Ok(resp) = client.get(format!("{}/v1/models", base_url)).send().await {
        if let Ok(text) = resp.text().await {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(models) = data["data"].as_array() {
                    if let Some(first) = models.first() {
                        if let Some(id) = first["id"].as_str() {
                            return Some(id.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Make an LLM call (blocking, non-streaming).
pub async fn llm_call(
    settings: &ModelSettings,
    system_prompt: &str,
    messages: &[LlmMessage],
    max_tokens: u32,
    temperature: f32,
) -> Result<String, String> {
    let client = reqwest::Client::new();

    // If model is empty, auto-detect from the server
    let model_name = if settings.model.is_empty() {
        let base = settings.url.trim_end_matches('/');
        match fetch_first_model(base).await {
            Some(m) => m,
            None => return Err("No models available on the server. Check the URL.".into()),
        }
    } else {
        settings.model.clone()
    };

    let mut all_messages = Vec::new();
    if !system_prompt.is_empty() {
        all_messages.push(LlmMessage {
            role: "system".into(),
            content: system_prompt.into(),
        });
    }
    all_messages.extend_from_slice(messages);

    // For Ollama/custom providers, request larger context window
    let options = if settings.provider == "custom" || settings.provider == "tina4" {
        Some(LlmOptions { num_ctx: 32768 })
    } else {
        None
    };

    let body = LlmRequest {
        model: model_name,
        messages: all_messages,
        max_tokens,
        temperature,
        stream: None,
        options,
    };

    // Build full API URL from base URL + provider-specific path
    let base_url = settings.url.trim_end_matches('/');
    let api_url = match settings.provider.as_str() {
        "anthropic" => format!("{}/v1/messages", base_url),
        "openai" => format!("{}/v1/chat/completions", base_url),
        "tina4" => format!("{}/v1/chat/completions", base_url),
        _ => {
            // Custom — auto-detect: if URL already has /v1/ path, use as-is, otherwise append
            if base_url.contains("/v1/") || base_url.contains("/api/") {
                base_url.to_string()
            } else {
                format!("{}/v1/chat/completions", base_url)
            }
        }
    };

    let mut req = client.post(&api_url)
        .header("Content-Type", "application/json")
        .json(&body);

    // Add auth header based on provider
    if !settings.api_key.is_empty() {
        if settings.provider == "anthropic" {
            req = req.header("x-api-key", &settings.api_key)
                     .header("anthropic-version", "2023-06-01");
        } else {
            req = req.header("Authorization", format!("Bearer {}", settings.api_key));
        }
    }

    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("Read failed: {}", e))?;

    if !status.is_success() {
        return Err(format!("LLM API error {}: {}", status, &text[..text.len().min(200)]));
    }

    // Parse OpenAI-compatible response
    let parsed: LlmResponse = serde_json::from_str(&text)
        .map_err(|e| format!("Parse failed: {} — body: {}", e, &text[..text.len().min(200)]))?;

    parsed.choices.first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "No response content".into())
}

/// Parse supervisor LLM response into a structured action.
pub fn parse_supervisor_action(response: &str) -> Option<SupervisorAction> {
    // Try to extract JSON from the response (might be wrapped in markdown or text)
    let trimmed = response.trim();

    // Direct JSON
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).ok();
    }

    // JSON in code block
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7;
        if let Some(end) = trimmed[json_start..].find("```") {
            let json_str = trimmed[json_start..json_start + end].trim();
            return serde_json::from_str(json_str).ok();
        }
    }

    // JSON anywhere in text
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            let json_str = &trimmed[start..=end];
            return serde_json::from_str(json_str).ok();
        }
    }

    // Not a structured action — treat as direct response
    Some(SupervisorAction {
        action: "respond".into(),
        message: Some(response.to_string()),
        delegate_to: None,
        context: None,
        files: None,
        prompt: None,
        error: None,
    })
}

/// Load escalations from `.tina4/chat/escalations.json`.
pub fn load_escalations(project_dir: &Path) -> Vec<Escalation> {
    let path = project_dir.join(".tina4").join("chat").join("escalations.json");
    if let Ok(s) = fs::read_to_string(&path) {
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Save escalations to `.tina4/chat/escalations.json`.
pub fn save_escalations(project_dir: &Path, escalations: &[Escalation]) {
    let path = project_dir.join(".tina4").join("chat").join("escalations.json");
    let _ = fs::write(&path, serde_json::to_string_pretty(escalations).unwrap_or_default());
}

/// Load thoughts from `.tina4/chat/thoughts.json`.
pub fn load_thoughts(project_dir: &Path) -> Vec<Thought> {
    let path = project_dir.join(".tina4").join("chat").join("thoughts.json");
    if let Ok(s) = fs::read_to_string(&path) {
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Save a new thought.
pub fn save_thought(project_dir: &Path, thought: &Thought) {
    let path = project_dir.join(".tina4").join("chat").join("thoughts.json");
    let mut thoughts = load_thoughts(project_dir);
    thoughts.push(thought.clone());
    // Keep last 50 thoughts
    if thoughts.len() > 50 {
        thoughts = thoughts[thoughts.len() - 50..].to_vec();
    }
    let _ = fs::write(&path, serde_json::to_string_pretty(&thoughts).unwrap_or_default());
}

/// Short Tina4 framework cheat-sheet baked into the binary as a fallback.
/// Used when we can't find the full framework docs on disk. Keep it
/// dense — this is what gets prepended to every coder message.
const TINA4_FALLBACK_CONTEXT: &str = r#"# Tina4 framework cheat-sheet

You are working in a Tina4 project. Conventions:
- Routes: `from tina4_python.core.router import get, post, noauth, secured`. `@noauth` / `@secured` / `@description` go ABOVE `@get`/`@post`. Example: `@noauth()` then `@post("/api/x")` on the innermost decorator.
- Always `response({...})`. NEVER `response.json(...)`.
- Path params: `{id:int}`, `{price:float}`, `{rest:path}`.
- DB: `from tina4_python.database import Database`. `Database("sqlite:///app.db", ...)`. `db.fetch(sql,[...])` returns `DatabaseResult`; iterate `.records` (list of dicts). `fetch_one` returns dict-or-None. Dict access only: `row["name"]`, never `row.name`. Transactions: `db.start_transaction/commit/rollback` — NEVER `db.execute("COMMIT")`.
- ORM: one class per file in `src/orm/`. `IntegerField(primary_key=True, auto_increment=True)`, `StringField()`. `User.find(1)`, `User.where("age>?",[18])`, `user.save()`.
- Migrations: REQUIRED for schema. `tina4 generate migration "create x"` then `tina4 migrate`. Never raw DDL outside migrations. SQLite uses `INTEGER PRIMARY KEY AUTOINCREMENT`; PostgreSQL `SERIAL`; MySQL `AUTO_INCREMENT`.
- Templates (Frond/Jinja2): `{% extends "base.twig" %}`. `{% elif %}` not `{% elseif %}`. `{{ x|raw }}` for unescaped. `{{ "a " ~ b }}` for string concat (NOT `+`). Always include `{{ form_token() }}` in forms and `placeholder` on every input.
- .env: `DATABASE_URL=sqlite:///app.db`, `TINA4_DEBUG=true`, `SECRET=...`, `TINA4_TOKEN_LIMIT=60`.
- Built-ins — never reinvent: `Queue(topic="x").push({...})` for background work, `Api(base_url, auth_header)` for HTTP, `Auth.hash_password/check_password` for passwords, `get_token/valid_token` for JWT, `@cached(True, max_age=120)` for response caching, `background(fn, interval)` for periodic tasks.
- Project layout: `src/routes/*.py` (auto-discovered), `src/orm/*.py` (models), `src/app/` (helpers), `src/templates/` (Twig), `src/scss/` (auto-compiled), `migrations/NNNNNN_description.sql`.
"#;

/// Try to locate the installed framework's CLAUDE.md so the coder
/// gets version-matched context. Falls back to the embedded
/// cheat-sheet above when we can't find anything. Always returns a
/// ready-to-prepend string (with trailing blank line) or empty when
/// we genuinely can't help.
pub fn load_framework_context(project_dir: &Path) -> String {
    // Candidate locations, in preference order. First hit wins.
    // The venv path depends on Python minor version — glob it.
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    // Python projects: look in the active venv's site-packages
    for venv in &[".venv", "venv"] {
        let lib = project_dir.join(venv).join("lib");
        if let Ok(entries) = fs::read_dir(&lib) {
            for e in entries.flatten() {
                let site = e.path().join("site-packages/tina4_python");
                candidates.push(site.join("CLAUDE.md"));
            }
        }
    }
    // PHP: vendor path
    candidates.push(project_dir.join("vendor/tina4stack/tina4php/CLAUDE.md"));
    // Ruby: bundle path — approximate
    candidates.push(project_dir.join("vendor/bundle/ruby").join("tina4/CLAUDE.md"));
    // Node.js
    candidates.push(project_dir.join("node_modules/tina4-nodejs/CLAUDE.md"));
    // Project-local override (user can drop their own)
    candidates.push(project_dir.join(".tina4/framework-context.md"));

    for p in candidates {
        if p.is_file() {
            if let Ok(text) = fs::read_to_string(&p) {
                if text.len() > 100 {
                    return format!("## Framework Reference\nSource: {}\n\n{}\n\n", p.display(), text);
                }
            }
        }
    }
    // Fallback — embedded short reference.
    format!("## Framework Reference (embedded fallback)\n\n{}\n\n", TINA4_FALLBACK_CONTEXT)
}

/// Scan project and build context string for the coder agent.
pub fn build_project_context(project_dir: &Path) -> String {
    let mut ctx = String::new();

    // Detect language
    let lang = if project_dir.join("app.py").exists() { "python" }
        else if project_dir.join("index.php").exists() || project_dir.join("composer.json").exists() { "php" }
        else if project_dir.join("app.rb").exists() || project_dir.join("Gemfile").exists() { "ruby" }
        else if project_dir.join("app.ts").exists() || project_dir.join("package.json").exists() { "nodejs" }
        else { "python" };
    ctx.push_str(&format!("Language: {}\n", lang));
    ctx.push_str(&format!("Project root: {}\n\n", project_dir.display()));

    // List existing route files with their first few lines
    let routes_dir = project_dir.join("src").join("routes");
    if routes_dir.exists() {
        ctx.push_str("## Existing route files:\n");
        if let Ok(entries) = fs::read_dir(&routes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    ctx.push_str(&format!("- src/routes/{}", name));
                    // Read first 5 lines to show the pattern
                    if let Ok(content) = fs::read_to_string(&path) {
                        let preview: String = content.lines().take(5).collect::<Vec<_>>().join("\n");
                        ctx.push_str(&format!("\n```\n{}\n```\n", preview));
                    } else {
                        ctx.push('\n');
                    }
                }
            }
        }
        ctx.push('\n');
    }

    // List existing ORM models
    let orm_dir = project_dir.join("src").join("orm");
    if orm_dir.exists() {
        ctx.push_str("## Existing ORM models:\n");
        if let Ok(entries) = fs::read_dir(&orm_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    ctx.push_str(&format!("- src/orm/{}", name));
                    if let Ok(content) = fs::read_to_string(&path) {
                        let preview: String = content.lines().take(10).collect::<Vec<_>>().join("\n");
                        ctx.push_str(&format!("\n```\n{}\n```\n", preview));
                    } else {
                        ctx.push('\n');
                    }
                }
            }
        }
        ctx.push('\n');
    }

    // List existing templates
    let tmpl_dir = project_dir.join("src").join("templates");
    if tmpl_dir.exists() {
        ctx.push_str("## Existing templates:\n");
        if let Ok(entries) = fs::read_dir(&tmpl_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    ctx.push_str(&format!("- src/templates/{}\n", name));
                }
            }
        }
        ctx.push('\n');
    }

    // List existing migrations
    let mig_dir = project_dir.join("migrations");
    if mig_dir.exists() {
        ctx.push_str("## Existing migrations (at project root):\n");
        if let Ok(entries) = fs::read_dir(&mig_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                ctx.push_str(&format!("- migrations/{}\n", name));
            }
        }
        ctx.push('\n');
    }

    // Read app.py to understand the entry point
    let app_file = match lang {
        "python" => "app.py",
        "php" => "index.php",
        "ruby" => "app.rb",
        _ => "app.ts",
    };
    if let Ok(content) = fs::read_to_string(project_dir.join(app_file)) {
        ctx.push_str(&format!("## {} (entry point):\n```\n{}\n```\n\n", app_file, content));
    }

    // .env for database config awareness
    if let Ok(content) = fs::read_to_string(project_dir.join(".env")) {
        // Only include non-secret lines (keys, not values)
        let safe: String = content.lines()
            .map(|line| {
                if let Some(pos) = line.find('=') {
                    format!("{}=***", &line[..pos])
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        ctx.push_str(&format!("## .env keys:\n{}\n\n", safe));
    }

    ctx
}

/// Scan project for issues (called by background thinking loop).
pub fn scan_project(project_dir: &Path) -> Vec<(String, String, String)> {
    // Returns: [(category, id, description)]
    let mut issues = Vec::new();

    // Check for uncommitted changes
    if let Ok(output) = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_dir)
        .output()
    {
        let status = String::from_utf8_lossy(&output.stdout);
        let changed_files: Vec<&str> = status.lines().collect();
        if changed_files.len() > 3 {
            issues.push((
                "uncommitted".into(),
                "uncommitted_files".into(),
                format!("{} uncommitted files in the project", changed_files.len()),
            ));
        }
    }

    // Check for routes without tests
    let routes_dir = project_dir.join("src").join("routes");
    let tests_dir_a = project_dir.join("tests");
    let tests_dir_b = project_dir.join("spec");
    if routes_dir.exists() {
        let route_count = fs::read_dir(&routes_dir)
            .map(|entries| entries.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "py" || ext == "php" || ext == "rb" || ext == "ts"))
                .count())
            .unwrap_or(0);

        let test_count = [&tests_dir_a, &tests_dir_b].iter()
            .filter_map(|d| fs::read_dir(d).ok())
            .flat_map(|entries| entries.filter_map(|e| e.ok()))
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("test_") || name.ends_with("_test.") || name.ends_with("_spec.")
            })
            .count();

        if route_count > 0 && test_count == 0 {
            issues.push((
                "untested".into(),
                "no_tests".into(),
                format!("{} routes with no test files at all", route_count),
            ));
        } else if route_count > test_count + 2 {
            issues.push((
                "untested".into(),
                "low_coverage".into(),
                format!("{} routes but only {} test files", route_count, test_count),
            ));
        }
    }

    // Check for missing .env.example
    if project_dir.join(".env").exists() && !project_dir.join(".env.example").exists() {
        issues.push((
            "convention".into(),
            "no_env_example".into(),
            "Project has .env but no .env.example — other developers won't know what vars are needed".into(),
        ));
    }

    issues
}

/// Background thinking loop — runs as a tokio task.
pub async fn background_thinking_loop(
    project_dir: PathBuf,
    settings: ChatSettings,
    thought_tx: tokio::sync::broadcast::Sender<String>,
) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // every 5 minutes
    // Skip the first tick (fires immediately)
    interval.tick().await;

    loop {
        interval.tick().await;

        let issues = scan_project(&project_dir);
        if issues.is_empty() {
            continue;
        }

        let mut escalations = load_escalations(&project_dir);
        let now = chrono_now();

        // Track new issues
        for (category, id, description) in &issues {
            let existing = escalations.iter_mut().find(|e| e.id == *id);
            if let Some(esc) = existing {
                if esc.dismissed || esc.acted_on { continue; }
                if esc.level < 3 {
                    esc.level += 1;
                    esc.last_prompted = now.clone();
                    esc.message = description.clone();
                }
            } else {
                escalations.push(Escalation {
                    id: id.clone(), category: category.clone(), level: 1,
                    message: description.clone(), first_seen: now.clone(),
                    last_prompted: now.clone(), dismissed: false, acted_on: false,
                });
            }
        }
        save_escalations(&project_dir, &escalations);

        // Pick the most important un-dismissed issue
        let active: Vec<&Escalation> = escalations.iter()
            .filter(|e| !e.dismissed && !e.acted_on && e.level >= 1)
            .collect();

        if let Some(top) = active.first() {
            // Ask the LLM to phrase it like a thoughtful colleague
            let reflection_prompt = format!(
                "You noticed this about the developer's project: {}\n\
                Escalation level: {} (1=gentle, 2=concerned, 3=urgent)\n\
                Category: {}\n\n\
                Write a single short message (2-3 sentences max) as if you're a friendly senior developer \
                who genuinely cares about the project. Be conversational, not robotic. \
                Show you understand WHY this matters, not just WHAT the issue is. \
                If level 3, express real concern about risk. \
                Don't use bullet points. Don't use headers. Just talk naturally.",
                top.message, top.level, top.category
            );

            let human_message = match llm_call(
                &settings.thinking, "",
                &[LlmMessage { role: "user".into(), content: reflection_prompt }],
                256, 0.7
            ).await {
                Ok(msg) => {
                    // Clean up — remove any JSON wrapping the LLM might add
                    let cleaned = msg.trim().trim_matches('"').to_string();
                    cleaned
                }
                Err(_) => top.message.clone(), // Fallback to raw message
            };

            let actions = match top.category.as_str() {
                "uncommitted" if top.level >= 3 => vec![
                    ThoughtAction { label: "Create backup branch".into(), action: "create_branch".into() },
                    ThoughtAction { label: "Not now".into(), action: "dismiss".into() },
                ],
                "uncommitted" => vec![
                    ThoughtAction { label: "Let's commit".into(), action: "commit".into() },
                    ThoughtAction { label: "I'm on it".into(), action: "dismiss".into() },
                ],
                "untested" if top.level >= 2 => vec![
                    ThoughtAction { label: "Help me write tests".into(), action: "scaffold_tests".into() },
                    ThoughtAction { label: "I'll handle it".into(), action: "dismiss".into() },
                ],
                "untested" => vec![
                    ThoughtAction { label: "Good idea, draft some".into(), action: "draft_tests".into() },
                    ThoughtAction { label: "Later".into(), action: "dismiss".into() },
                ],
                _ => vec![
                    ThoughtAction { label: "Tell me more".into(), action: "act".into() },
                    ThoughtAction { label: "Got it".into(), action: "dismiss".into() },
                ],
            };

            let thought = Thought {
                id: format!("{:x}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                timestamp: now.clone(),
                message: human_message,
                category: top.category.clone(),
                actions,
                dismissed: false,
            };

            save_thought(&project_dir, &thought);
            let thought_json = serde_json::to_string(&thought).unwrap_or_default();
            let _ = thought_tx.send(format!("event: thought\ndata: {}\n\n", thought_json));
        }
    }
}

/// Start the agent HTTP server (called by `tina4 serve` or `tina4 agent`).
pub fn run(port: u16) {
    println!("  {} Starting agent server on port {}", icon_play(), port);

    let project_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Scaffold agents if not present
    if !project_dir.join(".tina4").join("agents").exists() {
        scaffold_agents(&project_dir);
    }

    let agents = load_agents(&project_dir);
    println!("  {} Loaded {} agents: {}", icon_info(),
        agents.len(),
        agents.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "));

    // Start async runtime for the HTTP server + background thinking
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async move {
        let settings = load_chat_settings(&project_dir);
        let (thought_tx, _) = tokio::sync::broadcast::channel::<String>(32);

        // Spawn background thinking loop
        let bg_dir = project_dir.clone();
        let bg_settings = settings.clone();
        let bg_tx = thought_tx.clone();
        tokio::spawn(async move {
            background_thinking_loop(bg_dir, bg_settings, bg_tx).await;
        });

        println!("  {} Background thinking loop started (every 5 min)", icon_info());

        serve_agent_http(port, &project_dir, &agents, thought_tx).await;
    });
}

/// Tiny HTTP server for agent endpoints.
async fn serve_agent_http(port: u16, project_dir: &Path, agents: &[Agent], thought_tx: tokio::sync::broadcast::Sender<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener as AsyncTcpListener;

    let listener = AsyncTcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to bind agent port");

    println!("  {} Agent server listening on http://127.0.0.1:{}", icon_ok(), port);

    loop {
        let (mut stream, _addr) = match listener.accept().await {
            Ok(s) => s,
            Err(_) => continue,
        };

        let project_dir = project_dir.to_path_buf();
        let agents = agents.to_vec();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };

            let request = String::from_utf8_lossy(&buf[..n]);
            let first_line = request.lines().next().unwrap_or("");

            if first_line.starts_with("GET /health") {
                let body = r#"{"status":"ok"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            } else if first_line.starts_with("GET /agents") {
                let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
                let body = serde_json::to_string(&names).unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            } else if first_line.starts_with("GET /history") {
                let history = load_history(&project_dir);
                let body = serde_json::to_string(&history).unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            } else if first_line.starts_with("GET /thoughts") {
                let thoughts = load_thoughts(&project_dir);
                let body = serde_json::to_string(&thoughts).unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            } else if first_line.starts_with("POST /thoughts/dismiss") {
                // Dismiss a thought by ID
                let body_start = request.find("\r\n\r\n").unwrap_or(n) + 4;
                let body_str = &request[body_start..];
                #[derive(Deserialize)]
                struct DismissReq { id: String }
                if let Ok(req) = serde_json::from_str::<DismissReq>(body_str) {
                    let mut thoughts = load_thoughts(&project_dir);
                    if let Some(t) = thoughts.iter_mut().find(|t| t.id == req.id) {
                        t.dismissed = true;
                    }
                    let path = project_dir.join(".tina4").join("chat").join("thoughts.json");
                    let _ = fs::write(&path, serde_json::to_string_pretty(&thoughts).unwrap_or_default());

                    // Also dismiss the matching escalation
                    let mut escalations = load_escalations(&project_dir);
                    if let Some(e) = escalations.iter_mut().find(|e| !e.dismissed) {
                        e.dismissed = true;
                    }
                    save_escalations(&project_dir, &escalations);
                }
                let body = r#"{"ok":true}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            } else if first_line.starts_with("POST /chat") {
                // Extract body from HTTP request
                let body_start = request.find("\r\n\r\n").unwrap_or(n) + 4;
                let body_str = &request[body_start..];

                // Parse request
                #[derive(Deserialize)]
                struct ChatRequest {
                    message: String,
                    #[serde(default)]
                    thread_id: Option<String>,
                    #[serde(default)]
                    settings: Option<ChatSettings>,
                }

                let chat_req: ChatRequest = match serde_json::from_str(body_str) {
                    Ok(r) => r,
                    Err(e) => {
                        let err_body = format!(r#"{{"error":"Invalid request: {}"}}"#, e);
                        let resp = format!(
                            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                            err_body.len(), err_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }
                };

                let settings = chat_req.settings.unwrap_or_else(|| load_chat_settings(&project_dir));

                // Resolve model settings for the agent
                let supervisor = agents.iter().find(|a| a.name == "supervisor");
                let model_settings = &settings.thinking;

                // Save user message
                let user_msg = ChatMessage {
                    id: format!("{:x}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                    role: "user".into(),
                    content: chat_req.message.clone(),
                    timestamp: chrono_now(),
                    thread_id: chat_req.thread_id.clone(),
                    agent: None,
                };
                save_message(&project_dir, &user_msg);

                // SSE response headers
                let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\nX-Accel-Buffering: no\r\n\r\n";
                let _ = stream.write_all(headers.as_bytes()).await;

                // Status: thinking
                let _ = stream.write_all(
                    format!("event: status\ndata: {{\"text\":\"Analyzing request...\",\"agent\":\"supervisor\"}}\n\n").as_bytes()
                ).await;
                let _ = stream.flush().await;

                // Helper: send SSE event
                async fn sse_event(stream: &mut tokio::net::TcpStream, event: &str, data: &str) {
                    use tokio::io::AsyncWriteExt;
                    let _ = stream.write_all(format!("event: {}\ndata: {}\n\n", event, data).as_bytes()).await;
                    let _ = stream.flush().await;
                }

                fn sse_json(obj: &serde_json::Value) -> String {
                    serde_json::to_string(obj).unwrap_or_default()
                }

                // Resolve model for an agent by its config.model field
                fn resolve_model<'a>(agent_name: &str, agents: &[Agent], settings: &'a ChatSettings) -> &'a ModelSettings {
                    let model_type = agents.iter()
                        .find(|a| a.name == agent_name)
                        .map(|a| a.config.model.as_str())
                        .unwrap_or("thinking");
                    match model_type {
                        "vision" => &settings.vision,
                        "image-gen" => &settings.image_gen,
                        _ => &settings.thinking,
                    }
                }

                // Step 1: Call supervisor with conversation history + project context
                let supervisor_prompt = supervisor.map(|s| s.system_prompt.as_str()).unwrap_or("");

                // Build message history — last 20 messages for context
                let history = load_history(&project_dir);
                let recent: Vec<&ChatMessage> = history.iter()
                    .filter(|m| m.thread_id == chat_req.thread_id)
                    .rev().take(20).collect::<Vec<_>>().into_iter().rev().collect();

                let mut msgs: Vec<LlmMessage> = Vec::new();

                // Add project context as first system-like message
                let plans_dir = project_dir.join(".tina4").join("plans");
                let latest_plan = if plans_dir.exists() {
                    fs::read_dir(&plans_dir).ok()
                        .and_then(|entries| entries
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
                            .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok())))
                        .and_then(|entry| fs::read_to_string(entry.path()).ok())
                } else {
                    None
                };

                if let Some(ref plan) = latest_plan {
                    // Give supervisor awareness of the current plan
                    let plan_summary = if plan.len() > 800 { format!("{}...", &plan[..800]) } else { plan.clone() };
                    msgs.push(LlmMessage {
                        role: "system".into(),
                        content: format!("Current project plan:\n{}", plan_summary),
                    });
                }

                // Add conversation history
                for m in &recent {
                    let mut content = m.content.clone();
                    // Truncate long messages to save tokens
                    if content.len() > 600 {
                        content = format!("{}...(truncated)", &content[..600]);
                    }
                    msgs.push(LlmMessage {
                        role: if m.role == "user" { "user".into() } else { "assistant".into() },
                        content,
                    });
                }
                msgs.push(LlmMessage { role: "user".into(), content: chat_req.message.clone() });

                let supervisor_reply = match llm_call(model_settings, supervisor_prompt, &msgs, 2048, 0.3).await {
                    Ok(r) => r,
                    Err(e) => {
                        let escaped = e.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                        sse_event(&mut stream, "error", &format!("{{\"message\":\"{}\"}}", escaped)).await;
                        return;
                    }
                };

                // Step 2: Parse the supervisor's action
                let action = parse_supervisor_action(&supervisor_reply);

                match action {
                    Some(SupervisorAction { action: ref a, .. }) if a == "plan" => {
                        let ctx = action.as_ref().and_then(|a| a.context.clone()).unwrap_or_default();
                        sse_event(&mut stream, "status", &sse_json(&serde_json::json!({"text": "→ Planner: creating plan...", "agent": "planner"}))).await;

                        // Call planner agent
                        let planner = agents.iter().find(|a| a.name == "planner");
                        let planner_prompt = planner.map(|p| p.system_prompt.as_str()).unwrap_or("");
                        let planner_model = resolve_model("planner", &agents, &settings);

                        // Build planner context — no paths or tech details
                        let planner_msg = format!(
                            "Create an implementation plan for the following request:\n\n{}",
                            ctx
                        );
                        let planner_msgs = vec![LlmMessage { role: "user".into(), content: planner_msg }];

                        match llm_call(planner_model, planner_prompt, &planner_msgs, 4096, 0.2).await {
                            Ok(plan_content) => {
                                // Save plan to .tina4/plans/
                                let plan_name = format!("{}-plan.md", chrono_now().replace("Z", ""));
                                let plan_path = project_dir.join(".tina4").join("plans").join(&plan_name);
                                let _ = fs::write(&plan_path, &plan_content);

                                sse_event(&mut stream, "status", &sse_json(&serde_json::json!({
                                    "text": format!("Plan created: .tina4/plans/{}", plan_name),
                                    "agent": "planner"
                                }))).await;

                                // Send plan content + approval buttons as a single event
                                let plan_escaped = plan_content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "plan", &format!(
                                    "{{\"content\":\"{}\",\"agent\":\"planner\",\"file\":\".tina4/plans/{}\",\"approve\":true}}",
                                    plan_escaped, plan_name
                                )).await;

                                // Save assistant message
                                save_message(&project_dir, &ChatMessage {
                                    id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                                    role: "assistant".into(),
                                    content: plan_content,
                                    timestamp: chrono_now(),
                                    thread_id: chat_req.thread_id.clone(),
                                    agent: Some("planner".into()),
                                });
                            }
                            Err(e) => {
                                let escaped = e.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "error", &format!("{{\"message\":\"Planner failed: {}\"}}", escaped)).await;
                            }
                        }
                    }

                    Some(SupervisorAction { action: ref a, .. }) if a == "code" => {
                        let ctx = action.as_ref().and_then(|a| a.context.clone()).unwrap_or_default();
                        let files = action.as_ref().and_then(|a| a.files.clone()).unwrap_or_default();
                        sse_event(&mut stream, "status", &sse_json(&serde_json::json!({"text": "→ Coder: writing code...", "agent": "coder"}))).await;

                        let coder = agents.iter().find(|a| a.name == "coder");
                        let coder_prompt = coder.map(|c| c.system_prompt.as_str()).unwrap_or("");
                        let coder_model = resolve_model("coder", &agents, &settings);

                        let coder_msg = format!(
                            "Write the following code:\n\n{}\n\nFiles to create/modify: {:?}\n\nReturn each file as:\n## FILE: path/to/file\n```\ncontent\n```",
                            ctx, files
                        );
                        let coder_msgs = vec![LlmMessage { role: "user".into(), content: coder_msg }];

                        match llm_call(coder_model, coder_prompt, &coder_msgs, 4096, 0.1).await {
                            Ok(code_output) => {
                                // Parse file outputs and write them
                                let mut files_written = Vec::new();
                                for section in code_output.split("## FILE:") {
                                    let section = section.trim();
                                    if section.is_empty() { continue; }
                                    let mut lines = section.lines();
                                    if let Some(file_path) = lines.next() {
                                        let file_path = file_path.trim();
                                        // Extract content between ``` markers
                                        let remaining: String = lines.collect::<Vec<&str>>().join("\n");
                                        let content = if let Some(start) = remaining.find("```") {
                                            let after = &remaining[start + 3..];
                                            // Skip language identifier on first line
                                            let after = if let Some(nl) = after.find('\n') { &after[nl+1..] } else { after };
                                            if let Some(end) = after.find("```") { &after[..end] } else { after }
                                        } else {
                                            remaining.as_str()
                                        };

                                        let full_path = project_dir.join(file_path);
                                        if let Some(parent) = full_path.parent() {
                                            let _ = fs::create_dir_all(parent);
                                        }
                                        if fs::write(&full_path, content.trim()).is_ok() {
                                            files_written.push(file_path.to_string());
                                            sse_event(&mut stream, "status", &sse_json(&serde_json::json!({
                                                "text": format!("Written: {}", file_path),
                                                "agent": "coder"
                                            }))).await;
                                        }
                                    }
                                }

                                let msg = if files_written.is_empty() {
                                    code_output.clone()
                                } else {
                                    format!("Created {} files:\n{}", files_written.len(), files_written.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n"))
                                };

                                let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "message", &format!(
                                    "{{\"content\":\"{}\",\"agent\":\"coder\",\"files_changed\":{}}}", escaped,
                                    serde_json::to_string(&files_written).unwrap_or_default()
                                )).await;

                                save_message(&project_dir, &ChatMessage {
                                    id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                                    role: "assistant".into(),
                                    content: msg,
                                    timestamp: chrono_now(),
                                    thread_id: chat_req.thread_id.clone(),
                                    agent: Some("coder".into()),
                                });
                            }
                            Err(e) => {
                                let escaped = e.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "error", &format!("{{\"message\":\"Coder failed: {}\"}}", escaped)).await;
                            }
                        }
                    }

                    Some(SupervisorAction { action: ref a, .. }) if a == "execute_plan" => {
                        // Execute plan step by step
                        let plan_file = action.as_ref().and_then(|a| a.context.clone()).unwrap_or_default();
                        let plan_path = project_dir.join(&plan_file);
                        let plan_content = fs::read_to_string(&plan_path).unwrap_or_default();

                        if plan_content.is_empty() {
                            sse_event(&mut stream, "message", &format!(
                                "{{\"content\":\"I couldn't find the plan. Let me create a new one.\",\"agent\":\"supervisor\"}}"
                            )).await;
                        } else {
                            // Parse numbered steps from plan
                            let steps: Vec<String> = plan_content.lines()
                                .filter(|line| {
                                    let trimmed = line.trim();
                                    // Match lines starting with a number followed by . or )
                                    trimmed.len() > 2 && trimmed.chars().next().map_or(false, |c| c.is_ascii_digit())
                                        && (trimmed.contains(". ") || trimmed.contains(") "))
                                })
                                .map(|line| {
                                    let trimmed = line.trim();
                                    // Strip the number prefix
                                    if let Some(pos) = trimmed.find(". ") {
                                        trimmed[pos + 2..].to_string()
                                    } else if let Some(pos) = trimmed.find(") ") {
                                        trimmed[pos + 2..].to_string()
                                    } else {
                                        trimmed.to_string()
                                    }
                                })
                                .collect();

                            let total_steps = steps.len();
                            sse_event(&mut stream, "status", &sse_json(&serde_json::json!({
                                "text": format!("Executing plan — {} steps", total_steps),
                                "agent": "supervisor"
                            }))).await;

                            let coder = agents.iter().find(|a| a.name == "coder");
                            let coder_prompt = coder.map(|c| c.system_prompt.as_str()).unwrap_or("");
                            let coder_model = resolve_model("coder", &agents, &settings);

                            let mut all_files_written: Vec<String> = Vec::new();
                            let mut step_summaries: Vec<String> = Vec::new();

                            for (i, step) in steps.iter().enumerate() {
                                let step_num = i + 1;

                                // Tell the user what we're working on
                                let progress_msg = format!("Step {} of {}: {}", step_num, total_steps, step);
                                sse_event(&mut stream, "status", &sse_json(&serde_json::json!({
                                    "text": progress_msg.clone(),
                                    "agent": "coder"
                                }))).await;
                                sse_event(&mut stream, "message", &format!(
                                    "{{\"content\":\"**Step {} of {}:** {}\\n\\nWorking on this now...\",\"agent\":\"supervisor\"}}",
                                    step_num, total_steps, step.replace('\\', "\\\\").replace('"', "\\\"")
                                )).await;

                                // Send step to coder
                                let coder_msg = format!(
                                    "Implement this single step from the project plan:\n\n**Step {}:** {}\n\n\
                                    Full plan context:\n{}\n\n\
                                    Project directory: {}\n\n\
                                    Return each file as:\n## FILE: path/to/file\n```\ncontent\n```",
                                    step_num, step, plan_content, project_dir.display()
                                );
                                let coder_msgs = vec![LlmMessage { role: "user".into(), content: coder_msg }];

                                match llm_call(coder_model, coder_prompt, &coder_msgs, 4096, 0.1).await {
                                    Ok(code_output) => {
                                        // Parse and write files
                                        let mut step_files = Vec::new();
                                        for section in code_output.split("## FILE:") {
                                            let section = section.trim();
                                            if section.is_empty() { continue; }
                                            let mut lines = section.lines();
                                            if let Some(file_path) = lines.next() {
                                                let file_path = file_path.trim();
                                                let remaining: String = lines.collect::<Vec<&str>>().join("\n");
                                                let content = if let Some(start) = remaining.find("```") {
                                                    let after = &remaining[start + 3..];
                                                    let after = if let Some(nl) = after.find('\n') { &after[nl+1..] } else { after };
                                                    if let Some(end) = after.find("```") { &after[..end] } else { after }
                                                } else {
                                                    remaining.as_str()
                                                };

                                                let full_path = project_dir.join(file_path);
                                                if let Some(parent) = full_path.parent() {
                                                    let _ = fs::create_dir_all(parent);
                                                }
                                                if fs::write(&full_path, content.trim()).is_ok() {
                                                    step_files.push(file_path.to_string());
                                                    all_files_written.push(file_path.to_string());
                                                }
                                            }
                                        }

                                        // Report step completion
                                        let done_msg = if step_files.is_empty() {
                                            format!("Step {} complete.", step_num)
                                        } else {
                                            format!("Step {} complete — {} files updated.", step_num, step_files.len())
                                        };
                                        step_summaries.push(format!("{}. {} ✓", step_num, step));

                                        sse_event(&mut stream, "status", &sse_json(&serde_json::json!({
                                            "text": done_msg,
                                            "agent": "coder"
                                        }))).await;
                                    }
                                    Err(e) => {
                                        step_summaries.push(format!("{}. {} ✗ (failed)", step_num, step));
                                        let err_escaped = e.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                        sse_event(&mut stream, "message", &format!(
                                            "{{\"content\":\"Step {} had an issue: {}. Moving on...\",\"agent\":\"supervisor\"}}",
                                            step_num, err_escaped
                                        )).await;
                                    }
                                }
                            }

                            // Final summary
                            let summary = format!(
                                "All done! Here's what I built:\\n\\n{}\\n\\n{} files were created or updated.",
                                step_summaries.iter().map(|s| format!("- {}", s.replace('\\', "\\\\").replace('"', "\\\""))).collect::<Vec<_>>().join("\\n"),
                                all_files_written.len()
                            );
                            sse_event(&mut stream, "message", &format!(
                                "{{\"content\":\"{}\",\"agent\":\"supervisor\",\"files_changed\":{}}}",
                                summary, serde_json::to_string(&all_files_written).unwrap_or_default()
                            )).await;

                            // Save summary as message
                            save_message(&project_dir, &ChatMessage {
                                id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                                role: "assistant".into(),
                                content: format!("Plan executed: {} steps, {} files written", step_summaries.len(), all_files_written.len()),
                                timestamp: chrono_now(),
                                thread_id: chat_req.thread_id.clone(),
                                agent: Some("supervisor".into()),
                            });
                        }
                    }

                    Some(SupervisorAction { action: ref a, .. }) if a == "debug" => {
                        let err_msg = action.as_ref().and_then(|a| a.error.clone()).unwrap_or_default();
                        sse_event(&mut stream, "status", &sse_json(&serde_json::json!({"text": "→ Debug: analyzing error...", "agent": "debug"}))).await;

                        let debug_agent = agents.iter().find(|a| a.name == "debug");
                        let debug_prompt = debug_agent.map(|d| d.system_prompt.as_str()).unwrap_or("");
                        let debug_model = resolve_model("debug", &agents, &settings);
                        let debug_msgs = vec![LlmMessage { role: "user".into(), content: format!("Analyze this error and suggest a fix:\n\n{}", err_msg) }];

                        match llm_call(debug_model, debug_prompt, &debug_msgs, 4096, 0.2).await {
                            Ok(analysis) => {
                                let escaped = analysis.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "message", &format!("{{\"content\":\"{}\",\"agent\":\"debug\"}}", escaped)).await;
                                save_message(&project_dir, &ChatMessage {
                                    id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                                    role: "assistant".into(), content: analysis, timestamp: chrono_now(),
                                    thread_id: chat_req.thread_id.clone(), agent: Some("debug".into()),
                                });
                            }
                            Err(e) => {
                                let escaped = e.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "error", &format!("{{\"message\":\"Debug failed: {}\"}}", escaped)).await;
                            }
                        }
                    }

                    Some(SupervisorAction { action: ref a, message: Some(ref msg), .. }) if a == "respond" => {
                        // Direct response — no delegation needed
                        let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                        sse_event(&mut stream, "status", &sse_json(&serde_json::json!({"text": "Responding...", "agent": "supervisor"}))).await;
                        sse_event(&mut stream, "message", &format!("{{\"content\":\"{}\",\"agent\":\"supervisor\"}}", escaped)).await;

                        save_message(&project_dir, &ChatMessage {
                            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                            role: "assistant".into(), content: msg.clone(), timestamp: chrono_now(),
                            thread_id: chat_req.thread_id.clone(), agent: Some("supervisor".into()),
                        });
                    }

                    Some(SupervisorAction { action: ref a, .. }) if a == "generate_image" => {
                        let img_prompt = action.as_ref().and_then(|a| a.prompt.clone()).unwrap_or_default();
                        sse_event(&mut stream, "status", &sse_json(&serde_json::json!({"text": "→ Image Gen: generating image...", "agent": "image-gen"}))).await;

                        // Call image generation endpoint
                        let img_settings = &settings.image_gen;
                        let base_url = img_settings.url.trim_end_matches('/');
                        let img_url = if base_url.contains("/v1/") { base_url.to_string() } else { format!("{}/v1/images/generations", base_url) };

                        let client = reqwest::Client::new();
                        let img_body = serde_json::json!({
                            "model": img_settings.model,
                            "prompt": img_prompt,
                            "n": 1,
                            "size": "512x512"
                        });

                        let mut req = client.post(&img_url).header("Content-Type", "application/json").json(&img_body);
                        if !img_settings.api_key.is_empty() {
                            req = req.header("Authorization", format!("Bearer {}", img_settings.api_key));
                        }

                        match req.send().await {
                            Ok(resp) => {
                                let text = resp.text().await.unwrap_or_default();
                                match serde_json::from_str::<serde_json::Value>(&text) {
                                    Ok(data) => {
                                        // Extract image URL or base64 from response
                                        let img_data = data["data"][0]["url"].as_str()
                                            .or_else(|| data["data"][0]["b64_json"].as_str())
                                            .unwrap_or("");
                                        let is_b64 = data["data"][0]["b64_json"].is_string();

                                        let img_html = if is_b64 {
                                            format!("Generated image for: {}\\n\\n<img src=\\\"data:image/png;base64,{}\\\" style=\\\"max-width:100%;border-radius:8px\\\">", img_prompt.replace('"', "\\\""), img_data.replace('"', "\\\""))
                                        } else if !img_data.is_empty() {
                                            format!("Generated image for: {}\\n\\n<img src=\\\"{}\\\" style=\\\"max-width:100%;border-radius:8px\\\">", img_prompt.replace('"', "\\\""), img_data.replace('"', "\\\""))
                                        } else {
                                            format!("Image generated for: {}", img_prompt.replace('"', "\\\""))
                                        };

                                        sse_event(&mut stream, "message", &format!("{{\"content\":\"{}\",\"agent\":\"image-gen\"}}", img_html)).await;
                                    }
                                    Err(_) => {
                                        let escaped = format!("Image generation returned unexpected response").replace('"', "\\\"");
                                        sse_event(&mut stream, "message", &format!("{{\"content\":\"{}\",\"agent\":\"image-gen\"}}", escaped)).await;
                                    }
                                }
                            }
                            Err(e) => {
                                let escaped = format!("Image generation failed: {}", e).replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "error", &format!("{{\"message\":\"{}\"}}", escaped)).await;
                            }
                        }

                        save_message(&project_dir, &ChatMessage {
                            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                            role: "assistant".into(), content: format!("Generated image: {}", img_prompt), timestamp: chrono_now(),
                            thread_id: chat_req.thread_id.clone(), agent: Some("image-gen".into()),
                        });
                    }

                    Some(SupervisorAction { action: ref a, .. }) if a == "analyze_image" => {
                        sse_event(&mut stream, "status", &sse_json(&serde_json::json!({"text": "→ Vision: analyzing image...", "agent": "vision"}))).await;
                        // Vision requires image data — for now respond with a message
                        let msg = "I can see you want me to analyze an image. Please attach an image and I'll describe what I see.";
                        let escaped = msg.replace('"', "\\\"");
                        sse_event(&mut stream, "message", &format!("{{\"content\":\"{}\",\"agent\":\"vision\"}}", escaped)).await;

                        save_message(&project_dir, &ChatMessage {
                            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                            role: "assistant".into(), content: msg.to_string(), timestamp: chrono_now(),
                            thread_id: chat_req.thread_id.clone(), agent: Some("vision".into()),
                        });
                    }

                    _ => {
                        // Fallback — try to extract a message from the JSON, never show raw JSON
                        let display_msg = if let Some(ref act) = action {
                            act.message.clone()
                                .or_else(|| act.context.clone())
                                .or_else(|| act.prompt.clone())
                                .unwrap_or_else(|| "I'm processing your request...".to_string())
                        } else {
                            "I'm processing your request...".to_string()
                        };
                        let escaped = display_msg.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                        sse_event(&mut stream, "message", &format!("{{\"content\":\"{}\",\"agent\":\"supervisor\"}}", escaped)).await;

                        save_message(&project_dir, &ChatMessage {
                            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                            role: "assistant".into(), content: display_msg, timestamp: chrono_now(),
                            thread_id: chat_req.thread_id.clone(), agent: Some("supervisor".into()),
                        });
                    }
                }

                // Done
                sse_event(&mut stream, "status", &sse_json(&serde_json::json!({"text": "Done", "agent": "supervisor"}))).await;
                sse_event(&mut stream, "done", "{}").await;
            } else if first_line.starts_with("POST /execute") {
                // Direct plan execution — bypasses supervisor, goes straight to coder
                let body_start = request.find("\r\n\r\n").unwrap_or(n) + 4;
                let body_str = &request[body_start..];

                #[derive(Deserialize)]
                struct ExecRequest {
                    plan_file: String,
                    #[serde(default)]
                    settings: Option<ChatSettings>,
                    #[serde(default)]
                    resume: bool,
                }

                #[derive(Debug, Clone, Serialize, Deserialize, Default)]
                struct PlanState {
                    completed: Vec<usize>,
                    files: Vec<String>,
                }

                let exec_req: ExecRequest = match serde_json::from_str(body_str) {
                    Ok(r) => r,
                    Err(e) => {
                        let err_body = format!(r#"{{"error":"Invalid request: {}"}}"#, e);
                        let resp = format!(
                            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                            err_body.len(), err_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }
                };

                let settings = exec_req.settings.unwrap_or_else(|| load_chat_settings(&project_dir));

                // SSE headers
                let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\nX-Accel-Buffering: no\r\n\r\n";
                let _ = stream.write_all(headers.as_bytes()).await;

                async fn sse_ev(stream: &mut tokio::net::TcpStream, event: &str, data: &str) {
                    use tokio::io::AsyncWriteExt;
                    let _ = stream.write_all(format!("event: {}\ndata: {}\n\n", event, data).as_bytes()).await;
                    let _ = stream.flush().await;
                }

                fn sse_j(obj: &serde_json::Value) -> String {
                    serde_json::to_string(obj).unwrap_or_default()
                }

                // Read the plan
                let plan_path = project_dir.join(&exec_req.plan_file);
                let plan_content = fs::read_to_string(&plan_path).unwrap_or_default();

                if plan_content.is_empty() {
                    sse_ev(&mut stream, "error", &sse_j(&serde_json::json!({"message":"Plan file not found"}))).await;
                    sse_ev(&mut stream, "done", "{}").await;
                    return;
                }

                // Parse steps. We accept TWO plan formats — numbered lists
                // AND GitHub-style markdown checkboxes ("- [ ] step",
                // "* [x] step"). The dev-admin UI writes checkboxes
                // because it renders checkbox progress natively; hand-
                // written plans usually use numbered lists. Either way
                // we end up with a {text, done} struct per step so we
                // can skip already-completed work without needing a
                // separate state.json.
                #[derive(Clone)]
                struct Step { text: String, done: bool }

                let mut steps: Vec<Step> = Vec::new();
                for line in plan_content.lines() {
                    let trimmed = line.trim();
                    if trimmed.len() < 3 { continue; }

                    // Checkbox: `- [ ] X`, `* [ ] X`, `- [x] X` (case-insensitive x)
                    if (trimmed.starts_with("- ") || trimmed.starts_with("* "))
                        && trimmed.len() > 5 && trimmed.as_bytes()[2] == b'['
                        && trimmed.as_bytes()[4] == b']'
                    {
                        let box_char = trimmed.as_bytes()[3];
                        let done = box_char == b'x' || box_char == b'X';
                        let text = trimmed[5..].trim().to_string();
                        if !text.is_empty() { steps.push(Step { text, done }); }
                        continue;
                    }

                    // Numbered: `1. X` or `1) X`
                    let first = trimmed.chars().next().unwrap_or(' ');
                    if first.is_ascii_digit() && (trimmed.contains(". ") || trimmed.contains(") ")) {
                        let text = if let Some(pos) = trimmed.find(". ") {
                            trimmed[pos + 2..].to_string()
                        } else if let Some(pos) = trimmed.find(") ") {
                            trimmed[pos + 2..].to_string()
                        } else {
                            trimmed.to_string()
                        };
                        if !text.is_empty() { steps.push(Step { text, done: false }); }
                    }
                }

                let total = steps.len();

                // Load existing state for resume
                let state_path = plan_path.with_extension("state.json");
                let mut state: PlanState = if exec_req.resume {
                    fs::read_to_string(&state_path).ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default()
                } else {
                    PlanState::default()
                };

                let skip_count = state.completed.len();
                if skip_count > 0 {
                    sse_ev(&mut stream, "message", &format!(
                        "{{\"content\":\"Resuming from step {} — {} steps already done.\",\"agent\":\"supervisor\"}}",
                        skip_count + 1, skip_count
                    )).await;
                }

                sse_ev(&mut stream, "status", &sse_j(&serde_json::json!({"text": format!("Building — {} steps ({} remaining)", total, total - skip_count), "agent": "supervisor"}))).await;

                let coder = agents.iter().find(|a| a.name == "coder");
                let coder_prompt = coder.map(|c| c.system_prompt.as_str()).unwrap_or("");
                let coder_model_type = coder.map(|a| a.config.model.as_str()).unwrap_or("thinking");
                let coder_model = match coder_model_type { "vision" => &settings.vision, "image-gen" => &settings.image_gen, _ => &settings.thinking };

                let mut summaries: Vec<String> = Vec::new();
                let mut failed = false;

                for (i, step) in steps.iter().enumerate() {
                    let num = i + 1;
                    let step_text = step.text.clone();

                    // Skip completed steps — either marked in state.json
                    // (from an earlier run that was interrupted) OR
                    // already ticked in the markdown itself (the AI
                    // chat calls plan_complete_step which sets `[x]`).
                    if step.done || state.completed.contains(&num) {
                        summaries.push(format!("{}. {} ✓ (done earlier)", num, step_text));
                        if !state.completed.contains(&num) { state.completed.push(num); }
                        continue;
                    }

                    // Progress update
                    let step_escaped = step_text.replace('\\', "\\\\").replace('"', "\\\"");
                    sse_ev(&mut stream, "message", &format!(
                        "{{\"content\":\"**Step {} of {}:** {}\",\"agent\":\"supervisor\"}}",
                        num, total, step_escaped
                    )).await;
                    sse_ev(&mut stream, "status", &sse_j(&serde_json::json!({"text": format!("Step {}/{}: {}", num, total, step_text), "agent": "coder"}))).await;

                    // Build real project context by scanning files
                    let project_ctx = build_project_context(&project_dir);
                    let framework_ctx = load_framework_context(&project_dir);

                    // Call coder with full project + framework context.
                    // The framework cheat-sheet teaches it tina4 idioms
                    // (response() not response.json(), DatabaseResult.records,
                    // @noauth import path, etc.) so first-turn code is correct
                    // for the specific tina4 flavour in use.
                    let coder_msg = format!(
                        "{}## Project Context\n{}\n\n\
                        ## Task\nImplement step {} of {}:\n**{}**\n\n\
                        ## Full Plan\n{}\n\n\
                        Return each file as:\n## FILE: path/to/file\n```\ncontent\n```",
                        framework_ctx, project_ctx, num, total, step_text, plan_content
                    );
                    let coder_msgs = vec![LlmMessage { role: "user".into(), content: coder_msg }];

                    match llm_call(coder_model, coder_prompt, &coder_msgs, 4096, 0.1).await {
                        Ok(code_output) => {
                            let mut step_files = Vec::new();
                            for section in code_output.split("## FILE:") {
                                let section = section.trim();
                                if section.is_empty() { continue; }
                                let mut lines = section.lines();
                                if let Some(file_path) = lines.next() {
                                    let file_path = file_path.trim();
                                    let remaining: String = lines.collect::<Vec<&str>>().join("\n");
                                    let content = if let Some(start) = remaining.find("```") {
                                        let after = &remaining[start + 3..];
                                        let after = if let Some(nl) = after.find('\n') { &after[nl+1..] } else { after };
                                        if let Some(end) = after.find("```") { &after[..end] } else { after }
                                    } else { remaining.as_str() };

                                    let full_path = project_dir.join(file_path);
                                    if let Some(parent) = full_path.parent() { let _ = fs::create_dir_all(parent); }
                                    if fs::write(&full_path, content.trim()).is_ok() {
                                        step_files.push(file_path.to_string());
                                        state.files.push(file_path.to_string());
                                    }
                                }
                            }

                            // Mark step complete and save state immediately
                            state.completed.push(num);
                            let _ = fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap_or_default());

                            summaries.push(format!("{}. {} ✓", num, step_text));
                            sse_ev(&mut stream, "status", &sse_j(&serde_json::json!({"text": format!("Step {} done — {} files", num, step_files.len()), "agent": "coder"}))).await;
                        }
                        Err(e) => {
                            summaries.push(format!("{}. {} ✗", num, step_text));
                            failed = true;

                            // Save state so we can resume from here
                            let _ = fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap_or_default());

                            let err_esc = e.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                            sse_ev(&mut stream, "message", &format!(
                                "{{\"content\":\"Step {} failed: {}\\n\\nYou can resume from here.\",\"agent\":\"supervisor\"}}",
                                num, err_esc
                            )).await;

                            // Send resume event so frontend can show Resume button
                            sse_ev(&mut stream, "plan_failed", &format!(
                                "{{\"file\":\"{}\",\"completed\":{},\"total\":{},\"failed_step\":{}}}",
                                exec_req.plan_file.replace('\\', "\\\\").replace('"', "\\\""),
                                state.completed.len(), total, num
                            )).await;
                            break; // Stop on first failure
                        }
                    }
                }

                // Final summary
                let summary_lines = summaries.iter().map(|s| format!("- {}", s.replace('\\', "\\\\").replace('"', "\\\""))).collect::<Vec<_>>().join("\\n");
                if failed {
                    sse_ev(&mut stream, "message", &format!(
                        "{{\"content\":\"Progress so far:\\n\\n{}\\n\\n{} files created. Resume when ready.\",\"agent\":\"supervisor\",\"files_changed\":{}}}",
                        summary_lines, state.files.len(), serde_json::to_string(&state.files).unwrap_or_default()
                    )).await;
                } else {
                    // All done — clean up state file
                    let _ = fs::remove_file(&state_path);
                    sse_ev(&mut stream, "message", &format!(
                        "{{\"content\":\"All done!\\n\\n{}\\n\\n{} files created or updated.\",\"agent\":\"supervisor\",\"files_changed\":{}}}",
                        summary_lines, state.files.len(), serde_json::to_string(&state.files).unwrap_or_default()
                    )).await;
                }
                sse_ev(&mut stream, "done", "{}").await;

            } else if first_line.starts_with("OPTIONS") {
                // CORS preflight
                let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nAccess-Control-Max-Age: 86400\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes()).await;
            } else {
                let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });
    }
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without chrono dep
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    // Good enough for now — proper chrono can be added later
    format!("{}Z", secs)
}
