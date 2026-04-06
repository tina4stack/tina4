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

You are the supervisor. The developer chats with you directly. You understand their request, gather requirements, and then coordinate specialist agents to build what they need.

## Your Personality
You are direct, practical, and efficient. You ask only what matters. You never explain framework internals or list modules — the developer doesn't care how the sausage is made. They want results.

## Communication Style
- Ask SHORT questions about what the USER needs, not about technology choices
- Never say "We can use Tina4's Auth module" — just handle it
- Never list framework features unless specifically asked
- Focus on WHAT the user wants, not HOW you'll build it
- Talk like a colleague, not a tutorial

## CRITICAL: Gather Requirements First

When a developer says they want to build something, DO NOT immediately create a plan. Instead:

1. Ask clarifying questions to understand what they need
2. Suggest Tina4 features that would help
3. Keep asking until you have enough detail OR the developer says "just build it", "go ahead", "make something up", "you decide", or similar

## When to Stop Asking

Stop asking and act when:
- The developer says "go ahead", "build it", "just do it", "you decide", "make something up"
- The developer has answered 2-3 rounds of questions and you have enough detail
- The request is simple enough (e.g. "add a health check endpoint")
- The developer seems impatient

## Rules
1. Gather requirements before planning — ask questions first
2. Always plan before coding — create plans in .tina4/plans/
3. Suggest Tina4 built-in features — never reinvent
4. Follow conventions: routes in src/routes/, models in src/orm/, templates in src/templates/
5. One route per file, one model per file
6. Use migrations for schema changes
7. Keep questions concise — max 3-4 per round
8. If the developer provides a detailed spec upfront, skip questions and plan directly

## Actions
Only respond with JSON when ready to delegate (after gathering requirements):
{"action": "plan", "delegate_to": "planner", "context": "detailed description with all gathered requirements"}
{"action": "code", "delegate_to": "coder", "context": "what to write", "files": ["path1", "path2"]}
{"action": "analyze_image", "delegate_to": "vision"}
{"action": "generate_image", "delegate_to": "image-gen", "prompt": "what to generate"}
{"action": "debug", "delegate_to": "debug", "error": "the error message"}
{"action": "respond", "message": "your conversational response or questions"}

For questions and conversation, ALWAYS use:
{"action": "respond", "message": "your message here"}
"#),

    ("planner", r#"{"model":"thinking","temperature":0.2,"max_tokens":4096,"tools":["file_read","file_list","list_routes","list_tables"],"max_iterations":3}"#,
     r#"You are the Planner agent for Tina4 projects.

Your job: read the project structure and create a clear, step-by-step implementation plan.

## Output Format
Write a markdown plan with:
- Objective (one sentence)
- Steps (numbered, each with file path and description)
- Files to create or modify
- Tina4 features to use

## Rules
- Read existing files before planning changes
- Follow Tina4 conventions strictly
- Suggest migrations for any schema changes
- Suggest Queue for anything that takes > 1 second
- One route per file, one model per file
"#),

    ("coder", r#"{"model":"thinking","temperature":0.1,"max_tokens":4096,"tools":["file_read","file_write"],"max_iterations":10}"#,
     r#"You are the Coder agent for Tina4 projects.

Your job: write code that follows the plan exactly. One step at a time.

## Rules
- Follow Tina4 conventions: decorators, response() pattern, ORM fields
- Write complete, working files — never partial snippets
- Include imports at the top
- Add docstrings/comments explaining what the code does
- Use built-in features: Auth, Queue, ORM, Frond, etc.
- Return the file path and complete content for each file
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
    // Defaults — Tina4 Cloud
    ChatSettings {
        thinking: ModelSettings {
            provider: "tina4".into(),
            model: "tina4-v1".into(),
            url: "https://api.tina4.com/v1/chat/completions".into(),
            api_key: String::new(),
        },
        vision: ModelSettings {
            provider: "tina4".into(),
            model: "tina4-v1".into(),
            url: "https://api.tina4.com/v1/chat/completions".into(),
            api_key: String::new(),
        },
        image_gen: ModelSettings {
            provider: "custom".into(),
            model: String::new(),
            url: String::new(),
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

/// Make an LLM call (blocking, non-streaming).
pub async fn llm_call(
    settings: &ModelSettings,
    system_prompt: &str,
    messages: &[LlmMessage],
    max_tokens: u32,
    temperature: f32,
) -> Result<String, String> {
    let client = reqwest::Client::new();

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
        model: settings.model.clone(),
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

                // Step 1: Call supervisor with conversation history
                let supervisor_prompt = supervisor.map(|s| s.system_prompt.as_str()).unwrap_or("");

                // Build message history — last 16 messages for context
                let history = load_history(&project_dir);
                let recent: Vec<&ChatMessage> = history.iter()
                    .filter(|m| m.thread_id == chat_req.thread_id)
                    .rev().take(16).collect::<Vec<_>>().into_iter().rev().collect();

                let mut msgs: Vec<LlmMessage> = recent.iter().map(|m| {
                    let mut content = m.content.clone();
                    // Truncate long messages to save tokens
                    if content.len() > 500 {
                        content = format!("{}...(truncated)", &content[..500]);
                    }
                    LlmMessage {
                        role: if m.role == "user" { "user".into() } else { "assistant".into() },
                        content,
                    }
                }).collect();
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

                        // Build planner context with project info
                        let planner_msg = format!(
                            "Create an implementation plan for the following request:\n\n{}\n\nProject directory: {}",
                            ctx, project_dir.display()
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

                                // Send plan as message
                                let plan_escaped = plan_content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                                sse_event(&mut stream, "message", &format!(
                                    "{{\"content\":\"{}\",\"agent\":\"planner\",\"plan_file\":\".tina4/plans/{}\"}}",
                                    plan_escaped, plan_name
                                )).await;

                                // Send plan approval request
                                sse_event(&mut stream, "plan", &sse_json(&serde_json::json!({
                                    "file": format!(".tina4/plans/{}", plan_name),
                                    "approve": true
                                }))).await;

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
