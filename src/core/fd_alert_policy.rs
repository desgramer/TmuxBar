use crate::models::{AlertConfig, AlertLevel};

/// State machine that determines when to fire fd (file descriptor) notifications.
///
/// Notification trigger points use the configured thresholds:
/// - `warn_pct` (default 85): first warning notification
/// - `elevated_pct` (default 90): elevated notification
/// - `crit_pct` (default 95): critical zone -- each new integer percent triggers a notification
///
/// Key invariants:
/// - Never re-notify for the same percentage
/// - Dropping below `warn_pct` resets all tracking (re-entering triggers fresh notifications)
/// - In the critical zone (>= crit_pct), each new integer percent triggers a new notification
pub struct FdAlertPolicy {
    config: AlertConfig,
    last_notified_pct: Option<u8>,
}

impl FdAlertPolicy {
    pub fn new(config: AlertConfig) -> Self {
        Self {
            config,
            last_notified_pct: None,
        }
    }

    /// Evaluate the current fd usage percentage and return an alert if a notification
    /// should be dispatched. Returns `None` if no notification is needed.
    ///
    /// This method has side effects: it updates internal tracking state.
    pub fn evaluate(&mut self, current_pct: u8) -> Option<AlertLevel> {
        // Below warning threshold: reset all tracking, no notification.
        if current_pct < self.config.warn_pct {
            self.last_notified_pct = None;
            return None;
        }

        // Critical zone (>= crit_pct): notify for each new integer percent.
        if current_pct >= self.config.crit_pct {
            match self.last_notified_pct {
                Some(last) if last >= current_pct => return None,
                _ => {
                    self.last_notified_pct = Some(current_pct);
                    return Some(AlertLevel::Critical);
                }
            }
        }

        // Elevated zone (>= elevated_pct, < crit_pct): notify once when entering.
        if current_pct >= self.config.elevated_pct {
            match self.last_notified_pct {
                Some(last) if last >= self.config.elevated_pct => return None,
                _ => {
                    self.last_notified_pct = Some(self.config.elevated_pct);
                    return Some(AlertLevel::Elevated);
                }
            }
        }

        // Warning zone (>= warn_pct, < elevated_pct): notify once when entering.
        match self.last_notified_pct {
            Some(last) if last >= self.config.warn_pct => None,
            _ => {
                self.last_notified_pct = Some(self.config.warn_pct);
                Some(AlertLevel::Warning)
            }
        }
    }

    /// Returns the current alert level based purely on thresholds, without side effects.
    /// Used for icon colour which always reflects the current state.
    pub fn current_level(&self, current_pct: u8) -> AlertLevel {
        if current_pct >= self.config.crit_pct {
            AlertLevel::Critical
        } else if current_pct >= self.config.warn_pct {
            AlertLevel::Warning
        } else {
            AlertLevel::Normal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> FdAlertPolicy {
        FdAlertPolicy::new(AlertConfig::default())
    }

    #[test]
    fn test_below_threshold_returns_none() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(50), None);
    }

    #[test]
    fn test_first_warning_at_85() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
    }

    #[test]
    fn test_no_renotify_at_same_warning() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
        assert_eq!(policy.evaluate(86), None);
        assert_eq!(policy.evaluate(87), None);
    }

    #[test]
    fn test_elevated_at_90() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(90), Some(AlertLevel::Elevated));
    }

    #[test]
    fn test_critical_at_95() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(95), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_critical_per_percent() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(95), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(96), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(97), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_critical_no_renotify_same_pct() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(96), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(96), None);
        assert_eq!(policy.evaluate(96), None);
    }

    #[test]
    fn test_drop_below_resets() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
        assert_eq!(policy.evaluate(50), None);
        // Re-entering warn zone should trigger fresh warning
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
    }

    #[test]
    fn test_gradual_climb() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(50), None);
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
        assert_eq!(policy.evaluate(87), None);
        assert_eq!(policy.evaluate(90), Some(AlertLevel::Elevated));
        assert_eq!(policy.evaluate(93), None);
        assert_eq!(policy.evaluate(95), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(96), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(97), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(98), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(99), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_drop_from_critical_to_elevated() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(96), Some(AlertLevel::Critical));
        // Drop to elevated zone. last_notified was 96, which is >= elevated_pct (90),
        // so we already passed this threshold on the way up. No re-notification.
        assert_eq!(policy.evaluate(92), None);
    }

    #[test]
    fn test_drop_from_critical_to_below_then_back() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(96), Some(AlertLevel::Critical));
        // Drop below warn_pct resets everything
        assert_eq!(policy.evaluate(50), None);
        // Re-entering triggers fresh warning
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
    }

    #[test]
    fn test_current_level_pure() {
        let policy = default_policy();

        // Verify levels at various percentages
        assert_eq!(policy.current_level(0), AlertLevel::Normal);
        assert_eq!(policy.current_level(50), AlertLevel::Normal);
        assert_eq!(policy.current_level(84), AlertLevel::Normal);
        assert_eq!(policy.current_level(85), AlertLevel::Warning);
        assert_eq!(policy.current_level(89), AlertLevel::Warning);
        assert_eq!(policy.current_level(90), AlertLevel::Warning);
        assert_eq!(policy.current_level(94), AlertLevel::Warning);
        assert_eq!(policy.current_level(95), AlertLevel::Critical);
        assert_eq!(policy.current_level(99), AlertLevel::Critical);
        assert_eq!(policy.current_level(100), AlertLevel::Critical);

        // Verify calling current_level does not affect evaluate
        let mut policy2 = default_policy();
        let _ = policy2.current_level(95);
        let _ = policy2.current_level(99);
        // evaluate should still fire as if fresh
        assert_eq!(policy2.evaluate(85), Some(AlertLevel::Warning));
    }

    // -----------------------------------------------------------------------
    // Additional edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_jump_straight_to_critical() {
        let mut policy = default_policy();
        // Jumping from 0 to 99 should fire Critical (not Warning or Elevated)
        assert_eq!(policy.evaluate(99), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_drop_from_critical_to_warning_zone() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(96), Some(AlertLevel::Critical));
        // Drop to warning zone (85-89). last_notified was 96 >= warn_pct (85),
        // so we already passed warning threshold. No re-notification.
        assert_eq!(policy.evaluate(87), None);
    }

    #[test]
    fn test_critical_drop_then_rise_within_critical() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(98), Some(AlertLevel::Critical));
        // Drop within critical zone
        assert_eq!(policy.evaluate(96), None); // 96 < 98, already notified higher
        // Rise again past last_notified
        assert_eq!(policy.evaluate(99), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_boundary_values() {
        let mut policy = default_policy();
        // Exactly at warn_pct - 1
        assert_eq!(policy.evaluate(84), None);
        // Exactly at warn_pct
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
    }

    #[test]
    fn test_elevated_boundary() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(89), Some(AlertLevel::Warning));
        assert_eq!(policy.evaluate(90), Some(AlertLevel::Elevated));
        assert_eq!(policy.evaluate(94), None);
        assert_eq!(policy.evaluate(95), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_custom_config_thresholds() {
        let config = AlertConfig {
            warn_pct: 70,
            elevated_pct: 80,
            crit_pct: 90,
        };
        let mut policy = FdAlertPolicy::new(config);
        assert_eq!(policy.evaluate(69), None);
        assert_eq!(policy.evaluate(70), Some(AlertLevel::Warning));
        assert_eq!(policy.evaluate(75), None);
        assert_eq!(policy.evaluate(80), Some(AlertLevel::Elevated));
        assert_eq!(policy.evaluate(85), None);
        assert_eq!(policy.evaluate(90), Some(AlertLevel::Critical));
        assert_eq!(policy.evaluate(91), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_full_reset_cycle() {
        let mut policy = default_policy();
        // Full climb
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
        assert_eq!(policy.evaluate(90), Some(AlertLevel::Elevated));
        assert_eq!(policy.evaluate(95), Some(AlertLevel::Critical));
        // Drop and reset
        assert_eq!(policy.evaluate(50), None);
        // Full climb again -- all notifications should re-fire
        assert_eq!(policy.evaluate(85), Some(AlertLevel::Warning));
        assert_eq!(policy.evaluate(90), Some(AlertLevel::Elevated));
        assert_eq!(policy.evaluate(95), Some(AlertLevel::Critical));
    }

    #[test]
    fn test_zero_percent() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(0), None);
        assert_eq!(policy.current_level(0), AlertLevel::Normal);
    }

    #[test]
    fn test_hundred_percent() {
        let mut policy = default_policy();
        assert_eq!(policy.evaluate(100), Some(AlertLevel::Critical));
        assert_eq!(policy.current_level(100), AlertLevel::Critical);
    }
}
