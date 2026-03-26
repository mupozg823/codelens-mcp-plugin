# CodeLens MCP

**Open-source JetBrains plugin that exposes PSI-powered code intelligence via MCP (Model Context Protocol).**

Serena JetBrains Plugin의 오픈소스 대안입니다. JetBrains IDE의 강력한 PSI 엔진을 활용하여 AI 코딩 어시스턴트(Claude, GPT 등)가 심볼 단위로 코드를 분석하고 편집할 수 있게 합니다.

---

## Features

| Tool                             | Description                                              |
| -------------------------------- | -------------------------------------------------------- |
| `get_current_config`             | 현재 프로젝트/IDE/도구 등록 상태와 Serena 관련 경로 조회 |
| `get_project_modules`            | IntelliJ 모듈 구조, 루트, 의존성 조회                    |
| `get_open_files`                 | 현재 IDE에서 열린 파일과 선택된 파일 조회                |
| `get_file_problems`              | IntelliJ 하이라이팅 기반 파일 진단/문제 조회             |
| `get_symbols_overview`           | 파일/디렉토리의 심볼 구조 개요 반환                      |
| `find_symbol`                    | 이름으로 심볼 검색 (본문 포함 옵션)                      |
| `find_referencing_symbols`       | 심볼 참조 추적                                           |
| `search_for_pattern`             | 정규식 기반 코드 검색                                    |
| `get_type_hierarchy`             | 클래스의 상속/구현 관계와 멤버 구조 조회                 |
| `find_referencing_code_snippets` | 참조 지점의 주변 코드 스니펫 조회                        |
| `replace_symbol_body`            | 심볼 본문 교체                                           |
| `insert_after_symbol`            | 심볼 뒤에 코드 삽입                                      |
| `insert_before_symbol`           | 심볼 앞에 코드 삽입                                      |
| `rename_symbol`                  | IDE 리팩토링 기반 심볼 이름 변경                         |
| `read_file`                      | 파일 내용 일부 또는 전체 읽기                            |
| `list_dir`                       | 디렉터리 목록 조회                                       |
| `find_file`                      | 파일명 패턴으로 파일 검색                                |
| `create_text_file`               | 텍스트 파일 생성                                         |
| `delete_lines`                   | 파일의 특정 라인 삭제                                    |
| `insert_at_line`                 | 특정 라인에 텍스트 삽입                                  |
| `replace_lines`                  | 특정 라인 범위 교체                                      |
| `replace_content`                | 파일 내용 패턴 치환                                      |

### Serena Compatible

도구 이름과 파라미터가 Serena MCP와 동일하여, 기존 CLAUDE.md의 Serena-First 규칙을 수정 없이 사용할 수 있습니다.

최신 Serena 문서 기준으로 `ide`/`codex` context, 프로젝트 활성화, `.serena/memories/` 구조가 워크플로의 핵심입니다. CodeLens는 이에 맞춰 `get_current_config` 에서 현재 프로젝트, 인덱싱 상태, 등록된 도구, `.serena` 메모리 디렉터리 존재 여부를 함께 반환합니다.

### Supported Languages

| Language | Status   | Adapter                                  |
| -------- | -------- | ---------------------------------------- |
| Java     | ✅ Full  | `JavaLanguageAdapter`                    |
| Kotlin   | ✅ Full  | `KotlinLanguageAdapter`                  |
| Others   | ⚡ Basic | `GenericLanguageAdapter` (PSI 기반 폴백) |

> Python, JavaScript/TypeScript, Go 등은 추후 전용 어댑터 추가 예정

---

## Requirements

- **JetBrains IDE** 2025.1+ (IntelliJ IDEA, PyCharm, WebStorm 등)
- **JDK 21** (빌드 시)

---

## Installation

### From Marketplace (준비 중)

Settings → Plugins → Marketplace → "CodeLens MCP" 검색 → Install

### From Source

```bash
git clone https://github.com/mupozg823/codelens-mcp-plugin.git
cd codelens-mcp-plugin
./gradlew buildPlugin
```

빌드된 플러그인: `build/distributions/codelens-mcp-plugin-0.2.1.zip`

IDE에서 설치: Settings → Plugins → ⚙️ → Install Plugin from Disk

---

## Connecting to AI Assistants

### Claude Desktop

`claude_desktop_config.json`에 추가:

```json
{
  "mcpServers": {
    "jetbrains": {
      "command": "python3",
      "args": ["/absolute/path/to/codelens-mcp-plugin/jetbrains_sse_bridge.py"]
    }
  }
}
```

### Claude Code

```bash
claude mcp add jetbrains -- python3 /absolute/path/to/codelens-mcp-plugin/jetbrains_sse_bridge.py
```

### 다른 MCP 클라이언트

JetBrains의 내장 MCP Server를 통해 연결됩니다. 최신 IDE에서는 로컬 SSE endpoint를 직접 노출할 수 있으며, MCP 클라이언트나 bridge는 현재 IDE가 제공하는 transport와 호환되어야 합니다.

repo에 포함된 bridge:

```bash
python3 jetbrains_sse_bridge.py
```

필요하면 포트를 직접 지정할 수 있습니다:

```bash
python3 jetbrains_sse_bridge.py --host 127.0.0.1 --port 64342 --sse-path /sse
```

문제가 생기면 먼저 다음으로 IDE endpoint 자체를 검증하세요:

```bash
python3 test-mcp-tools.py
```

`tools/list` 가 실패하지만 `/sse` 는 열린 경우에는 CodeLens 플러그인 문제가 아니라 bridge/proxy 버전 불일치일 가능성이 큽니다. 이 경우 `jetbrains_sse_bridge.py` 를 먼저 사용하세요.

---

## Development

### Prerequisites

| Tool          | Version              |
| ------------- | -------------------- |
| JDK           | 21+                  |
| IntelliJ IDEA | 2025.1+              |
| Gradle        | 8.13+ (wrapper 포함) |

### Build & Run

```bash
# 개발 IDE에서 플러그인 실행 (샌드박스)
./gradlew runIde

# 빌드
./gradlew buildPlugin

# 테스트
./gradlew test

# 플러그인 검증
./gradlew verifyPlugin
```

### Project Structure

```
src/main/kotlin/com/codelens/
├── model/          # Data classes (SymbolInfo, ReferenceInfo, etc.)
├── util/           # PsiUtils, JsonBuilder
├── services/       # PSI service layer (interfaces + implementations)
│   ├── LanguageAdapter.kt          # Language-specific PSI abstraction
│   ├── JavaLanguageAdapter.kt      # Java PSI support
│   ├── KotlinLanguageAdapter.kt    # Kotlin PSI support
│   ├── SymbolService[Impl].kt      # Symbol analysis
│   ├── ReferenceService[Impl].kt   # Reference tracking
│   ├── SearchService[Impl].kt      # Pattern search
│   └── ModificationService[Impl].kt # Code modifications
├── tools/          # MCP tools (one class per tool)
│   ├── BaseMcpTool.kt             # Abstract base
│   ├── ToolRegistry.kt            # Tool registration
│   └── [18 tool implementations]
└── plugin/         # Plugin lifecycle & UI
    ├── CodeLensStartupActivity.kt
    ├── CodeLensConfigurable.kt     # Settings page
    └── [Action classes]
```

---

## Architecture

```
Claude Code / Claude Desktop
         │ MCP Protocol (Stdio)
         │
Compatible MCP bridge or client
         │ MCP over IDE-managed transport
         │
JetBrains MCP Server
         │
JetBrains IDE
  └── CodeLens MCP Plugin
        ├── MCP Tools (22 tools)
        ├── PSI Service Layer
        └── Language Adapters (Java, Kotlin, Generic)
              └── IntelliJ PSI Engine
```

---

## Comparison

| Feature              | Serena MCP (Free)      | Serena JetBrains (Paid) | CodeLens MCP    |
| -------------------- | ---------------------- | ----------------------- | --------------- |
| Code Analysis Engine | LSP                    | JetBrains PSI           | JetBrains PSI   |
| License              | Open Source            | Paid                    | **Open Source** |
| Language Support     | 40+ (via LSP)          | All JetBrains           | All JetBrains   |
| Library Indexing     | Partial                | Full                    | Full            |
| Extra Setup          | Language Server needed | Plugin only             | **Plugin only** |

---

## Roadmap

- [x] JetBrains 2025.2+ `mcp.tool` 확장 포인트 통합
- [ ] Python 언어 어댑터
- [ ] JavaScript/TypeScript 언어 어댑터
- [ ] Go 언어 어댑터
- [ ] GotoSymbolContributor 기반 고속 검색
- [ ] Structural Search and Replace 통합
- [ ] 성능 최적화 (캐싱, 비동기)
- [ ] `get_file_problems` 에 quick-fix / suppress / scope 정보 추가
- [ ] JetBrains Marketplace 배포

---

## Contributing

기여를 환영합니다! [CONTRIBUTING.md](CONTRIBUTING.md)를 참고해주세요.

---

## License

[Apache License 2.0](LICENSE)

---

## Acknowledgments

- [Serena](https://github.com/oraios/serena) — 영감을 준 프로젝트
- [JetBrains](https://www.jetbrains.com/) — IntelliJ Platform SDK
- [MCP](https://modelcontextprotocol.io/) — Model Context Protocol 표준
