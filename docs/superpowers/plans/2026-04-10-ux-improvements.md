# TmuxBar UX 개선 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings 메뉴, 세션별 Kill, base-index 호환, 다국어(ko/en/ja/zh)를 구현하여 TmuxBar의 UX를 완성한다.

**Architecture:** i18n 모듈을 기반 레이어로 추가하고, 기존 UI/core 서비스에 Language 파라미터를 전달하는 방식. 세션 메뉴를 서브메뉴 구조로 변경하여 Attach/Kill을 분리. base-index는 TmuxAdapter 트레이트 확장으로 해결.

**Tech Stack:** Rust, objc2/objc2-app-kit (NSMenu/NSAlert), serde/toml, chrono

**설계 문서:** `docs/superpowers/specs/2026-04-10-ux-improvements-design.md`

---

## 파일 구조

| 파일 | 변경 | 책임 |
|------|------|------|
| `src/i18n.rs` | **신규** | Language enum + 모든 번역 함수 |
| `src/main.rs` | 수정 | `mod i18n;` 추가 |
| `src/models.rs` | 수정 | `AppCommand::OpenSettings` 추가, `TmuxAdapter::get_global_option` 추가 |
| `src/infra/config.rs` | 수정 | `GeneralConfig.language` 필드 추가 |
| `src/infra/tmux_client.rs` | 수정 | `get_global_option` 구현 |
| `src/ui/session_menu.rs` | 수정 | 서브메뉴 구조, Language 파라미터, Kill 태그 |
| `src/ui/menu_action_handler.rs` | 수정 | Kill 태그 처리, NSAlert, OpenSettings |
| `src/ui/notifications.rs` | 수정 | 번역 함수에 Language 파라미터 |
| `src/core/snapshot_service.rs` | 수정 | base-index 보정 |
| `src/core/restart_service.rs` | 수정 | Language 파라미터 전달 |
| `src/app.rs` | 수정 | Language를 AppState에 추가, 모든 배선 |

---

### Task 1: i18n 모듈 생성

**Files:**
- Create: `src/i18n.rs`
- Modify: `src/main.rs:7` — `mod i18n;` 추가

- [ ] **Step 1: i18n 모듈 작성**

```rust
// src/i18n.rs

// ---------------------------------------------------------------------------
// Language
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Ko,
    En,
    Ja,
    Zh,
}

impl Language {
    /// Parse a language code string. Unknown codes fall back to English.
    pub fn from_code(code: &str) -> Self {
        match code.to_lowercase().as_str() {
            "ko" => Language::Ko,
            "ja" => Language::Ja,
            "zh" => Language::Zh,
            _ => Language::En,
        }
    }
}

impl Default for Language {
    fn default() -> Self {
        Language::En
    }
}

// ---------------------------------------------------------------------------
// Menu strings
// ---------------------------------------------------------------------------

pub fn menu_new_session(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "새 세션...",
        Language::En => "New Session...",
        Language::Ja => "新しいセッション...",
        Language::Zh => "新建会话...",
    }
}

pub fn menu_attach(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "연결",
        Language::En => "Attach",
        Language::Ja => "接続",
        Language::Zh => "连接",
    }
}

pub fn menu_kill_session(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "세션 종료",
        Language::En => "Kill",
        Language::Ja => "セッション終了",
        Language::Zh => "终止会话",
    }
}

pub fn menu_kill_server(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "서버 종료",
        Language::En => "Kill Server",
        Language::Ja => "サーバー終了",
        Language::Zh => "终止服务器",
    }
}

pub fn menu_settings(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "설정",
        Language::En => "Settings",
        Language::Ja => "設定",
        Language::Zh => "设置",
    }
}

pub fn menu_quit(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "TmuxBar 종료",
        Language::En => "Quit",
        Language::Ja => "TmuxBar 終了",
        Language::Zh => "退出 TmuxBar",
    }
}

// ---------------------------------------------------------------------------
// Alert strings (NSAlert for kill confirmation)
// ---------------------------------------------------------------------------

pub fn alert_kill_title(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "세션 종료",
        Language::En => "Kill Session",
        Language::Ja => "セッション終了",
        Language::Zh => "终止会话",
    }
}

pub fn alert_kill_confirm(lang: &Language, name: &str) -> String {
    match lang {
        Language::Ko => format!("'{}' 세션을 종료하시겠습니까?", name),
        Language::En => format!("Kill session '{}'?", name),
        Language::Ja => format!("セッション '{}' を終了しますか？", name),
        Language::Zh => format!("确定终止会话 '{}'？", name),
    }
}

pub fn alert_cancel(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "취소",
        Language::En => "Cancel",
        Language::Ja => "キャンセル",
        Language::Zh => "取消",
    }
}

pub fn alert_confirm_kill(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "종료",
        Language::En => "Kill",
        Language::Ja => "終了",
        Language::Zh => "终止",
    }
}

// ---------------------------------------------------------------------------
// Notification strings
// ---------------------------------------------------------------------------

pub fn notif_fd_title(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "파일 디스크립터 경고",
        Language::En => "File Descriptor Alert",
        Language::Ja => "ファイルディスクリプタ警告",
        Language::Zh => "文件描述符警告",
    }
}

pub fn notif_fd_warn(lang: &Language, pct: u8) -> String {
    match lang {
        Language::Ko => format!("파일 디스크립터 사용량 {}%", pct),
        Language::En => format!("File descriptor usage at {}%", pct),
        Language::Ja => format!("ファイルディスクリプタ使用率 {}%", pct),
        Language::Zh => format!("文件描述符使用率 {}%", pct),
    }
}

pub fn notif_fd_elevated(lang: &Language, pct: u8) -> String {
    match lang {
        Language::Ko => format!("⚠ 파일 디스크립터 사용량 {}% — 임계치 접근 중", pct),
        Language::En => format!("⚠ File descriptor usage at {}% — approaching critical", pct),
        Language::Ja => format!("⚠ ファイルディスクリプタ使用率 {}% — 危険域に接近中", pct),
        Language::Zh => format!("⚠ 文件描述符使用率 {}% — 接近临界值", pct),
    }
}

pub fn notif_fd_critical(lang: &Language, pct: u8) -> String {
    match lang {
        Language::Ko => format!("🔴 위험: 파일 디스크립터 사용량 {}%! tmux 서버 재시작을 고려하세요.", pct),
        Language::En => format!("🔴 CRITICAL: File descriptor usage at {}%! Consider restarting tmux server.", pct),
        Language::Ja => format!("🔴 危険: ファイルディスクリプタ使用率 {}%! tmuxサーバーの再起動を検討してください。", pct),
        Language::Zh => format!("🔴 危险：文件描述符使用率 {}%！请考虑重启 tmux 服务器。", pct),
    }
}

pub fn notif_inactivity_title(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "비활성 세션 경고",
        Language::En => "Inactivity Alert",
        Language::Ja => "非アクティブセッション警告",
        Language::Zh => "会话闲置警告",
    }
}

pub fn notif_inactivity_body(lang: &Language, name: &str, mins: u64) -> String {
    match lang {
        Language::Ko => format!("'{}' 세션이 {}분간 비활성 상태입니다", name, mins),
        Language::En => format!("Session '{}' has been inactive for {} minutes", name, mins),
        Language::Ja => format!("セッション '{}' が {}分間非アクティブです", name, mins),
        Language::Zh => format!("会话 '{}' 已闲置 {} 分钟", name, mins),
    }
}

pub fn notif_restart_success_title(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "재시작 성공",
        Language::En => "Restart Successful",
        Language::Ja => "再起動成功",
        Language::Zh => "重启成功",
    }
}

pub fn notif_restart_fail_title(lang: &Language) -> &'static str {
    match lang {
        Language::Ko => "재시작 실패",
        Language::En => "Restart Failed",
        Language::Ja => "再起動失敗",
        Language::Zh => "重启失败",
    }
}

pub fn notif_restart_success_body(lang: &Language, details: &str) -> String {
    match lang {
        Language::Ko => format!("tmux 서버가 성공적으로 재시작되었습니다. {}", details),
        Language::En => format!("tmux server restarted successfully. {}", details),
        Language::Ja => format!("tmuxサーバーが正常に再起動されました。{}", details),
        Language::Zh => format!("tmux 服务器已成功重启。{}", details),
    }
}

pub fn notif_restart_fail_body(lang: &Language, details: &str) -> String {
    match lang {
        Language::Ko => format!("tmux 서버 재시작 실패: {}", details),
        Language::En => format!("tmux server restart failed: {}", details),
        Language::Ja => format!("tmuxサーバーの再起動に失敗: {}", details),
        Language::Zh => format!("tmux 服务器重启失败：{}", details),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_code_known() {
        assert_eq!(Language::from_code("ko"), Language::Ko);
        assert_eq!(Language::from_code("en"), Language::En);
        assert_eq!(Language::from_code("ja"), Language::Ja);
        assert_eq!(Language::from_code("zh"), Language::Zh);
    }

    #[test]
    fn test_from_code_case_insensitive() {
        assert_eq!(Language::from_code("KO"), Language::Ko);
        assert_eq!(Language::from_code("Ja"), Language::Ja);
        assert_eq!(Language::from_code("ZH"), Language::Zh);
    }

    #[test]
    fn test_from_code_unknown_falls_back_to_en() {
        assert_eq!(Language::from_code("fr"), Language::En);
        assert_eq!(Language::from_code(""), Language::En);
        assert_eq!(Language::from_code("xyz"), Language::En);
    }

    #[test]
    fn test_default_is_en() {
        assert_eq!(Language::default(), Language::En);
    }

    #[test]
    fn test_menu_strings_not_empty() {
        for lang in &[Language::Ko, Language::En, Language::Ja, Language::Zh] {
            assert!(!menu_new_session(lang).is_empty());
            assert!(!menu_attach(lang).is_empty());
            assert!(!menu_kill_session(lang).is_empty());
            assert!(!menu_kill_server(lang).is_empty());
            assert!(!menu_settings(lang).is_empty());
            assert!(!menu_quit(lang).is_empty());
        }
    }

    #[test]
    fn test_alert_strings() {
        for lang in &[Language::Ko, Language::En, Language::Ja, Language::Zh] {
            assert!(!alert_kill_title(lang).is_empty());
            assert!(alert_kill_confirm(lang, "test").contains("test"));
            assert!(!alert_cancel(lang).is_empty());
            assert!(!alert_confirm_kill(lang).is_empty());
        }
    }

    #[test]
    fn test_notif_fd_strings_contain_pct() {
        for lang in &[Language::Ko, Language::En, Language::Ja, Language::Zh] {
            assert!(notif_fd_warn(lang, 85).contains("85"));
            assert!(notif_fd_elevated(lang, 92).contains("92"));
            assert!(notif_fd_critical(lang, 97).contains("97"));
        }
    }

    #[test]
    fn test_notif_inactivity_contains_name_and_mins() {
        for lang in &[Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let body = notif_inactivity_body(lang, "dev", 42);
            assert!(body.contains("dev"));
            assert!(body.contains("42"));
        }
    }

    #[test]
    fn test_notif_restart_contains_details() {
        for lang in &[Language::Ko, Language::En, Language::Ja, Language::Zh] {
            assert!(notif_restart_success_body(lang, "3 sessions").contains("3 sessions"));
            assert!(notif_restart_fail_body(lang, "timeout").contains("timeout"));
        }
    }
}
```

- [ ] **Step 2: main.rs에 모듈 등록**

`src/main.rs`에 `mod i18n;` 추가:

```rust
mod app;
mod core;
mod i18n;
mod infra;
mod models;
mod ui;
```

- [ ] **Step 3: 테스트 실행**

Run: `cargo test i18n`
Expected: 모든 i18n 테스트 PASS

- [ ] **Step 4: 커밋**

```bash
git add src/i18n.rs src/main.rs
git commit -m "feat: add i18n module with ko/en/ja/zh translations"
```

---

### Task 2: Config에 language 필드 추가

**Files:**
- Modify: `src/infra/config.rs:118-130` — `GeneralConfig`에 `language` 필드

- [ ] **Step 1: GeneralConfig에 language 필드 추가**

`src/infra/config.rs`의 `GeneralConfig`를 수정:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeneralConfig {
    pub launch_at_login: bool,
    pub language: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            launch_at_login: true,
            language: "en".to_string(),
        }
    }
}
```

- [ ] **Step 2: 기본 config 생성 시 language 주석 포함**

`src/infra/config.rs`의 `AppConfig::load()`에서 기본 파일 생성 부분 수정. `toml::to_string_pretty()` 이후에 `[general]` 섹션 위에 주석을 삽입:

```rust
// load() 메서드 내, 기본 파일 생성 부분:
let toml_str = toml::to_string_pretty(&default_cfg)
    .context("failed to serialize default config")?;
// Add language hint comment above the language field
let toml_str = toml_str.replace(
    "language = \"en\"",
    "# 한국어 = \"ko\", English = \"en\", 日本語 = \"ja\", 中文 = \"zh\"\nlanguage = \"en\"",
);
std::fs::write(&path, &toml_str)
    .with_context(|| format!("failed to write default config to {}", path.display()))?;
```

- [ ] **Step 3: 기존 테스트 실행**

Run: `cargo test infra::config`
Expected: 기존 config 테스트 모두 PASS. `language` 필드는 `#[serde(default)]`이므로 기존 설정 파일에 없어도 기본값 "en" 적용.

- [ ] **Step 4: 커밋**

```bash
git add src/infra/config.rs
git commit -m "feat: add language field to config (default: en)"
```

---

### Task 3: AppCommand::OpenSettings 추가

**Files:**
- Modify: `src/models.rs:203-210` — `AppCommand` enum

- [ ] **Step 1: OpenSettings 변형 추가**

`src/models.rs`의 `AppCommand` enum에 추가:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    CreateSession { name: String },
    AttachSession { name: String },
    KillSession { name: String },
    KillServer,
    RestartServer,
    OpenSettings,
    Quit,
}
```

- [ ] **Step 2: 빌드 확인**

Run: `cargo build 2>&1 | tail -3`
Expected: 빌드 성공 (exhaustive match 경고 가능 — app.rs에서 처리 안 한 상태)

- [ ] **Step 3: 커밋**

```bash
git add src/models.rs
git commit -m "feat: add OpenSettings variant to AppCommand"
```

---

### Task 4: TmuxAdapter에 get_global_option 추가

**Files:**
- Modify: `src/models.rs:9-25` — `TmuxAdapter` 트레이트
- Modify: `src/infra/tmux_client.rs:159-249` — `TmuxClient` 구현
- Modify: `src/core/snapshot_service.rs:225-333` — MockTmux
- Modify: `src/core/session_manager.rs:209-259` — MockTmux
- Modify: `src/core/restart_service.rs` — MockTmux (테스트 내)

- [ ] **Step 1: TmuxAdapter 트레이트에 메서드 추가**

`src/models.rs`의 `TmuxAdapter` 트레이트에 추가:

```rust
pub trait TmuxAdapter: Send + Sync {
    // ... 기존 메서드들 ...
    fn select_layout(&self, target: &str, layout: &str) -> anyhow::Result<()>;
    /// Query a global tmux option by name (e.g., "base-index").
    fn get_global_option(&self, name: &str) -> anyhow::Result<String>;
}
```

- [ ] **Step 2: TmuxClient 구현**

`src/infra/tmux_client.rs`의 `impl TmuxAdapter for TmuxClient` 블록 끝에 추가:

```rust
    fn get_global_option(&self, name: &str) -> Result<String> {
        self.run_tmux(&["show-option", "-gv", name])
    }
```

- [ ] **Step 3: 모든 MockTmux에 구현 추가**

각 테스트 파일의 `MockTmux`에 추가. 3개 파일:

`src/core/snapshot_service.rs` MockTmux (`impl TmuxAdapter` 블록 끝):
```rust
        fn get_global_option(&self, name: &str) -> Result<String> {
            self.record(format!("get_global_option:{name}"));
            match name {
                "base-index" => Ok("0".to_string()),
                "pane-base-index" => Ok("0".to_string()),
                _ => Ok("0".to_string()),
            }
        }
```

`src/core/session_manager.rs` MockTmux (`impl TmuxAdapter` 블록 끝):
```rust
        fn get_global_option(&self, _name: &str) -> anyhow::Result<String> {
            Ok("0".to_string())
        }
```

`src/core/restart_service.rs` MockTmux (`impl TmuxAdapter` 블록 끝):
```rust
        fn get_global_option(&self, _name: &str) -> anyhow::Result<String> {
            Ok("0".to_string())
        }
```

- [ ] **Step 4: 테스트 실행**

Run: `cargo test`
Expected: 151+ tests PASS (기존 테스트 모두 유지)

- [ ] **Step 5: 커밋**

```bash
git add src/models.rs src/infra/tmux_client.rs src/core/snapshot_service.rs src/core/session_manager.rs src/core/restart_service.rs
git commit -m "feat: add get_global_option to TmuxAdapter trait"
```

---

### Task 5: Notification 다국어 지원

**Files:**
- Modify: `src/ui/notifications.rs` — 모든 format 함수에 Language 파라미터 추가
- Modify: `src/core/restart_service.rs` — Language 파라미터 전달

- [ ] **Step 1: notifications.rs의 format 함수들을 i18n 기반으로 변경**

`src/ui/notifications.rs`를 수정. `use crate::i18n::{...}` import 추가하고, format 함수들에 `Language` 파라미터 추가:

```rust
use crate::i18n::Language;
use crate::i18n;
```

`format_fd_alert_message` 변경:
```rust
pub(crate) fn format_fd_alert_message(pct: u8, level: &AlertLevel, lang: &Language) -> Option<(String, String)> {
    match level {
        AlertLevel::Normal => None,
        AlertLevel::Warning => Some((
            i18n::notif_fd_title(lang).to_string(),
            i18n::notif_fd_warn(lang, pct),
        )),
        AlertLevel::Elevated => Some((
            i18n::notif_fd_title(lang).to_string(),
            i18n::notif_fd_elevated(lang, pct),
        )),
        AlertLevel::Critical => Some((
            i18n::notif_fd_title(lang).to_string(),
            i18n::notif_fd_critical(lang, pct),
        )),
    }
}
```

`format_inactivity_message` 변경:
```rust
pub(crate) fn format_inactivity_message(session_name: &str, mins: u64, lang: &Language) -> (String, String) {
    (
        i18n::notif_inactivity_title(lang).to_string(),
        i18n::notif_inactivity_body(lang, session_name, mins),
    )
}
```

`format_restart_result_message` 변경:
```rust
pub(crate) fn format_restart_result_message(success: bool, details: &str, lang: &Language) -> (String, String) {
    if success {
        (
            i18n::notif_restart_success_title(lang).to_string(),
            i18n::notif_restart_success_body(lang, details),
        )
    } else {
        (
            i18n::notif_restart_fail_title(lang).to_string(),
            i18n::notif_restart_fail_body(lang, details),
        )
    }
}
```

`NotificationService`의 공개 메서드들에도 `lang` 파라미터 추가:

```rust
    pub fn send_fd_alert(&self, pct: u8, level: &AlertLevel, lang: &Language) -> Result<()> {
        let Some((subtitle, body)) = format_fd_alert_message(pct, level, lang) else {
            return Ok(());
        };
        self.send_notification("TmuxBar", &subtitle, &body)
    }

    pub fn send_inactivity_alert(&self, session_name: &str, mins: u64, lang: &Language) -> Result<()> {
        let (subtitle, body) = format_inactivity_message(session_name, mins, lang);
        self.send_notification("TmuxBar", &subtitle, &body)
    }

    pub fn send_restart_result(&self, success: bool, details: &str, lang: &Language) -> Result<()> {
        let (subtitle, body) = format_restart_result_message(success, details, lang);
        self.send_notification("TmuxBar", &subtitle, &body)
    }
```

- [ ] **Step 2: 테스트 업데이트**

기존 테스트의 format 함수 호출에 `&Language::En` 추가:

```rust
    // 예시: 모든 format_fd_alert_message 호출에 &Language::En 추가
    fn normal_level_returns_none() {
        assert_eq!(format_fd_alert_message(50, &AlertLevel::Normal, &Language::En), None);
        // ...
    }
    // 기타 모든 테스트도 동일하게 &Language::En 추가
```

`send_fd_alert_normal_is_ok` 테스트도 수정:
```rust
    fn send_fd_alert_normal_is_ok() {
        let svc = NotificationService::new();
        let result = svc.send_fd_alert(50, &AlertLevel::Normal, &Language::En);
        assert!(result.is_ok());
    }
```

- [ ] **Step 3: restart_service.rs에 Language 파라미터 전달**

`src/core/restart_service.rs`에 import 추가:
```rust
use crate::i18n::Language;
```

`execute_restart` 시그니처 변경:
```rust
    pub fn execute_restart(&self, lang: &Language) -> Result<()> {
```

`execute_restart` 내부의 `send_restart_result` 호출에 `lang` 추가:
```rust
        // Phase 1 실패 시:
        self.notification_service.send_restart_result(false, &detail, lang)

        // 최종 알림:
        self.notification_service.send_restart_result(overall_success, &details, lang)
```

- [ ] **Step 4: 테스트 실행**

Run: `cargo test`
Expected: PASS (restart_service 테스트에서 `execute_restart()` 호출도 `&Language::En` 추가 필요)

restart_service 테스트의 `execute_restart()` 호출을 `execute_restart(&Language::En)`으로 변경.

- [ ] **Step 5: 커밋**

```bash
git add src/ui/notifications.rs src/core/restart_service.rs
git commit -m "feat: add i18n support to notifications and restart service"
```

---

### Task 6: 세션 메뉴 서브메뉴 + 다국어

**Files:**
- Modify: `src/ui/session_menu.rs` — 서브메뉴 구조, Kill 태그, Language 파라미터

- [ ] **Step 1: 태그 상수 추가 및 build_menu 시그니처 변경**

`src/ui/session_menu.rs` 상단에 import 추가:
```rust
use crate::i18n::{self, Language};
```

태그 상수 추가:
```rust
/// Tags 0..999 are reserved for session attach items (index-based).
/// Tags 2000..2999 are reserved for session kill items.
pub const TAG_KILL_SESSION_BASE: isize = 2000;
pub const TAG_NEW_SESSION: isize = 1000;
pub const TAG_KILL_SERVER: isize = 1001;
pub const TAG_SETTINGS: isize = 1002;
pub const TAG_QUIT: isize = 1003;
```

`build_menu` 시그니처 변경:
```rust
    pub fn build_menu(
        mtm: MainThreadMarker,
        sessions: &[Session],
        handler: Option<&MenuActionHandler>,
        lang: &Language,
    ) -> Retained<NSMenu> {
```

- [ ] **Step 2: 세션 항목을 서브메뉴 구조로 변경**

`build_menu` 내부의 세션 루프를 서브메뉴 방식으로 변경:

```rust
        // --- Session items (each with submenu: Attach / Kill) ---
        for (idx, session) in sessions.iter().enumerate() {
            let title = format_session_title(session);
            let session_item = {
                let ns_title = NSString::from_str(&title);
                let empty = NSString::from_str("");
                // No action on parent item — user picks from submenu.
                unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &ns_title,
                        None,
                        &empty,
                    )
                }
            };

            // Build submenu with Attach + Kill
            let submenu = NSMenu::initWithTitle(
                NSMenu::alloc(mtm),
                &NSString::from_str(&session.name),
            );

            let attach_item = make_item(mtm, i18n::menu_attach(lang), handler);
            attach_item.setTag(idx as isize);
            submenu.addItem(&attach_item);

            let kill_item = make_item(mtm, i18n::menu_kill_session(lang), handler);
            kill_item.setTag(TAG_KILL_SESSION_BASE + idx as isize);
            submenu.addItem(&kill_item);

            session_item.setSubmenu(Some(&submenu));
            menu.addItem(&session_item);
        }
```

- [ ] **Step 3: 고정 항목을 i18n 문자열로 변경**

```rust
        // --- Fixed action items ---
        let new_session = make_item(mtm, i18n::menu_new_session(lang), handler);
        new_session.setTag(TAG_NEW_SESSION);
        menu.addItem(&new_session);

        let kill_server = make_item(mtm, i18n::menu_kill_server(lang), handler);
        kill_server.setTag(TAG_KILL_SERVER);
        menu.addItem(&kill_server);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let settings = make_item(mtm, i18n::menu_settings(lang), handler);
        settings.setTag(TAG_SETTINGS);
        menu.addItem(&settings);

        let quit = make_item(mtm, i18n::menu_quit(lang), handler);
        quit.setTag(TAG_QUIT);
        menu.addItem(&quit);
```

- [ ] **Step 4: 태그 상수 테스트 업데이트**

기존 `test_tag_constants_do_not_overlap_session_range` 테스트에 Kill 범위 검증 추가:

```rust
    #[test]
    fn test_kill_session_tag_range_does_not_overlap() {
        // Session attach tags: 0..999
        // Fixed tags: 1000..1999
        // Session kill tags: 2000..2999
        assert!(TAG_KILL_SESSION_BASE >= 2000);
        assert!(TAG_NEW_SESSION < TAG_KILL_SESSION_BASE);
    }
```

- [ ] **Step 5: 테스트 실행**

Run: `cargo build 2>&1 | tail -5`
Expected: 빌드 성공 (app.rs에서의 `build_menu` 호출은 아직 lang 파라미터 미반영 — Task 9에서 처리)

참고: `build_menu` 시그니처가 변경되어 `app.rs`에서 컴파일 에러 발생 예상. 이는 Task 9에서 해결.

- [ ] **Step 6: 커밋**

```bash
git add src/ui/session_menu.rs
git commit -m "feat: session submenu (attach/kill) with i18n labels"
```

---

### Task 7: Menu Action Handler — Kill 확인 + Settings 처리

**Files:**
- Modify: `src/ui/menu_action_handler.rs` — Kill 태그, NSAlert, OpenSettings

- [ ] **Step 1: import 추가 및 TAG_KILL_SESSION_BASE 사용**

`src/ui/menu_action_handler.rs` 상단 import에 추가:

```rust
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSAlert, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};
use tokio::sync::mpsc;

use crate::i18n::{self, Language};
use crate::models::AppCommand;
use crate::ui::session_menu::{TAG_KILL_SERVER, TAG_KILL_SESSION_BASE, TAG_NEW_SESSION, TAG_QUIT, TAG_SETTINGS};
```

- [ ] **Step 2: Ivars에 Language 필드 추가**

```rust
pub(crate) struct MenuActionHandlerIvars {
    cmd_tx: mpsc::Sender<AppCommand>,
    session_names: RefCell<Vec<String>>,
    language: RefCell<Language>,
}
```

- [ ] **Step 3: menu_item_clicked에 Kill 태그 + NSAlert 처리 추가**

`menu_item_clicked` 메서드의 `let cmd = match tag` 블록을 수정:

```rust
            let cmd = match tag {
                TAG_NEW_SESSION => {
                    let ts = chrono::Utc::now().timestamp() % 100_000;
                    Some(AppCommand::CreateSession {
                        name: format!("s{ts}"),
                    })
                }
                TAG_KILL_SERVER => Some(AppCommand::KillServer),
                TAG_SETTINGS => Some(AppCommand::OpenSettings),
                TAG_QUIT => {
                    let _ = ivars.cmd_tx.try_send(AppCommand::Quit);
                    NSApplication::sharedApplication(self.mtm()).terminate(None);
                    return;
                }
                // Attach: tags 0..999
                idx if (0..1000).contains(&idx) => {
                    let names = ivars.session_names.borrow();
                    names.get(idx as usize).map(|name: &String| AppCommand::AttachSession {
                        name: name.clone(),
                    })
                }
                // Kill: tags 2000..2999
                idx if (TAG_KILL_SESSION_BASE..TAG_KILL_SESSION_BASE + 1000).contains(&idx) => {
                    let session_idx = (idx - TAG_KILL_SESSION_BASE) as usize;
                    let names = ivars.session_names.borrow();
                    let name = match names.get(session_idx) {
                        Some(n) => n.clone(),
                        None => return,
                    };
                    drop(names); // release borrow before showing alert

                    let lang = ivars.language.borrow();
                    // Show confirmation NSAlert
                    let alert = unsafe { NSAlert::new(self.mtm()) };
                    alert.setAlertStyle(NSAlertStyle::Warning);
                    alert.setMessageText(&NSString::from_str(i18n::alert_kill_title(&lang)));
                    alert.setInformativeText(&NSString::from_str(&i18n::alert_kill_confirm(&lang, &name)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_confirm_kill(&lang)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_cancel(&lang)));

                    let response = unsafe { alert.runModal() };
                    if response == NSAlertFirstButtonReturn {
                        Some(AppCommand::KillSession { name })
                    } else {
                        None
                    }
                }
                _ => None,
            };
```

- [ ] **Step 4: new() 및 update 메서드 수정**

```rust
impl MenuActionHandler {
    pub fn new(mtm: MainThreadMarker, cmd_tx: mpsc::Sender<AppCommand>, lang: Language) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MenuActionHandlerIvars {
            cmd_tx,
            session_names: RefCell::new(Vec::new()),
            language: RefCell::new(lang),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn update_session_names(&self, names: Vec<String>) {
        *self.ivars().session_names.borrow_mut() = names;
    }

    pub fn update_language(&self, lang: Language) {
        *self.ivars().language.borrow_mut() = lang;
    }

    pub fn action_sel() -> Sel {
        sel!(menuItemClicked:)
    }
}
```

- [ ] **Step 5: 빌드 확인**

Run: `cargo build 2>&1 | tail -10`
Expected: 빌드 에러 (app.rs에서 `MenuActionHandler::new` 시그니처 불일치 — Task 9에서 해결)

- [ ] **Step 6: 커밋**

```bash
git add src/ui/menu_action_handler.rs
git commit -m "feat: kill confirmation with NSAlert, settings dispatch, i18n"
```

---

### Task 8: base-index 스냅샷 복원 수정

**Files:**
- Modify: `src/core/snapshot_service.rs:109-141` — `restore_session`
- Modify: `src/core/snapshot_service.rs` (테스트 섹션) — MockTmux + 새 테스트

- [ ] **Step 1: restore_session에 base-index 조회 추가**

`src/core/snapshot_service.rs`의 `restore_session` 메서드를 수정:

```rust
    pub fn restore_session(&self, snapshot: &SessionSnapshot) -> Result<()> {
        // Query tmux base indices for correct window/pane targeting.
        let base_index: u32 = self
            .tmux
            .get_global_option("base-index")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let pane_base_index: u32 = self
            .tmux
            .get_global_option("pane-base-index")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        self.tmux.new_session(&snapshot.name)?;

        for (window_enum_idx, window) in snapshot.windows.iter().enumerate() {
            let window_idx = base_index + window_enum_idx as u32;

            // The first window is created automatically with new_session.
            if window_enum_idx > 0 {
                self.tmux.new_window(&snapshot.name, &window.name)?;
            }

            let window_idx_str = window_idx.to_string();

            // Split additional panes (pane at pane_base_index exists already).
            for _ in 1..window.panes.len() {
                self.tmux.split_window(&snapshot.name, &window_idx_str)?;
            }

            // Send each pane to its recorded working directory.
            for (pane_enum_idx, pane) in window.panes.iter().enumerate() {
                let pane_idx = pane_base_index + pane_enum_idx as u32;
                let target = format!("{}:{}.{}", snapshot.name, window_idx, pane_idx);
                let escaped_dir = pane.working_dir.replace('\'', "'\\''");
                self.tmux
                    .send_keys(&target, &format!("cd '{escaped_dir}'"))?;
            }

            // Apply the recorded layout for this window.
            let layout_target = format!("{}:{}", snapshot.name, window_idx);
            self.tmux.select_layout(&layout_target, &window.layout)?;
        }

        Ok(())
    }
```

- [ ] **Step 2: base-index=1 테스트용 MockTmux 확장**

snapshot_service.rs의 MockTmux에 base-index 설정 가능하도록 필드 추가:

```rust
    struct MockTmux {
        sessions: Vec<RawSession>,
        panes: Vec<RawPane>,
        windows: Vec<RawWindow>,
        fail_sessions: Vec<String>,
        base_index: String,
        pane_base_index: String,
        calls: Mutex<Vec<String>>,
    }

    impl MockTmux {
        fn new(
            sessions: Vec<RawSession>,
            windows: Vec<RawWindow>,
            panes: Vec<RawPane>,
        ) -> Self {
            Self {
                sessions,
                windows,
                panes,
                fail_sessions: Vec::new(),
                base_index: "0".to_string(),
                pane_base_index: "0".to_string(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn with_fail_sessions(mut self, names: Vec<&str>) -> Self {
            self.fail_sessions = names.iter().map(|s| s.to_string()).collect();
            self
        }

        fn with_base_index(mut self, base: &str, pane_base: &str) -> Self {
            self.base_index = base.to_string();
            self.pane_base_index = pane_base.to_string();
            self
        }
        // ... record(), call_log() unchanged
    }
```

MockTmux의 `get_global_option` 구현을 업데이트:
```rust
        fn get_global_option(&self, name: &str) -> Result<String> {
            self.record(format!("get_global_option:{name}"));
            match name {
                "base-index" => Ok(self.base_index.clone()),
                "pane-base-index" => Ok(self.pane_base_index.clone()),
                _ => Ok("0".to_string()),
            }
        }
```

- [ ] **Step 3: base-index=1 테스트 추가**

```rust
    #[test]
    fn test_restore_session_with_base_index_one() {
        let mock = Arc::new(
            MockTmux::new(vec![], vec![], vec![])
                .with_base_index("1", "0"),
        );
        let (svc, _tmp) = make_service(mock.clone());

        let snapshot = SessionSnapshot {
            name: "proj".to_string(),
            windows: vec![
                WindowSnapshot {
                    name: "code".to_string(),
                    layout: "main-vertical".to_string(),
                    panes: vec![
                        PaneSnapshot { working_dir: "/src".to_string(), index: 0 },
                        PaneSnapshot { working_dir: "/test".to_string(), index: 1 },
                    ],
                },
                WindowSnapshot {
                    name: "logs".to_string(),
                    layout: "even-horizontal".to_string(),
                    panes: vec![
                        PaneSnapshot { working_dir: "/var/log".to_string(), index: 0 },
                    ],
                },
            ],
        };

        svc.restore_session(&snapshot).expect("restore should succeed");

        let log = mock.call_log();

        // Window indices should be 1, 2 (not 0, 1) due to base-index=1
        assert!(log.contains(&"split_window:proj:1".to_string()), "split should target window 1: {log:?}");
        assert!(log.contains(&"send_keys:proj:1.0:cd '/src'".to_string()), "pane target should use window 1: {log:?}");
        assert!(log.contains(&"send_keys:proj:1.1:cd '/test'".to_string()), "{log:?}");
        assert!(log.contains(&"send_keys:proj:2.0:cd '/var/log'".to_string()), "second window should be 2: {log:?}");
        assert!(log.contains(&"select_layout:proj:1:main-vertical".to_string()), "{log:?}");
        assert!(log.contains(&"select_layout:proj:2:even-horizontal".to_string()), "{log:?}");
    }

    #[test]
    fn test_restore_session_with_pane_base_index_one() {
        let mock = Arc::new(
            MockTmux::new(vec![], vec![], vec![])
                .with_base_index("0", "1"),
        );
        let (svc, _tmp) = make_service(mock.clone());

        let snapshot = SessionSnapshot {
            name: "app".to_string(),
            windows: vec![WindowSnapshot {
                name: "main".to_string(),
                layout: "tiled".to_string(),
                panes: vec![
                    PaneSnapshot { working_dir: "/a".to_string(), index: 0 },
                    PaneSnapshot { working_dir: "/b".to_string(), index: 1 },
                ],
            }],
        };

        svc.restore_session(&snapshot).expect("restore should succeed");

        let log = mock.call_log();

        // Pane indices should be 1, 2 (not 0, 1) due to pane-base-index=1
        assert!(log.contains(&"send_keys:app:0.1:cd '/a'".to_string()), "pane 0 should become index 1: {log:?}");
        assert!(log.contains(&"send_keys:app:0.2:cd '/b'".to_string()), "pane 1 should become index 2: {log:?}");
    }
```

- [ ] **Step 4: 기존 테스트가 여전히 통과하는지 확인**

Run: `cargo test core::snapshot_service`
Expected: 기존 테스트 PASS + 새 테스트 2개 PASS. 기존 테스트는 base-index="0"이 기본이므로 동작 변경 없음.

- [ ] **Step 5: 커밋**

```bash
git add src/core/snapshot_service.rs
git commit -m "fix: respect tmux base-index and pane-base-index in snapshot restore"
```

---

### Task 9: app.rs 통합 배선

**Files:**
- Modify: `src/app.rs` — Language를 AppState에 추가, 모든 호출 경로에 Language 전달

- [ ] **Step 1: AppState에 language 필드 추가**

`src/app.rs` 상단에 import 추가:
```rust
use crate::i18n::Language;
use crate::infra::config::AppConfig;
```

`AppState` struct에 필드 추가:
```rust
struct AppState {
    sessions: Vec<Session>,
    alert_level: AlertLevel,
    fd_percent: u8,
    language: Language,
    status_item_ptr: Option<StatusItemPtr>,
    action_handler_ptr: Option<ActionHandlerPtr>,
}
```

`Default` 구현에 추가:
```rust
impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            alert_level: AlertLevel::Normal,
            fd_percent: 0,
            language: Language::default(),
            status_item_ptr: None,
            action_handler_ptr: None,
        }
    }
}
```

- [ ] **Step 2: run() 함수에서 Language 초기화 및 전달**

config 로드 직후에 Language 설정:
```rust
    let language = Language::from_code(&config.general.language);
```

AppState 초기화 시 language 설정:
```rust
    let shared_state = Arc::new(Mutex::new(AppState {
        language,
        ..AppState::default()
    }));
```

MenuActionHandler 생성 시 language 전달:
```rust
    let action_handler = MenuActionHandler::new(mtm, cmd_tx.clone(), language);
```

초기 메뉴 빌드에 language 전달:
```rust
    {
        let state = shared_state.lock().unwrap();
        let menu = SessionMenuBuilder::build_menu(mtm, &state.sessions, Some(&action_handler), &state.language);
        menu_bar.set_menu(&menu);
    }
```

- [ ] **Step 3: 타이머 스레드의 메뉴 빌드에 language 전달**

타이머 스레드 내부에서 language 읽기:
```rust
            let state = ui_state.lock().unwrap();
            let alert_level = state.alert_level.clone();
            let sessions = state.sessions.clone();
            let language = state.language;
            let ptr_opt = state.status_item_ptr;
            let handler_opt = state.action_handler_ptr;
            drop(state);

            if let Some(sip) = ptr_opt {
                dispatch2::DispatchQueue::main().exec_async(move || {
                    if let Some(mtm) = MainThreadMarker::new() {
                        unsafe {
                            menu_bar::apply_alert_level_raw(sip.as_ptr(), &alert_level, mtm);
                            let handler_ref =
                                handler_opt.map(|ahp| &*ahp.as_ptr());
                            let menu =
                                SessionMenuBuilder::build_menu(mtm, &sessions, handler_ref, &language);
                            menu_bar::set_menu_raw(sip.as_ptr(), &menu);
                        }
                    }
                });
            }
```

- [ ] **Step 4: setup_initial_menu에 language 전달**

```rust
fn setup_initial_menu(
    shared_state: &Arc<Mutex<AppState>>,
    menu_bar: &MenuBarApp,
    handler: &MenuActionHandler,
    config: &AppConfig,
    mtm: MainThreadMarker,
) {
    let language = Language::from_code(&config.general.language);
    // ... 기존 코드 ...
    match session_mgr.list_sessions() {
        Ok(sessions) => {
            let menu = SessionMenuBuilder::build_menu(mtm, &sessions, Some(handler), &language);
            menu_bar.set_menu(&menu);
        }
        Err(e) => {
            tracing::warn!("Failed to list sessions for initial menu: {e:#}");
            let menu = SessionMenuBuilder::build_menu(mtm, &[], Some(handler), &language);
            menu_bar.set_menu(&menu);
        }
    }
}
```

- [ ] **Step 5: EventServices의 알림 호출에 language 전달**

`handle_monitor_event` 시작 부분에서 shared_state로부터 language 읽기:

```rust
fn handle_monitor_event(
    event: &MonitorEvent,
    services: &mut EventServices,
    shared_state: &Arc<Mutex<AppState>>,
) {
    // Read current language from shared state (updated by config hot-reload).
    let language = {
        let state = shared_state.lock().unwrap();
        state.language
    };

    // ... 기존 코드 ...
```

notification 호출에 `&language` 전달:
```rust
    // 1. fd alert notification:
    if let Err(e) = services
        .notification_service
        .send_fd_alert(event.fd_percent, &level, &language)

    // 4. inactivity notification:
    if let Err(e) = services
        .notification_service
        .send_inactivity_alert(session_name, mins.max(0) as u64, &language)
```

`bg_services` 생성은 기존과 동일 (EventServices에 language 필드 추가 불필요).

- [ ] **Step 6: OpenSettings 커맨드 처리**

`run_background`의 커맨드 매치에 `OpenSettings` 추가:

```rust
            AppCommand::OpenSettings => {
                let config_path = AppConfig::config_path();
                match std::process::Command::new("open").arg(&config_path).status() {
                    Ok(s) if s.success() => {
                        tracing::info!("Opened config file: {}", config_path.display());
                    }
                    Ok(s) => {
                        tracing::error!("Failed to open config: exit {s}");
                    }
                    Err(e) => {
                        tracing::error!("Failed to open config: {e}");
                    }
                }
            }
```

- [ ] **Step 7: RestartServer에 language 전달**

```rust
            AppCommand::RestartServer => {
                match &restart_service {
                    Some(svc) => {
                        let svc = Arc::clone(svc);
                        let lang = {
                            let state = shared_state.lock().unwrap();
                            state.language
                        };
                        let result = tokio::task::spawn_blocking(move || {
                            svc.execute_restart(&lang)
                        }).await;
                        // ... 기존 에러 처리 ...
                    }
                    // ...
                }
            }
```

참고: `run_background`에서 `shared_state`를 캡처해야 함. 기존 코드에서 `shared_state`는 이미 `BackgroundServices`에 포함되어 있으므로, `RestartServer` 핸들러에서 `shared_state.lock()`으로 language 읽기.

- [ ] **Step 8: config 핫리로드에서 language 갱신**

config watcher 콜백에 `shared_state`와 `action_handler_ptr` 접근 추가. `run()` 함수의 config watcher 생성 부분 수정:

```rust
    let watcher_state = Arc::clone(&shared_state);
    let _config_watcher = match ConfigWatcher::start(
        AppConfig::config_path(),
        move |new_cfg: AppConfig| {
            tracing::info!("Config reloaded");

            // Update language
            let new_lang = Language::from_code(&new_cfg.general.language);
            {
                let mut state = watcher_state.lock().unwrap();
                state.language = new_lang;
            }

            // Update FdAlertPolicy thresholds
            match watcher_fd_policy.lock() {
                // ... 기존 코드 그대로 ...
            }

            // Update InactivityDetector timeout
            match watcher_inactivity.lock() {
                // ... 기존 코드 그대로 ...
            }

            // Sync LaunchAgent
            // ... 기존 코드 그대로 ...
        },
    ) {
```

- [ ] **Step 9: 빌드 및 테스트**

Run: `cargo build 2>&1 | tail -5`
Expected: 빌드 성공

Run: `cargo test`
Expected: 모든 테스트 PASS

Run: `cargo clippy`
Expected: 0 warnings

- [ ] **Step 10: 커밋**

```bash
git add src/app.rs
git commit -m "feat: wire i18n, settings, and kill through app orchestration"
```

---

### Task 10: 기존 config.toml 업데이트 + 최종 검증

**Files:**
- 없음 (기존 config 파일 삭제 후 재생성 테스트)

- [ ] **Step 1: 기존 config 삭제 후 재생성 테스트**

기존 config를 삭제하고 앱을 실행하여 language 주석이 포함된 새 config가 생성되는지 확인:

```bash
rm ~/.config/tmuxbar/config.toml
cargo run &
sleep 2
kill %1
cat ~/.config/tmuxbar/config.toml
```

Expected: config.toml에 `language = "en"` 필드와 위에 `# 한국어 = "ko", English = "en"...` 주석이 있어야 함.

- [ ] **Step 2: language를 "ko"로 변경 후 테스트**

```bash
# config.toml에서 language = "ko"로 변경
sed -i '' 's/language = "en"/language = "ko"/' ~/.config/tmuxbar/config.toml
```

앱 실행 후 메뉴가 한국어로 표시되는지 확인.

- [ ] **Step 3: 전체 테스트 + clippy**

Run: `cargo test`
Expected: 모든 테스트 PASS

Run: `cargo clippy`
Expected: 0 warnings

- [ ] **Step 4: 최종 커밋**

```bash
git add -A
git commit -m "chore: final verification of UX improvements"
```
