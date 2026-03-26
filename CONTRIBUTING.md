# Contributing to CodeLens MCP

기여를 환영합니다! 이 가이드를 읽고 참여해주세요.

## 시작하기

### 개발 환경

1. **JDK 21** 설치
2. **IntelliJ IDEA 2025.1+** 설치 (Community 또는 Ultimate)
3. 레포 클론:
   ```bash
   git clone https://github.com/YOUR_USERNAME/codelens-mcp-plugin.git
   cd codelens-mcp-plugin
   ```
4. IntelliJ에서 프로젝트 열기 → Gradle sync 완료 대기

### 빌드 & 테스트

```bash
./gradlew buildPlugin    # 빌드
./gradlew test           # 테스트
./gradlew runIde         # 샌드박스 IDE에서 실행
./gradlew verifyPlugin   # 플러그인 검증
```

## 기여 방법

### Issue 먼저

새로운 기능이나 버그 수정을 시작하기 전에 먼저 Issue를 열어주세요. 중복 작업을 방지하고, 방향을 미리 논의할 수 있습니다.

### Pull Request 프로세스

1. `main`에서 feature 브랜치 생성: `git checkout -b feature/my-feature`
2. 코드 작성 및 테스트 추가
3. `./gradlew test && ./gradlew verifyPlugin` 통과 확인
4. PR 생성 — 변경 사항 설명 포함
5. 코드 리뷰 후 머지

### 커밋 메시지 형식

```
<type>: <description>

[optional body]
```

타입:
- `feat`: 새 기능
- `fix`: 버그 수정
- `refactor`: 리팩토링
- `docs`: 문서
- `test`: 테스트
- `chore`: 빌드/CI 설정

## 기여 영역

### 언어 어댑터 추가

새로운 언어 지원은 가장 환영하는 기여입니다:

1. `services/` 에 `XxxLanguageAdapter.kt` 생성
2. `LanguageAdapter` 인터페이스 구현
3. `META-INF/` 에 `xxx-support.xml` 추가
4. `plugin.xml` 에 `optional depends` 추가
5. 테스트 작성

### 현재 필요한 언어

- Python (`Pythonid` 플러그인)
- JavaScript/TypeScript (`JavaScript` 플러그인)
- Go (`org.jetbrains.plugins.go`)
- Rust (`org.rust.lang`)
- PHP (`com.jetbrains.php`)

### MCP 도구 추가

1. `tools/` 에 `XxxTool.kt` 생성
2. `BaseMcpTool` 상속
3. `ToolRegistry.kt` 에 등록
4. 테스트 작성

## 코드 스타일

- Kotlin 공식 코딩 컨벤션 준수
- `any` 대신 `unknown` 사용 (TypeScript 참여 시)
- 500줄 이하 파일 유지
- 의미 있는 테스트 작성

## 라이선스

기여한 코드는 [Apache License 2.0](LICENSE) 하에 배포됩니다.
