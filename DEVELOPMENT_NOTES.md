# CodeLens MCP Plugin - 추가 개발 포인트

> 현재 상태: 프로젝트 스캐폴딩 + 핵심 구현 완료 (빌드 전)
> 작성일: 2026-03-26

---

## 현재 완성된 것

| 레이어 | 파일 수 | 상태 |
|--------|--------|------|
| 모델 (data class) | 4 | SymbolInfo, ReferenceInfo, SearchResult, ModificationResult |
| 유틸리티 | 2 | PsiUtils, JsonBuilder |
| 언어 어댑터 | 3 | Generic, Java, Kotlin |
| 서비스 (인터페이스+구현) | 8 | Symbol, Reference, Search, Modification |
| MCP 도구 | 9 | Base + 8개 도구 (Phase 1 + Phase 2) |
| 플러그인 인프라 | 4 | Startup, Settings, Actions |
| 빌드 설정 | 3 | build.gradle.kts, settings.gradle.kts, plugin.xml |
| **합계** | **33 파일** | |

---

## 반드시 해야 할 것 (빌드 전)

### 1. Gradle Wrapper 설정
```bash
# 프로젝트 루트에서 실행
gradle wrapper --gradle-version=8.13
```
현재 `gradlew`가 없어서 빌드할 수 없음. IntelliJ에서 프로젝트를 열면 자동으로 생성될 수도 있지만, 명시적으로 설정하는 것이 안전.

### 2. JetBrains MCP 확장 포인트 API 확인
현재 `mcp.tool` 확장 포인트는 plugin.xml에서 주석 처리됨. JetBrains 2025.2+ 환경에서:

```xml
<!-- 이 부분을 실제 API에 맞춰 업데이트 필요 -->
<mcp.tool implementation="com.codelens.tools.GetSymbolsOverviewTool"/>
```

**확인 필요:**
- `mcp.tool` 확장 포인트의 정확한 인터페이스 (어떤 메서드를 구현해야 하는지)
- BaseMcpTool이 해당 인터페이스를 상속하도록 수정
- IntelliJ IDEA 2025.2 SDK 문서에서 `com.intellij.mcpServer` 플러그인 API 참조

### 3. 서비스 등록 방식 수정
plugin.xml에서 서비스 인터페이스/구현 쌍으로 등록했는데, IntelliJ Platform 2024.3+에서는 `@Service` 어노테이션만으로 충분할 수 있음. 두 방식 중 하나를 선택하고 통일해야 함:

```kotlin
// 방법 A: plugin.xml 등록 (현재)
// 방법 B: @Service 어노테이션만 사용 (plugin.xml에서 제거)
@Service(Service.Level.PROJECT)
class SymbolServiceImpl(private val project: Project) : SymbolService
```

### 4. 컴파일 에러 수정
실제 빌드 시 아래 이슈들이 발생할 수 있음:
- `FilenameIndex.getAllFilesByExt` API가 변경되었을 수 있음
- `RenameProcessor` 생성자 시그니처가 다를 수 있음
- Kotlin PSI 클래스 (`KtFile`, `KtClass` 등)의 패키지 변경 가능

---

## 기능적으로 더 개발할 것

### 5. 추가 언어 어댑터 (Priority: High)
현재 Java, Kotlin만 구현됨. 다음 언어 추가 필요:

| 언어 | PSI 클래스 | 플러그인 의존성 |
|------|-----------|---------------|
| Python | PyFunction, PyClass | Pythonid (PyCharm 전용) |
| JavaScript/TypeScript | JSFunction, ES6Class | JavaScript |
| Go | GoFunctionDeclaration, GoTypeSpec | org.jetbrains.plugins.go |
| Rust | RsFunction, RsStructItem | org.rust.lang |

각 언어마다 `LanguageAdapter` 구현체를 만들고, 해당 언어 플러그인이 설치되어 있을 때만 로드되도록 `optional depends` 처리.

### 6. mcp.tool 확장 포인트 통합 (Priority: High)
JetBrains 내장 MCP가 사용하는 실제 인터페이스를 확인한 후, BaseMcpTool을 해당 인터페이스에 맞게 수정:

```kotlin
// 예상되는 인터페이스 (확인 필요)
interface McpTool {
    val name: String
    val description: String
    val inputSchema: JsonObject
    suspend fun execute(args: JsonObject): McpToolResult
}
```

### 7. GotoSymbolContributor 기반 검색 (Priority: Medium)
현재 `findSymbolInProject`는 파일을 순회하며 심볼을 찾는데, 이는 비효율적. IntelliJ의 `ChooseByNameContributor` 또는 `GotoSymbolModel2`를 활용하면 인덱스 기반으로 훨씬 빠르게 검색 가능:

```kotlin
// 더 효율적인 심볼 검색
val model = GotoSymbolModel2(project)
val elements = model.getElementsByName(name, false, pattern)
```

### 8. 타입 계층 도구 (Priority: Medium)
`get_type_hierarchy` 도구 추가 — 클래스의 상속 관계를 보여주는 것:

```kotlin
class GetTypeHierarchyTool : BaseMcpTool() {
    // TypeHierarchyProvider.createHierarchyBrowser() 활용
}
```

### 9. Structural Search (Priority: Low)
IntelliJ의 Structural Search and Replace (SSR) API를 활용한 고급 패턴 매칭:

```kotlin
// 예: "method that returns null" 같은 구조적 패턴 검색
val searchContext = StructuralSearchProfile.createSearchContext(...)
```

### 10. 성능 최적화 (Priority: Medium)
- **캐싱**: 심볼 목록을 파일 변경까지 캐시
- **비동기**: 긴 검색은 코루틴으로 처리
- **결과 스트리밍**: 대량 결과를 페이지네이션으로 반환
- **인덱스 활용**: `FileBasedIndex`, `StubIndex` 적극 활용

### 11. 에러 핸들링 강화 (Priority: Medium)
- Dumb Mode 재시도 로직 (exponential backoff)
- 파일 인코딩 감지 및 처리
- 대용량 파일 (>10MB) 경고 및 스킵
- 바이너리 파일 감지

### 12. 테스트 (Priority: High)
테스트 코드 작성이 필요함:

```kotlin
// 예: SymbolService 테스트
class SymbolServiceTest : BasePlatformTestCase() {
    fun testGetSymbolsOverview() {
        val file = myFixture.configureByText("Test.java", """
            public class MyClass {
                public void myMethod() {}
            }
        """)
        val service = project.service<SymbolService>()
        val symbols = service.getSymbolsOverview(file.virtualFile.path, depth = 2)
        assertEquals(1, symbols.size)
        assertEquals("MyClass", symbols[0].name)
        assertEquals(1, symbols[0].children.size)
    }
}
```

### 13. Serena 호환성 검증 (Priority: High)
Serena MCP의 실제 요청/응답 포맷을 확인하고 완전히 호환되는지 테스트:
- Serena의 GitHub 소스에서 도구 스키마 추출
- 응답 JSON 포맷 비교
- 기존 CLAUDE.md 규칙으로 실제 동작 검증

---

## 배포 관련

### 14. Marketplace 준비
- 아이콘 디자인 (40x40 SVG)
- 스크린샷 (설정 화면, 동작 예시)
- 라이선스 파일 (Apache 2.0 또는 MIT)
- CHANGELOG.md 작성

### 15. CI/CD
```yaml
# GitHub Actions 예시
- name: Build
  run: ./gradlew buildPlugin

- name: Verify
  run: ./gradlew verifyPlugin

- name: Test
  run: ./gradlew test
```

---

## 개발 시작 순서 (권장)

1. **IntelliJ에서 프로젝트 열기** → Gradle sync → 컴파일 에러 수정
2. **mcp.tool API 확인** → BaseMcpTool 수정
3. **runIde로 샌드박스 테스트** → Phase 1 도구 동작 확인
4. **JetBrains MCP smoke test** → `python3 test-mcp-tools.py` 로 IDE SSE endpoint 실제 통신 검증
5. **Phase 2 도구 테스트** → 코드 수정 기능 검증
6. **추가 언어 어댑터** → Python, JS/TS
7. **테스트 작성** → 단위 + 통합
8. **Marketplace 배포** → 아이콘, 문서, CI/CD
