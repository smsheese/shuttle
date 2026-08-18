use crate::config::{AppConfig, NotificationConfig};
use crate::db::Database;
use crate::models::{Account, Conversation};
use chrono::{Local, NaiveTime};

pub fn notify(title: &str, body: &str) {
    tracing::info!(
        target: "shuttle::notification",
        title = %title,
        body = %body,
        "desktop notification"
    );
    let preview: String = body.chars().take(180).collect();
    let result = notify_rust::Notification::new()
        .summary(title)
        .body(&preview)
        .appname("Shuttle")
        .timeout(notify_rust::Timeout::Milliseconds(8000))
        .show();
    if let Err(e) = result {
        tracing::debug!(target: "shuttle::notification", "native notify failed: {e}");
    }
}

pub fn notify_message(sender: &str, preview: &str) {
    notify(sender, preview);
}

pub fn in_quiet_hours(cfg: &NotificationConfig) -> bool {
    if !cfg.quiet_hours_enabled {
        return false;
    }
    let Ok(start) = NaiveTime::parse_from_str(&cfg.quiet_hours_start, "%H:%M") else {
        return false;
    };
    let Ok(end) = NaiveTime::parse_from_str(&cfg.quiet_hours_end, "%H:%M") else {
        return false;
    };
    let now = Local::now().time();
    if start <= end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

/// Most specific wins: muted always blocks. Then chat → account → app-wide + quiet hours.
pub fn should_notify(
    cfg: &AppConfig,
    account: &Account,
    conv: &Conversation,
) -> bool {
    if conv.muted || account.muted {
        return false;
    }
    if let Some(chat) = conv.notify_enabled {
        if !chat {
            return false;
        }
    } else if let Some(acct) = account.notify_enabled {
        if !acct {
            return false;
        }
    }
    if !cfg.notifications.enabled || in_quiet_hours(&cfg.notifications) {
        return false;
    }
    true
}

pub fn should_send_receipt(account: &Account, conv: &Conversation) -> bool {
    conv.send_receipts.unwrap_or(account.send_receipts)
}

pub fn should_notify_ids(
    db: &Database,
    cfg: &AppConfig,
    account_id: &str,
    conversation_id: &str,
) -> bool {
    let Ok(accounts) = db.list_accounts() else {
        return false;
    };
    let Some(account) = accounts.into_iter().find(|a| a.id == account_id) else {
        return false;
    };
    let Ok(conv) = db.get_conversation(conversation_id) else {
        return false;
    };
    should_notify(cfg, &account, &conv)
}
