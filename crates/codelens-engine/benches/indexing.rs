use codelens_engine::{
    GraphCache, ProjectRoot, SymbolIndex, content_hash, find_circular_dependencies,
    get_blast_radius, get_callers, get_symbols_overview, search_for_pattern, search_symbols_hybrid,
};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::fs;

/// Create a realistic multi-language fixture project for benchmarking.
fn create_fixture() -> (tempfile::TempDir, ProjectRoot) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Python files
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/service.py"),
        r#"
from src.models import User
from src.utils import validate

class UserService:
    def __init__(self, db):
        self.db = db

    def get_user(self, user_id: int) -> User:
        validate(user_id)
        return self.db.find(user_id)

    def create_user(self, name: str, email: str) -> User:
        user = User(name=name, email=email)
        self.db.save(user)
        return user

    def delete_user(self, user_id: int) -> bool:
        user = self.get_user(user_id)
        return self.db.delete(user)

    def list_users(self, limit: int = 100):
        return self.db.query("SELECT * FROM users LIMIT %s", limit)
"#,
    )
    .unwrap();

    fs::write(
        root.join("src/models.py"),
        r#"
class User:
    def __init__(self, name: str, email: str, id: int = None):
        self.name = name
        self.email = email
        self.id = id

    def to_dict(self):
        return {"name": self.name, "email": self.email, "id": self.id}

class Permission:
    def __init__(self, role: str):
        self.role = role

    def can_access(self, resource: str) -> bool:
        return self.role == "admin"
"#,
    )
    .unwrap();

    fs::write(
        root.join("src/utils.py"),
        r#"
import re

def validate(value):
    if value is None:
        raise ValueError("value cannot be None")
    return True

def sanitize(text: str) -> str:
    return re.sub(r'[<>]', '', text)

def format_name(first: str, last: str) -> str:
    return f"{first} {last}".strip()
"#,
    )
    .unwrap();

    // TypeScript files
    fs::write(
        root.join("src/api.ts"),
        r#"
import { UserService } from './service';
import { Request, Response } from 'express';

export class ApiController {
    constructor(private userService: UserService) {}

    async getUser(req: Request, res: Response): Promise<void> {
        const user = await this.userService.findById(req.params.id);
        res.json(user);
    }

    async createUser(req: Request, res: Response): Promise<void> {
        const user = await this.userService.create(req.body);
        res.status(201).json(user);
    }
}

export function healthCheck(): { status: string } {
    return { status: 'ok' };
}
"#,
    )
    .unwrap();

    fs::write(
        root.join("src/service.ts"),
        r#"
export interface User {
    id: string;
    name: string;
    email: string;
}

export class UserService {
    async findById(id: string): Promise<User | null> {
        return null;
    }

    async create(data: Partial<User>): Promise<User> {
        return { id: '1', name: data.name || '', email: data.email || '' };
    }

    async delete(id: string): Promise<boolean> {
        return true;
    }
}
"#,
    )
    .unwrap();

    // Rust file
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub struct Config {
    pub name: String,
    pub port: u16,
}

impl Config {
    pub fn new(name: &str, port: u16) -> Self {
        Self { name: name.to_owned(), port }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() { return Err("empty name".into()); }
        if self.port == 0 { return Err("invalid port".into()); }
        Ok(())
    }
}

pub fn run(config: &Config) -> bool {
    config.validate().is_ok()
}

pub trait Handler {
    fn handle(&self, input: &str) -> String;
}
"#,
    )
    .unwrap();

    // Java file
    fs::write(
        root.join("src/Main.java"),
        r#"
public class Main {
    public static void main(String[] args) {
        System.out.println("Hello");
    }

    public static int add(int a, int b) {
        return a + b;
    }

    public static String greet(String name) {
        return "Hello " + name;
    }
}
"#,
    )
    .unwrap();

    // Go file
    fs::write(
        root.join("src/main.go"),
        r#"
package main

type Server struct {
    Port int
    Host string
}

func NewServer(host string, port int) *Server {
    return &Server{Port: port, Host: host}
}

func (s *Server) Start() error {
    return nil
}

func (s *Server) Stop() {
}
"#,
    )
    .unwrap();

    let project = ProjectRoot::new(root).expect("project");
    (dir, project)
}

fn bench_refresh_all(c: &mut Criterion) {
    let (_dir, project) = create_fixture();

    c.bench_function("refresh_all (9 files, 5 langs)", |b| {
        b.iter(|| {
            let index = SymbolIndex::new_memory(black_box(project.clone()));
            index.refresh_all().unwrap();
        })
    });
}

fn bench_find_symbol_exact(c: &mut Criterion) {
    let (_dir, project) = create_fixture();
    let index = SymbolIndex::new_memory(project.clone());
    index.refresh_all().unwrap();

    c.bench_function("find_symbol exact (UserService)", |b| {
        b.iter(|| {
            index
                .find_symbol(black_box("UserService"), None, false, true, 10)
                .unwrap();
        })
    });
}

fn bench_find_symbol_fuzzy(c: &mut Criterion) {
    let (_dir, project) = create_fixture();
    let index = SymbolIndex::new_memory(project.clone());
    index.refresh_all().unwrap();

    c.bench_function("find_symbol substring (user)", |b| {
        b.iter(|| {
            index
                .find_symbol(black_box("user"), None, false, false, 50)
                .unwrap();
        })
    });
}

fn bench_get_symbols_overview(c: &mut Criterion) {
    let (_dir, project) = create_fixture();

    c.bench_function("get_symbols_overview (service.py)", |b| {
        b.iter(|| {
            get_symbols_overview(black_box(&project), "src/service.py", 2).unwrap();
        })
    });
}

fn bench_search_for_pattern(c: &mut Criterion) {
    let (_dir, project) = create_fixture();

    c.bench_function("search_for_pattern (def.*user)", |b| {
        b.iter(|| {
            search_for_pattern(black_box(&project), r"def.*user", None, 50, 0, 0).unwrap();
        })
    });
}

fn bench_search_symbols_hybrid(c: &mut Criterion) {
    let (_dir, project) = create_fixture();
    let index = SymbolIndex::new_memory(project.clone());
    index.refresh_all().unwrap();

    c.bench_function("search_symbols_hybrid (Usr, fuzzy)", |b| {
        b.iter(|| {
            search_symbols_hybrid(black_box(&project), "Usr", 20, 0.6).unwrap();
        })
    });
}

fn bench_blast_radius(c: &mut Criterion) {
    let (_dir, project) = create_fixture();
    let cache = GraphCache::new(0);

    c.bench_function("get_blast_radius (models.py)", |b| {
        b.iter(|| {
            let _ = get_blast_radius(black_box(&project), "src/models.py", 3, &cache);
        })
    });
}

fn bench_get_callers(c: &mut Criterion) {
    let (_dir, project) = create_fixture();
    let cache = GraphCache::new(0);

    c.bench_function("get_callers (validate)", |b| {
        b.iter(|| {
            get_callers(black_box(&project), "validate", None, 50, Some(&cache)).unwrap();
        })
    });
}

fn bench_circular_deps(c: &mut Criterion) {
    let (_dir, project) = create_fixture();
    let cache = GraphCache::new(0);

    c.bench_function("find_circular_dependencies", |b| {
        b.iter(|| {
            find_circular_dependencies(black_box(&project), 50, &cache).unwrap();
        })
    });
}

fn bench_content_hash(c: &mut Criterion) {
    let data = vec![0u8; 100_000]; // 100KB

    c.bench_function("content_hash (100KB)", |b| {
        b.iter(|| {
            content_hash(black_box(&data));
        })
    });
}

/// Create a stress fixture with enough repeated modules and symbols to make
/// candidate-scoring cost visible in Criterion.
fn create_scoring_stress_fixture() -> (tempfile::TempDir, ProjectRoot) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();

    for i in 0..20 {
        let content = format!(
            r#"
pub struct Widget{i} {{
    pub name: String,
    pub value: i32,
}}

impl Widget{i} {{
    pub fn new(name: &str, value: i32) -> Self {{
        Self {{ name: name.to_owned(), value }}
    }}

    pub fn process_data(&self) -> i32 {{
        self.value * 2
    }}

    pub fn validate_input(&self) -> bool {{
        !self.name.is_empty() && self.value > 0
    }}
}}
"#,
            i = i
        );
        fs::write(root.join(format!("src/widget_{i}.rs")), content).unwrap();
    }

    // Add a mod.rs to tie them together
    let mods: String = (0..20).map(|i| format!("pub mod widget_{i};\n")).collect();
    fs::write(root.join("src/lib.rs"), mods).unwrap();

    let project = ProjectRoot::new(root).expect("project");
    (dir, project)
}

fn build_scoring_stress_index() -> (tempfile::TempDir, ProjectRoot, SymbolIndex) {
    let (dir, project) = create_scoring_stress_fixture();
    let index = SymbolIndex::new_memory(project.clone());
    index.refresh_all().unwrap();
    (dir, project, index)
}

fn bench_scoring_stress_nl(c: &mut Criterion) {
    let (_dir, project, _index) = build_scoring_stress_index();

    c.bench_function("search_hybrid_stress (NL, 20 modules)", |b| {
        b.iter(|| {
            search_symbols_hybrid(black_box(&project), "process data from widget", 20, 0.6)
                .unwrap();
        })
    });
}

fn bench_scoring_stress_identifier(c: &mut Criterion) {
    let (_dir, project, _index) = build_scoring_stress_index();

    c.bench_function("search_hybrid_stress (ident, 20 modules)", |b| {
        b.iter(|| {
            search_symbols_hybrid(black_box(&project), "validate_input", 20, 0.6).unwrap();
        })
    });
}

fn bench_ranked_context_cached_stress_nl(c: &mut Criterion) {
    let (_dir, _project, index) = build_scoring_stress_index();

    c.bench_function("ranked_context_cached_stress (NL, 20 modules)", |b| {
        b.iter(|| {
            index
                .get_ranked_context_cached(
                    black_box("process data from widget"),
                    None,
                    4000,
                    false,
                    2,
                    None,
                    std::collections::HashMap::new(),
                )
                .unwrap();
        })
    });
}

fn bench_ranked_context_cached_stress_identifier(c: &mut Criterion) {
    let (_dir, _project, index) = build_scoring_stress_index();

    c.bench_function("ranked_context_cached_stress (ident, 20 modules)", |b| {
        b.iter(|| {
            index
                .get_ranked_context_cached(
                    black_box("validate_input"),
                    None,
                    4000,
                    false,
                    2,
                    None,
                    std::collections::HashMap::new(),
                )
                .unwrap();
        })
    });
}

criterion_group!(
    benches,
    bench_refresh_all,
    bench_find_symbol_exact,
    bench_find_symbol_fuzzy,
    bench_get_symbols_overview,
    bench_search_for_pattern,
    bench_search_symbols_hybrid,
    bench_blast_radius,
    bench_get_callers,
    bench_circular_deps,
    bench_content_hash,
    bench_scoring_stress_nl,
    bench_scoring_stress_identifier,
    bench_ranked_context_cached_stress_nl,
    bench_ranked_context_cached_stress_identifier,
);
criterion_main!(benches);
