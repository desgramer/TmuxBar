# TmuxBar UX 개선 설계서

**날짜:** 2026-04-10
**범위:** Settings 메뉴, 세션별 Kill, base-index 호환, 다국어(i18n)

---

## 1. Settings 메뉴

### 동작
"설정" 메뉴 클릭 → `open ~/.config/tmuxbar/config.toml` 실행 → macOS 기본 텍스트 에디터에서 열림.
config.toml은 핫리로드가 이미 구현되어 있으므로 저장 즉시 반영.

### 변경 사항
- `models.rs`: `AppCommand::OpenSettings` 추가
- `menu_action_handler.rs`: `TAG_SETTINGS` 분기에서 `AppCommand::OpenSettings` 전송
- `app.rs`: 커맨드 처리 루프에서 `std::process::Command::new("open").arg(config_path).status()` 실행

### 에러 처리
- `open` 명령 실패 시 `tracing::error!` 로그. 사용자에게 별도 알림 없음 (파일이 없을 수 없는 경우).

---

## 2. 세션별 Kill

### 메뉴 구조
기존 세션 항목을 서브메뉴로 변경:

```
● tmuxbar (2h 30m) — nvim [CPU 1.2% MEM 45MB]  ▸  [ 연결 ]
                                                    [ 종료 ]
● dev (10m) — bash [CPU 0.1% MEM 12MB]          ▸  [ 연결 ]
                                                    [ 종료 ]
───────────────
새 세션...
서버 종료
설정
종료
```

### 확인 경고
Kill 클릭 시 NSAlert 표시:
- **메시지:** "'tmuxbar' 세션을 종료하시겠습니까?"
- **버튼:** "종료" / "취소"
- 사용자가 "종료" 클릭 시에만 `AppCommand::KillSession` 전송

### 태그 체계
기존 태그 범위를 확장:
- 0~999: Attach (기존)
- 2000~2999: Kill (신규)

Kill 태그 = 2000 + 세션 인덱스. 핸들러에서 태그 범위로 Attach/Kill 구분.

### 변경 사항
- `session_menu.rs`: 세션 항목을 NSMenuItem + NSMenu(서브메뉴)로 변경. 서브메뉴에 "연결"/"종료" 항목 추가.
- `menu_action_handler.rs`: 2000~2999 태그 범위 처리. Kill 시 NSAlert 표시 후 확인된 경우만 `AppCommand::KillSession` 전송.
- `models.rs`: `AppCommand::KillSession`은 이미 존재하므로 추가 변경 없음.

### 제약 사항
- NSAlert는 메인 스레드에서 실행. `menu_action_handler`는 MainThreadOnly이므로 직접 호출 가능.

---

## 3. base-index 호환

### 문제
`snapshot_service.rs`의 `restore_session()`이 윈도우 인덱스를 0부터 순서대로 사용.
`tmux set -g base-index 1` 사용자는 복원 시 윈도우 타겟 불일치.

### 수정 방향
복원 시 tmux의 실제 base-index 값을 조회하여 인덱스 보정.

### 변경 사항

**TmuxAdapter 트레이트 확장:**
```rust
fn get_global_option(&self, name: &str) -> Result<String>;
```

**TmuxClient 구현:**
```rust
fn get_global_option(&self, name: &str) -> Result<String> {
    self.run_tmux(&["show-option", "-gv", name])
}
```

**SnapshotService::restore_session() 수정:**
1. `tmux show-option -gv base-index` 조회 (기본값 0)
2. `tmux show-option -gv pane-base-index` 조회 (기본값 0)
3. 첫 번째 윈도우: `new-session`이 자동 생성 → base-index로 타겟팅
4. 추가 윈도우: `new-window` 생성 → 인덱스 자동 할당 (tmux가 알아서 처리)
5. pane 타겟: `{session}:{window}.{pane_base_index + offset}` 형식 사용

**MockTmux 업데이트:**
- `get_global_option` 메서드 추가. 테스트에서 base-index 값을 주입 가능하도록.

---

## 4. 다국어 지원 (i18n)

### 지원 언어
| 코드 | 언어 |
|------|------|
| `ko` | 한국어 |
| `en` | English |
| `ja` | 日本語 |
| `zh` | 中文(简体) |

### config.toml
```toml
[general]
launch_at_login = true
# 한국어 = "ko", English = "en", 日本語 = "ja", 中文 = "zh"
language = "ko"
```

기본값: `"en"`

### 구현 방식
`src/i18n.rs` 모듈 신규 생성. 외부 리소스 파일 없이 코드 내 하드코딩.

```rust
pub enum Language { Ko, En, Ja, Zh }

impl Language {
    pub fn from_code(code: &str) -> Self { ... }  // 잘못된 코드 → En 폴백
}
```

번역 함수:
```rust
pub fn menu_new_session(lang: &Language) -> &'static str { ... }
pub fn menu_attach(lang: &Language) -> &'static str { ... }
pub fn menu_kill_session(lang: &Language) -> &'static str { ... }
pub fn menu_kill_server(lang: &Language) -> &'static str { ... }
pub fn menu_settings(lang: &Language) -> &'static str { ... }
pub fn menu_quit(lang: &Language) -> &'static str { ... }
pub fn alert_kill_confirm(lang: &Language, name: &str) -> String { ... }
pub fn alert_kill_title(lang: &Language) -> &'static str { ... }
pub fn alert_cancel(lang: &Language) -> &'static str { ... }
pub fn alert_confirm_kill(lang: &Language) -> &'static str { ... }
// 알림 메시지도 포함
```

### 번역 문자열 목록

| 키 | ko | en | ja | zh |
|----|----|----|----|----|
| menu_new_session | 새 세션... | New Session... | 新しいセッション... | 新建会话... |
| menu_attach | 연결 | Attach | 接続 | 连接 |
| menu_kill_session | 세션 종료 | Kill | セッション終了 | 终止会话 |
| menu_kill_server | 서버 종료 | Kill Server | サーバー終了 | 终止服务器 |
| menu_settings | 설정 | Settings | 設定 | 设置 |
| menu_quit | TmuxBar 종료 | Quit | TmuxBar 終了 | 退出 TmuxBar |
| alert_kill_title | 세션 종료 | Kill Session | セッション終了 | 终止会话 |
| alert_kill_confirm | '{name}' 세션을 종료하시겠습니까? | Kill session '{name}'? | セッション '{name}' を終了しますか？ | 确定终止会话 '{name}'？ |
| alert_cancel | 취소 | Cancel | キャンセル | 取消 |
| alert_confirm_kill | 종료 | Kill | 終了 | 终止 |
| notif_fd_alert | 파일 디스크립터 {pct}% 사용 중 | File descriptors at {pct}% | ファイルディスクリプタ {pct}% 使用中 | 文件描述符使用率 {pct}% |
| notif_inactivity | '{name}' 세션이 {mins}분간 비활성 | Session '{name}' inactive for {mins}m | セッション '{name}' が {mins}分間非アクティブ | 会话 '{name}' 已闲置 {mins} 分钟 |
| notif_restart_ok | 서버 재시작 완료 | Server restart complete | サーバー再起動完了 | 服务器重启完成 |
| notif_restart_fail | 서버 재시작 실패 | Server restart failed | サーバー再起動失敗 | 服务器重启失败 |

### 전달 방식
- `AppConfig`에 `language: String` 필드 추가 (`[general]` 섹션)
- `Language`를 `Arc<Mutex<AppState>>`의 필드로 추가. config 핫리로드 시 갱신. 메뉴 빌드/알림 시점에 현재 값 읽기.
- `SessionMenuBuilder::build_menu()`에 `Language` 파라미터 추가
- NSAlert 생성 시 `Language`로 번역된 문자열 사용
- 알림(NotificationService) 메시지도 `Language`로 번역

### 핫리로드
config.toml에서 language 변경 시 다음 메뉴 갱신 주기(3초)에 반영.
별도 핫리로드 로직 불필요 — 메뉴 빌드 시 매번 현재 config의 language를 읽으면 됨.

---

## 영향 범위 요약

| 파일 | 변경 내용 |
|------|-----------|
| `src/i18n.rs` | **신규** — Language enum + 번역 함수 |
| `src/models.rs` | AppCommand::OpenSettings 추가 |
| `src/infra/config.rs` | language 필드 추가, 기본값 "en" |
| `src/ui/session_menu.rs` | 세션 서브메뉴 구조 변경, Language 파라미터 |
| `src/ui/menu_action_handler.rs` | Kill 태그 처리, NSAlert, OpenSettings |
| `src/core/snapshot_service.rs` | base-index 보정 로직 |
| `src/infra/tmux_client.rs` | get_global_option 구현 |
| `src/app.rs` | OpenSettings 커맨드 처리, Language 전달 |
| 기존 테스트 | MockTmux에 get_global_option 추가 |
