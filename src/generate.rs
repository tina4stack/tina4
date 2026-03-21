use crate::detect;
use colored::Colorize;
use std::fs;
use std::path::Path;

/// Run the `tina4 generate <what> <name>` command.
pub fn run(what: &str, name: &str) {
    let lang = match detect::detect_language() {
        Some(info) => info.language,
        None => {
            eprintln!(
                "{} No Tina4 project detected. Run: tina4 init <language> <path>",
                "✗".red()
            );
            std::process::exit(1);
        }
    };

    match what {
        "model" => generate_model(&lang, name),
        "route" => generate_route(&lang, name),
        "migration" => generate_migration(name),
        "middleware" => generate_middleware(&lang, name),
        _ => {
            eprintln!(
                "{} Unknown generator: {}",
                "✗".red(),
                what.yellow()
            );
            eprintln!("  Available generators: model, route, migration, middleware");
            std::process::exit(1);
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

fn ensure_dir(dir: &str) {
    if !Path::new(dir).exists() {
        fs::create_dir_all(dir).unwrap_or_else(|e| {
            eprintln!("{} Failed to create directory {}: {}", "✗".red(), dir, e);
            std::process::exit(1);
        });
    }
}

fn write_file(path: &str, content: &str) {
    if Path::new(path).exists() {
        eprintln!(
            "{} File already exists: {}",
            "✗".red(),
            path.yellow()
        );
        std::process::exit(1);
    }
    fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("{} Failed to write {}: {}", "✗".red(), path, e);
        std::process::exit(1);
    });
    println!("{} Created {}", "✓".green(), path.cyan());
}

fn to_snake(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap());
    }
    result
}

fn to_plural(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with('s') {
        lower
    } else if lower.ends_with('y') {
        format!("{}ies", &lower[..lower.len() - 1])
    } else {
        format!("{}s", lower)
    }
}

// ── Model ────────────────────────────────────────────────────────

fn generate_model(lang: &str, name: &str) {
    match lang {
        "python" => {
            let dir = "src/orm";
            ensure_dir(dir);
            let path = format!("{}/{}.py", dir, name);
            let content = format!(
                r#"from tina4_python import ORM, IntegerField, StringField


class {name}(ORM):
    id = IntegerField(primary_key=True, auto_increment=True)
    name = StringField()
    email = StringField()
"#,
                name = name
            );
            write_file(&path, &content);
        }
        "php" => {
            let dir = "src/orm";
            ensure_dir(dir);
            let path = format!("{}/{}.php", dir, name);
            let content = format!(
                r#"<?php

class {name} extends \Tina4\ORM {{
    public ?int $id = null;
    public string $name = '';
    public string $email = '';
}}
"#,
                name = name
            );
            write_file(&path, &content);
        }
        "ruby" => {
            let dir = "src/orm";
            ensure_dir(dir);
            let snake = to_snake(name);
            let path = format!("{}/{}.rb", dir, snake);
            let content = format!(
                r#"class {name} < Tina4::ORM
  integer_field :id, primary_key: true, auto_increment: true
  string_field :name
  string_field :email
end
"#,
                name = name
            );
            write_file(&path, &content);
        }
        "nodejs" => {
            let dir = "src/models";
            ensure_dir(dir);
            let table = to_plural(name);
            let path = format!("{}/{}.ts", dir, name);
            let content = format!(
                r#"import {{ BaseModel }} from "tina4-nodejs";

export class {name} extends BaseModel {{
  static tableName = "{table}";
  static fields = {{
    id: {{ type: "integer", primaryKey: true, autoIncrement: true }},
    name: {{ type: "string" }},
    email: {{ type: "string" }},
  }};
}}
"#,
                name = name,
                table = table,
            );
            write_file(&path, &content);
        }
        _ => unsupported(lang),
    }
}

// ── Route ────────────────────────────────────────────────────────

fn generate_route(lang: &str, name: &str) {
    // name is a URL path like /api/users — strip leading slash
    let route_path = name.trim_start_matches('/');

    match lang {
        "python" => {
            let dir = format!("src/routes/{}", route_path);
            ensure_dir(&dir);
            let path = format!("{}.py", dir.trim_end_matches('/'));
            // Flatten to single file with all CRUD verbs
            let content = format!(
                r#"from tina4_python import get, post, put, delete


@get("/{route}")
async def get_list(request, response):
    """List all."""
    return response.json({{"data": []}})


@get("/{route}/{{id}}")
async def get_one(request, response):
    """Get by id."""
    return response.json({{"data": {{}}}})


@post("/{route}")
async def create(request, response):
    """Create new."""
    return response.json({{"message": "created"}}, 201)


@put("/{route}/{{id}}")
async def update(request, response):
    """Update by id."""
    return response.json({{"message": "updated"}})


@delete("/{route}/{{id}}")
async def remove(request, response):
    """Delete by id."""
    return response.json({{"message": "deleted"}})
"#,
                route = route_path
            );
            write_file(&path, &content);
        }
        "php" => {
            let dir = format!("src/routes/{}", route_path);
            ensure_dir(&dir);
            let path = format!("{}.php", dir.trim_end_matches('/'));
            let content = format!(
                r#"<?php

\Tina4\Get::add("/{route}", function (\Tina4\Request $request, \Tina4\Response $response) {{
    return $response->json(["data" => []]);
}});

\Tina4\Get::add("/{route}/{{id}}", function (\Tina4\Request $request, \Tina4\Response $response) {{
    return $response->json(["data" => []]);
}});

\Tina4\Post::add("/{route}", function (\Tina4\Request $request, \Tina4\Response $response) {{
    return $response->json(["message" => "created"], 201);
}});

\Tina4\Put::add("/{route}/{{id}}", function (\Tina4\Request $request, \Tina4\Response $response) {{
    return $response->json(["message" => "updated"]);
}});

\Tina4\Delete::add("/{route}/{{id}}", function (\Tina4\Request $request, \Tina4\Response $response) {{
    return $response->json(["message" => "deleted"]);
}});
"#,
                route = route_path
            );
            write_file(&path, &content);
        }
        "ruby" => {
            let dir = format!("src/routes/{}", route_path);
            ensure_dir(&dir);
            let path = format!("{}.rb", dir.trim_end_matches('/'));
            let content = format!(
                r#"Tina4.get "/{route}" do |request, response|
  response.json(data: [])
end

Tina4.get "/{route}/:id" do |request, response|
  response.json(data: {{}})
end

Tina4.post "/{route}" do |request, response|
  response.json({{ message: "created" }}, 201)
end

Tina4.put "/{route}/:id" do |request, response|
  response.json(message: "updated")
end

Tina4.delete "/{route}/:id" do |request, response|
  response.json(message: "deleted")
end
"#,
                route = route_path
            );
            write_file(&path, &content);
        }
        "nodejs" => {
            // Node.js uses file-based routing: one file per method
            let base = format!("src/routes/{}", route_path);
            let id_dir = format!("{}/[id]", base);
            ensure_dir(&base);
            ensure_dir(&id_dir);

            // GET list
            let content = r#"import type { Tina4Request, Tina4Response } from "tina4-nodejs";

export const meta = { summary: "List all", tags: ["auto-generated"] };

export default async function (req: Tina4Request, res: Tina4Response) {
  res.json({ data: [] });
}
"#;
            write_file(&format!("{}/get.ts", base), content);

            // POST create
            let content = r#"import type { Tina4Request, Tina4Response } from "tina4-nodejs";

export const meta = { summary: "Create new", tags: ["auto-generated"] };

export default async function (req: Tina4Request, res: Tina4Response) {
  res.json({ message: "created" }, 201);
}
"#;
            write_file(&format!("{}/post.ts", base), content);

            // GET by id
            let content = r#"import type { Tina4Request, Tina4Response } from "tina4-nodejs";

export const meta = { summary: "Get by id", tags: ["auto-generated"] };

export default async function (req: Tina4Request, res: Tina4Response) {
  const { id } = req.params;
  res.json({ data: { id } });
}
"#;
            write_file(&format!("{}/get.ts", id_dir), content);

            // PUT by id
            let content = r#"import type { Tina4Request, Tina4Response } from "tina4-nodejs";

export const meta = { summary: "Update by id", tags: ["auto-generated"] };

export default async function (req: Tina4Request, res: Tina4Response) {
  const { id } = req.params;
  res.json({ message: "updated", id });
}
"#;
            write_file(&format!("{}/put.ts", id_dir), content);

            // DELETE by id
            let content = r#"import type { Tina4Request, Tina4Response } from "tina4-nodejs";

export const meta = { summary: "Delete by id", tags: ["auto-generated"] };

export default async function (req: Tina4Request, res: Tina4Response) {
  const { id } = req.params;
  res.json({ message: "deleted", id });
}
"#;
            write_file(&format!("{}/delete.ts", id_dir), content);
        }
        _ => unsupported(lang),
    }
}

// ── Migration ────────────────────────────────────────────────────

fn generate_migration(name: &str) {
    let dir = "migrations";
    ensure_dir(dir);

    let now = chrono_now();
    let table = to_plural(name.trim_start_matches("create_"));
    let path = format!("{}/{}_{}.sql", dir, now, name);

    let content = format!(
        r#"-- Migration: {name}
-- Created: {timestamp}

CREATE TABLE {table} (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
"#,
        name = name,
        timestamp = now_iso(),
        table = table,
    );

    write_file(&path, &content);
}

/// Returns YYYYMMDDHHMMSS for migration filenames.
fn chrono_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Convert epoch seconds to date-time components
    let secs = now as i64;
    let days = secs / 86400;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 to y/m/d
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        year, month, day, hours, minutes, seconds
    )
}

fn now_iso() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let secs = now as i64;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ── Middleware ────────────────────────────────────────────────────

fn generate_middleware(lang: &str, name: &str) {
    let dir = "src/middleware";
    ensure_dir(dir);

    match lang {
        "python" => {
            let snake = to_snake(name);
            let path = format!("{}/{}.py", dir, snake);
            let content = format!(
                r#"from tina4_python import Middleware


class {name}(Middleware):
    async def process(self, request, response):
        auth = request.headers.get("Authorization")
        if not auth:
            return response.json({{"error": "Unauthorized"}}, 401)
        return None
"#,
                name = name
            );
            write_file(&path, &content);
        }
        "php" => {
            let path = format!("{}/{}.php", dir, name);
            let content = format!(
                r#"<?php

class {name} extends \Tina4\Middleware {{
    public function process(\Tina4\Request $request, \Tina4\Response $response): ?\Tina4\Response {{
        $auth = $request->getHeader("Authorization");
        if (empty($auth)) {{
            return $response->json(["error" => "Unauthorized"], 401);
        }}
        return null;
    }}
}}
"#,
                name = name
            );
            write_file(&path, &content);
        }
        "ruby" => {
            let snake = to_snake(name);
            let path = format!("{}/{}.rb", dir, snake);
            let content = format!(
                r#"class {name} < Tina4::Middleware
  def process(request, response)
    auth = request.headers["Authorization"]
    return response.json({{ error: "Unauthorized" }}, 401) unless auth

    nil
  end
end
"#,
                name = name
            );
            write_file(&path, &content);
        }
        "nodejs" => {
            let snake = to_snake(name);
            let path = format!("{}/{}.ts", dir, snake);
            let content = format!(
                r#"import type {{ Tina4Request, Tina4Response }} from "tina4-nodejs";

/**
 * {name} middleware — checks for Authorization header.
 */
export default async function {camel}(
  req: Tina4Request,
  res: Tina4Response,
  next: () => Promise<void>,
): Promise<void> {{
  const auth = req.headers["authorization"];
  if (!auth) {{
    res.json({{ error: "Unauthorized" }}, 401);
    return;
  }}
  await next();
}}
"#,
                name = name,
                camel = to_camel(name),
            );
            write_file(&path, &content);
        }
        _ => unsupported(lang),
    }
}

fn to_camel(name: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for (i, ch) in name.chars().enumerate() {
        if ch == '_' || ch == '-' {
            capitalize_next = true;
        } else if i == 0 {
            result.push(ch.to_lowercase().next().unwrap());
        } else if capitalize_next {
            result.push(ch.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn unsupported(lang: &str) {
    eprintln!(
        "{} Unsupported language: {}",
        "✗".red(),
        lang.yellow()
    );
    std::process::exit(1);
}
