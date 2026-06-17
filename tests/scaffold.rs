//! Integration tests for the scaffolding tools (`tina4 generate ...`, and a
//! gated `tina4 init`).
//!
//! `generate` is pure templating + project detection — no language runtime
//! needed — so the whole matrix (4 languages × {model, route, migration,
//! middleware}) runs deterministically here, including regression coverage for
//! the two bugs found by manual testing:
//!   * migration names must be slugified (no spaces in the filename / SQL)
//!   * `generate route foo` must NOT leave a spurious empty `src/routes/foo/`
//!
//! The `init` tests need the language toolchains + network, so they are
//! `#[ignore]`d by default — run them with `cargo test -- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_tina4");
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A throwaway project directory pre-seeded with the marker file(s) that make
/// `detect_language()` recognise the language. Each test gets a unique dir, so
/// the suite is parallel-safe (no global cwd mutation — every command runs with
/// `.current_dir(dir)`).
struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(markers: &[(&str, &str)]) -> Project {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tina4-scaffold-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp project dir");
        for (rel, content) in markers {
            fs::write(dir.join(rel), content).expect("write marker");
        }
        Project { dir }
    }

    fn python() -> Project { Project::new(&[("app.py", "")]) }
    fn php() -> Project { Project::new(&[("composer.json", r#"{"require":{"tina4stack/tina4-php":"^3.0"}}"#)]) }
    fn ruby() -> Project { Project::new(&[("app.rb", "")]) }
    fn nodejs() -> Project { Project::new(&[("app.ts", "")]) }

    fn generate(&self, what: &str, name: &str) -> Output {
        Command::new(BIN)
            .args(["generate", what, name])
            .current_dir(&self.dir)
            .output()
            .expect("run tina4 generate")
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_else(|_| panic!("read {rel}"))
    }

    /// The single .sql file under migrations/ (filename only).
    fn migration_filename(&self) -> String {
        let mut entries: Vec<String> = fs::read_dir(self.path("migrations"))
            .expect("migrations dir exists")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".sql"))
            .collect();
        entries.sort();
        entries.pop().expect("at least one migration .sql")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn ok(out: &Output) -> bool {
    out.status.success()
}

// ── model ─────────────────────────────────────────────────────────

#[test]
fn model_python() {
    let p = Project::python();
    assert!(ok(&p.generate("model", "Product")));
    assert!(p.path("src/orm/Product.py").is_file());
    assert!(p.read("src/orm/Product.py").contains("class Product(ORM)"));
}

#[test]
fn model_php() {
    let p = Project::php();
    assert!(ok(&p.generate("model", "Product")));
    assert!(p.path("src/orm/Product.php").is_file());
    assert!(p.read("src/orm/Product.php").contains("class Product extends"));
}

#[test]
fn model_ruby() {
    let p = Project::ruby();
    assert!(ok(&p.generate("model", "Product")));
    // Ruby filenames are snake_case.
    assert!(p.path("src/orm/product.rb").is_file());
    assert!(p.read("src/orm/product.rb").contains("class Product < Tina4::ORM"));
}

#[test]
fn model_nodejs() {
    let p = Project::nodejs();
    assert!(ok(&p.generate("model", "Product")));
    assert!(p.path("src/models/Product.ts").is_file());
    assert!(p.read("src/models/Product.ts").contains("class Product"));
}

// ── route (regression: no spurious directory) ──────────────────────

#[test]
fn route_python_no_spurious_dir() {
    let p = Project::python();
    assert!(ok(&p.generate("route", "products")));
    assert!(p.path("src/routes/products.py").is_file());
    // The bug: a spurious empty src/routes/products/ directory.
    assert!(!p.path("src/routes/products").is_dir(), "spurious src/routes/products/ dir");
    assert!(p.read("src/routes/products.py").contains("/products"));
}

#[test]
fn route_php_no_spurious_dir() {
    let p = Project::php();
    assert!(ok(&p.generate("route", "products")));
    assert!(p.path("src/routes/products.php").is_file());
    assert!(!p.path("src/routes/products").is_dir(), "spurious src/routes/products/ dir");
}

#[test]
fn route_ruby_no_spurious_dir() {
    let p = Project::ruby();
    assert!(ok(&p.generate("route", "products")));
    assert!(p.path("src/routes/products.rb").is_file());
    assert!(!p.path("src/routes/products").is_dir(), "spurious src/routes/products/ dir");
}

#[test]
fn route_nodejs_is_directory_based() {
    // Node uses file-based routing by design: src/routes/products/get.ts etc.
    let p = Project::nodejs();
    assert!(ok(&p.generate("route", "products")));
    assert!(p.path("src/routes/products/get.ts").is_file());
    assert!(p.path("src/routes/products/post.ts").is_file());
    assert!(p.path("src/routes/products/[id]/get.ts").is_file());
}

#[test]
fn route_nested_path_creates_parent_dirs() {
    // `generate route api/users` → src/routes/api/users.py, with src/routes/api/
    // existing as the parent (NOT src/routes/api/users/ as a dir).
    let p = Project::python();
    assert!(ok(&p.generate("route", "api/users")));
    assert!(p.path("src/routes/api/users.py").is_file());
    assert!(p.path("src/routes/api").is_dir());
    assert!(!p.path("src/routes/api/users").is_dir(), "spurious nested dir");
}

// ── migration (regression: slugified name, valid SQL identifier) ────

#[test]
fn migration_slugifies_spaces() {
    let p = Project::python();
    assert!(ok(&p.generate("migration", "create products")));
    let fname = p.migration_filename();
    assert!(!fname.contains(' '), "migration filename has a space: {fname}");
    assert!(fname.ends_with("_create_products.sql"), "unexpected name: {fname}");
    // <14-digit-timestamp>_create_products.sql
    let ts = &fname[..14];
    assert!(ts.chars().all(|c| c.is_ascii_digit()), "no timestamp prefix: {fname}");
    let sql = p.read(&format!("migrations/{fname}"));
    assert!(sql.contains("CREATE TABLE products"), "bad table name in SQL:\n{sql}");
}

#[test]
fn migration_handles_mixed_text() {
    let p = Project::python();
    assert!(ok(&p.generate("migration", "Add Users Table")));
    let fname = p.migration_filename();
    assert!(fname.ends_with("_add_users_table.sql"), "unexpected name: {fname}");
}

// ── middleware ──────────────────────────────────────────────────────

#[test]
fn middleware_python_snake_case_file() {
    let p = Project::python();
    assert!(ok(&p.generate("middleware", "RateLimit")));
    assert!(p.path("src/middleware/rate_limit.py").is_file());
    assert!(p.read("src/middleware/rate_limit.py").contains("class RateLimit"));
}

#[test]
fn middleware_all_languages() {
    let php = Project::php();
    assert!(ok(&php.generate("middleware", "RateLimit")));
    assert!(php.path("src/middleware/RateLimit.php").is_file());

    let rb = Project::ruby();
    assert!(ok(&rb.generate("middleware", "RateLimit")));
    assert!(rb.path("src/middleware/rate_limit.rb").is_file());

    let node = Project::nodejs();
    assert!(ok(&node.generate("middleware", "RateLimit")));
    assert!(node.path("src/middleware/rate_limit.ts").is_file());
}

// ── safety ──────────────────────────────────────────────────────────

#[test]
fn generate_refuses_to_overwrite() {
    let p = Project::python();
    assert!(ok(&p.generate("model", "Product")));
    // Second time must fail rather than clobber an existing file.
    assert!(!ok(&p.generate("model", "Product")), "should refuse to overwrite");
}

#[test]
fn generate_outside_project_fails() {
    // No marker file → not a Tina4 project → must error, not scaffold blindly.
    let empty = Project::new(&[]);
    assert!(!ok(&empty.generate("model", "Product")));
}

// ── init (gated: needs the language toolchain + network) ────────────

#[test]
#[ignore = "needs uv + network; run with: cargo test -- --ignored"]
fn init_python_scaffolds_runnable_project() {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("tina4-init-{}-{}", std::process::id(), n));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let out = Command::new(BIN)
        .args(["init", "python", "app"])
        .current_dir(&base)
        .env("TINA4_INIT_NO_SERVE", "1")
        .output()
        .expect("run tina4 init");
    assert!(out.status.success(), "init failed: {}", String::from_utf8_lossy(&out.stderr));
    let proj = base.join("app");
    assert!(proj.join("app.py").is_file());
    assert!(proj.join("pyproject.toml").is_file());
    assert!(Path::new(&proj).join(".env").is_file());
    let _ = fs::remove_dir_all(&base);
}
