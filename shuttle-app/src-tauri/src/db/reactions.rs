//! Message reactions (an emoji one person attaches to one message).
//!
//! Connectors report reactions as facts: `(chat, target message remote id, reactor, emoji,
//! time)`. A reaction is NOT a message: it never touches a chat's time, preview or unread
//! state, and it never creates a chat. Rows are keyed by the target's remote id, so a reaction
//! that arrives before its message (history order) attaches as soon as the message exists.
//!
//! One reactor has one reaction per message: a newer fact replaces the older one, an empty
//! emoji deletes it, and a fact older than the stored one is ignored (history replayed after
//! a live event must not undo it).

use super::*;
use std::collections::BTreeMap;

/// One reaction fact from a connector.
#[derive(Debug, Clone)]
pub struct ReactionFact {
    pub target_remote_id: String,
    /// Stable id of the reactor (`me` for the account itself, else a JID).
    pub reactor_id: String,
    pub reactor_name: Option<String>,
    /// Empty means the reaction was removed.
    pub emoji: String,
    pub at: DateTime<Utc>,
    pub from_me: bool,
}

/// What changed, so the caller can tell the UI.
#[derive(Debug, Default)]
pub struct ReactionOutcome {
    pub conversation_id: Option<String>,
    /// Remote ids of the messages whose reactions changed.
    pub changed_targets: Vec<String>,
}

fn at_string(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Group raw rows `(reactor_id, name, emoji, from_me)` (oldest first) per emoji.
fn group_rows(rows: &[(String, Option<String>, String, bool)]) -> Vec<ReactionGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, ReactionGroup> = BTreeMap::new();
    for (reactor, name, emoji, from_me) in rows {
        let label = if *from_me {
            "You".to_string()
        } else {
            name.clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| reactor.split('@').next().unwrap_or(reactor).to_string())
        };
        let g = groups.entry(emoji.clone()).or_insert_with(|| {
            order.push(emoji.clone());
            ReactionGroup {
                emoji: emoji.clone(),
                count: 0,
                names: Vec::new(),
                mine: false,
            }
        });
        g.count += 1;
        g.names.push(label);
        g.mine |= *from_me;
    }
    let mut out: Vec<ReactionGroup> = order
        .into_iter()
        .filter_map(|e| groups.remove(&e))
        .collect();
    // Most used first; ties keep the order in which the emoji first appeared.
    out.sort_by(|a, b| b.count.cmp(&a.count));
    out
}

/// Reactions of the given targets (remote ids) in one chat, grouped per message.
pub(super) fn reactions_by_target(
    conn: &Connection,
    conversation_id: &str,
) -> Result<HashMap<String, Vec<ReactionGroup>>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT target_remote_id, reactor_id, reactor_name, emoji, from_me
         FROM message_reactions WHERE conversation_id = ?1
         ORDER BY target_remote_id, unixepoch(reacted_at, 'subsec'), reactor_id",
    )?;
    let mut raw: HashMap<String, Vec<(String, Option<String>, String, bool)>> = HashMap::new();
    let rows = stmt.query_map(params![conversation_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)? != 0,
        ))
    })?;
    for row in rows {
        let (target, reactor, name, emoji, from_me) = row?;
        raw.entry(target)
            .or_default()
            .push((reactor, name, emoji, from_me));
    }
    Ok(raw
        .into_iter()
        .map(|(target, rows)| (target, group_rows(&rows)))
        .collect())
}

impl Database {
    /// Apply reaction facts for one chat. Returns which target messages changed.
    pub fn apply_reactions(
        &self,
        account_id: &str,
        remote: &str,
        alts: &[String],
        facts: &[ReactionFact],
    ) -> Result<ReactionOutcome, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let Some(conversation_id) = super::chat::find_conversation_id(&conn, account_id, remote, alts)?
        else {
            return Ok(ReactionOutcome::default());
        };
        let tx = conn.unchecked_transaction()?;
        let mut changed: Vec<String> = Vec::new();
        for f in facts {
            if f.target_remote_id.is_empty() || f.reactor_id.is_empty() {
                continue;
            }
            let existing: Option<(String, String)> = tx
                .query_row(
                    "SELECT emoji, reacted_at FROM message_reactions
                     WHERE conversation_id = ?1 AND target_remote_id = ?2 AND reactor_id = ?3",
                    params![conversation_id, f.target_remote_id, f.reactor_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((_, stored_at)) = &existing {
                if parse_stored_datetime(stored_at).is_some_and(|t| t > f.at) {
                    continue; // stale: a newer reaction from this person is already stored
                }
            }
            let touched = if f.emoji.trim().is_empty() {
                if existing.is_none() {
                    false
                } else {
                    tx.execute(
                        "DELETE FROM message_reactions
                         WHERE conversation_id = ?1 AND target_remote_id = ?2 AND reactor_id = ?3",
                        params![conversation_id, f.target_remote_id, f.reactor_id],
                    )?;
                    true
                }
            } else if existing.as_ref().is_some_and(|(e, _)| *e == f.emoji) {
                // Same emoji again: keep the row, only learn a better name / newer time.
                tx.execute(
                    "UPDATE message_reactions SET reacted_at = ?4,
                        reactor_name = COALESCE(?5, reactor_name)
                     WHERE conversation_id = ?1 AND target_remote_id = ?2 AND reactor_id = ?3",
                    params![
                        conversation_id,
                        f.target_remote_id,
                        f.reactor_id,
                        at_string(f.at),
                        f.reactor_name
                    ],
                )?;
                false
            } else {
                tx.execute(
                    "INSERT OR REPLACE INTO message_reactions
                        (conversation_id, target_remote_id, reactor_id, reactor_name, emoji, from_me, reacted_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        conversation_id,
                        f.target_remote_id,
                        f.reactor_id,
                        f.reactor_name,
                        f.emoji.trim(),
                        f.from_me as i64,
                        at_string(f.at)
                    ],
                )?;
                true
            };
            if touched && !changed.contains(&f.target_remote_id) {
                changed.push(f.target_remote_id.clone());
            }
        }
        tx.commit()?;
        Ok(ReactionOutcome {
            conversation_id: Some(conversation_id),
            changed_targets: changed,
        })
    }

    /// Current grouped reactions of the given messages (remote ids) in one chat.
    pub fn reactions_for_targets(
        &self,
        conversation_id: &str,
        targets: &[String],
    ) -> Result<HashMap<String, Vec<ReactionGroup>>, DbError> {
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let mut all = reactions_by_target(&conn, conversation_id)?;
        all.retain(|k, _| targets.contains(k));
        Ok(all)
    }
}
