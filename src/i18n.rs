/// Supported UI languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    Ko,
    #[default]
    En,
    Ja,
    Zh,
}

impl Language {
    /// Parse a BCP-47-style language code (case-insensitive).
    /// Unknown codes fall back to `En`.
    pub fn from_code(code: &str) -> Self {
        match code.to_ascii_lowercase().as_str() {
            "ko" | "ko-kr" => Language::Ko,
            "en" | "en-us" | "en-gb" => Language::En,
            "ja" | "ja-jp" => Language::Ja,
            "zh" | "zh-cn" | "zh-tw" | "zh-hans" | "zh-hant" => Language::Zh,
            _ => Language::En,
        }
    }
}

// ── Menu strings ─────────────────────────────────────────────────────────────

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

// ── Alert strings ─────────────────────────────────────────────────────────────

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
        Language::Ko => format!("'{name}' 세션을 종료하시겠습니까?"),
        Language::En => format!("Kill session '{name}'?"),
        Language::Ja => format!("セッション '{name}' を終了しますか？"),
        Language::Zh => format!("确定终止会话 '{name}'？"),
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

// ── Notification strings ──────────────────────────────────────────────────────

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
        Language::Ko => format!("파일 디스크립터 사용량 {pct}%"),
        Language::En => format!("File descriptor usage at {pct}%"),
        Language::Ja => format!("ファイルディスクリプタ使用率 {pct}%"),
        Language::Zh => format!("文件描述符使用率 {pct}%"),
    }
}

pub fn notif_fd_elevated(lang: &Language, pct: u8) -> String {
    match lang {
        Language::Ko => format!("⚠ 파일 디스크립터 사용량 {pct}% — 위험 수준에 근접"),
        Language::En => format!("⚠ File descriptor usage at {pct}% — approaching critical"),
        Language::Ja => format!("⚠ ファイルディスクリプタ使用率 {pct}% — 危険水準に近づいています"),
        Language::Zh => format!("⚠ 文件描述符使用率 {pct}% — 接近临界值"),
    }
}

pub fn notif_fd_critical(lang: &Language, pct: u8) -> String {
    match lang {
        Language::Ko => format!("🔴 파일 디스크립터 사용량 {pct}% — 재시작을 고려하세요"),
        Language::En => format!("🔴 File descriptor usage at {pct}% — consider restarting"),
        Language::Ja => format!("🔴 ファイルディスクリプタ使用率 {pct}% — 再起動を検討してください"),
        Language::Zh => format!("🔴 文件描述符使用率 {pct}% — 建议重启"),
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
        Language::Ko => format!("'{name}' 세션이 {mins}분간 비활성 상태입니다"),
        Language::En => format!("Session '{name}' has been inactive for {mins} minutes"),
        Language::Ja => format!("セッション '{name}' が {mins} 分間非アクティブです"),
        Language::Zh => format!("会话 '{name}' 已闲置 {mins} 分钟"),
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
        Language::Ko => format!("tmux 서버가 성공적으로 재시작되었습니다. {details}"),
        Language::En => format!("tmux server restarted successfully. {details}"),
        Language::Ja => format!("tmux サーバーが正常に再起動されました。{details}"),
        Language::Zh => format!("tmux 服务器已成功重启。{details}"),
    }
}

pub fn notif_restart_fail_body(lang: &Language, details: &str) -> String {
    match lang {
        Language::Ko => format!("tmux 서버 재시작 실패: {details}"),
        Language::En => format!("tmux server restart failed: {details}"),
        Language::Ja => format!("tmux サーバーの再起動に失敗しました: {details}"),
        Language::Zh => format!("tmux 服务器重启失败: {details}"),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // from_code — known codes
    #[test]
    fn from_code_known() {
        assert_eq!(Language::from_code("ko"), Language::Ko);
        assert_eq!(Language::from_code("en"), Language::En);
        assert_eq!(Language::from_code("ja"), Language::Ja);
        assert_eq!(Language::from_code("zh"), Language::Zh);
    }

    // from_code — regional variants
    #[test]
    fn from_code_regional_variants() {
        assert_eq!(Language::from_code("ko-kr"), Language::Ko);
        assert_eq!(Language::from_code("en-us"), Language::En);
        assert_eq!(Language::from_code("en-gb"), Language::En);
        assert_eq!(Language::from_code("ja-jp"), Language::Ja);
        assert_eq!(Language::from_code("zh-cn"), Language::Zh);
        assert_eq!(Language::from_code("zh-tw"), Language::Zh);
    }

    // from_code — case insensitive
    #[test]
    fn from_code_case_insensitive() {
        assert_eq!(Language::from_code("KO"), Language::Ko);
        assert_eq!(Language::from_code("EN"), Language::En);
        assert_eq!(Language::from_code("JA"), Language::Ja);
        assert_eq!(Language::from_code("ZH"), Language::Zh);
        assert_eq!(Language::from_code("Ko"), Language::Ko);
        assert_eq!(Language::from_code("En-US"), Language::En);
    }

    // from_code — unknown falls back to En
    #[test]
    fn from_code_unknown_fallback() {
        assert_eq!(Language::from_code("fr"), Language::En);
        assert_eq!(Language::from_code("de"), Language::En);
        assert_eq!(Language::from_code(""), Language::En);
        assert_eq!(Language::from_code("zzz"), Language::En);
    }

    // Default is En
    #[test]
    fn default_is_en() {
        assert_eq!(Language::default(), Language::En);
    }

    // All static menu strings are non-empty
    #[test]
    fn menu_strings_non_empty() {
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            assert!(!menu_new_session(&lang).is_empty());
            assert!(!menu_attach(&lang).is_empty());
            assert!(!menu_kill_session(&lang).is_empty());
            assert!(!menu_kill_server(&lang).is_empty());
            assert!(!menu_settings(&lang).is_empty());
            assert!(!menu_quit(&lang).is_empty());
        }
    }

    // All static alert strings are non-empty
    #[test]
    fn alert_strings_non_empty() {
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            assert!(!alert_kill_title(&lang).is_empty());
            assert!(!alert_cancel(&lang).is_empty());
            assert!(!alert_confirm_kill(&lang).is_empty());
        }
    }

    // All static notification strings are non-empty
    #[test]
    fn notif_static_strings_non_empty() {
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            assert!(!notif_fd_title(&lang).is_empty());
            assert!(!notif_inactivity_title(&lang).is_empty());
            assert!(!notif_restart_success_title(&lang).is_empty());
            assert!(!notif_restart_fail_title(&lang).is_empty());
        }
    }

    // alert_kill_confirm contains session name
    #[test]
    fn alert_kill_confirm_contains_name() {
        let name = "my-session";
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let s = alert_kill_confirm(&lang, name);
            assert!(s.contains(name), "lang={lang:?} missing name in: {s}");
        }
    }

    // notif_fd_warn contains percentage
    #[test]
    fn notif_fd_warn_contains_pct() {
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let s = notif_fd_warn(&lang, 87);
            assert!(s.contains("87"), "lang={lang:?} missing pct in: {s}");
        }
    }

    // notif_fd_elevated contains ⚠ and percentage
    #[test]
    fn notif_fd_elevated_prefix_and_pct() {
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let s = notif_fd_elevated(&lang, 91);
            assert!(s.contains('⚠'), "lang={lang:?} missing ⚠ in: {s}");
            assert!(s.contains("91"), "lang={lang:?} missing pct in: {s}");
        }
    }

    // notif_fd_critical contains 🔴 and percentage
    #[test]
    fn notif_fd_critical_prefix_and_pct() {
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let s = notif_fd_critical(&lang, 96);
            assert!(s.contains('🔴'), "lang={lang:?} missing 🔴 in: {s}");
            assert!(s.contains("96"), "lang={lang:?} missing pct in: {s}");
        }
    }

    // notif_inactivity_body contains name and minutes
    #[test]
    fn notif_inactivity_body_contains_name_and_mins() {
        let name = "dev";
        let mins = 30u64;
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let s = notif_inactivity_body(&lang, name, mins);
            assert!(s.contains(name), "lang={lang:?} missing name in: {s}");
            assert!(s.contains("30"), "lang={lang:?} missing mins in: {s}");
        }
    }

    // notif_restart_success_body contains details
    #[test]
    fn notif_restart_success_body_contains_details() {
        let details = "3 sessions restored";
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let s = notif_restart_success_body(&lang, details);
            assert!(s.contains(details), "lang={lang:?} missing details in: {s}");
        }
    }

    // notif_restart_fail_body contains details
    #[test]
    fn notif_restart_fail_body_contains_details() {
        let details = "tmux not found";
        for lang in [Language::Ko, Language::En, Language::Ja, Language::Zh] {
            let s = notif_restart_fail_body(&lang, details);
            assert!(s.contains(details), "lang={lang:?} missing details in: {s}");
        }
    }
}
