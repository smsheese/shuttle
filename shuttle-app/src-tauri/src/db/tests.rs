//! Rule tests for the chat state (R1-R8), against a temporary database.

use super::chat::{
    compare_conversations, compute_wa_title, ConversationEvent, IngestKind, SendReconcile,
    TitleInputs,
};
use super::*;
use chrono::Duration;
use serde_json::{json, Map, Value};

const WA_PHONE: &str = "15550001111@s.whatsapp.net";
const WA_LID: &str = "9988776655@lid";
const WA_GROUP: &str = "120363000000@g.us";

struct Fx {
    _dir: tempfile::TempDir,
    db: Database,
    account: String,
    t0: DateTime<Utc>,
}

fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).unwrap();
    let account = db.create_account("whatsapp", "WA").unwrap().id;
    Fx {
        _dir: dir,
        db,
        account,
        t0: Utc::now(),
    }
}

/// Seconds after the inbox baseline (so messages count as unread-eligible).
fn at(fx: &Fx, secs: i64) -> DateTime<Utc> {
    fx.t0 + Duration::seconds(secs)
}

fn names(v: Value) -> Option<Map<String, Value>> {
    Some(v.as_object().unwrap().clone())
}

fn conv_ev(remote: &str, nm: Option<Value>) -> ConversationEvent {
    ConversationEvent {
        remote_id: remote.into(),
        names: nm.and_then(names),
        conversation_type: Some(jid_type(remote)),
        ..Default::default()
    }
}

fn jid_type(remote: &str) -> ConversationType {
    super::chat::jid_conversation_type(remote)
}

fn msg(id: &str, body: &str, ts: DateTime<Utc>, from_me: bool) -> Message {
    Message {
        id: Uuid::new_v4().to_string(),
        conversation_id: String::new(),
        remote_id: Some(id.into()),
        sender_id: None,
        sender_name: Some("Someone".into()),
        direction: if from_me {
            MessageDirection::Outbound
        } else {
            MessageDirection::Inbound
        },
        body: body.into(),
        timestamp: ts,
        status: if from_me {
            MessageStatus::Sent
        } else {
            MessageStatus::Delivered
        },
        metadata: json!({ "id": id, "text": body }),
        starred: false,
        pinned: false,
        reactions: Vec::new(),
        reply: None,
    }
}

fn ingest(
    fx: &Fx,
    remote: &str,
    alts: &[&str],
    history: bool,
    m: Message,
) -> super::chat::IngestOutcome {
    let alts: Vec<String> = alts.iter().map(|s| s.to_string()).collect();
    let preview = message_preview(&m.body, &m.metadata);
    fx.db
        .ingest_message(&fx.account, remote, &alts, history, m, &preview, "Sender")
        .unwrap()
}

fn conv(fx: &Fx, remote: &str) -> Conversation {
    fx.db
        .get_conversation_by_remote(&fx.account, remote)
        .unwrap()
        .unwrap()
}

fn summary(c: &Conversation) -> (String, Option<i64>, Option<String>, i64) {
    (
        c.title.clone(),
        c.last_message_at.map(|t| t.timestamp_millis()),
        c.last_message_preview.clone(),
        c.unread_count,
    )
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, head.clone());
            out.push(p);
        }
    }
    out
}

#[derive(Clone)]
enum Fact {
    Conv(ConversationEvent),
    Msg {
        id: &'static str,
        body: &'static str,
        secs: i64,
        from_me: bool,
        history: bool,
    },
}

fn apply(fx: &Fx, remote: &str, f: &Fact) {
    match f {
        Fact::Conv(ev) => {
            fx.db.apply_conversation_event(&fx.account, ev).unwrap();
        }
        Fact::Msg {
            id,
            body,
            secs,
            from_me,
            history,
        } => {
            ingest(
                fx,
                remote,
                &[],
                *history,
                msg(id, body, at(fx, *secs), *from_me),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[test]
fn same_facts_in_any_order_give_same_state() {
    // Absolute times are fixed relative to one t0 shared by every run.
    let t0 = Utc::now();
    let ts = |s: i64| t0 + Duration::seconds(s);
    let ev_a = ConversationEvent {
        last_message_at: Some(ts(30)),
        preview: Some("third".into()),
        ..conv_ev(WA_PHONE, Some(json!({ "push": "Pat" })))
    };
    let ev_b = ConversationEvent {
        last_message_at: Some(ts(10)),
        preview: Some("first".into()),
        ..conv_ev(WA_PHONE, Some(json!({ "contact": "Pat Contact" })))
    };
    let facts = vec![
        Fact::Conv(ev_a),
        Fact::Conv(ev_b),
        Fact::Msg {
            id: "m1",
            body: "first",
            secs: 10,
            from_me: false,
            history: true,
        },
        Fact::Msg {
            id: "m3",
            body: "third",
            secs: 30,
            from_me: false,
            history: false,
        },
        Fact::Msg {
            id: "m2",
            body: "second",
            secs: 20,
            from_me: false,
            history: false,
        },
    ];
    let mut baseline_state = None;
    for order in permutations(&facts) {
        let mut f = fx();
        f.t0 = t0; // message times are relative to the same instant in every run
        for fact in &order {
            apply(&f, WA_PHONE, fact);
        }
        let got = summary(&conv(&f, WA_PHONE));
        match &baseline_state {
            None => baseline_state = Some(got),
            Some(want) => assert_eq!(&got, want),
        }
    }
    let (title, last, preview, unread) = baseline_state.unwrap();
    assert_eq!(title, "Pat Contact");
    assert_eq!(last, Some(ts(30).timestamp_millis()));
    assert_eq!(preview.as_deref(), Some("third"));
    assert_eq!(unread, 3);
}

#[test]
fn chat_time_never_moves_backwards() {
    let f = fx();
    let mut ev = conv_ev(WA_PHONE, Some(json!({})));
    ev.last_message_at = Some(at(&f, 50));
    ev.preview = Some("newest".into());
    f.db.apply_conversation_event(&f.account, &ev).unwrap();

    let mut older = conv_ev(WA_PHONE, Some(json!({})));
    older.last_message_at = Some(at(&f, 5));
    older.preview = Some("old".into());
    let out = f.db.apply_conversation_event(&f.account, &older).unwrap();
    assert!(!out.changed, "an older event changes nothing");

    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("old1", "older body", at(&f, 2), false),
    );
    let c = conv(&f, WA_PHONE);
    assert_eq!(
        c.last_message_at.unwrap().timestamp_millis(),
        at(&f, 50).timestamp_millis()
    );
    assert_eq!(c.last_message_preview.as_deref(), Some("newest"));
}

#[test]
fn unread_follows_the_read_marker_for_history_and_live_alike() {
    let f = fx();
    // Older than the inbox baseline: bulk history never counts.
    ingest(
        &f,
        WA_PHONE,
        &[],
        true,
        msg("h0", "ancient", f.t0 - Duration::days(1), false),
    );
    assert_eq!(conv(&f, WA_PHONE).unread_count, 0);

    ingest(
        &f,
        WA_PHONE,
        &[],
        true,
        msg("h1", "history", at(&f, 10), false),
    );
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("l1", "live", at(&f, 20), false),
    );
    assert_eq!(conv(&f, WA_PHONE).unread_count, 2);

    // An outbound message (even delivered as history) reads the chat up to its time.
    ingest(
        &f,
        WA_PHONE,
        &[],
        true,
        msg("o1", "reply", at(&f, 30), true),
    );
    assert_eq!(conv(&f, WA_PHONE).unread_count, 0);
    // A late inbound message older than the marker stays read; a newer one counts.
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("l2", "late but old", at(&f, 25), false),
    );
    assert_eq!(conv(&f, WA_PHONE).unread_count, 0);
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("l3", "new", at(&f, 40), false),
    );
    assert_eq!(conv(&f, WA_PHONE).unread_count, 1);

    // chat.read from another device clears it.
    let out =
        f.db.apply_chat_read(&f.account, WA_PHONE, &[], at(&f, 45))
            .unwrap();
    assert!(out.changed);
    assert_eq!(conv(&f, WA_PHONE).unread_count, 0);
    let again =
        f.db.apply_chat_read(&f.account, WA_PHONE, &[], at(&f, 45))
            .unwrap();
    assert!(!again.changed, "repeating the same read is a no-op");

    // Manual unread keeps at least one until the chat is read again.
    let id = conv(&f, WA_PHONE).id;
    f.db.mark_unread(&id).unwrap();
    assert_eq!(conv(&f, WA_PHONE).unread_count, 1);
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("l4", "another", at(&f, 60), false),
    );
    // Manual-unread is "at least one", not an extra: one real unread message gives 1.
    assert_eq!(conv(&f, WA_PHONE).unread_count, 1);
    f.db.mark_conversation_read(&id).unwrap();
    assert_eq!(conv(&f, WA_PHONE).unread_count, 0);
}

#[test]
fn titles_follow_the_precedence_rules() {
    let f = fx();
    let title = |remote: &str| conv(&f, remote).title;

    // Message-first creation never uses the sender's name.
    ingest(&f, WA_PHONE, &[], false, msg("a1", "hi", at(&f, 1), false));
    assert_eq!(title(WA_PHONE), "+15550001111");
    ingest(&f, WA_GROUP, &[], false, msg("g1", "hi", at(&f, 1), false));
    assert_eq!(title(WA_GROUP), "Group");

    f.db.apply_conversation_event(
        &f.account,
        &conv_ev(WA_PHONE, Some(json!({ "push": "Push" }))),
    )
    .unwrap();
    assert_eq!(title(WA_PHONE), "Push");
    f.db.apply_conversation_event(
        &f.account,
        &conv_ev(WA_PHONE, Some(json!({ "verified": "Verified Co" }))),
    )
    .unwrap();
    assert_eq!(title(WA_PHONE), "Verified Co");
    f.db.apply_conversation_event(
        &f.account,
        &conv_ev(WA_PHONE, Some(json!({ "contact": "Saved" }))),
    )
    .unwrap();
    assert_eq!(title(WA_PHONE), "Saved");
    // A later event without names (or with other keys) never erases what is known.
    f.db.apply_conversation_event(&f.account, &conv_ev(WA_PHONE, Some(json!({}))))
        .unwrap();
    assert_eq!(title(WA_PHONE), "Saved");
    // Messages never change the title of an existing chat.
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("a2", "again", at(&f, 2), false),
    );
    assert_eq!(title(WA_PHONE), "Saved");

    f.db.apply_conversation_event(
        &f.account,
        &conv_ev(WA_GROUP, Some(json!({ "subject": "Team" }))),
    )
    .unwrap();
    assert_eq!(title(WA_GROUP), "Team");

    // Address-book row in `contacts` wins over event names, via refresh too.
    f.db.upsert_contact(&f.account, WA_PHONE, "Book Name")
        .unwrap();
    f.db.refresh_titles(&f.account).unwrap();
    assert_eq!(title(WA_PHONE), "Book Name");

    // Self chat.
    let me = "15559998888@s.whatsapp.net";
    let mut ev = conv_ev(me, Some(json!({})));
    ev.is_self = true;
    f.db.apply_conversation_event(&f.account, &ev).unwrap();
    assert_eq!(title(me), "Message yourself");
    let mut ev = conv_ev(me, Some(json!({ "contact": "Me Myself" })));
    ev.is_self = true;
    f.db.apply_conversation_event(&f.account, &ev).unwrap();
    assert_eq!(title(me), "Me Myself (You)");
}

#[test]
fn title_function_cases() {
    let none = Map::new();
    let direct = ConversationType::Direct;
    let t = |names: &Map<String, Value>, stored: &str, contact: Option<&str>| {
        compute_wa_title(&TitleInputs {
            remote_id: WA_PHONE,
            conversation_type: &direct,
            is_self: false,
            names,
            contact_name: contact,
            stored_title: stored,
        })
    };
    assert_eq!(t(&none, "", None), "+15550001111");
    assert_eq!(t(&none, "+15550001111", None), "+15550001111");
    assert_eq!(t(&none, "Old Good Title", None), "Old Good Title");
    let n = names(json!({ "push": "P", "verified": "V", "contact": "C" })).unwrap();
    assert_eq!(t(&n, "x", Some("Book")), "Book");
    assert_eq!(t(&n, "x", None), "C");
}

#[test]
fn legacy_events_keep_todays_title_logic() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).unwrap();
    let acct = db.create_account("telegram", "TG").unwrap().id;
    let ev = |title: &str, history: bool| ConversationEvent {
        remote_id: "1234".into(),
        legacy_title: Some(title.into()),
        history,
        ..Default::default()
    };
    db.apply_conversation_event(&acct, &ev("Bob", false))
        .unwrap();
    db.apply_conversation_event(&acct, &ev("Robert", false))
        .unwrap();
    let c = db
        .get_conversation_by_remote(&acct, "1234")
        .unwrap()
        .unwrap();
    assert_eq!(c.title, "Bob", "a live event does not replace a good title");
    db.apply_conversation_event(&acct, &ev("Robert", true))
        .unwrap();
    let c = db
        .get_conversation_by_remote(&acct, "1234")
        .unwrap()
        .unwrap();
    assert_eq!(c.title, "Robert", "bulk sync may replace it");

    // Message-first creation for another connector falls back to the sender name.
    let m = msg("t1", "hello", Utc::now() + Duration::seconds(5), false);
    db.ingest_message(&acct, "777", &[], false, m, "hello", "Alice")
        .unwrap();
    let c = db
        .get_conversation_by_remote(&acct, "777")
        .unwrap()
        .unwrap();
    assert_eq!(c.title, "Alice");
    assert_eq!(c.unread_count, 1);
}

#[test]
fn admin_only_group_facts_merge_into_the_chat_and_never_erase() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).unwrap();
    let acct = db.create_account("whatsapp", "WA").unwrap().id;
    let group = "120363427536439238@g.us";
    let ev = |announce: Option<bool>, admin: Option<bool>| ConversationEvent {
        remote_id: group.into(),
        conversation_type: Some(ConversationType::Group),
        announce_only: announce,
        self_is_admin: admin,
        ..Default::default()
    };
    let facts = |db: &Database| {
        let md = db
            .get_conversation_by_remote(&acct, group)
            .unwrap()
            .unwrap()
            .metadata;
        (
            md.get("announce_only").and_then(Value::as_bool),
            md.get("self_is_admin").and_then(Value::as_bool),
        )
    };
    db.apply_conversation_event(&acct, &ev(Some(true), Some(false)))
        .unwrap();
    assert_eq!(facts(&db), (Some(true), Some(false)));

    // An event that says nothing about sending keeps both stored facts.
    let out = db
        .apply_conversation_event(&acct, &ev(None, None))
        .unwrap();
    assert!(
        out.conversation.is_none(),
        "an unchanged chat is not rewritten"
    );
    assert_eq!(facts(&db), (Some(true), Some(false)));

    // One fact at a time: an admin promotion, then the group opened to members.
    db.apply_conversation_event(&acct, &ev(None, Some(true)))
        .unwrap();
    assert_eq!(facts(&db), (Some(true), Some(true)));
    let out = db
        .apply_conversation_event(&acct, &ev(Some(false), None))
        .unwrap();
    assert!(out.changed);
    assert_eq!(facts(&db), (Some(false), Some(true)));

    // A chat that never reported them stays unknown, which the UI reads as writable.
    db.apply_conversation_event(
        &acct,
        &ConversationEvent {
            remote_id: "628111111111@s.whatsapp.net".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let direct = db
        .get_conversation_by_remote(&acct, "628111111111@s.whatsapp.net")
        .unwrap()
        .unwrap();
    assert!(direct.metadata.get("announce_only").is_none());
}

#[test]
fn alias_rename_and_merge() {
    let f = fx();
    // One chat known only by its LID, with a message.
    ingest(
        &f,
        WA_LID,
        &[],
        false,
        msg("x1", "hello", at(&f, 10), false),
    );
    let lid_id = conv(&f, WA_LID).id;

    // The phone JID becomes known: the row is renamed, ids and messages survive.
    let mut ev = conv_ev(WA_PHONE, Some(json!({ "push": "Pat" })));
    ev.alt_remote_ids = vec![WA_LID.into()];
    let out = f.db.apply_conversation_event(&f.account, &ev).unwrap();
    assert!(out.merged.is_empty());
    let c = conv(&f, WA_PHONE);
    assert_eq!(c.id, lid_id);
    assert!(f
        .db
        .get_conversation_by_remote(&f.account, WA_LID)
        .unwrap()
        .is_none());
    assert_eq!(c.title, "Pat");
    assert_eq!(f.db.list_messages(&c.id, 10).unwrap().len(), 1);
    // A later message that still names the old JID lands in the same chat.
    ingest(
        &f,
        WA_LID,
        &[],
        false,
        msg("x2", "again", at(&f, 20), false),
    );
    assert_eq!(f.db.list_messages(&c.id, 10).unwrap().len(), 2);

    // Two separate rows for one person get merged into the remote_id row.
    let other_lid = "5544332211@lid";
    ingest(
        &f,
        other_lid,
        &[],
        false,
        msg("x2", "dup of x2", at(&f, 20), false),
    );
    ingest(
        &f,
        other_lid,
        &[],
        false,
        msg("y1", "only here", at(&f, 30), false),
    );
    let other = conv(&f, other_lid);
    f.db.patch_conversation(
        &other.id,
        &ConversationPatch {
            pinned: Some(true),
            notes: Some("remember".into()),
            ..empty_patch()
        },
    )
    .unwrap();
    let todo = f.db.add_todo(&other.id, &f.account, "call", None).unwrap();
    let mut ev = conv_ev(WA_PHONE, Some(json!({})));
    ev.alt_remote_ids = vec![other_lid.into()];
    let out = f.db.apply_conversation_event(&f.account, &ev).unwrap();
    assert_eq!(out.merged.len(), 1);
    assert_eq!(out.merged[0].removed_id, other.id);
    assert_eq!(out.merged[0].into_id, c.id);
    let merged = conv(&f, WA_PHONE);
    assert!(
        merged.pinned && merged.notes == "remember",
        "gaps are filled from the other chat"
    );
    let msgs = f.db.list_messages(&merged.id, 20).unwrap();
    let ids: Vec<_> = msgs.iter().filter_map(|m| m.remote_id.clone()).collect();
    assert_eq!(ids.len(), 3, "x1, x2 once, y1: {ids:?}");
    assert!(ids.contains(&"y1".to_string()));
    assert_eq!(
        merged.last_message_at.unwrap().timestamp(),
        at(&f, 30).timestamp()
    );
    assert_eq!(
        merged.unread_count,
        4 - 1,
        "x1, x2, y1 unread (3), duplicate not double counted"
    );
    assert!(f
        .db
        .get_conversation_by_remote(&f.account, other_lid)
        .unwrap()
        .is_none());
    assert!(f.db.get_conversation(&other.id).is_err());
    let todos = f.db.list_todos(&merged.id).unwrap();
    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0].id, todo.id);
}

fn empty_patch() -> ConversationPatch {
    ConversationPatch {
        pinned: None,
        archived: None,
        muted: None,
        workspace_id: None,
        clear_workspace: None,
        priority_group: None,
        clear_priority: None,
        notes: None,
        notify_enabled: None,
        clear_notify: None,
        send_receipts: None,
        clear_receipts: None,
        auto_download: None,
    }
}

fn local_send(f: &Fx, conv_id: &str, body: &str, secs: i64) -> Message {
    let m = Message {
        id: Uuid::new_v4().to_string(),
        conversation_id: conv_id.into(),
        remote_id: None,
        sender_id: None,
        sender_name: Some("You".into()),
        direction: MessageDirection::Outbound,
        body: body.into(),
        timestamp: at(f, secs),
        status: MessageStatus::Pending,
        metadata: json!({}),
        starred: false,
        pinned: false,
        reactions: Vec::new(),
        reply: None,
    };
    assert!(f.db.insert_message(&m).unwrap());
    m
}

#[test]
fn send_ok_error_and_echo_adoption() {
    let f = fx();
    f.db.apply_conversation_event(&f.account, &conv_ev(WA_PHONE, Some(json!({}))))
        .unwrap();
    let cid = conv(&f, WA_PHONE).id;

    // ok with the remote id: row gets remote_id and status sent.
    let m = local_send(&f, &cid, "hi", 10);
    match f
        .db
        .reconcile_send_ok(Some(&f.account), &m.id, "WAID1")
        .unwrap()
    {
        SendReconcile::Updated { message, .. } => {
            assert_eq!(message.remote_id.as_deref(), Some("WAID1"));
            assert_eq!(message.status, MessageStatus::Sent);
        }
        other => panic!("{other:?}"),
    }
    // The echo of the same message is a duplicate, not a second row.
    let out = ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("WAID1", "hi", at(&f, 10), true),
    );
    assert_eq!(out.kind, IngestKind::Duplicate);
    assert_eq!(f.db.list_messages(&cid, 10).unwrap().len(), 1);

    // error with request id: failed (only while pending).
    let m2 = local_send(&f, &cid, "oops", 20);
    match f.db.reconcile_send_error(None, &m2.id).unwrap() {
        SendReconcile::Updated { message, .. } => assert_eq!(message.status, MessageStatus::Failed),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        f.db.reconcile_send_error(None, &m.id).unwrap(),
        SendReconcile::Nothing
    ));

    // Echo adoption: echo arrives before the ok reply.
    let m3 = local_send(&f, &cid, "later", 40);
    let out = ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("WAID3", "later", at(&f, 41), true),
    );
    assert_eq!(out.kind, IngestKind::Adopted);
    let adopted = out.message.unwrap();
    assert_eq!(adopted.id, m3.id);
    assert_eq!(adopted.remote_id.as_deref(), Some("WAID3"));
    assert_eq!(adopted.status, MessageStatus::Sent);
    assert_eq!(f.db.list_messages(&cid, 20).unwrap().len(), 3);
    // ok after adoption changes nothing.
    assert!(matches!(
        f.db.reconcile_send_ok(Some(&f.account), &m3.id, "WAID3")
            .unwrap(),
        SendReconcile::Nothing
    ));

    // Outside the 180 s window, or with another body, no adoption.
    let m4 = local_send(&f, &cid, "far", 100);
    let out = ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("WAID4", "far", at(&f, 100 + 400), true),
    );
    assert_eq!(out.kind, IngestKind::Inserted);
    let _ = m4;

    // Echo stored as its own row, then the ok reply arrives: the local row is removed.
    let m5 = local_send(&f, &cid, "five", 600);
    let out = ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("WAID5", "different body", at(&f, 601), true),
    );
    assert_eq!(out.kind, IngestKind::Inserted);
    match f
        .db
        .reconcile_send_ok(Some(&f.account), &m5.id, "WAID5")
        .unwrap()
    {
        SendReconcile::Removed { message_id, .. } => assert_eq!(message_id, m5.id),
        other => panic!("{other:?}"),
    }
    assert!(f.db.get_message(&cid, &m5.id).is_err());
}

#[test]
fn outbound_message_clears_unread() {
    let f = fx();
    ingest(&f, WA_PHONE, &[], false, msg("i1", "q", at(&f, 5), false));
    assert_eq!(conv(&f, WA_PHONE).unread_count, 1);
    let cid = conv(&f, WA_PHONE).id;
    local_send(&f, &cid, "answer", 10);
    assert_eq!(conv(&f, WA_PHONE).unread_count, 0);
}

#[test]
fn duplicate_events_perform_no_update() {
    let f = fx();
    let ev = ConversationEvent {
        last_message_at: Some(at(&f, 5)),
        preview: Some("p".into()),
        alt_remote_ids: vec![WA_LID.into()],
        ..conv_ev(WA_PHONE, Some(json!({ "push": "P" })))
    };
    f.db.apply_conversation_event(&f.account, &ev).unwrap();
    ingest(
        &f,
        WA_PHONE,
        &[WA_LID],
        false,
        msg("d1", "p", at(&f, 5), false),
    );
    let inbox = f.db.inbox(&f.account).unwrap();
    let before: i64 = inbox
        .conn
        .lock()
        .query_row("PRAGMA data_version", [], |r| r.get(0))
        .unwrap();
    let changes_before: i64 = inbox
        .conn
        .lock()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    let out = f.db.apply_conversation_event(&f.account, &ev).unwrap();
    assert!(!out.changed && out.conversation.is_none());
    let out = ingest(
        &f,
        WA_PHONE,
        &[WA_LID],
        false,
        msg("d1", "p", at(&f, 5), false),
    );
    assert_eq!(out.kind, IngestKind::Duplicate);
    let changes_after: i64 = inbox
        .conn
        .lock()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    assert_eq!(changes_before, changes_after, "no row was written");
    let _ = before;
}

fn plain_conv(f: &Fx, account: &str, remote: &str, title: &str, secs: Option<i64>, pinned: bool) {
    f.db.apply_conversation_event(
        account,
        &ConversationEvent {
            remote_id: remote.into(),
            legacy_title: Some(title.into()),
            last_message_at: secs.map(|s| at(f, s)),
            pinned: Some(pinned),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn sql_pagination_matches_the_rust_comparator() {
    let f = fx();
    let second = f.db.create_account("telegram", "TG").unwrap().id;
    for (i, (title, secs, pinned)) in [
        ("bravo", Some(10), false),
        ("Alpha", Some(10), false),
        ("alpha", Some(10), false),
        ("charlie", Some(50), false),
        ("pinned old", Some(1), true),
        ("no time b", None, false),
        ("No time a", None, false),
        ("delta", Some(30), false),
        ("echo", Some(30), false),
    ]
    .into_iter()
    .enumerate()
    {
        plain_conv(&f, &f.account, &format!("a{i}"), title, secs, pinned);
        plain_conv(
            &f,
            &second,
            &format!("b{i}"),
            &format!("{title} 2"),
            secs.map(|s| s + 3),
            pinned && i % 2 == 0,
        );
    }
    let all =
        f.db.list_conversations(None, None, None, false, 0, 1000)
            .unwrap();
    assert_eq!(all.len(), 18);
    let mut sorted = all.clone();
    sorted.sort_by(compare_conversations);
    assert_eq!(
        all.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
        sorted.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
    );
    for (off, lim) in [(0, 5), (3, 5), (5, 7), (16, 10), (17, 1)] {
        let page =
            f.db.list_conversations(None, None, None, false, off, lim)
                .unwrap();
        let want: Vec<_> = sorted
            .iter()
            .skip(off as usize)
            .take(lim as usize)
            .map(|c| c.id.clone())
            .collect();
        assert_eq!(
            page.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            want,
            "offset {off} limit {lim}"
        );
    }
    let one =
        f.db.list_conversations(Some(&f.account), None, None, false, 2, 4)
            .unwrap();
    let mut only: Vec<_> = sorted
        .iter()
        .filter(|c| c.account_id == f.account)
        .cloned()
        .collect();
    only.sort_by(compare_conversations);
    assert_eq!(
        one.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
        only.iter()
            .skip(2)
            .take(4)
            .map(|c| c.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        f.db.count_conversations(None, None, None, false).unwrap(),
        18
    );
    assert_eq!(
        f.db.count_conversations(Some(&second), None, None, false)
            .unwrap(),
        9
    );

    // Workspace filter: account override, conversation override, default.
    f.db.create_workspace("Work").unwrap();
    let ws =
        f.db.list_workspaces()
            .unwrap()
            .into_iter()
            .find(|w| w.name == "Work")
            .unwrap()
            .id;
    f.db.patch_account(
        &second,
        &AccountPatch {
            workspace_id: Some(ws.clone()),
            ..empty_account_patch()
        },
    )
    .unwrap();
    assert_eq!(
        f.db.count_conversations(None, Some(&ws), None, false)
            .unwrap(),
        9
    );
    assert_eq!(
        f.db.count_conversations(None, Some("default"), None, false)
            .unwrap(),
        9
    );
    let first_of_a =
        f.db.get_conversation_by_remote(&f.account, "a0")
            .unwrap()
            .unwrap();
    f.db.patch_conversation(
        &first_of_a.id,
        &ConversationPatch {
            workspace_id: Some(ws.clone()),
            ..empty_patch()
        },
    )
    .unwrap();
    assert_eq!(
        f.db.count_conversations(None, Some(&ws), None, false)
            .unwrap(),
        10
    );
    let listed =
        f.db.list_conversations(None, Some(&ws), None, false, 0, 100)
            .unwrap();
    assert_eq!(listed.len(), 10);
}

fn empty_account_patch() -> AccountPatch {
    serde_json::from_value(json!({})).unwrap()
}

#[test]
fn upgrade_migration_preserves_an_existing_badge() {
    let dir = tempfile::tempdir().unwrap();
    let (account, inbox_path) = {
        let db = Database::open(dir.path()).unwrap();
        let id = db.create_account("whatsapp", "WA").unwrap().id;
        let path = db.inbox_path(&id);
        (id, path)
    };
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", inbox_path.display()));
    }
    let base = Utc::now() - Duration::days(2);
    {
        // The pre-upgrade schema: no read marker, no inbox_meta.
        let old = Connection::open(&inbox_path).unwrap();
        old.execute_batch(
            "CREATE TABLE contacts (id TEXT PRIMARY KEY, account_id TEXT NOT NULL, remote_id TEXT NOT NULL,
                display_name TEXT NOT NULL, avatar_url TEXT, metadata TEXT NOT NULL DEFAULT '{}',
                UNIQUE(account_id, remote_id));
             CREATE TABLE conversations (id TEXT PRIMARY KEY, account_id TEXT NOT NULL, remote_id TEXT NOT NULL,
                contact_id TEXT, title TEXT NOT NULL, conversation_type TEXT NOT NULL DEFAULT 'direct',
                unread_count INTEGER NOT NULL DEFAULT 0, last_message_at TEXT, last_message_preview TEXT,
                pinned INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0,
                muted INTEGER NOT NULL DEFAULT 0, metadata TEXT NOT NULL DEFAULT '{}', updated_at TEXT,
                UNIQUE(account_id, remote_id));
             CREATE TABLE messages (id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                remote_id TEXT, sender_id TEXT, sender_name TEXT, direction TEXT NOT NULL, body TEXT NOT NULL,
                timestamp TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', metadata TEXT NOT NULL DEFAULT '{}');",
        )
        .unwrap();
        for (cid, remote, unread) in [
            ("c1", "1@s.whatsapp.net", 2_i64),
            ("c2", "2@s.whatsapp.net", 0),
        ] {
            old.execute(
                "INSERT INTO conversations (id, account_id, remote_id, title, unread_count) VALUES (?1, ?2, ?3, 't', ?4)",
                params![cid, account, remote, unread],
            )
            .unwrap();
            for i in 0..4 {
                old.execute(
                    "INSERT INTO messages (id, conversation_id, remote_id, direction, body, timestamp)
                     VALUES (?1, ?2, ?3, 'inbound', 'b', ?4)",
                    params![
                        format!("{cid}-{i}"),
                        cid,
                        format!("r{cid}{i}"),
                        (base + Duration::hours(i)).to_rfc3339()
                    ],
                )
                .unwrap();
            }
        }
    }
    let db = Database::open(dir.path()).unwrap();
    let c1 = db.get_conversation("c1").unwrap();
    assert_eq!(c1.unread_count, 2, "the badge survives the upgrade");
    // A recompute (new inbound message) keeps the old unread messages and adds the new one.
    let m = msg("fresh", "new", Utc::now() + Duration::seconds(5), false);
    db.ingest_message(&account, "1@s.whatsapp.net", &[], false, m, "new", "x")
        .unwrap();
    assert_eq!(db.get_conversation("c1").unwrap().unread_count, 3);
    // A chat with no unread stays at zero and old history stays read.
    assert_eq!(db.get_conversation("c2").unwrap().unread_count, 0);
    let m = msg("old", "old", base + Duration::minutes(10), false);
    db.ingest_message(&account, "2@s.whatsapp.net", &[], true, m, "old", "x")
        .unwrap();
    assert_eq!(db.get_conversation("c2").unwrap().unread_count, 0);
    // Reading the chat clears the badge.
    db.mark_conversation_read("c1").unwrap();
    assert_eq!(db.get_conversation("c1").unwrap().unread_count, 0);
    // Migration is idempotent.
    drop(db);
    let db = Database::open(dir.path()).unwrap();
    assert_eq!(db.get_conversation("c1").unwrap().unread_count, 0);
}

#[test]
fn status_chats_are_not_listed() {
    let f = fx();
    plain_conv(&f, &f.account, "status@broadcast", "Status", Some(5), false);
    assert_eq!(
        f.db.count_conversations(None, None, None, false).unwrap(),
        0
    );
}

#[test]
fn send_accepted_without_a_remote_id_marks_the_row_sent_once() {
    let f = fx();
    f.db.apply_conversation_event(&f.account, &conv_ev(WA_PHONE, Some(json!({}))))
        .unwrap();
    let cid = conv(&f, WA_PHONE).id;
    let m = local_send(&f, &cid, "hello", 10);
    match f.db.mark_send_accepted(Some(&f.account), &m.id).unwrap() {
        SendReconcile::Updated { message, .. } => {
            assert_eq!(message.status, MessageStatus::Sent);
            assert!(message.remote_id.is_none());
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        f.db.mark_send_accepted(Some(&f.account), &m.id).unwrap(),
        SendReconcile::Nothing
    ));
}

#[test]
fn account_colours_and_unread_totals_are_independent() {
    let f = fx();
    let other = f.db.create_account("whatsapp", "Other").unwrap();
    let patch = AccountPatch {
        accent_color: Some("#12ab34".into()),
        ..Default::default()
    };
    let updated = f.db.patch_account(&f.account, &patch).unwrap();
    assert_eq!(updated.metadata["accent_color"], "#12ab34");
    let bad = AccountPatch {
        accent_color: Some("red".into()),
        ..Default::default()
    };
    assert!(f.db.patch_account(&f.account, &bad).is_err());
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        msg("unread-a", "Hi", at(&f, 10), false),
    );
    let totals = f.db.unread_by_account().unwrap();
    assert_eq!(totals[&f.account], 1);
    assert_eq!(totals[&other.id], 0);
}

#[test]
fn channel_messages_do_not_count_toward_sidebar_unread() {
    let f = fx();
    ingest(&f, "120363@newsletter", &[], false, msg("chan-1", "News", at(&f, 10), false));
    assert_eq!(f.db.unread_by_account().unwrap()[&f.account], 0);
    assert_eq!(f.db.total_unread().unwrap(), 0);
}

#[test]
fn newest_message_lookups_use_the_epoch_index() {
    let f = fx();
    ingest(&f, "1@s.whatsapp.net", &[], false, msg("m1", "hi", at(&f, 10), false));
    let inbox = f.db.inbox(&f.account).unwrap();
    let conn = inbox.conn.lock();
    for sql in [
        "SELECT * FROM messages WHERE conversation_id = 'c' ORDER BY unixepoch(timestamp, 'subsec') DESC, id LIMIT 1",
        "SELECT MAX(unixepoch(timestamp, 'subsec')) FROM messages WHERE conversation_id = 'c'",
        "SELECT COUNT(*) FROM messages WHERE conversation_id = 'c' AND direction = 'inbound' AND unixepoch(timestamp, 'subsec') >= 1.0",
    ] {
        let plan: String = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(plan.contains("idx_messages_conv_epoch"), "{sql} -> {plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{sql} -> {plan}");
    }
}

#[test]
fn data_profile_counts_rows_without_exposing_content() {
    let f = fx();
    ingest(&f, "1@s.whatsapp.net", &[], false, msg("m1", "hi", at(&f, 10), false));
    ingest(&f, "2@s.whatsapp.net", &[], false, msg("m2", "yo", at(&f, 11), false));
    let p = f.db.data_profile().unwrap();
    assert_eq!(p.accounts, 1);
    assert_eq!(p.conversations, 2);
    assert_eq!(p.messages, 2);
    assert_eq!(p.largest_account_messages, 2);
}

// ---------------------------------------------------------------------------------------------
// Media cache: auto-download preferences and retention bookkeeping
// ---------------------------------------------------------------------------------------------

#[test]
fn auto_download_resolution_is_chat_then_account_then_builtin() {
    let none = AutoDownloadPrefs::default();
    // Built-in: images (and stickers) on; videos, files, voice and audio off.
    assert!(AutoDownloadPrefs::allows_auto("image", &none, &none));
    assert!(AutoDownloadPrefs::allows_auto("sticker", &none, &none));
    assert!(!AutoDownloadPrefs::allows_auto("video", &none, &none));
    assert!(!AutoDownloadPrefs::allows_auto("document", &none, &none));
    assert!(!AutoDownloadPrefs::allows_auto("audio", &none, &none));
    assert!(!AutoDownloadPrefs::allows_auto("ptt", &none, &none));
    // Contacts, polls and the like are never downloadable.
    assert!(!AutoDownloadPrefs::allows_auto("location", &none, &none));
    // Account default overrides the built-in one.
    let account = AutoDownloadPrefs {
        images: Some(false),
        videos: Some(true),
        files: Some(true),
    };
    assert!(!AutoDownloadPrefs::allows_auto("image", &none, &account));
    assert!(!AutoDownloadPrefs::allows_auto("sticker", &none, &account));
    assert!(AutoDownloadPrefs::allows_auto("video", &none, &account));
    // Voice notes and audio follow Files.
    assert!(AutoDownloadPrefs::allows_auto("ptt", &none, &account));
    assert!(AutoDownloadPrefs::allows_auto("document", &none, &account));
    // A chat override beats the account, per group; unset groups inherit.
    let chat = AutoDownloadPrefs {
        images: Some(true),
        videos: Some(false),
        files: None,
    };
    assert!(AutoDownloadPrefs::allows_auto("image", &chat, &account));
    assert!(!AutoDownloadPrefs::allows_auto("video", &chat, &account));
    assert!(AutoDownloadPrefs::allows_auto("document", &chat, &account));
}

#[test]
fn auto_download_choices_persist_on_account_and_chat_and_can_be_cleared() {
    let f = fx();
    ingest(&f, WA_PHONE, &[], false, msg("m1", "hi", at(&f, 5), false));
    let c = conv(&f, WA_PHONE);
    assert!(c.auto_download.is_empty());
    assert!(f.db.get_account(&f.account).unwrap().auto_download.is_empty());

    let prefs = AutoDownloadPrefs {
        images: None,
        videos: Some(true),
        files: Some(false),
    };
    let updated = f
        .db
        .patch_conversation(
            &c.id,
            &ConversationPatch {
                auto_download: Some(prefs),
                ..empty_patch()
            },
        )
        .unwrap();
    assert_eq!(updated.auto_download, prefs);
    assert_eq!(conv(&f, WA_PHONE).auto_download, prefs);
    let acct = f
        .db
        .patch_account(
            &f.account,
            &AccountPatch {
                auto_download: Some(AutoDownloadPrefs {
                    images: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(acct.auto_download.images, Some(false));
    assert_eq!(acct.auto_download.videos, None);
    // An unrelated patch keeps the choice; an all-unset patch resets to inherit.
    f.db.patch_conversation(
        &c.id,
        &ConversationPatch {
            pinned: Some(true),
            ..empty_patch()
        },
    )
    .unwrap();
    assert_eq!(conv(&f, WA_PHONE).auto_download, prefs);
    f.db.patch_conversation(
        &c.id,
        &ConversationPatch {
            auto_download: Some(AutoDownloadPrefs::default()),
            ..empty_patch()
        },
    )
    .unwrap();
    assert!(conv(&f, WA_PHONE).auto_download.is_empty());
}

#[test]
fn purged_files_lose_their_media_path_and_nothing_else() {
    let f = fx();
    let mut gone = msg("gone", "[video]", at(&f, 5), false);
    gone.metadata = json!({
        "media_type": "video", "media_path": "/cache/a/media/gone.mp4",
        "thumbnail_path": "/cache/a/thumbs/gone.jpg", "file_length": 1234
    });
    let mut kept = msg("kept", "[image]", at(&f, 6), false);
    kept.metadata = json!({ "media_type": "image", "media_path": "/cache/a/media/kept.jpg" });
    let mut legacy = msg("legacy", "[image]", at(&f, 7), false);
    legacy.metadata = json!({ "media_type": "image", "media_path": "/Docs/shuttle/old.jpg" });
    for m in [gone, kept, legacy] {
        ingest(&f, WA_PHONE, &[], false, m);
    }
    let cid = conv(&f, WA_PHONE).id;
    let purged: std::collections::HashSet<String> =
        ["/cache/a/media/gone.mp4".to_string()].into_iter().collect();
    let affected = f.db.clear_media_paths(&purged).unwrap();
    assert_eq!(affected, vec![(f.account.clone(), 1)]);
    let gone = f.db.list_messages(&cid, 50).unwrap();
    let gone = gone.iter().find(|m| m.remote_id.as_deref() == Some("gone")).unwrap();
    assert!(gone.metadata.get("media_path").is_none());
    // The thumbnail, size and type stay so the UI can draw the placeholder + download button.
    assert_eq!(gone.metadata["thumbnail_path"], "/cache/a/thumbs/gone.jpg");
    assert_eq!(gone.metadata["file_length"], 1234);
    assert_eq!(gone.metadata["media_type"], "video");
    let all = f.db.list_messages(&cid, 50).unwrap();
    let path_of = |remote: &str| {
        all.iter()
            .find(|m| m.remote_id.as_deref() == Some(remote))
            .and_then(|m| m.metadata.get("media_path").cloned())
    };
    assert_eq!(path_of("kept"), Some(json!("/cache/a/media/kept.jpg")));
    assert_eq!(path_of("legacy"), Some(json!("/Docs/shuttle/old.jpg")));
    assert!(f.db.clear_media_paths(&Default::default()).unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Reactions
// ---------------------------------------------------------------------------------------------

fn react(
    target: &str,
    reactor: &str,
    name: &str,
    emoji: &str,
    at: DateTime<Utc>,
    from_me: bool,
) -> super::ReactionFact {
    super::ReactionFact {
        target_remote_id: target.into(),
        reactor_id: reactor.into(),
        reactor_name: Some(name.into()),
        emoji: emoji.into(),
        at,
        from_me,
    }
}

fn listed(fx: &Fx, remote: &str) -> Vec<Message> {
    let c = conv(fx, remote);
    fx.db.list_messages(&c.id, 100).unwrap()
}

#[test]
fn reactions_replace_remove_and_aggregate() {
    let f = fx();
    ingest(&f, WA_GROUP, &[], false, msg("M1", "photo", at(&f, 1), false));
    let apply = |facts: Vec<super::ReactionFact>| {
        f.db.apply_reactions(&f.account, WA_GROUP, &[], &facts).unwrap()
    };

    let out = apply(vec![
        react("M1", "111@s.whatsapp.net", "Dad", "❤️", at(&f, 10), false),
        react("M1", "222@s.whatsapp.net", "Mum", "❤️", at(&f, 11), false),
        react("M1", "me", "You", "👍", at(&f, 12), true),
    ]);
    assert_eq!(out.changed_targets, vec!["M1".to_string()]);
    let m = &listed(&f, WA_GROUP)[0];
    assert_eq!(m.reactions.len(), 2);
    assert_eq!(m.reactions[0].emoji, "❤️");
    assert_eq!(m.reactions[0].count, 2);
    assert_eq!(m.reactions[0].names, vec!["Dad", "Mum"]);
    assert!(!m.reactions[0].mine);
    assert_eq!(m.reactions[1].emoji, "👍");
    assert!(m.reactions[1].mine);
    assert_eq!(m.reactions[1].names, vec!["You"]);

    // A new reaction from the same person replaces the old one.
    apply(vec![react("M1", "111@s.whatsapp.net", "Dad", "😂", at(&f, 20), false)]);
    let m = &listed(&f, WA_GROUP)[0];
    let by: Vec<(&str, usize)> = m.reactions.iter().map(|g| (g.emoji.as_str(), g.count)).collect();
    assert!(by.contains(&("❤️", 1)) && by.contains(&("😂", 1)) && by.contains(&("👍", 1)));

    // An older fact (history replayed after a live one) never undoes the newer reaction.
    let out = apply(vec![react("M1", "111@s.whatsapp.net", "Dad", "❤️", at(&f, 5), false)]);
    assert!(out.changed_targets.is_empty());
    assert!(listed(&f, WA_GROUP)[0].reactions.iter().any(|g| g.emoji == "😂"));

    // Same emoji again is not a change.
    let out = apply(vec![react("M1", "111@s.whatsapp.net", "Dad", "😂", at(&f, 30), false)]);
    assert!(out.changed_targets.is_empty());

    // An empty emoji removes the reaction; removing again is a no-op.
    let out = apply(vec![react("M1", "111@s.whatsapp.net", "Dad", "", at(&f, 40), false)]);
    assert_eq!(out.changed_targets, vec!["M1".to_string()]);
    assert!(!listed(&f, WA_GROUP)[0].reactions.iter().any(|g| g.emoji == "😂"));
    let out = apply(vec![react("M1", "111@s.whatsapp.net", "Dad", "", at(&f, 41), false)]);
    assert!(out.changed_targets.is_empty());

    // Removing the last reaction leaves the message with none.
    apply(vec![
        react("M1", "222@s.whatsapp.net", "Mum", "", at(&f, 50), false),
        react("M1", "me", "You", "", at(&f, 51), true),
    ]);
    assert!(listed(&f, WA_GROUP)[0].reactions.is_empty());
}

#[test]
fn reaction_waits_for_its_message_and_never_moves_the_chat() {
    let f = fx();
    // The chat exists (one older message); the target message is not stored yet.
    ingest(&f, WA_PHONE, &[], false, msg("OLD", "hello", at(&f, 1), false));
    let before = summary(&conv(&f, WA_PHONE));
    assert_eq!(before.3, 1);

    let out = f
        .db
        .apply_reactions(
            &f.account,
            WA_PHONE,
            &[],
            &[react("LATE", WA_PHONE, "Bob", "❤️", at(&f, 100), false)],
        )
        .unwrap();
    assert_eq!(out.changed_targets, vec!["LATE".to_string()]);
    // A reaction is not a message: time, preview and unread are untouched.
    assert_eq!(summary(&conv(&f, WA_PHONE)), before);
    assert_eq!(listed(&f, WA_PHONE).len(), 1);

    // The target arrives later and the stored reaction attaches to it.
    ingest(&f, WA_PHONE, &[], true, msg("LATE", "the message", at(&f, 50), false));
    let msgs = listed(&f, WA_PHONE);
    let late = msgs.iter().find(|m| m.remote_id.as_deref() == Some("LATE")).unwrap();
    assert_eq!(late.reactions.len(), 1);
    assert_eq!(late.reactions[0].emoji, "❤️");
    assert_eq!(late.reactions[0].names, vec!["Bob"]);
    let old = msgs.iter().find(|m| m.remote_id.as_deref() == Some("OLD")).unwrap();
    assert!(old.reactions.is_empty());
}

#[test]
fn reaction_for_an_unknown_chat_creates_nothing() {
    let f = fx();
    let out = f
        .db
        .apply_reactions(
            &f.account,
            WA_GROUP,
            &[],
            &[react("M1", "111@s.whatsapp.net", "Dad", "❤️", at(&f, 10), false)],
        )
        .unwrap();
    assert!(out.conversation_id.is_none());
    assert!(f
        .db
        .get_conversation_by_remote(&f.account, WA_GROUP)
        .unwrap()
        .is_none());
}

#[test]
fn reactions_follow_a_chat_merge_and_resolve_through_aliases() {
    let f = fx();
    // Same person known under a LID chat and a phone chat; the LID chat has a reacted message.
    ingest(&f, WA_LID, &[], false, msg("L1", "from lid", at(&f, 1), false));
    f.db.apply_reactions(
        &f.account,
        WA_LID,
        &[],
        &[react("L1", WA_LID, "Bob", "🙏", at(&f, 5), false)],
    )
    .unwrap();
    ingest(&f, WA_PHONE, &[], false, msg("P1", "from phone", at(&f, 2), false));
    // A later event names both JIDs: the chats merge into the phone chat.
    f.db.apply_conversation_event(
        &f.account,
        &ConversationEvent {
            alt_remote_ids: vec![WA_LID.into()],
            ..conv_ev(WA_PHONE, None)
        },
    )
    .unwrap();
    let msgs = listed(&f, WA_PHONE);
    let l1 = msgs.iter().find(|m| m.remote_id.as_deref() == Some("L1")).unwrap();
    assert_eq!(l1.reactions.len(), 1, "reactions must survive the merge");

    // A reaction addressed to the retired LID jid still finds the merged chat.
    let out = f
        .db
        .apply_reactions(
            &f.account,
            WA_LID,
            &[WA_PHONE.to_string()],
            &[react("P1", "me", "You", "👍", at(&f, 9), true)],
        )
        .unwrap();
    assert_eq!(out.changed_targets, vec!["P1".to_string()]);
    assert!(listed(&f, WA_PHONE)
        .iter()
        .find(|m| m.remote_id.as_deref() == Some("P1"))
        .unwrap()
        .reactions[0]
        .mine);
}

#[test]
fn a_known_group_message_gets_its_real_author_but_no_new_row() {
    let f = fx();
    // An older build stored the message under the group's own JID.
    let mut bad = msg("G1", "hello", at(&f, 1), false);
    bad.sender_id = Some(WA_GROUP.into());
    bad.sender_name = Some("120363000000".into());
    ingest(&f, WA_GROUP, &[], true, bad);
    let before = summary(&conv(&f, WA_GROUP));

    let mut fixed = msg("G1", "hello", at(&f, 1), false);
    fixed.sender_id = Some("919742604484@s.whatsapp.net".into());
    fixed.sender_name = Some("Samah".into());
    let out = ingest(&f, WA_GROUP, &[], true, fixed);
    assert_eq!(out.kind, IngestKind::Duplicate);
    let msgs = listed(&f, WA_GROUP);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].sender_id.as_deref(), Some("919742604484@s.whatsapp.net"));
    assert_eq!(msgs[0].sender_name.as_deref(), Some("Samah"));
    assert_eq!(summary(&conv(&f, WA_GROUP)), before);

    // A real author is never overwritten by a later report.
    let mut other = msg("G1", "hello", at(&f, 1), false);
    other.sender_id = Some("919000000000@s.whatsapp.net".into());
    other.sender_name = Some("Someone else".into());
    ingest(&f, WA_GROUP, &[], true, other);
    assert_eq!(listed(&f, WA_GROUP)[0].sender_name.as_deref(), Some("Samah"));
}

// ---------------------------------------------------------------------------------------------
// Preview direction and author (chat-list arrow and group "Name:" prefix)
// ---------------------------------------------------------------------------------------------

fn group_msg(id: &str, body: &str, ts: DateTime<Utc>, from_me: bool, sender: Option<&str>) -> Message {
    let mut m = msg(id, body, ts, from_me);
    m.sender_id = sender.map(|s| s.to_string());
    m.sender_name = sender.map(|_| "Samah".to_string());
    m
}

#[test]
fn preview_direction_and_sender_follow_the_latest_message() {
    let fx = fx();
    ingest(&fx, WA_GROUP, &[], false, group_msg("g1", "Hey test", at(&fx, 1), false, Some(WA_PHONE)));
    let c = conv(&fx, WA_GROUP);
    assert_eq!(c.last_message_from_me, Some(false));
    assert_eq!(c.last_message_sender_id.as_deref(), Some(WA_PHONE));
    assert_eq!(c.last_message_sender_name.as_deref(), Some("Samah"));

    // An outbound reply replaces the preview, and the author is cleared.
    ingest(&fx, WA_GROUP, &[], false, group_msg("g2", "On my way", at(&fx, 2), true, None));
    let c = conv(&fx, WA_GROUP);
    assert_eq!(c.last_message_preview.as_deref(), Some("On my way"));
    assert_eq!(c.last_message_from_me, Some(true));
    assert_eq!(c.last_message_sender_id, None);
    assert_eq!(c.last_message_sender_name, None);

    // Same text from someone else, later: only the author changes.
    ingest(&fx, WA_GROUP, &[], false, group_msg("g3", "On my way", at(&fx, 3), false, Some(WA_PHONE)));
    let c = conv(&fx, WA_GROUP);
    assert_eq!(c.last_message_from_me, Some(false));
    assert_eq!(c.last_message_sender_id.as_deref(), Some(WA_PHONE));

    // An older history message never touches the preview or its source.
    ingest(&fx, WA_GROUP, &[], true, group_msg("g0", "ancient", at(&fx, -50), true, None));
    let c = conv(&fx, WA_GROUP);
    assert_eq!(c.last_message_preview.as_deref(), Some("On my way"));
    assert_eq!(c.last_message_from_me, Some(false));
}

#[test]
fn preview_source_is_filled_when_the_message_follows_a_conversation_event() {
    let fx = fx();
    let mut ev = conv_ev(WA_PHONE, Some(json!({ "contact": "Ann" })));
    ev.last_message_at = Some(at(&fx, 5));
    ev.preview = Some("hello".into());
    fx.db.apply_conversation_event(&fx.account, &ev).unwrap();
    // A conversation event alone does not say who wrote it.
    assert_eq!(conv(&fx, WA_PHONE).last_message_from_me, None);
    ingest(&fx, WA_PHONE, &[], false, msg("d1", "hello", at(&fx, 5), false));
    let c = conv(&fx, WA_PHONE);
    assert_eq!(c.last_message_preview.as_deref(), Some("hello"));
    assert_eq!(c.last_message_from_me, Some(false));
}

#[test]
fn preview_source_survives_migration_for_existing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let account;
    {
        let db = Database::open(dir.path()).unwrap();
        account = db.create_account("whatsapp", "WA").unwrap().id;
        let t = Utc::now();
        db.ingest_message(&account, WA_PHONE, &[], false, msg("a1", "mine", t, true), "mine", "S")
            .unwrap();
        db.ingest_message(
            &account,
            WA_GROUP,
            &[],
            false,
            group_msg("a2", "theirs", t, false, Some(WA_PHONE)),
            "theirs",
            "S",
        )
        .unwrap();
    }
    // Simulate a database from before the columns: unknown source, migration not yet run.
    let path = dir.path().join("accounts").join(&account).join("inbox.sqlite");
    {
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute(
            "UPDATE conversations SET last_message_from_me = NULL,
                last_message_sender_id = NULL, last_message_sender_name = NULL",
            [],
        )
        .unwrap();
        raw.execute("DELETE FROM inbox_meta WHERE key = 'preview_source_v1'", [])
            .unwrap();
    }
    let db = Database::open(dir.path()).unwrap();
    let direct = db.get_conversation_by_remote(&account, WA_PHONE).unwrap().unwrap();
    assert_eq!(direct.last_message_from_me, Some(true));
    assert_eq!(direct.last_message_sender_id, None);
    let group = db.get_conversation_by_remote(&account, WA_GROUP).unwrap().unwrap();
    assert_eq!(group.last_message_from_me, Some(false));
    assert_eq!(group.last_message_sender_id.as_deref(), Some(WA_PHONE));
    assert_eq!(group.last_message_sender_name.as_deref(), Some("Samah"));
}

// ---- replies ---------------------------------------------------------------------------

fn reply_msg(id: &str, body: &str, secs: i64, f: &Fx, reply_to: Value) -> Message {
    let mut m = msg(id, body, at(f, secs), false);
    m.metadata = json!({ "id": id, "text": body, "reply_to": reply_to });
    m
}

fn seed_chat(f: &Fx) -> String {
    f.db.apply_conversation_event(&f.account, &conv_ev(WA_PHONE, Some(json!({}))))
        .unwrap();
    conv(f, WA_PHONE).id
}

#[test]
fn reply_quote_is_built_from_the_stored_original() {
    let f = fx();
    let cid = seed_chat(&f);
    let mut orig = msg("ORIG", "are you coming tonight?", at(&f, 1), false);
    orig.sender_name = Some("Bob".into());
    ingest(&f, WA_PHONE, &[], false, orig);
    // The connector's snippet is stale on purpose: the stored text wins.
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        reply_msg("R1", "yes!", 2, &f, json!({ "remote_id": "ORIG", "snippet": "old text" })),
    );
    let list = f.db.list_messages(&cid, 10).unwrap();
    let reply = list.iter().find(|m| m.remote_id.as_deref() == Some("R1")).unwrap();
    let p = reply.reply.as_ref().expect("resolved reply");
    assert!(p.available);
    assert_eq!(p.snippet, "are you coming tonight?");
    assert_eq!(p.sender_name.as_deref(), Some("Bob"));
    assert!(!p.from_me);
    let orig_row = list.iter().find(|m| m.remote_id.as_deref() == Some("ORIG")).unwrap();
    assert_eq!(p.message_id.as_deref(), Some(orig_row.id.as_str()));
    assert!(orig_row.reply.is_none());
}

#[test]
fn reply_to_own_message_is_you_and_media_gets_a_label() {
    let f = fx();
    let cid = seed_chat(&f);
    let mut photo = msg("PHOTO", "[image]", at(&f, 1), true);
    photo.metadata = json!({ "id": "PHOTO", "media_type": "image", "thumbnail_path": "/cache/t.jpg" });
    ingest(&f, WA_PHONE, &[], false, photo);
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        reply_msg("R1", "nice", 2, &f, json!({ "remote_id": "PHOTO" })),
    );
    let list = f.db.list_messages(&cid, 10).unwrap();
    let p = list
        .iter()
        .find(|m| m.remote_id.as_deref() == Some("R1"))
        .and_then(|m| m.reply.clone())
        .unwrap();
    assert!(p.from_me);
    assert_eq!(p.sender_name.as_deref(), Some("You"));
    assert_eq!(p.snippet, ""); // captionless: the UI words it "📷 Photo"
    assert_eq!(p.media_type.as_deref(), Some("image"));
    assert_eq!(p.thumbnail_path.as_deref(), Some("/cache/t.jpg"));
}

#[test]
fn reply_outside_the_loaded_window_still_resolves() {
    let f = fx();
    let cid = seed_chat(&f);
    ingest(&f, WA_PHONE, &[], true, msg("OLD", "from long ago", at(&f, 1), false));
    for i in 0..5 {
        ingest(&f, WA_PHONE, &[], true, msg(&format!("F{i}"), "filler", at(&f, 10 + i), false));
    }
    ingest(
        &f,
        WA_PHONE,
        &[],
        true,
        reply_msg("R1", "late answer", 100, &f, json!({ "remote_id": "OLD" })),
    );
    let list = f.db.list_messages(&cid, 3).unwrap();
    assert_eq!(list.len(), 3);
    assert!(!list.iter().any(|m| m.remote_id.as_deref() == Some("OLD")));
    let p = list.last().unwrap().reply.clone().unwrap();
    assert!(p.available);
    assert_eq!(p.snippet, "from long ago");
}

#[test]
fn missing_original_falls_back_to_the_connector_snippet() {
    let f = fx();
    let cid = seed_chat(&f);
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        reply_msg(
            "R1",
            "ok",
            2,
            &f,
            json!({ "remote_id": "GONE", "snippet": "what time?", "sender": WA_PHONE }),
        ),
    );
    ingest(
        &f,
        WA_PHONE,
        &[],
        false,
        reply_msg(
            "R2",
            "ok",
            3,
            &f,
            json!({ "remote_id": "GONE2", "media_type": "video", "from_me": true }),
        ),
    );
    let list = f.db.list_messages(&cid, 10).unwrap();
    let a = list[0].reply.clone().unwrap();
    assert!(!a.available);
    assert_eq!(a.message_id, None);
    assert_eq!(a.snippet, "what time?");
    let b = list[1].reply.clone().unwrap();
    assert_eq!(b.snippet, "");
    assert_eq!(b.media_type.as_deref(), Some("video"));
    assert_eq!(b.sender_name.as_deref(), Some("You"));
}

#[test]
fn later_report_adds_the_reply_link_to_a_stored_message() {
    let f = fx();
    let cid = seed_chat(&f);
    ingest(&f, WA_PHONE, &[], true, msg("ORIG", "q", at(&f, 1), false));
    ingest(&f, WA_PHONE, &[], true, msg("R1", "a", at(&f, 2), false));
    assert!(f.db.list_messages(&cid, 10).unwrap()[1].reply.is_none());
    // History repair pass: same message again, now with the fact.
    let out = ingest(
        &f,
        WA_PHONE,
        &[],
        true,
        reply_msg("R1", "a", 2, &f, json!({ "remote_id": "ORIG" })),
    );
    assert_eq!(out.kind, IngestKind::Duplicate);
    let list = f.db.list_messages(&cid, 10).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].reply.clone().unwrap().snippet, "q");
    // The chat row is untouched by a metadata-only report.
    assert_eq!(conv(&f, WA_PHONE).last_message_preview.as_deref(), Some("a"));
}

#[test]
fn reply_target_gives_the_connector_remote_id_and_group_author() {
    let f = fx();
    f.db.apply_conversation_event(&f.account, &conv_ev(WA_GROUP, Some(json!({}))))
        .unwrap();
    let cid = conv(&f, WA_GROUP).id;
    let mut orig = msg("GM1", "hello all", at(&f, 1), false);
    orig.sender_id = Some(WA_PHONE.into());
    orig.sender_name = Some("Samah".into());
    ingest(&f, WA_GROUP, &[], false, orig);
    let local = f.db.list_messages(&cid, 10).unwrap()[0].id.clone();
    let t = f.db.reply_target(&local).unwrap();
    assert_eq!(t.remote_id, "GM1");
    assert_eq!(t.sender.as_deref(), Some(WA_PHONE));
    assert_eq!(t.conversation_id, cid);
    assert_eq!(t.meta["snippet"], "hello all");
    assert_eq!(t.meta["sender_name"], "Samah");
    assert_eq!(t.meta["from_me"], false);

    // A message that has no remote id yet cannot be replied to.
    let pending = local_send(&f, &cid, "still sending", 5);
    assert!(f.db.reply_target(&pending.id).is_err());
}

#[test]
fn sent_reply_keeps_its_link_when_the_echo_arrives_first_or_thinner() {
    let f = fx();
    let cid = seed_chat(&f);
    ingest(&f, WA_PHONE, &[], false, msg("ORIG", "question", at(&f, 1), false));
    let orig_local = f.db.list_messages(&cid, 10).unwrap()[0].id.clone();
    let target = f.db.reply_target(&orig_local).unwrap();

    // The reply is stored locally with the link and shows it right away.
    let mut sent = Message {
        id: Uuid::new_v4().to_string(),
        conversation_id: cid.clone(),
        remote_id: None,
        sender_id: None,
        sender_name: Some("You".into()),
        direction: MessageDirection::Outbound,
        body: "answer".into(),
        timestamp: at(&f, 2),
        status: MessageStatus::Pending,
        metadata: json!({ "reply_to": target.meta }),
        starred: false,
        pinned: false,
        reactions: Vec::new(),
        reply: None,
    };
    f.db.insert_message(&sent).unwrap();
    f.db.resolve_reply(&mut sent);
    assert_eq!(sent.reply.as_ref().unwrap().snippet, "question");

    // The echo wins the race, with text WhatsApp normalised (so it is not adopted into the
    // pending row) and a bare `remote_id` link.
    let echo = {
        let mut m = msg("WA2", "answer (edited by server)", at(&f, 2), true);
        m.metadata = json!({ "id": "WA2", "text": "answer", "reply_to": { "remote_id": "ORIG" } });
        m
    };
    ingest(&f, WA_PHONE, &[], false, echo);
    // ... but the later send ack finds it and drops the local duplicate.
    match f.db.reconcile_send_ok(Some(&f.account), &sent.id, "WA2").unwrap() {
        SendReconcile::Removed { .. } => {}
        other => panic!("{other:?}"),
    }
    let list = f.db.list_messages(&cid, 10).unwrap();
    assert_eq!(list.len(), 2);
    let kept = list.iter().find(|m| m.remote_id.as_deref() == Some("WA2")).unwrap();
    let p = kept.reply.clone().unwrap();
    assert_eq!(p.snippet, "question");
    assert_eq!(kept.metadata["reply_to"]["from_me"], false);

    // Adoption path: the local row survives with its own (fuller) link.
    let mut second = Message { id: Uuid::new_v4().to_string(), body: "again".into(), timestamp: at(&f, 3), ..sent.clone() };
    second.remote_id = None;
    second.reply = None;
    f.db.insert_message(&second).unwrap();
    let echo2 = {
        let mut m = msg("WA3", "again", at(&f, 3), true);
        m.metadata = json!({ "id": "WA3", "reply_to": { "remote_id": "ORIG" } });
        m
    };
    let out = ingest(&f, WA_PHONE, &[], false, echo2);
    assert_eq!(out.kind, IngestKind::Adopted);
    let adopted = out.message.unwrap();
    assert_eq!(adopted.reply.unwrap().snippet, "question");
}

#[test]
fn merging_reply_facts_keeps_known_fields() {
    let merged = merge_json_objects(
        json!({ "reply_to": { "remote_id": "A", "snippet": "kept", "from_me": true } }),
        json!({ "reply_to": { "remote_id": "A", "sender": "x@s.whatsapp.net" } }),
    );
    assert_eq!(merged["reply_to"]["snippet"], "kept");
    assert_eq!(merged["reply_to"]["sender"], "x@s.whatsapp.net");
    assert_eq!(merged["reply_to"]["from_me"], true);
}

#[test]
fn contact_picker_rows_carry_saved_flag_and_chat_facts() {
    let f = fx();
    let other = "15550002222@s.whatsapp.net";
    f.db.upsert_contact_flagged(&f.account, WA_PHONE, "Ann", Some(true)).unwrap();
    f.db.upsert_contact_flagged(&f.account, other, "Group Only", Some(false)).unwrap();
    // Re-sync with the same facts changes nothing; a flipped fact is stored.
    f.db.upsert_contact_flagged(&f.account, other, "Group Only", Some(false)).unwrap();
    f.db.apply_conversation_event(&f.account, &conv_ev(WA_PHONE, Some(json!({ "contact": "Ann" }))))
        .unwrap();

    let rows = f.db.list_contacts(&f.account).unwrap();
    let ann = rows.iter().find(|c| c.remote_id == WA_PHONE).unwrap();
    let go = rows.iter().find(|c| c.remote_id == other).unwrap();
    assert_eq!(ann.metadata["saved"], json!(true));
    assert_eq!(ann.metadata["has_chat"], json!(true));
    assert_eq!(go.metadata["saved"], json!(false));
    assert_eq!(go.metadata["has_chat"], json!(false));

    f.db.upsert_contact_flagged(&f.account, other, "Group Only", Some(true)).unwrap();
    let rows = f.db.list_contacts(&f.account).unwrap();
    assert_eq!(rows.iter().find(|c| c.remote_id == other).unwrap().metadata["saved"], json!(true));
    // A sync without the fact leaves the stored one alone.
    f.db.upsert_contact(&f.account, other, "Renamed").unwrap();
    let rows = f.db.list_contacts(&f.account).unwrap();
    let go = rows.iter().find(|c| c.remote_id == other).unwrap();
    assert_eq!(go.display_name, "Renamed");
    assert_eq!(go.metadata["saved"], json!(true));
}

fn change(f: &Fx, remote: &str, target: &str, c: super::MessageChange) -> super::chat::ChangeOutcome {
    f.db
        .apply_message_change(&f.account, remote, &[], target, &c)
        .unwrap()
}

fn by_remote(f: &Fx, remote: &str, id: &str) -> Option<Message> {
    listed(f, remote)
        .into_iter()
        .find(|m| m.remote_id.as_deref() == Some(id))
}

#[test]
fn an_edit_replaces_the_text_marks_it_and_follows_the_preview() {
    let f = fx();
    ingest(&f, WA_PHONE, &[], false, msg("M1", "first", at(&f, 1), true));
    ingest(&f, WA_PHONE, &[], false, msg("M2", "teh typo", at(&f, 2), true));
    let edit = |text: &str, secs| super::MessageChange::Edited {
        text: text.into(),
        at: at(&f, secs),
    };

    let out = change(&f, WA_PHONE, "M2", edit("the typo", 60));
    let SendReconcile::Updated { message, .. } = out.message else {
        panic!("expected an updated message");
    };
    assert_eq!(message.body, "the typo");
    assert!(message.metadata["edited_at"].is_string());
    assert_eq!(out.conversation.unwrap().last_message_preview.as_deref(), Some("the typo"));
    assert_eq!(conv(&f, WA_PHONE).last_message_at, Some(at(&f, 2)), "chat time never moves");

    let again = change(&f, WA_PHONE, "M2", edit("the typo", 60));
    assert!(matches!(again.message, SendReconcile::Nothing), "re-applied fact is a no-op");
    let older = change(&f, WA_PHONE, "M2", edit("stale", 30));
    assert!(matches!(older.message, SendReconcile::Nothing));
    assert_eq!(by_remote(&f, WA_PHONE, "M2").unwrap().body, "the typo");

    let out = change(&f, WA_PHONE, "M1", edit("first, edited", 90));
    assert!(matches!(out.message, SendReconcile::Updated { .. }));
    assert!(out.conversation.is_none(), "an older message does not own the preview");
    assert_eq!(conv(&f, WA_PHONE).last_message_preview.as_deref(), Some("the typo"));

    let unknown = change(&f, WA_PHONE, "NOPE", edit("x", 99));
    assert!(matches!(unknown.message, SendReconcile::Nothing));
}

#[test]
fn a_revoke_leaves_a_tombstone_that_a_later_sync_cannot_refill() {
    let f = fx();
    let mut photo = msg("P1", "look", at(&f, 1), false);
    photo.metadata = json!({"id": "P1", "media_type": "image", "media_path": "/c/p1.jpg"});
    ingest(&f, WA_GROUP, &[], false, photo.clone());
    f.db
        .apply_reactions(
            &f.account,
            WA_GROUP,
            &[],
            &[react("P1", "me", "You", "👍", at(&f, 2), true)],
        )
        .unwrap();

    let out = change(&f, WA_GROUP, "P1", super::MessageChange::Revoked { at: at(&f, 5) });
    let SendReconcile::Updated { message, .. } = out.message else {
        panic!("expected an updated message");
    };
    assert_eq!(message.body, "");
    assert!(super::is_revoked(&message.metadata));
    assert!(message.metadata.get("media_path").is_none());
    assert_eq!(
        out.conversation.unwrap().last_message_preview.as_deref(),
        Some("This message was deleted")
    );
    let stored = by_remote(&f, WA_GROUP, "P1").unwrap();
    assert!(stored.reactions.is_empty(), "reactions go with the content");
    assert_eq!(stored.timestamp, at(&f, 1), "the tombstone keeps its place");

    ingest(&f, WA_GROUP, &[], true, photo);
    let stored = by_remote(&f, WA_GROUP, "P1").unwrap();
    assert_eq!(stored.body, "");
    assert!(stored.metadata.get("media_type").is_none());

    let edit = super::MessageChange::Edited { text: "back".into(), at: at(&f, 9) };
    assert!(matches!(change(&f, WA_GROUP, "P1", edit).message, SendReconcile::Nothing));
    let again = super::MessageChange::Revoked { at: at(&f, 6) };
    assert!(matches!(change(&f, WA_GROUP, "P1", again).message, SendReconcile::Nothing));
}

#[test]
fn delete_for_me_removes_the_row_for_good() {
    let f = fx();
    ingest(&f, WA_PHONE, &[], false, msg("M1", "keep", at(&f, 1), false));
    ingest(&f, WA_PHONE, &[], false, msg("M2", "drop me", at(&f, 2), false));
    let local_id = by_remote(&f, WA_PHONE, "M2").unwrap().id;

    let out = change(&f, WA_PHONE, "M2", super::MessageChange::DeletedForMe);
    match out.message {
        SendReconcile::Removed { message_id, .. } => assert_eq!(message_id, local_id),
        other => panic!("expected removal, got {other:?}"),
    }
    assert_eq!(out.conversation.unwrap().last_message_preview.as_deref(), Some("keep"));
    assert!(by_remote(&f, WA_PHONE, "M2").is_none());

    let resync = ingest(&f, WA_PHONE, &[], true, msg("M2", "drop me", at(&f, 2), false));
    assert_eq!(resync.kind, IngestKind::Duplicate);
    assert!(by_remote(&f, WA_PHONE, "M2").is_none(), "a later sync does not bring it back");

    let twice = change(&f, WA_PHONE, "M2", super::MessageChange::DeletedForMe);
    assert!(matches!(twice.message, SendReconcile::Nothing));

    let early = change(&f, WA_PHONE, "M3", super::MessageChange::DeletedForMe);
    assert!(matches!(early.message, SendReconcile::Nothing));
    ingest(&f, WA_PHONE, &[], false, msg("M3", "late arrival", at(&f, 3), false));
    assert!(by_remote(&f, WA_PHONE, "M3").is_none(), "deleted before it arrived");

    let none = f
        .db
        .apply_message_change(&f.account, "unknown@s.whatsapp.net", &[], "M1", &super::MessageChange::DeletedForMe)
        .unwrap();
    assert!(matches!(none.message, SendReconcile::Nothing));
}

#[test]
fn change_facts_parse_from_the_connector_wire_shape() {
    let parse = |v: Value| serde_json::from_value::<super::MessageChange>(v).unwrap();
    assert_eq!(
        parse(json!({"kind": "edited", "text": "x", "at": "2026-01-01T00:00:00+00:00"})),
        super::MessageChange::Edited {
            text: "x".into(),
            at: "2026-01-01T00:00:00Z".parse().unwrap()
        }
    );
    assert!(matches!(
        parse(json!({"kind": "revoked", "at": "2026-01-01T00:00:00.123Z"})),
        super::MessageChange::Revoked { .. }
    ));
    assert_eq!(parse(json!({"kind": "deleted_for_me"})), super::MessageChange::DeletedForMe);
}

#[test]
fn a_group_sender_resolves_to_their_direct_chat_by_jid_or_alias_and_nothing_is_created() {
    let f = fx();
    ingest(&f, WA_LID, &[], false, msg("x1", "hello", at(&f, 10), false));
    let mut ev = conv_ev(WA_PHONE, Some(json!({ "push": "Pat" })));
    ev.alt_remote_ids = vec![WA_LID.into()];
    f.db.apply_conversation_event(&f.account, &ev).unwrap();
    ingest(&f, WA_GROUP, &[], false, msg("g1", "hi all", at(&f, 20), false));
    let chat = conv(&f, WA_PHONE).id;
    let before = f.db.count_conversations(None, None, None, false).unwrap();

    for sender in [WA_PHONE, WA_LID] {
        let found = f.db.sender_contact(&f.account, sender).unwrap();
        assert_eq!(found.conversation.map(|c| c.id), Some(chat.clone()), "{sender}");
    }

    let stranger = "15550009999@s.whatsapp.net";
    f.db.upsert_contact(&f.account, stranger, "Robin").unwrap();
    let found = f.db.sender_contact(&f.account, stranger).unwrap();
    assert!(found.conversation.is_none());
    assert_eq!(found.contact.map(|c| c.display_name).as_deref(), Some("Robin"));
    assert!(
        f.db.sender_contact(&f.account, WA_GROUP).unwrap().conversation.is_none(),
        "a group is not a direct chat"
    );
    assert_eq!(f.db.count_conversations(None, None, None, false).unwrap(), before);
}

#[test]
fn a_caption_from_a_later_report_replaces_the_placeholder_body() {
    let f = fx();
    let mut first = msg("R1", "[video]", at(&f, 10), false);
    first.metadata = json!({ "id": "R1", "text": "[video]", "media_type": "video" });
    ingest(&f, WA_GROUP, &[], false, first);

    // WhatsApp reported the video before its caption; the caption arrives with a later report.
    let mut later = msg("R1", "3 BHK, monthly", at(&f, 10), false);
    later.metadata = json!({ "id": "R1", "text": "3 BHK, monthly", "media_type": "video", "filename": "v.mp4" });
    let out = ingest(&f, WA_GROUP, &[], false, later);
    assert_eq!(out.kind, IngestKind::Duplicate);

    let c = conv(&f, WA_GROUP);
    let stored = f.db.list_messages(&c.id, 10).unwrap();
    assert_eq!(stored[0].body, "3 BHK, monthly");
    assert_eq!(stored[0].metadata["media_type"], "video", "media facts still merge");
    assert_eq!(c.last_message_preview.as_deref(), Some("3 BHK, monthly"));
}

#[test]
fn stored_unknown_bodies_are_repaired_from_the_caption() {
    let dir = tempfile::tempdir().unwrap();
    let account = {
        let db = Database::open(dir.path()).unwrap();
        let account = db.create_account("whatsapp", "WA").unwrap().id;
        let mut m = msg("R1", "unknown", Utc::now(), false);
        m.metadata = json!({ "id": "R1", "text": "3 BHK, monthly", "preview": "3 BHK, monthly" });
        let preview = message_preview(&m.body, &m.metadata);
        db.ingest_message(&account, WA_GROUP, &[], false, m, &preview, "Sender")
            .unwrap();
        let mut empty = msg("R2", "unknown", Utc::now() + Duration::seconds(5), false);
        empty.metadata = json!({ "id": "R2", "text": "unknown", "preview": "unknown" });
        let preview = message_preview(&empty.body, &empty.metadata);
        db.ingest_message(&account, WA_PHONE, &[], false, empty, &preview, "Sender")
            .unwrap();
        account
    };

    let db = Database::open(dir.path()).unwrap();
    let c = db
        .get_conversation_by_remote(&account, WA_GROUP)
        .unwrap()
        .unwrap();
    let stored: Vec<String> = db
        .list_messages(&c.id, 10)
        .unwrap()
        .into_iter()
        .map(|m| m.body)
        .collect();
    assert_eq!(stored, vec!["3 BHK, monthly".to_string()]);
    assert_eq!(c.last_message_preview.as_deref(), Some("3 BHK, monthly"));

    // A message with no caption keeps no placeholder: blank, like any bodyless message.
    let direct = db
        .get_conversation_by_remote(&account, WA_PHONE)
        .unwrap()
        .unwrap();
    let blank = db.list_messages(&direct.id, 10).unwrap();
    assert_eq!(blank[0].body, "");
}

// ---------------------------------------------------------------------------------------------
// Polls: a vote is folded into the poll message, and replaying it changes nothing (R9)
// ---------------------------------------------------------------------------------------------

fn poll_msg(
    fx: &Fx,
    remote: &str,
    poll_id: &str,
    question: &str,
    options: &[(&str, &str)],
    multiple: bool,
    secs: i64,
) {
    let mut m = msg(poll_id, question, at(fx, secs), true);
    let opts: Vec<Value> = options
        .iter()
        .map(|(id, text)| json!({ "id": id, "text": text, "votes": 0, "voters": [], "mine": false }))
        .collect();
    m.metadata = json!({
        "media_type": "poll",
        "question": question,
        "poll": { "question": question, "multiple": multiple, "options": opts },
    });
    ingest(fx, remote, &[], false, m);
}

fn vote(fx: &Fx, remote: &str, poll_id: &str, voter: &str, selected: &[&str]) -> super::chat::PollOutcome {
    vote_named(fx, remote, poll_id, voter, voter, selected)
}

fn vote_named(
    fx: &Fx,
    remote: &str,
    poll_id: &str,
    voter: &str,
    name: &str,
    selected: &[&str],
) -> super::chat::PollOutcome {
    fx.db
        .apply_poll_change(
            &fx.account,
            remote,
            &[],
            poll_id,
            None,
            Some(&crate::db::PollVoteFact {
                voter_id: voter.into(),
                voter_name: Some(name.into()),
                selected: selected.iter().map(|s| s.to_string()).collect(),
            }),
        )
        .unwrap()
}

/// The stored poll of one message, as the UI reads it.
fn stored_poll(fx: &Fx, remote: &str, poll_id: &str) -> crate::models::PollData {
    let c = conv(fx, remote);
    let m = fx
        .db
        .list_messages(&c.id, 50)
        .unwrap()
        .into_iter()
        .find(|m| m.remote_id.as_deref() == Some(poll_id))
        .expect("poll message is stored");
    serde_json::from_value(m.metadata.get("poll").cloned().unwrap_or_default()).unwrap()
}

fn counts(poll: &crate::models::PollData) -> Vec<u32> {
    poll.options.iter().map(|o| o.votes).collect()
}

fn three_options(fx: &Fx) {
    poll_msg(
        fx,
        WA_PHONE,
        "P1",
        "Where shall we eat?",
        &[("h1", "Pizza"), ("h2", "Sushi"), ("h3", "Tacos")],
        false,
        10,
    );
}

#[test]
fn a_poll_message_stores_its_question_and_options() {
    let fx = fx();
    three_options(&fx);
    let poll = stored_poll(&fx, WA_PHONE, "P1");
    assert_eq!(poll.question, "Where shall we eat?");
    assert_eq!(poll.options.len(), 3);
    assert_eq!(counts(&poll), vec![0, 0, 0]);
    assert!(!poll.multiple);
    assert_eq!(
        conv(&fx, WA_PHONE).last_message_preview.as_deref(),
        Some("Poll: Where shall we eat?"),
        "the chat preview names the poll"
    );
}

#[test]
fn a_first_vote_counts_once_and_names_its_voter() {
    let fx = fx();
    three_options(&fx);
    let out = vote_named(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", "Ann", &["h2"]);
    assert!(out.changed);
    let poll = stored_poll(&fx, WA_PHONE, "P1");
    assert_eq!(counts(&poll), vec![0, 1, 0]);
    assert_eq!(poll.options[1].voters[0].name.as_deref(), Some("Ann"));
    assert!(!poll.options[1].mine, "a vote by a contact is not mine");
}

#[test]
fn replaying_the_same_vote_changes_nothing() {
    let fx = fx();
    three_options(&fx);
    vote(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", &["h1"]);
    let out = vote(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", &["h1"]);
    assert!(!out.changed, "a replayed vote reports no change");
    assert_eq!(counts(&stored_poll(&fx, WA_PHONE, "P1")), vec![1, 0, 0]);
}

#[test]
fn a_changed_vote_moves_instead_of_adding_one() {
    let fx = fx();
    three_options(&fx);
    vote(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", &["h1"]);
    vote(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", &["h3"]);
    let poll = stored_poll(&fx, WA_PHONE, "P1");
    assert_eq!(counts(&poll), vec![0, 0, 1]);
    assert_eq!(poll.options[0].voters.len(), 0);
}

#[test]
fn a_multiple_select_vote_counts_on_every_option_it_picked() {
    let fx = fx();
    poll_msg(
        &fx,
        WA_PHONE,
        "P2",
        "Pick the cakes",
        &[("c1", "Chocolate"), ("c2", "Lemon"), ("c3", "Cheesecake")],
        true,
        10,
    );
    vote(&fx, WA_PHONE, "P2", "ann@s.whatsapp.net", &["c1", "c2"]);
    let poll = stored_poll(&fx, WA_PHONE, "P2");
    assert!(poll.multiple);
    assert_eq!(counts(&poll), vec![1, 1, 0]);
}

#[test]
fn my_own_vote_is_marked_mine() {
    let fx = fx();
    three_options(&fx);
    vote_named(&fx, WA_PHONE, "P1", "me", "You", &["h1"]);
    let poll = stored_poll(&fx, WA_PHONE, "P1");
    assert_eq!(counts(&poll), vec![1, 0, 0]);
    assert!(poll.options[0].mine);
    assert!(!poll.options[1].mine);
}

#[test]
fn clearing_a_vote_removes_it() {
    let fx = fx();
    three_options(&fx);
    vote(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", &["h2"]);
    let out = vote(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", &[]);
    assert!(out.changed);
    assert_eq!(counts(&stored_poll(&fx, WA_PHONE, "P1")), vec![0, 0, 0]);
}

#[test]
fn a_vote_that_arrives_before_the_poll_is_dropped_safely() {
    let fx = fx();
    three_options(&fx);
    let out = vote(&fx, WA_PHONE, "P9", "ann@s.whatsapp.net", &["h1"]);
    assert!(!out.changed, "no poll message to fold into");
    assert!(out.message.is_none());
    let stored: Vec<String> = fx
        .db
        .list_messages(&conv(&fx, WA_PHONE).id, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.body)
        .collect();
    assert_eq!(stored.len(), 1, "the vote did not become a message");
}

#[test]
fn an_edited_poll_keeps_the_votes_already_folded() {
    let fx = fx();
    three_options(&fx);
    vote(&fx, WA_PHONE, "P1", "ann@s.whatsapp.net", &["h1"]);
    let definition = crate::models::PollData {
        question: "Where shall we eat tonight?".into(),
        multiple: true,
        options: vec![
            crate::models::PollOptionData { id: "h1".into(), text: "Pizza place".into(), ..Default::default() },
            crate::models::PollOptionData { id: "h2".into(), text: "Sushi".into(), ..Default::default() },
            crate::models::PollOptionData { id: "h4".into(), text: "Noodles".into(), ..Default::default() },
        ],
    };
    let out = fx
        .db
        .apply_poll_change(&fx.account, WA_PHONE, &[], "P1", Some(&definition), None)
        .unwrap();
    assert!(out.changed);
    let poll = stored_poll(&fx, WA_PHONE, "P1");
    assert_eq!(poll.question, "Where shall we eat tonight?");
    assert!(poll.multiple);
    assert_eq!(counts(&poll), vec![1, 0, 0], "the vote follows its option id");
    assert_eq!(
        poll.options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(),
        vec!["Pizza place", "Sushi", "Noodles"],
        "a removed option is gone, a new one starts empty"
    );
    assert_eq!(poll.options[0].voters[0].id, "ann@s.whatsapp.net");
}
