"""Unit tests for the WhatsApp connector (stdlib only: python3 -m unittest discover -s connectors/tests)."""

from __future__ import annotations

import base64
import importlib.util
import io
import json
import os
import sqlite3
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

# Keep every cache/state file the connector writes out of the real Shuttle data folder.
import atexit  # noqa: E402
import shutil  # noqa: E402

_TEST_DATA = tempfile.mkdtemp(prefix="shuttle-connector-tests-")
os.environ["SHUTTLE_DATA_DIR"] = _TEST_DATA
for _var in ("SHUTTLE_FILES_ROOT", "SHUTTLE_FILES_DIR"):
    os.environ.pop(_var, None)
atexit.register(shutil.rmtree, _TEST_DATA, ignore_errors=True)

CONNECTORS = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(CONNECTORS))

_spec = importlib.util.spec_from_file_location("whatsapp_connector", CONNECTORS / "whatsapp-connector.py")
wa = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(wa)

import shuttle_ipc  # noqa: E402

T0 = "2025-03-01T10:00:00+00:00"
T1 = "2025-03-01T10:05:00+00:00"
T2 = "2025-03-01T10:10:00+00:00"
PHONE = "628111111111@s.whatsapp.net"
LID = "999000111@lid"
SELF_A = "628100000001@s.whatsapp.net"
SELF_B = "628100000002@s.whatsapp.net"


class FakeGowa:
    """Just enough of GOWA's REST surface, honouring limit/offset/start_time like the real one."""

    def __init__(self) -> None:
        self.chats: list[dict] = []
        self.messages: dict[str, list[dict]] = {}
        self.contacts: list[dict] | None = []
        self.groups: list[dict] = []
        self.calls: list[tuple[str, str, dict | None]] = []
        self.device_id = "acc-a"
        self.base_url = "http://127.0.0.1:1"
        self.auth = ("u", "p")
        self.posts: list[tuple[str, dict | None]] = []
        self.post_code = 200

    def get(self, path, query=None, **kw):
        self.calls.append(("GET", path, query))
        query = query or {}
        if path == "/chats":
            off, lim = int(query.get("offset", 0)), int(query.get("limit", 25))
            return 200, {"results": {"data": self.chats[off : off + lim]}}
        if path == "/user/my/contacts":
            if self.contacts is None:
                return 500, {}
            return 200, {"results": {"data": self.contacts}}
        if path == "/user/my/groups":
            return 200, {"results": {"data": self.groups}}
        if path.startswith("/chat/") and path.endswith("/messages"):
            jid = urllib_unquote(path[len("/chat/") : -len("/messages")])
            rows = list(self.messages.get(jid, []))
            start = query.get("start_time")
            if start:
                s = shuttle_ipc.parse_timestamp_strict(start)
                rows = [r for r in rows if shuttle_ipc.parse_timestamp_strict(r["timestamp"]) >= s]
            rows.sort(key=lambda r: shuttle_ipc.parse_timestamp_strict(r["timestamp"]), reverse=True)
            off, lim = int(query.get("offset", 0)), int(query.get("limit", 50))
            return 200, {"results": {"data": rows[off : off + lim]}}
        return 404, {}

    def post(self, path, **kw):
        self.posts.append((path, kw.get("json_body")))
        if self.post_code != 200:
            return self.post_code, {"message": "boom"}
        return 200, {"results": {"message_id": "WAMID1"}}

    def post_multipart(self, path, fields, files=None, timeout=60.0):
        self.posts.append((path, fields))
        return self.post(path)

    def patch(self, path, **kw):
        return 200, {}


def urllib_unquote(s: str) -> str:
    import urllib.parse

    return urllib.parse.unquote(s)


def make_row(mid, ts, text="hi", from_me=False, sender=PHONE, **extra):
    row = {
        "id": mid,
        "chat_jid": PHONE,
        "sender_jid": sender,
        "sender_display_name": "",
        "content": text,
        "timestamp": ts,
        "is_from_me": from_me,
    }
    row.update(extra)
    return row


class Capture:
    """Patches connector.send and collects output."""

    def __init__(self) -> None:
        self.out: list[dict] = []
        self._p = mock.patch.object(wa, "send", side_effect=self.out.append)

    def __enter__(self):
        self._p.start()
        return self

    def __exit__(self, *a):
        self._p.stop()

    def events(self, name=None):
        return [m for m in self.out if m.get("type") == "event" and (name is None or m["event"] == name)]


def make_session(account="acc-a", self_jid=SELF_A, client=None, **kw):
    s = wa.WhatsAppSession(account, state=wa.SyncState(None))
    s._self_jid = self_jid
    s._connected = True
    s.client = client or FakeGowa()
    s._contacts_loaded = True
    for k, v in kw.items():
        setattr(s, k, v)
    return s


class LateMetadataTests(unittest.TestCase):
    def test_empty_initial_sync_recovers_names_and_pins_outside_recent_page(self):
        s = make_session(_self_name="My Shop", _self_aliases={"My Shop"})
        group = "120@g.us"
        with Capture() as cap, mock.patch.object(wa, "load_chat_settings", return_value={}) as settings:
            s._emit_conversation(s._chat_ref(PHONE), {}, last_message_at=T0)
            s._emit_conversation(s._chat_ref(group), {}, last_message_at=T1)
            s._refresh_metadata(force=True)  # Initial link returns an empty contacts book.
            self.assertTrue(s._contacts_loaded)
            s.client.contacts = [{"jid": PHONE, "name": "Saved Bob"}, {"jid": SELF_A, "name": "My Shop"}]
            s._refresh_contacts()  # An open-chat refresh may load names before metadata export.
            s.client.groups = [{"JID": group, "Name": "Community"}]
            settings.return_value = {PHONE: {"pinned": True, "archived": False}}
            s._metadata_checked_at = float("-inf")
            s._history_done = True
            s.catch_up_recent()  # /chats is empty: cached rows still have to refresh.
            updates = {x["payload"]["remote_id"]: x["payload"] for x in cap.events("conversation.updated")}
            self.assertEqual(updates[PHONE]["names"]["contact"], "Saved Bob")
            self.assertTrue(updates[PHONE]["pinned"])
            self.assertEqual(updates[PHONE]["last_message_at"], T0)
            self.assertEqual(updates[group]["names"]["subject"], "Community")
            self.assertEqual(s._compose_names(s._chat_ref(SELF_A)), {"contact": "My Shop"})
            self.assertEqual(len(cap.events('contacts.synced')), 1)
            self.assertFalse(any(path.startswith('/chat/') and path.endswith('/messages') and 'status' not in path for _, path, _ in s.client.calls))
            before = len(s.client.calls)
            self.assertFalse(s._refresh_metadata())
            self.assertEqual(len(s.client.calls), before)  # Rate limit metadata traffic.
            s.client.groups = []
            s.client.contacts = None
            settings.return_value = {PHONE: {"pinned": False, "archived": True}}
            s._refresh_metadata(force=True)
            latest = [x['payload'] for x in cap.events('conversation.updated') if x['payload']['remote_id'] == PHONE][-1]
            self.assertFalse(latest['pinned'])
            self.assertTrue(latest['archived'])
            self.assertEqual(latest['names']['contact'], 'Saved Bob')
            self.assertEqual(s._compose_names(s._chat_ref(group), row={'name': 'Group 120'})['subject'], 'Community')
            fresh_session = make_session()
            self.assertEqual(fresh_session._compose_names(fresh_session._chat_ref(group), row={'name': 'Group 120'}), {})

    def test_late_history_dumps_are_checked_after_initial_sync(self):
        s = make_session()
        s._history_done = True
        with Capture(), mock.patch.object(s, '_refresh_metadata', return_value=True), mock.patch.object(s, '_import_history_dumps') as ingest:
            s.catch_up_recent()
            ingest.assert_called_once()

    def test_shared_connector_media_roots_remain_per_account(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(wa._account_files, {}, clear=True), Capture():
            dispatcher = wa.Dispatcher()
            dispatcher.handle({'type': 'get_status', 'files_dir': str(Path(tmp)/'Whatsapp'/'Personal')}, 'acc-a')
            dispatcher.handle({'type': 'get_status', 'files_dir': str(Path(tmp)/'Whatsapp'/'Work')}, 'acc-b')
            a = wa.files_dir('acc-a')
            b = wa.files_dir('acc-b')
            self.assertEqual(a, Path(tmp)/'Whatsapp'/'Personal')
            self.assertEqual(b, Path(tmp)/'Whatsapp'/'Work')
            self.assertTrue((a/'media').is_dir())
            self.assertNotEqual(a, b)


class MediaDownloadTests(unittest.TestCase):
    def test_socket_connected_does_not_mean_authenticated(self):
        s = make_session()
        self.assertFalse(s._status_logged_in({'is_connected': True, 'is_logged_in': False}))
        self.assertTrue(s._status_logged_in({'is_connected': True, 'is_logged_in': True}))

    def test_cdn_failure_has_safe_specific_reason(self):
        client = mock.Mock()
        client.get.return_value = (500, {'message': 'failed to download media: download failed with status code 403'})
        result = wa.download_gowa_media(client, 'acc-a', 'mid', PHONE)
        self.assertIn('resend', result['error'])
        self.assertNotIn(PHONE, result['error'])
        client.download_to_file.assert_not_called()

    def test_streaming_failure_preserves_cached_file_and_removes_partial(self):
        class Response(io.BytesIO):
            headers = {'Content-Length': '10'}
            def read(self, size=-1):
                self_size = size
                if self_size != 64 * 1024:
                    raise AssertionError('Download must use bounded reads')
                return super().read(size)
        client = wa.GowaClient('http://127.0.0.1:1', ('u', 'p'), 'acc-a')
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'video.mp4'
            path.write_bytes(b'previous')
            with mock.patch.object(wa.urllib.request, 'urlopen', return_value=Response(b'short')):
                with self.assertRaises(OSError):
                    client.download_to_file('/media/test', path, max_bytes=100)
            self.assertEqual(path.read_bytes(), b'previous')
            self.assertEqual(list(Path(tmp).glob('*.part')), [])
            with mock.patch.object(wa.urllib.request, 'urlopen', return_value=Response(b'0123456789')):
                client.download_to_file('/media/test', path, max_bytes=100)
            self.assertEqual(path.read_bytes(), b'0123456789')

    def test_media_above_old_preview_limit_is_saved(self):
        client = mock.Mock()
        client.get.return_value = (200, {'results': {'file_url': '/media/test', 'media_type': 'video', 'filename': 'clip.mp4'}})
        raw = b'x' * 3_000_000
        client.download.return_value = raw
        client.download_to_file.side_effect = lambda url, path, **kw: path.write_bytes(raw)
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(wa, 'media_cache_file', return_value=Path(tmp)/'clip.mp4'):
            result = wa.download_gowa_media(client, 'acc-a', 'mid', PHONE)
            self.assertIsNotNone(result)
            self.assertEqual(Path(result['media_path']).stat().st_size, len(raw))


JPEG = b"\xff\xd8\xff\xe0" + b"j" * 120 + b"\xff\xd9"


def thumb_wrap(mid, key="imageMessage", thumb=JPEG, **extra):
    import base64

    part = {"mimetype": "image/jpeg", "fileLength": 12345, **extra}
    if thumb is not None:
        part["JPEGThumbnail"] = base64.b64encode(thumb).decode()
    return {"message": {"key": {"ID": mid, "fromMe": False}, "message": {key: part}, "messageTimestamp": 1740823200}}


def text_wrap(mid, text):
    return {
        "message": {
            "key": {"ID": mid, "fromMe": False},
            "message": {"conversation": text},
            "messageTimestamp": 1740823200,
        }
    }


class MediaCacheTests(unittest.TestCase):
    """Downloads go to the app-private cache, never to the user's Documents folder."""

    def test_default_cache_is_under_the_data_dir_not_documents(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(os.environ, {"SHUTTLE_DATA_DIR": tmp}, clear=False), \
                mock.patch.dict(wa._account_files, {}, clear=True):
            for var in ("SHUTTLE_FILES_ROOT", "SHUTTLE_FILES_DIR"):
                os.environ.pop(var, None)
            root = wa.files_dir("acc/1")
            self.assertEqual(root, Path(tmp) / "media_cache" / "acc_1")
            for sub in ("media", "avatars", "thumbs"):
                self.assertTrue((root / sub).is_dir())
            media = wa.media_cache_file("acc/1", "MSG-1", "video", "clip.mp4")
            self.assertEqual(media.parent, root / "media")
            self.assertNotIn("Documents", str(media))

    def test_core_supplied_roots_win(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(wa._account_files, {}, clear=True), \
                mock.patch.dict(os.environ, {"SHUTTLE_FILES_ROOT": str(Path(tmp) / "cache")}):
            self.assertEqual(wa.files_dir("acc-a"), Path(tmp) / "cache" / "acc-a")
            wa._account_files["acc-b"] = Path(tmp) / "elsewhere"
            self.assertEqual(wa.files_dir("acc-b"), Path(tmp) / "elsewhere")

    def test_download_removes_gowas_own_copy_and_touches_cached_file(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(os.environ, {"SHUTTLE_DATA_DIR": tmp}):
            gowa_media = Path(tmp) / "gowa" / "statics" / "media"
            gowa_media.mkdir(parents=True)
            scratch = gowa_media / "123-abc.jpg"
            scratch.write_bytes(b"gowa copy")
            keep = gowa_media / "other.jpg"
            keep.write_bytes(b"other")
            client = mock.Mock()
            url = "http://127.0.0.1:1/statics/media/123-abc.jpg"
            client.get.return_value = (200, {"results": {"file_url": url, "media_type": "image", "filename": "p.jpg"}})
            target = Path(tmp) / "cache" / "p.jpg"
            target.parent.mkdir()
            client.download_to_file.side_effect = lambda u, path, **kw: path.write_bytes(b"image")
            with mock.patch.object(wa, "media_cache_file", return_value=target), \
                    mock.patch.object(wa, "gowa_home", return_value=Path(tmp) / "gowa"):
                got = wa.download_gowa_media(client, "acc-a", "MID", PHONE)
                self.assertEqual(got["media_path"], str(target))
                self.assertFalse(scratch.exists())
                self.assertTrue(keep.exists())
                # A second call reuses the cached file and marks it as just used.
                old = time.time() - 40 * 86400
                os.utime(target, (old, old))
                scratch.write_bytes(b"again")
                client.download_to_file.reset_mock()
                wa.download_gowa_media(client, "acc-a", "MID", PHONE)
                client.download_to_file.assert_not_called()
                self.assertGreater(target.stat().st_mtime, time.time() - 60)
                self.assertFalse(scratch.exists())

    def test_gowa_cleanup_never_leaves_gowas_media_folder(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(wa, "gowa_home", return_value=Path(tmp) / "gowa"):
            outside = Path(tmp) / "secret.txt"
            outside.write_text("keep")
            (Path(tmp) / "gowa" / "statics" / "media").mkdir(parents=True)
            wa.discard_gowa_copy("http://127.0.0.1:1/statics/media/..%2F..%2F..%2Fsecret.txt")
            wa.discard_gowa_copy("http://127.0.0.1:1/other/secret.txt")
            self.assertTrue(outside.exists())

    def test_media_info_reports_size_type_and_dimensions(self):
        text, extra = wa.message_body_and_media(
            {"media_type": "video", "media_info": {"mime_type": "video/mp4", "file_length": 5_000_000, "width": 640, "height": 360, "seconds": 12}}
        )
        self.assertEqual(extra["file_length"], 5_000_000)
        self.assertEqual(extra["mime"], "video/mp4")
        self.assertEqual((extra["width"], extra["height"], extra["duration"]), (640, 360, 12))
        _, doc = wa.message_body_and_media({"media_type": "document", "media_info": {"file_name": "a.pdf", "file_length": 10}})
        self.assertEqual(doc["filename"], "a.pdf")
        # REST rows keep their flat fields.
        _, flat = wa.message_body_and_media({"media_type": "image", "file_length": 99, "filename": "x.jpg"})
        self.assertEqual((flat["file_length"], flat["filename"]), (99, "x.jpg"))


class ThumbnailTests(unittest.TestCase):
    def test_extracts_only_real_embedded_jpegs(self):
        mid, raw = wa.history_message_thumbnail(thumb_wrap("T1"))
        self.assertEqual((mid, raw), ("T1", JPEG))
        self.assertEqual(wa.history_message_thumbnail(thumb_wrap("T2", key="videoMessage"))[0], "T2")
        self.assertEqual(wa.history_message_thumbnail(thumb_wrap("T3", key="documentMessage"))[0], "T3")
        self.assertIsNone(wa.history_message_thumbnail(thumb_wrap("T4", thumb=None)))
        self.assertIsNone(wa.history_message_thumbnail(thumb_wrap("T5", thumb=b"not a jpeg at all")))
        self.assertIsNone(wa.history_message_thumbnail({"message": {"key": {"ID": "T6"}, "message": {"conversation": "hi"}}}))
        self.assertIsNone(wa.history_message_thumbnail({"message": {"message": {"imageMessage": {"JPEGThumbnail": "/9g="}}}}))

    def test_thumbnails_are_stored_small_atomic_and_per_account(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(wa._account_files, {"a": Path(tmp) / "a", "b": Path(tmp) / "b"}, clear=True):
            path = wa.save_thumbnail("a", "MID/1", JPEG)
            self.assertEqual(path.parent, Path(tmp) / "a" / "thumbs")
            self.assertEqual(path.read_bytes(), JPEG)
            self.assertFalse(wa.thumbnail_file("b", "MID/1").exists())
            self.assertIsNone(wa.save_thumbnail("a", "BIG", b"\xff\xd8" + b"x" * 70_000))
            self.assertIsNone(wa.save_thumbnail("a", "PNG", b"\x89PNG...."))
            self.assertEqual(list((Path(tmp) / "a" / "thumbs").glob("*.part")), [])

    def test_messages_carry_the_stored_preview_as_a_fact(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(wa._account_files, {"acc-a": Path(tmp)}, clear=True):
            wa.save_thumbnail("acc-a", "IMG1", JPEG)
            s = make_session()
            ref = {"remote_id": PHONE, "alts": []}
            with Capture() as cap:
                s._emit_message(ref, {"id": "IMG1", "text": "[image]", "preview": "x", "timestamp": T0, "from_me": False, "extra": {"media_type": "image"}}, False)
                s._emit_message(ref, {"id": "VID9", "text": "[video]", "preview": "x", "timestamp": T1, "from_me": False, "extra": {"media_type": "video"}}, False)
                s._emit_message(ref, {"id": "IMG1", "text": "hello", "preview": "x", "timestamp": T2, "from_me": False, "extra": {}}, False)
            got = [m["payload"]["message"] for m in cap.events("message.received")]
            self.assertTrue(got[0]["thumbnail_path"].endswith("IMG1.jpg"))
            self.assertNotIn("thumbnail_path", got[1])  # no preview known: report nothing
            self.assertNotIn("thumbnail_path", got[2])  # plain text never gets one

    def test_dump_import_extracts_previews_once_and_reports_them(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "storages"
            store.mkdir()
            cache = Path(tmp) / "cache"
            dump = store / "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
            status = {"ID": "status@broadcast", "messages": [thumb_wrap("ST1")]}
            chat = {"ID": PHONE, "name": "Bob", "messages": [thumb_wrap("IMG7"), thumb_wrap("VID7", key="videoMessage"), thumb_wrap("NOTHUMB", thumb=None)]}
            dump.write_text(json.dumps({"conversations": [chat, status]}))
            s = make_session()
            real_hf = wa.history_files
            with mock.patch.dict(wa._account_files, {"acc-a": cache}, clear=True), \
                    mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    s._import_history_dumps()
                    facts = [i for e in cap.events("media.thumbnails") for i in e["payload"]["items"]]
                    self.assertEqual(sorted(f["message_id"] for f in facts), ["IMG7", "ST1", "VID7"])
                    self.assertTrue(all(Path(f["thumbnail_path"]).is_file() for f in facts))
                    self.assertTrue(all(str(cache) in f["thumbnail_path"] for f in facts))
                    self.assertIn(dump.name, s.state.thumb_files)
                    again = len(cap.out)
                    s._import_history_dumps()
                    self.assertEqual(len(cap.out), again)
                # An account whose dumps were imported by an older build gets the backfill pass.
                old = make_session()
                old.state.mark_imported([dump.name])
                old.state.mark_reactions_imported([dump.name])
                (cache / "thumbs" / "IMG7.jpg").unlink()
                with Capture() as cap2:
                    old._import_history_dumps()
                self.assertEqual(len(cap2.events("message.received")), 0)
                items = [i for e in cap2.events("media.thumbnails") for i in e["payload"]["items"]]
                self.assertIn("IMG7", [i["message_id"] for i in items])

    def test_sync_state_remembers_processed_dumps(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "state.json"
            st = wa.SyncState(path)
            st.mark_thumbs_done(["d1.json"])
            st.save()
            self.assertEqual(wa.SyncState(path).thumb_files, {"d1.json"})

    def test_previews_written_to_another_root_are_extracted_again(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp) / "cache" / "thumbs"
            elsewhere = Path(tmp) / "Documents" / "thumbs"
            cache.mkdir(parents=True)
            elsewhere.mkdir(parents=True)
            (elsewhere / "OLD1.jpg").write_bytes(JPEG)
            path = Path(tmp) / "state.json"
            st = wa.SyncState(path)
            st.mark_thumbs_done(["d1.json", "d2.json"], 1)
            st.save()
            restored = wa.SyncState(path)
            self.assertEqual(restored.thumb_files, {"d1.json", "d2.json"})
            self.assertNotEqual(restored.thumbs_layout, wa.THUMBS_LAYOUT)
            # The done set describes previews that are not where the UI looks.
            self.assertTrue(restored.bind_thumbs(cache))
            self.assertEqual(restored.thumb_files, set())
            self.assertEqual(restored.thumbs_count, 0)
            self.assertEqual(restored.thumbs_root, str(cache))
            # Bound to this root once: nothing left to do.
            self.assertFalse(restored.bind_thumbs(cache))
            saved = Path(tmp) / "next.json"
            restored.path = saved
            restored.save()
            self.assertEqual(wa.SyncState(saved).thumbs_root, str(cache))
            self.assertEqual(wa.SyncState(saved).thumbs_layout, wa.THUMBS_LAYOUT)

    def test_previews_missing_from_the_current_root_are_extracted_again(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp) / "cache" / "thumbs"
            cache.mkdir(parents=True)
            st = wa.SyncState(None)
            st.mark_thumbs_done(["d1.json"], 2)
            # A state that never recorded a folder adopts the current one.
            self.assertTrue(st.bind_thumbs(cache))
            st.mark_thumbs_done(["d1.json"], 2)
            (cache / "A.jpg").write_bytes(JPEG)
            (cache / "B.jpg").write_bytes(JPEG)
            self.assertFalse(st.bind_thumbs(cache))
            (cache / "B.jpg").unlink()
            self.assertTrue(st.bind_thumbs(cache))
            self.assertEqual(st.thumb_files, set())

    def test_a_session_whose_previews_are_gone_extracts_them_again(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "storages"
            store.mkdir()
            cache = Path(tmp) / "cache"
            dump = store / "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
            dump.write_text(json.dumps({"conversations": [{"ID": PHONE, "name": "Bob", "messages": [thumb_wrap("IMG8")]}]}))
            real_hf = wa.history_files
            with mock.patch.dict(wa._account_files, {"acc-a": cache}, clear=True), \
                    mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                done = make_session()
                with Capture():
                    done._import_history_dumps()
                self.assertTrue(wa.thumbnail_file("acc-a", "IMG8").is_file())
                # An older build marked the dump done but its previews went to Documents.
                for f in (cache / "thumbs").glob("*.jpg"):
                    f.unlink()
                again = make_session()
                again.state.thumb_files = set(done.state.thumb_files)
                again.state.thumbs_layout = done.state.thumbs_layout
                again.state.thumbs_root = done.state.thumbs_root
                again.state.thumbs_count = done.state.thumbs_count
                again.state.mark_imported([dump.name])
                with Capture() as cap:
                    again._import_history_dumps()
                items = [i for e in cap.events("media.thumbnails") for i in e["payload"]["items"]]
                self.assertEqual([i["message_id"] for i in items], ["IMG8"])
                self.assertTrue(wa.thumbnail_file("acc-a", "IMG8").is_file())


class StatusFeedTests(unittest.TestCase):
    """The status rail: every page of GOWA's rows, plus the statuses only the dumps keep."""

    def _dump(self, store: Path, entries, mappings=None):
        dump = store / "history-1700-628100000001:1@s.whatsapp.net-2-INITIAL_STATUS_V3.json"
        dump.write_text(
            json.dumps(
                {
                    "syncType": "INITIAL_STATUS_V3",
                    "statusV3Messages": entries,
                    "phoneNumberToLidMappings": mappings or [],
                }
            )
        )
        return dump

    def _feed(self, session, cap):
        session._emit_status_feed(force=True)
        return cap.events("status.feed")[-1]["payload"]["items"]

    def test_every_page_of_statuses_reaches_the_feed(self):
        client = FakeGowa()
        client.messages["status@broadcast"] = [
            make_row(f"S{i}", f"2025-03-01T10:{i % 60:02d}:30+00:00", text="hi", sender=PHONE)
            for i in range(250)
        ]
        with Capture() as cap:
            items = self._feed(make_session(client=client), cap)
        self.assertEqual(sum(len(i["posts"]) for i in items), 250)

    def test_a_link_status_carries_its_card_and_stored_preview(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "storages"
            store.mkdir()
            cache = Path(tmp) / "cache"
            url = "https://www.instagram.com/reel/xyz"
            self._dump(
                store,
                [
                    {
                        "key": {"remoteJID": "status@broadcast", "fromMe": False, "ID": "LINK1"},
                        "message": {
                            "messageContextInfo": {},
                            "extendedTextMessage": {
                                "text": f"look {url}",
                                "matchedText": url,
                                "title": "A reel",
                                "JPEGThumbnail": base64.b64encode(JPEG).decode(),
                            },
                        },
                        "messageTimestamp": 1740823200,
                        "participant": LID,
                    }
                ],
                [{"pnJID": PHONE, "lidJID": LID}],
            )
            real_hf = wa.history_files
            with mock.patch.dict(wa._account_files, {"acc-a": cache}, clear=True), \
                    mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    items = self._feed(make_session(), cap)
            self.assertEqual([i["sender_id"] for i in items], [PHONE])
            post = items[0]["posts"][0]
            self.assertEqual(post["media_type"], "text")
            self.assertEqual(post["link_preview"]["url"], url)
            self.assertEqual(post["link_preview"]["title"], "A reel")
            thumb = post["link_preview"]["thumbnail_path"]
            self.assertTrue(thumb.endswith(".jpg"))
            self.assertTrue(Path(thumb).is_file())
            self.assertNotIn("thumbnail_path", post)

    def test_a_media_status_carries_its_preview_and_no_card(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "storages"
            store.mkdir()
            cache = Path(tmp) / "cache"
            self._dump(
                store,
                [
                    {
                        "key": {"remoteJID": "status@broadcast", "fromMe": False, "ID": "IMG1"},
                        "message": {
                            "imageMessage": {
                                "caption": "beach",
                                "JPEGThumbnail": base64.b64encode(JPEG).decode(),
                            },
                            "messageContextInfo": {},
                        },
                        "messageTimestamp": 1740823200,
                        "participant": LID,
                    }
                ],
                [{"pnJID": PHONE, "lidJID": LID}],
            )
            real_hf = wa.history_files
            with mock.patch.dict(wa._account_files, {"acc-a": cache}, clear=True), \
                    mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    items = self._feed(make_session(), cap)
            post = items[0]["posts"][0]
            self.assertEqual(post["media_type"], "image")
            self.assertEqual(post["text"], "beach")
            self.assertTrue(post["thumbnail_path"].endswith(".jpg"))
            self.assertTrue(Path(post["thumbnail_path"]).is_file())
            self.assertNotIn("link_preview", post)

    def test_a_status_the_dump_and_gowa_both_have_is_reported_once(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "storages"
            store.mkdir()
            cache = Path(tmp) / "cache"
            self._dump(
                store,
                [
                    {
                        "key": {"remoteJID": "status@broadcast", "fromMe": False, "ID": "DUP1"},
                        "message": {"conversation": "hello"},
                        "messageTimestamp": 1740823200,
                        "participant": PHONE,
                    }
                ],
            )
            client = FakeGowa()
            client.messages["status@broadcast"] = [make_row("DUP1", T0, text="hello", sender=PHONE)]
            real_hf = wa.history_files
            with mock.patch.dict(wa._account_files, {"acc-a": cache}, clear=True), \
                    mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    items = self._feed(make_session(client=client), cap)
            self.assertEqual(len(items), 1)
            self.assertEqual([p["id"] for p in items[0]["posts"]], ["DUP1"])
            self.assertEqual(items[0]["count"], 1)

    def test_status_dumps_are_read_once(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "storages"
            store.mkdir()
            cache = Path(tmp) / "cache"
            dump = self._dump(
                store,
                [
                    {
                        "key": {"remoteJID": "status@broadcast", "ID": "ONCE1"},
                        "message": {"conversation": "hi"},
                        "messageTimestamp": 1740823200,
                        "participant": PHONE,
                    }
                ],
            )
            real_hf = wa.history_files
            with mock.patch.dict(wa._account_files, {"acc-a": cache}, clear=True), \
                    mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                s = make_session()
                with Capture() as cap:
                    first = self._feed(s, cap)
                self.assertEqual(sum(len(i["posts"]) for i in first), 1)
                self.assertIn(dump.name, s.state.status_files)
                with Capture() as cap2:
                    again = self._feed(s, cap2)
            self.assertEqual(sum(len(i["posts"]) for i in again), 0)

    def test_link_preview_needs_a_real_url(self):
        self.assertIsNone(wa.status_link_preview({"extendedTextMessage": {"text": "no link"}}))
        self.assertIsNone(wa.status_link_preview({"conversation": "hi"}))
        card = wa.status_link_preview(
            {"extendedTextMessage": {"text": "see https://a.example/x", "title": "T"}}
        )
        self.assertEqual(card["url"], "https://a.example/x")
        self.assertEqual(card["title"], "T")
        self.assertIsNone(card["thumbnail"])


class StrictTimestampTests(unittest.TestCase):
    def test_valid_and_invalid(self):
        p = shuttle_ipc.parse_timestamp_strict
        self.assertEqual(p("2025-03-01T10:00:00Z").isoformat(), "2025-03-01T10:00:00+00:00")
        self.assertEqual(p(1740823200).year, 2025)
        self.assertEqual(p("1740823200000").year, 2025)
        for bad in (None, "", "  ", 0, "0", -5, "0001-01-01T00:00:00Z", "garbage", True, float("nan"), 5000000000000000):
            self.assertIsNone(p(bad), repr(bad))
        self.assertIsNone(shuttle_ipc.to_rfc3339_strict(None))
        # The lenient helper other connectors use is unchanged.
        self.assertTrue(shuttle_ipc.to_rfc3339(None))

    def test_rows_without_time_or_id_are_dropped(self):
        s = make_session()
        ref = s._chat_ref(PHONE)
        rows = [
            make_row("A", T0),
            make_row("B", None),
            make_row("C", "0001-01-01T00:00:00Z"),
            make_row("", T1),
        ]
        with Capture() as cap:
            n = s._emit_rows(ref, list(reversed(rows)), history=True)
        self.assertEqual(n, 1)
        msgs = cap.events("message.received")
        self.assertEqual([m["payload"]["message"]["id"] for m in msgs], ["A"])
        self.assertEqual(msgs[0]["payload"]["message"]["timestamp"], T0)

    def test_conversation_omits_invalid_last_message_at(self):
        s = make_session()
        ref = s._chat_ref(PHONE)
        with Capture() as cap:
            s._emit_conversation(ref, {}, last_message_at="0001-01-01T00:00:00Z")
        payload = cap.events("conversation.updated")[0]["payload"]
        self.assertNotIn("last_message_at", payload)
        for banned in ("force_recency", "replace_title", "list_rank", "unread_count"):
            self.assertNotIn(banned, payload)


class HistoryDumpTests(unittest.TestCase):
    def test_dump_filtering_by_own_jid(self):
        with tempfile.TemporaryDirectory() as d:
            store = Path(d)
            names = [
                "history-1700-628100000001:12@s.whatsapp.net-1-INITIAL_BOOTSTRAP.json",
                "history-1700-628100000001@s.whatsapp.net-2-RECENT.json",
                "history-1700-628100000002:3@s.whatsapp.net-1-RECENT.json",
                "history-1700-6281000000011@s.whatsapp.net-1-RECENT.json",
                "unrelated.json",
            ]
            for n in names:
                (store / n).write_text("{}")
            got = {p.name for p in wa.history_files(SELF_A, store)}
            self.assertEqual(got, set(names[:2]))
            got_b = {p.name for p in wa.history_files("628100000002:9@s.whatsapp.net", store)}
            self.assertEqual(got_b, {names[2]})
            self.assertEqual(wa.history_files(None, store), [])

    def test_import_once_and_never_in_refresh(self):
        with tempfile.TemporaryDirectory() as d:
            store = Path(d)
            dump = store / "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
            dump.write_text(
                json.dumps(
                    {
                        "conversations": [
                            {
                                "ID": PHONE,
                                "name": "Bob",
                                "messages": [
                                    {
                                        "message": {
                                            "key": {"ID": "H1", "fromMe": False},
                                            "message": {"conversation": "from dump"},
                                            "messageTimestamp": 1740823200,
                                        }
                                    },
                                    {
                                        "message": {
                                            "key": {"ID": "H2", "fromMe": False},
                                            "message": {"conversation": "no time"},
                                        }
                                    },
                                ],
                            }
                        ]
                    }
                )
            )
            other = store / "history-1700-628100000002:1@s.whatsapp.net-1-RECENT.json"
            other.write_text(json.dumps({"conversations": [{"ID": "1@s.whatsapp.net", "messages": []}]}))
            s = make_session()
            real_hf = wa.history_files
            with mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    s._import_history_dumps()
                    first = cap.events("message.received")
                    self.assertEqual([m["payload"]["message"]["id"] for m in first], ["H1"])
                    self.assertTrue(all(m["payload"]["history"] for m in first))
                    self.assertIn(dump.name, s.state.imported_files)
                    self.assertNotIn(other.name, s.state.imported_files)
                    n_before = len(cap.out)
                    s._import_history_dumps()
                    self.assertEqual(len(cap.out), n_before)
                # open-chat refresh must not touch dumps at all
                with mock.patch.object(wa, "history_files", side_effect=AssertionError("dump parsed")):
                    with mock.patch.object(wa, "parse_history_conversations", side_effect=AssertionError("dump parsed")):
                        with Capture() as cap2:
                            s.refresh_chat(PHONE)
                self.assertEqual(cap2.events("chat.synced")[0]["payload"]["remote_id"], PHONE)


class HistoryMergeTests(unittest.TestCase):
    """`parse_history_conversations` merges dumps: one row per chat, one row per message id."""

    def _dumps(self, store: Path) -> tuple[Path, Path, Path]:
        older = store / "history-1700-628100000001:1@s.whatsapp.net-1-INITIAL_BOOTSTRAP.json"
        newer = store / "history-1700-628100000001:1@s.whatsapp.net-2-RECENT.json"
        broken = store / "history-1700-628100000001:1@s.whatsapp.net-3-PUSH_NAME.json"
        older.write_text(
            json.dumps(
                {
                    "conversations": [
                        {
                            "ID": PHONE,
                            "messages": [
                                text_wrap("M1", "older only"),
                                text_wrap("M2", "older copy"),
                                "not a wrap",
                            ],
                        },
                        {"ID": "status@broadcast", "messages": [text_wrap("S1", "a status")]},
                        {
                            "ID": "120363000000000000@broadcast",
                            "messages": [text_wrap("B1", "a broadcast")],
                        },
                        {"pnJID": "628222222222@s.whatsapp.net", "messages": []},
                    ]
                }
            )
        )
        newer.write_text(
            json.dumps(
                {
                    "conversations": [
                        {
                            "ID": PHONE,
                            "name": "Bob",
                            "conversationTimestamp": 1740823300,
                            "messages": [
                                text_wrap("M2", "newer copy"),
                                text_wrap("M3", "newer only"),
                                {"msgOrderID": "M4", "message": {"conversation": "no key id"}},
                                {
                                    "message": {
                                        "key": {"id": "M5"},
                                        "message": {"conversation": "lowercase id"},
                                    }
                                },
                                {"message": {"key": {"ID": "", "fromMe": False}}},
                            ],
                        }
                    ]
                }
            )
        )
        broken.write_text("{half a dump")
        return older, newer, broken

    def test_first_row_wins_and_the_later_dump_brings_its_messages(self):
        with tempfile.TemporaryDirectory() as d:
            older, newer, broken = self._dumps(Path(d))
            convs, readable = wa.parse_history_conversations([older, newer, broken])
            self.assertEqual([p.name for p in readable], [older.name, newer.name])
            self.assertEqual(
                [c.get("ID") or c.get("pnJID") for c in convs],
                [PHONE, "628222222222@s.whatsapp.net"],
            )
            merged = convs[0]
            # The later dump's copy of a message replaces the earlier one, and a message that
            # only the earlier dump had is not carried over (that is what the merge has always
            # produced). A wrap with no id of its own lands under its position in each of the two
            # passes the merge makes over a chat's messages, so it shows up twice.
            self.assertEqual(
                [w["message"].get("key", {}).get("ID") for w in merged["messages"]],
                ["M2", "M3", None, None, "", ""],
            )
            self.assertEqual(
                [w["message"].get("key", {}).get("id") for w in merged["messages"][2:4]],
                [None, "M5"],
            )
            self.assertEqual(
                merged["messages"][0]["message"]["message"]["conversation"], "newer copy"
            )
            self.assertEqual((merged["name"], merged["conversationTimestamp"]), ("Bob", 1740823300))


class GowaEnvironmentTests(unittest.TestCase):
    """The env the sidecar is started with: Go's heap limit is set, and an operator's wins."""

    def _start(self, extra_env: dict[str, str]) -> dict[str, str]:
        started: dict[str, dict[str, str]] = {}

        class FakeProc:
            pid = 4242

            def kill(self) -> None:
                pass

        def fake_popen(cmd, **kwargs):
            started["env"] = dict(kwargs["env"])
            return FakeProc()

        with tempfile.TemporaryDirectory() as d:
            with mock.patch.dict(os.environ, {"SHUTTLE_DATA_DIR": d, **extra_env}):
                for var in ("SHUTTLE_GOWA_URL", "GOMEMLIMIT"):
                    if var not in extra_env:
                        os.environ.pop(var, None)
                with (
                    mock.patch.object(wa, "find_gowa_binary", lambda: Path("/bin/false")),
                    mock.patch.object(wa, "_find_gowa_pids", lambda binary: []),
                    mock.patch.object(wa, "wait_http", lambda *a, **k: True),
                    mock.patch.object(wa.subprocess, "Popen", fake_popen),
                ):
                    wa.load_or_start_gowa()
        return started["env"]

    def test_the_sidecar_gets_a_go_heap_limit(self):
        self.assertEqual(self._start({})["GOMEMLIMIT"], wa.GOWA_MEMORY_LIMIT)

    def test_an_operator_value_wins(self):
        self.assertEqual(self._start({"GOMEMLIMIT": "128MiB"})["GOMEMLIMIT"], "128MiB")


class ChatSettingsTests(unittest.TestCase):
    def test_filtered_to_own_jid(self):
        with tempfile.TemporaryDirectory() as d:
            db = Path(d) / "whatsapp.db"
            con = sqlite3.connect(db)
            con.execute(
                "CREATE TABLE whatsmeow_chat_settings (our_jid TEXT, chat_jid TEXT, muted_until INTEGER, pinned INTEGER, archived INTEGER)"
            )
            con.executemany(
                "INSERT INTO whatsmeow_chat_settings VALUES (?,?,?,?,?)",
                [
                    ("628100000001:7@s.whatsapp.net", PHONE, 0, 1, 0),
                    (SELF_B, "628222@s.whatsapp.net", 0, 1, 1),
                ],
            )
            con.commit()
            con.close()
            a = wa.load_chat_settings({}, SELF_A, db)
            self.assertEqual(a[PHONE], {"pinned": True, "archived": False})
            self.assertNotIn("628222@s.whatsapp.net", a)
            b = wa.load_chat_settings({}, SELF_B, db)
            self.assertIn("628222@s.whatsapp.net", b)
            self.assertNotIn(PHONE, b)
            self.assertEqual(wa.load_chat_settings({}, None, db), {})


class StartChatTests(unittest.TestCase):
    """New > Contact: number clean-up, the on-WhatsApp check, and the saved-contact fact."""

    def test_typed_numbers_are_normalised(self):
        for raw in ("+974 8899 8899", "00974-8899-8899", " (974) 88998899 ", "97488998899"):
            self.assertEqual(wa.normalize_phone_number(raw), "97488998899", raw)
        for raw in ("", "abc", "12345", "1234567890123456", "+00"):
            self.assertIsNone(wa.normalize_phone_number(raw), raw)

    def _check(self, client, phone="+974 8899 8899"):
        s = make_session(client=client)
        with Capture() as cap:
            s.check_number(phone, "req-1")
        (ev,) = cap.events("number.checked")
        self.assertEqual(ev["account_id"], "acc-a")
        self.assertEqual(ev["payload"]["request_id"], "req-1")
        return client, ev["payload"]

    def test_check_number_reports_on_whatsapp(self):
        class Gowa(FakeGowa):
            def get(self, path, query=None, **kw):
                if path == "/user/check":
                    self.calls.append(("GET", path, query))
                    return 200, {"results": {"is_on_whatsapp": query["phone"] == "97488998899"}}
                return super().get(path, query, **kw)

        client, ok = self._check(Gowa())
        self.assertIs(ok["on_whatsapp"], True)
        self.assertEqual(ok["jid"], "97488998899@s.whatsapp.net")
        self.assertIn(("GET", "/user/check", {"phone": "97488998899"}), client.calls)
        _, no = self._check(Gowa(), "+974 1111 2222")
        self.assertIs(no["on_whatsapp"], False)

    def test_check_number_that_cannot_be_made_is_unknown_not_false(self):
        class Down(FakeGowa):
            def get(self, path, query=None, **kw):
                if path == "/user/check":
                    return 500, {}
                return super().get(path, query, **kw)

        _, out = self._check(Down())
        self.assertIsNone(out["on_whatsapp"])
        self.assertTrue(out["error"])
        _, bad = self._check(FakeGowa(), "12")
        self.assertIsNone(bad["on_whatsapp"])
        self.assertEqual(bad["error"], "invalid number")

    def test_saved_contacts_are_this_accounts_address_book_only(self):
        with tempfile.TemporaryDirectory() as d:
            db = Path(d) / "whatsapp.db"
            con = sqlite3.connect(db)
            con.execute(
                "CREATE TABLE whatsmeow_contacts (our_jid TEXT, their_jid TEXT, first_name TEXT, full_name TEXT, push_name TEXT)"
            )
            con.executemany(
                "INSERT INTO whatsmeow_contacts VALUES (?,?,?,?,?)",
                [
                    ("628100000001:7@s.whatsapp.net", PHONE, "Ann", "Ann Lee", "annie"),
                    ("628100000001:7@s.whatsapp.net", "628222@s.whatsapp.net", "", "", "Group Only"),
                    ("628100000001:7@s.whatsapp.net", "628333@s.whatsapp.net", None, None, None),
                    (SELF_B, "628444@s.whatsapp.net", "Other", "Other Account", ""),
                ],
            )
            con.commit()
            con.close()
            saved = wa.load_saved_contact_jids(SELF_A, db)
            self.assertIn(PHONE, saved)
            self.assertNotIn("628222@s.whatsapp.net", saved)
            self.assertNotIn("628333@s.whatsapp.net", saved)
            self.assertNotIn("628444@s.whatsapp.net", saved)
            self.assertIsNone(wa.load_saved_contact_jids(None, db))
            self.assertIsNone(wa.load_saved_contact_jids(SELF_A, Path(d) / "missing.db"))

    def test_contacts_synced_carries_the_saved_fact(self):
        contacts = {PHONE: "Ann", "628222@s.whatsapp.net": "Group Only"}
        with Capture() as cap:
            wa.emit_contacts_synced("acc-a", contacts, {PHONE})
            wa.emit_contacts_synced("acc-a", contacts, None)
        with_fact, without = (e["payload"]["contacts"] for e in cap.events("contacts.synced"))
        self.assertEqual({c["remote_id"]: c["saved"] for c in with_fact}, {PHONE: True, "628222@s.whatsapp.net": False})
        self.assertTrue(all("saved" not in c for c in without))


class MediaExtensionTests(unittest.TestCase):
    """Cached downloads always get an extension the OS and other apps can use."""

    def test_filename_suffix_wins_then_mime_then_magic_then_media_type(self):
        pick = wa.pick_media_extension
        self.assertEqual(pick("image", "Photo.PNG", "image/jpeg"), (".png", True))
        # GOWA names with no suffix fall through to the mime type.
        self.assertEqual(pick("image", "AC798AA7", "image/png"), (".png", True))
        self.assertEqual(pick("document", None, "application/pdf; charset=binary"), (".pdf", True))
        self.assertEqual(pick("image", None, None, b"\x89PNG\r\n\x1a\n....."), (".png", True))
        self.assertEqual(pick("image", None, "application/octet-stream", JPEG), (".jpg", True))
        self.assertEqual(pick("video", None, None), (".mp4", False))
        self.assertEqual(pick("sticker", "", None), (".webp", False))
        self.assertEqual(pick("document", None, None), (".bin", False))
        # A "suffix" that is really a long word is not an extension.
        self.assertEqual(pick("image", "holiday.notanextension", "image/webp"), (".webp", True))

    def test_magic_bytes(self):
        magic = wa._ext_from_magic
        self.assertEqual(magic(JPEG), ".jpg")
        self.assertEqual(magic(b"RIFF\x00\x00\x00\x00WEBPVP8 "), ".webp")
        self.assertEqual(magic(b"GIF89a\x01\x00"), ".gif")
        self.assertEqual(magic(b"\x00\x00\x00\x18ftypmp42\x00\x00"), ".mp4")
        self.assertEqual(magic(b"\x00\x00\x00\x14ftypqt  \x00\x00"), ".mov")
        self.assertEqual(magic(b"\x00\x00\x00\x18ftypheic\x00\x00"), ".heic")
        self.assertEqual(magic(b"OggS\x00\x02"), ".ogg")
        self.assertEqual(magic(b"%PDF-1.7"), ".pdf")
        self.assertEqual(magic(b"plain text"), "")

    def test_cache_file_uses_mime_when_filename_has_no_suffix(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(wa._account_files, {}, clear=True), \
                mock.patch.dict(os.environ, {"SHUTTLE_FILES_ROOT": tmp}):
            self.assertEqual(wa.media_cache_file("a", "ID1", "image", "ID1", "image/jpeg").name, "ID1.jpg")
            self.assertEqual(wa.media_cache_file("a", "ID1", "image", None, None).name, "ID1.jpg")
            self.assertEqual(wa.media_cache_file("a", "ID2", "document", "notes.txt").name, "ID2.txt")

    def _download(self, tmp, body, results, message_id="ID1"):
        client = mock.Mock()
        client.get.return_value = (200, {"results": {"file_url": "http://127.0.0.1:1/other/x", **results}})
        client.download_to_file.side_effect = lambda u, path, **kw: path.write_bytes(body)
        env = {"SHUTTLE_FILES_ROOT": tmp}
        with mock.patch.dict(wa._account_files, {}, clear=True), mock.patch.dict(os.environ, env):
            return wa.download_gowa_media(client, "acc", message_id, PHONE), client

    def test_download_without_a_filename_suffix_is_renamed_by_magic_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            got, _ = self._download(tmp, JPEG, {"media_type": "image", "filename": "ID1"})
            self.assertTrue(got["media_path"].endswith("ID1.jpg"), got)
            self.assertEqual(Path(got["media_path"]).read_bytes(), JPEG)
            # A video served with no hints and PNG bytes is named for what it is.
            got, _ = self._download(tmp, b"\x89PNG\r\n\x1a\n" + b"x" * 20, {"media_type": "image"}, "ID2")
            self.assertTrue(got["media_path"].endswith("ID2.png"), got)

    def test_download_uses_reported_mime_without_sniffing(self):
        with tempfile.TemporaryDirectory() as tmp:
            got, _ = self._download(tmp, b"not an image", {"media_type": "image", "mime_type": "image/webp"})
            self.assertTrue(got["media_path"].endswith("ID1.webp"), got)

    def test_second_download_reuses_a_renamed_or_extensionless_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            got, client = self._download(tmp, JPEG, {"media_type": "image"})
            first = got["media_path"]
            client.download_to_file.reset_mock()
            with mock.patch.dict(wa._account_files, {}, clear=True), mock.patch.dict(os.environ, {"SHUTTLE_FILES_ROOT": tmp}):
                again = wa.download_gowa_media(client, "acc", "ID1", PHONE)
            client.download_to_file.assert_not_called()
            self.assertEqual(again["media_path"], first)
            # An extensionless file left by an older build is adopted and given its extension.
            legacy = Path(first).with_name("OLD1")
            legacy.write_bytes(JPEG)
            with mock.patch.dict(wa._account_files, {}, clear=True), mock.patch.dict(os.environ, {"SHUTTLE_FILES_ROOT": tmp}):
                got = wa.download_gowa_media(client, "acc", "OLD1", PHONE)
            client.download_to_file.assert_not_called()
            self.assertEqual(got["media_path"], str(legacy) + ".jpg")
            self.assertFalse(legacy.exists())

    def test_find_cached_media_ignores_partial_and_other_ids(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(wa._account_files, {}, clear=True), \
                mock.patch.dict(os.environ, {"SHUTTLE_FILES_ROOT": tmp}):
            folder = wa.files_dir("acc") / "media"
            (folder / "AB.jpg.part").write_bytes(b"x")
            (folder / "ABC.jpg").write_bytes(b"x")
            self.assertIsNone(wa.find_cached_media("acc", "AB"))
            (folder / "AB.png").write_bytes(b"x")
            self.assertEqual(wa.find_cached_media("acc", "AB"), folder / "AB.png")


class FilesTests(unittest.TestCase):
    def test_per_account_folders(self):
        with tempfile.TemporaryDirectory() as d:
            with mock.patch.dict(os.environ, {"SHUTTLE_FILES_ROOT": d, "SHUTTLE_FILES_DIR": str(Path(d) / "x")}):
                a = wa.avatar_cache_file("acc/a", PHONE)
                b = wa.media_cache_file("acc b", "MID1", "image")
                self.assertEqual(a.parent, Path(d) / "acc_a" / "avatars")
                self.assertEqual(b.parent, Path(d) / "acc_b" / "media")
                self.assertEqual(wa.safe_dir_name("a.b-c_d é"), "a_b-c_d__")


class NamesAndIdentityTests(unittest.TestCase):
    def test_valid_names_and_titles(self):
        own = {"My Shop"}
        self.assertIsNone(wa.valid_chat_name("My Shop", PHONE, own))
        self.assertIsNone(wa.valid_chat_name("+62 811 1111", PHONE, own))
        self.assertIsNone(wa.valid_chat_name("628111111111", PHONE, own))
        self.assertIsNone(wa.valid_chat_name(PHONE, PHONE, own))
        self.assertIsNone(wa.valid_chat_name("+62 ∙∙∙ 11", PHONE, own))
        self.assertEqual(wa.valid_chat_name("Bob", PHONE, own), "Bob")
        self.assertEqual(wa.choose_title(PHONE, {"contact": "Bob", "push": "B"}), "Bob")
        self.assertEqual(wa.choose_title(PHONE, {"verified": "Acme", "push": "B"}), "Acme")
        self.assertEqual(wa.choose_title(PHONE, {}), "+628111111111")
        self.assertEqual(wa.choose_title(LID, {}), "999000111")
        self.assertEqual(wa.choose_title("1@g.us", {"subject": "Team"}, is_group=True), "Team")
        self.assertEqual(wa.choose_title("1@g.us", {}, is_group=True), "Group")
        self.assertEqual(wa.choose_title(SELF_A, {}, is_self=True), "Message yourself")
        self.assertEqual(wa.choose_title(SELF_A, {"contact": "Me"}, is_self=True), "Me (You)")

    def test_never_own_name_and_row_name_is_push(self):
        s = make_session(_self_name="My Shop", _self_aliases={"My Shop"})
        s._contacts = {PHONE: "Bobby", wa.jid_user_part(PHONE): "Bobby"}
        ref = s._chat_ref(PHONE)
        names = s._compose_names(ref, row={"name": "My Shop"})
        self.assertEqual(names, {"contact": "Bobby"})
        names = s._compose_names(ref, row={"name": "Robert B"})
        self.assertEqual(names, {"contact": "Bobby", "push": "Robert B"})
        # same as the saved contact name: not a push name
        ref2 = s._chat_ref("628222222222@s.whatsapp.net")
        s._contacts["628222222222@s.whatsapp.net"] = "Carl"
        self.assertEqual(s._compose_names(ref2, row={"name": "Carl"}), {"contact": "Carl"})
        # group subject comes from the GOWA chat name
        g = s._chat_ref("120@g.us")
        self.assertEqual(s._compose_names(g, row={"name": "Team"}), {"subject": "Team"})
        # a number is never a name
        self.assertEqual(s._compose_names(s._chat_ref("628333@s.whatsapp.net"), row={"name": "+62 833"}), {})

    def test_live_push_and_verified_merge(self):
        s = make_session()
        ref = s._chat_ref(PHONE)
        s._compose_names(ref, push="Bob Push")
        names = s._compose_names(ref, verified="Bob Biz")
        self.assertEqual(names, {"push": "Bob Push", "verified": "Bob Biz"})
        # a missing source never erases an earlier one
        self.assertEqual(s._compose_names(ref), names)

    def test_profile_lookup_emits_business_name_for_unsaved_chat(self):
        client = FakeGowa()
        original_get = client.get
        def get(path, query=None, **kwargs):
            if path == "/user/info":
                return 200, {"results": {"data": [{"verified_name": "Enjaz alosbah", "push_name": "Enjaz"}]}}
            if path == "/user/business-profile":
                return 200, {"results": {"data": {"description": "Business"}}}
            return original_get(path, query=query, **kwargs)
        client.get = get
        session = make_session(client=client)
        with Capture() as cap:
            session.fetch_contact_profile(PHONE)
        update = cap.events("conversation.updated")[-1]["payload"]
        self.assertEqual(update["names"]["verified"], "Enjaz alosbah")
        self.assertEqual(update["names"]["push"], "Enjaz")

    def test_contacts_failure_keeps_previous(self):
        s = make_session()
        s._contacts = {PHONE: "Bob"}
        s.client.contacts = None
        s._refresh_contacts()
        self.assertEqual(s._contacts, {PHONE: "Bob"})
        s.client.contacts = [{"jid": PHONE, "name": "Bobby"}]
        s._refresh_contacts()
        self.assertEqual(s._contacts[PHONE], "Bobby")

    def test_canonical_and_alt_ids_and_promotion(self):
        s = make_session()
        with Capture() as cap:
            ref = s._chat_ref(LID)
            self.assertEqual(ref["remote_id"], LID)
            s._emit_conversation(ref, {}, last_message_at=T0)
            first = cap.events("conversation.updated")[-1]["payload"]
            self.assertEqual(first["remote_id"], LID)
            self.assertNotIn("alt_remote_ids", first)
            # mapping becomes known (e.g. from a webhook carrying both forms)
            s._learn_lid(LID, PHONE)
            merged = cap.events("conversation.updated")[-1]["payload"]
        self.assertEqual(merged["remote_id"], PHONE)
        self.assertEqual(merged["alt_remote_ids"], [LID])
        self.assertEqual(merged["last_message_at"], T0)
        # a mapped @lid chat is canonicalised straight away, with the LID kept as alias
        ref2 = s._chat_ref(LID)
        self.assertEqual((ref2["remote_id"], ref2["alts"]), (PHONE, [LID]))
        # device suffix is stripped
        self.assertEqual(s._chat_ref("628111111111:5@s.whatsapp.net")["remote_id"], PHONE)

    def test_lid_map_reload_is_rate_limited(self):
        s = make_session()
        calls = []
        with mock.patch.object(wa, "load_lid_map", side_effect=lambda: calls.append(1) or {}):
            s._chat_ref("1@lid")
            s._chat_ref("2@lid")
            s._chat_ref("3@lid")
        self.assertEqual(len(calls), 1)

    def test_conversation_dedup(self):
        s = make_session()
        ref = s._chat_ref(PHONE)
        with Capture() as cap:
            self.assertTrue(s._emit_conversation(ref, {"contact": "Bob"}, last_message_at=T1))
            self.assertFalse(s._emit_conversation(ref, {"contact": "Bob"}, last_message_at=T1))
            # older time never rewinds
            self.assertFalse(s._emit_conversation(ref, {"contact": "Bob"}, last_message_at=T0))
            self.assertTrue(s._emit_conversation(ref, {"contact": "Bobby"}, last_message_at=T1))
            self.assertTrue(s._emit_conversation(ref, {"contact": "Bobby"}, last_message_at=T2, archived=True))
        self.assertEqual(len(cap.events("conversation.updated")), 3)
        p = cap.events("conversation.updated")[0]["payload"]
        self.assertEqual(p["names"], {"contact": "Bob"})
        self.assertIs(p["is_self"], False)
        self.assertEqual(p["title"], "Bob")


def message_webhook(session_id, device_jid, chat_id, mid, ts, body, from_name="Bob Push", **extra):
    payload = {
        "id": mid,
        "timestamp": ts,
        "is_from_me": False,
        "chat_id": chat_id,
        "from": chat_id,
        "from_name": from_name,
        "body": body,
    }
    payload.update(extra)
    return {"event": "message", "device_id": device_jid, "session_id": session_id, "payload": payload}


class WebhookTests(unittest.TestCase):
    def setUp(self):
        self.a = make_session("acc-a", SELF_A)
        self.b = make_session("acc-b", SELF_B)
        self.sessions = {"acc-a": self.a, "acc-b": self.b}

    def test_routing(self):
        r = wa.route_webhook
        self.assertIs(r({"session_id": "acc-b"}, self.sessions), self.b)
        self.assertIs(r({"device_id": "628100000001:4@s.whatsapp.net"}, self.sessions), self.a)
        self.assertIsNone(r({"device_id": "6289999@s.whatsapp.net"}, self.sessions))
        self.assertIsNone(r({}, self.sessions))
        self.assertIsNone(r({"session_id": "nope"}, self.sessions))
        self.assertIs(r({}, {"acc-a": self.a}), self.a)

    def test_message_goes_to_right_account(self):
        payload = message_webhook(
            "acc-b", SELF_B, PHONE, "M1", T1, "hello", chat_lid=LID, from_lid=LID, verified_name="Bob Biz"
        )
        with Capture() as cap:
            session = wa.route_webhook(payload, self.sessions)
            session._handle_ws(payload)
        self.assertIs(session, self.b)
        self.assertTrue(cap.out)
        self.assertTrue(all(m["account_id"] == "acc-b" for m in cap.out))
        conv, msg = cap.out[0], cap.out[1]
        self.assertEqual(conv["event"], "conversation.updated")
        cp = conv["payload"]
        self.assertEqual(cp["remote_id"], PHONE)
        self.assertEqual(cp["alt_remote_ids"], [LID])
        self.assertEqual(cp["names"], {"push": "Bob Push", "verified": "Bob Biz"})
        self.assertEqual(cp["last_message_at"], T1)
        self.assertFalse(cp["history"])
        self.assertEqual(cp["title"], "Bob Biz")
        self.assertEqual(msg["event"], "message.received")
        mp = msg["payload"]
        self.assertEqual(mp["conversation_id"], PHONE)
        self.assertEqual(mp["alt_remote_ids"], [LID])
        self.assertFalse(mp["history"])
        self.assertEqual(mp["message"]["id"], "M1")
        self.assertEqual(mp["message"]["timestamp"], T1)
        self.assertEqual(mp["message"]["text"], "hello")
        self.assertEqual(mp["message"]["sender_name"], "Bob Push")
        # nothing was written for the other account
        self.assertEqual(self.a._chat_cache, {})

    def test_own_messages_do_not_set_push_and_bad_rows_dropped(self):
        own = message_webhook("acc-a", SELF_A, PHONE, "M2", T1, "mine", from_name="My Name", is_from_me=True)
        own["payload"]["from"] = SELF_A
        noid = message_webhook("acc-a", SELF_A, PHONE, "", T1, "x")
        nots = message_webhook("acc-a", SELF_A, PHONE, "M3", "0001-01-01T00:00:00Z", "x")
        with Capture() as cap:
            self.a._handle_ws(own)
            self.a._handle_ws(noid)
            self.a._handle_ws(nots)
        convs = cap.events("conversation.updated")
        self.assertEqual(convs[0]["payload"]["names"], {})
        self.assertEqual([m["event"] for m in cap.out if m["event"].startswith("message.")], ["message.sent"])

    def test_chat_read_from_self_receipt(self):
        def ack(frm, rtype="read"):
            return {
                "event": "message.ack",
                "timestamp": T2,
                "device_id": SELF_A,
                "session_id": "acc-a",
                "payload": {"chat_id": PHONE, "chat_lid": LID, "from": frm, "receipt_type": rtype, "ids": ["M1"]},
            }

        with Capture() as cap:
            self.a._handle_ws(ack("628100000001@s.whatsapp.net"))
            self.a._handle_ws(ack("628100000001@s.whatsapp.net", "read-self"))
            self.a._handle_ws(ack(PHONE))  # the contact read our message: not a self-read
            self.a._handle_ws(ack("628100000001@s.whatsapp.net", "delivered"))
        reads = cap.events("chat.read")
        self.assertEqual(len(reads), 2)
        self.assertEqual(
            reads[0]["payload"],
            {"remote_id": PHONE, "alt_remote_ids": [LID], "read_through": T2},
        )
        self.assertEqual(reads[0]["account_id"], "acc-a")


class CatchUpTests(unittest.TestCase):
    def test_cursor_second_cycle_silent_then_new_message_once(self):
        s = make_session()
        g = s.client
        g.chats = [{"jid": PHONE, "name": "Bob", "last_message_time": T1}]
        g.messages[PHONE] = [make_row("A", T0), make_row("B", T1)]
        s._listed.add(PHONE)
        s.state.advance_cursor(PHONE, T1)  # as if the connect sync / backfill already emitted these
        s._tail[PHONE] = (wa._epoch(T1), {"B"})
        with Capture() as cap:
            s._catch_up_locked()
            first_cycle = list(cap.out)
            cap.out.clear()
            s._catch_up_locked()
        self.assertFalse([m for m in first_cycle if m["event"].startswith("message.")])
        self.assertEqual(cap.out, [], "second cycle with no new data must emit nothing")
        # a new message lands
        g.chats[0]["last_message_time"] = T2
        g.messages[PHONE].append(make_row("C", T2, "new"))
        with Capture() as cap:
            s._catch_up_locked()
            msgs = cap.events("message.received")
            self.assertEqual([m["payload"]["message"]["id"] for m in msgs], ["C"])
            self.assertFalse(msgs[0]["payload"]["history"])
            self.assertTrue(cap.events("inbox.catchup"))
            n = len(cap.out)
            s._catch_up_locked()
            self.assertEqual(len(cap.out), n, "the same message is emitted once")
        self.assertEqual(s.state.cursor_for([PHONE]), T2)
        # only newer messages were requested
        msg_calls = [q for (_, p, q) in g.calls if p.endswith("/messages") and "status" not in p]
        self.assertTrue(all(q.get("start_time") for q in msg_calls))

    def test_missed_message_in_the_cursor_second_is_recovered_by_sweep(self):
        s = make_session()
        g = s.client
        g.chats = [{"jid": PHONE, "name": "Bob", "last_message_time": T1}]
        # B was delivered live; B2 arrived in the same second but its webhook was missed.
        g.messages[PHONE] = [make_row("B", T1), make_row("B2", T1, "missed")]
        s._listed.add(PHONE)
        s.state.advance_cursor(PHONE, T1)
        s._tail[PHONE] = (wa._epoch(T1), {"B"})
        with Capture() as cap:
            s._catch_up_locked()
            self.assertFalse(cap.events("message.received"), "chat time did not move: no fetch")
            s._catchup_cycles = s.SWEEP_EVERY - 1
            s._catch_up_locked()
            ids = [m["payload"]["message"]["id"] for m in cap.events("message.received")]
            self.assertEqual(ids, ["B2"])
            s._catchup_cycles = s.SWEEP_EVERY - 1
            s._catch_up_locked()
            ids = [m["payload"]["message"]["id"] for m in cap.events("message.received")]
        self.assertEqual(ids, ["B2"], "a later sweep does not emit it again")

    def test_failed_fetch_does_not_move_the_cursor(self):
        s = make_session()
        g = s.client
        g.chats = [{"jid": PHONE, "name": "Bob", "last_message_time": T2}]
        g.messages[PHONE] = [make_row("C", T2)]
        s._listed.add(PHONE)
        s.state.advance_cursor(PHONE, T1)
        real_get = g.get

        def failing_get(path, **kw):
            if path.endswith("/messages"):
                return 500, {"message": "boom"}
            return real_get(path, **kw)

        with Capture() as cap:
            with mock.patch.object(g, "get", failing_get):
                s._catch_up_locked()
            self.assertFalse(cap.events("message.received"))
            self.assertEqual(s.state.cursor_for([PHONE]), T1)
            s._catch_up_locked()
            ids = [m["payload"]["message"]["id"] for m in cap.events("message.received")]
        self.assertEqual(ids, ["C"])

    def test_new_chat_is_fetched_once(self):
        s = make_session()
        g = s.client
        g.chats = [{"jid": PHONE, "name": "Bob", "last_message_time": T1}]
        g.messages[PHONE] = [make_row("A", T1)]
        with Capture() as cap:
            s._catch_up_locked()
            ids = [m["payload"]["message"]["id"] for m in cap.events("message.received")]
            s._catch_up_locked()
            ids2 = [m["payload"]["message"]["id"] for m in cap.events("message.received")]
        self.assertEqual(ids, ["A"])
        self.assertEqual(ids2, ["A"])

    def test_chat_awaiting_backfill_is_not_fetched(self):
        s = make_session()
        g = s.client
        g.chats = [{"jid": PHONE, "name": "Bob", "last_message_time": T1}]
        g.messages[PHONE] = [make_row("A", T1)]
        s._listed.add(PHONE)
        with Capture() as cap:
            s._catch_up_locked()
        self.assertFalse(cap.events("message.received"))
        self.assertFalse([c for c in g.calls if c[1].endswith("/messages") and "status" not in c[1]])


class SyncStateTests(unittest.TestCase):
    def test_persist_roundtrip_corrupt_and_missing(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "x" / "sync-state.json"
            st = wa.SyncState(path)  # missing file is fine
            st.advance_cursor("a", T1)
            st.advance_cursor("a", T0)  # never rewinds
            st.mark_backfilled(["a"])
            st.mark_imported(["f.json"])
            st.save()
            again = wa.SyncState(path)
            self.assertEqual(again.cursor_for(["a"]), T1)
            self.assertTrue(again.is_backfilled(["a"]))
            self.assertIn("f.json", again.imported_files)
            path.write_text("{not json")
            broken = wa.SyncState(path)  # corrupt file is tolerated
            self.assertIsNone(broken.cursor_for(["a"]))
            self.assertFalse(list(path.parent.glob("*.tmp")))

    def test_backfill_runs_once(self):
        s = make_session()
        g = s.client
        g.messages[PHONE] = [make_row("A", T0), make_row("B", T1)]
        ref = s._chat_ref(PHONE)
        with Capture() as cap, mock.patch.object(wa.time, "sleep"):
            s._backfill_run([ref])
            self.assertEqual(len(cap.events("message.received")), 2)
            self.assertTrue(all(m["payload"]["history"] for m in cap.events("message.received")))
            s._start_backfill([ref])
            thread = s._backfill_thread
            self.assertIsNone(thread)  # nothing left to do


class ProtocolTests(unittest.TestCase):
    def run_requests(self, session, reqs):
        d = wa.Dispatcher()
        d.sessions[session.account_id] = session
        with Capture() as cap:
            for r in reqs:
                d.handle(r, session.account_id)
        return cap

    def test_account_id_on_every_reply_and_send_contract(self):
        g = FakeGowa()
        g.messages[PHONE] = [make_row("A", T0)]
        s = make_session(client=g)
        reqs = [
            {"type": "send_message", "account_id": "acc-a", "conversation_id": PHONE, "text": "hi", "request_id": "R1"},
            {"type": "send_message", "account_id": "acc-a", "conversation_id": PHONE, "text": "hi"},
            {"type": "send_attachment", "account_id": "acc-a", "conversation_id": PHONE, "kind": "image", "request_id": "R2"},
            {"type": "send_attachment", "account_id": "acc-a", "conversation_id": PHONE, "kind": "image", "data_base64": "aGk=", "request_id": "R3"},
            {"type": "mark_read", "account_id": "acc-a", "conversation_id": PHONE, "message_id": "M9"},
            {"type": "mark_read", "account_id": "acc-a", "conversation_id": PHONE},
            {"type": "sync_chat", "account_id": "acc-a", "conversation_id": PHONE},
            {"type": "get_status", "account_id": "acc-a"},
            {"type": "submit_auth", "account_id": "acc-a"},
            {"type": "create_group", "account_id": "acc-a", "title": "", "participants": []},
            {"type": "download_media", "account_id": "acc-a", "conversation_id": PHONE, "message_id": ""},
            {"type": "fetch_contact_profile", "account_id": "acc-a", "conversation_id": PHONE},
        ]
        with mock.patch.object(s, "device_status", return_value={"is_logged_in": True}):
            cap = self.run_requests(s, reqs)
        self.assertTrue(cap.out)
        for m in cap.out:
            self.assertEqual(m.get("account_id"), "acc-a", m)
        oks = [m for m in cap.out if m["type"] == "ok"]
        self.assertEqual(
            oks[0],
            {"type": "ok", "account_id": "acc-a", "request_id": "R1", "message_id": "WAMID1", "conversation_id": PHONE},
        )
        self.assertEqual(oks[1]["request_id"], "WAMID1")  # legacy request without request_id
        err = [m for m in cap.out if m["type"] == "error" and m.get("request_id") == "R2"]
        self.assertEqual(len(err), 1)
        self.assertEqual(oks[2]["request_id"], "R3")
        self.assertEqual(oks[2]["message_id"], "WAMID1")
        # mark_read used the message id from the request, and only then
        read_posts = [p for p in g.posts if p[0].endswith("/read")]
        self.assertEqual(read_posts, [("/message/M9/read", {"phone": PHONE})])

    def test_send_failure_carries_request_id(self):
        g = FakeGowa()
        g.post_code = 500
        s = make_session(client=g)
        cap = self.run_requests(
            s, [{"type": "send_message", "account_id": "acc-a", "conversation_id": PHONE, "text": "x", "request_id": "R7"}]
        )
        self.assertEqual(len(cap.out), 1)
        self.assertEqual(cap.out[0]["type"], "error")
        self.assertEqual(cap.out[0]["request_id"], "R7")
        self.assertEqual(cap.out[0]["account_id"], "acc-a")

    def test_unexpected_exception_replies_with_ids(self):
        s = make_session()
        d = wa.Dispatcher()
        d.sessions["acc-a"] = s
        with mock.patch.object(s, "send_text", side_effect=RuntimeError("kaput")):
            with Capture() as cap:
                w = wa.AccountWorker("acc-a", d.handle)
                w.submit({"type": "send_message", "account_id": "acc-a", "conversation_id": PHONE, "text": "x", "request_id": "R8"})
                deadline = time.time() + 3
                while not cap.out and time.time() < deadline:
                    time.sleep(0.01)
                w.close()
        self.assertEqual(cap.out[0]["request_id"], "R8")
        self.assertEqual(cap.out[0]["account_id"], "acc-a")

    def test_slow_account_does_not_delay_another(self):
        release = threading.Event()
        started = threading.Event()

        class Slow:
            account_id = "slow"
            client = object()
            _connected = True

            def download_media(self, jid, mid):
                started.set()
                release.wait(5)
                wa.send({"type": "event", "event": "media.downloaded", "account_id": "slow", "payload": {}})

            def shutdown(self):
                pass

        class Fast:
            account_id = "fast"
            client = object()
            _connected = True

            def emit_avatar(self, jid):
                wa.send({"type": "event", "event": "avatar.updated", "account_id": "fast", "payload": {}})

            def shutdown(self):
                pass

        d = wa.Dispatcher()
        d.sessions.update({"slow": Slow(), "fast": Fast()})
        with Capture() as cap:
            d.dispatch({"type": "download_media", "account_id": "slow", "conversation_id": "x", "message_id": "m"})
            self.assertTrue(started.wait(2))
            t0 = time.time()
            d.dispatch({"type": "fetch_avatar", "account_id": "fast", "conversation_id": "x"})
            deadline = t0 + 2
            while not [m for m in cap.out if m.get("account_id") == "fast" and m["type"] == "ok"] and time.time() < deadline:
                time.sleep(0.01)
            fast_done = [m for m in cap.out if m.get("account_id") == "fast" and m["type"] == "ok"]
            self.assertTrue(fast_done, "fast account was blocked by the slow one")
            self.assertLess(time.time() - t0, 1.5)
            self.assertFalse([m for m in cap.out if m.get("account_id") == "slow"])
            release.set()
            deadline = time.time() + 3
            while not [m for m in cap.out if m.get("account_id") == "slow"] and time.time() < deadline:
                time.sleep(0.01)
        self.assertTrue([m for m in cap.out if m.get("account_id") == "slow"])
        d.shutdown_all()

    def test_handshake_and_missing_account(self):
        d = wa.Dispatcher()
        with Capture() as cap:
            self.assertTrue(d.dispatch({"type": "handshake"}))
            self.assertTrue(d.dispatch({"type": "connect"}))
            self.assertFalse(d.dispatch({"type": "shutdown"}))
        self.assertEqual(cap.out[0]["type"], "handshake_ok")
        self.assertEqual(cap.out[1]["type"], "error")

    def test_send_is_serialized(self):
        lines: list[str] = []

        class Out:
            def write(self, s):
                lines.append(s)

            def flush(self):
                pass

        with mock.patch.object(sys, "stdout", Out()):
            threads = [
                threading.Thread(target=lambda i=i: [wa.send({"type": "ok", "n": i}) for _ in range(50)])
                for i in range(6)
            ]
            [t.start() for t in threads]
            [t.join() for t in threads]
        self.assertEqual(len(lines), 300)
        for line in lines:
            self.assertTrue(line.endswith("\n"))
            json.loads(line)


class ConnectSyncTests(unittest.TestCase):
    def test_connect_sync_flags_and_no_legacy_fields(self):
        g = FakeGowa()
        g.chats = [
            {"jid": PHONE, "name": "Bob", "last_message_time": T1, "archived": False},
            {"jid": "120@g.us", "name": "Team", "last_message_time": T0},
        ]
        g.messages[PHONE] = [make_row("A", T0), make_row("B", T1)]
        s = make_session(client=g)
        with mock.patch.object(wa, "load_self_profile", return_value=(SELF_A, "Me", {"Me"})), mock.patch.object(
            wa, "load_lid_map", return_value={}
        ), mock.patch.object(wa, "load_chat_settings", return_value={}), mock.patch.object(
            wa, "history_files", return_value=[]
        ), mock.patch.object(wa.time, "sleep"), mock.patch.object(s, "_prefetch_avatars"):
            with Capture() as cap:
                s.sync_history(force=True)
                if s._backfill_thread:
                    s._backfill_thread.join(5)
        order = [m["event"] for m in cap.events()]
        self.assertEqual(order[0], "history.sync.started")
        self.assertIn("history.sync.completed", order)
        for m in cap.events("conversation.updated"):
            p = m["payload"]
            self.assertIn("names", p)
            self.assertIn("is_self", p)
            for banned in ("force_recency", "replace_title", "list_rank", "unread_count"):
                self.assertNotIn(banned, p)
        for m in cap.events("message.received"):
            self.assertTrue(m["payload"]["history"])
        # conversation precedes the first message of the chat
        first_msg = next(i for i, m in enumerate(cap.out) if m.get("event") == "message.received")
        first_conv = next(
            i for i, m in enumerate(cap.out) if m.get("event") == "conversation.updated" and m["payload"]["remote_id"] == PHONE
        )
        self.assertLess(first_conv, first_msg)
        self.assertEqual(s.state.cursor_for([PHONE]), T1)
        self.assertTrue(s.state.is_backfilled([PHONE]))
        group = [m for m in cap.events("conversation.updated") if m["payload"]["remote_id"] == "120@g.us"][0]["payload"]
        self.assertEqual(group["names"], {"subject": "Team"})
        self.assertEqual(group["conversation_type"], "group")

    def test_status_feed_only_when_changed(self):
        g = FakeGowa()
        g.messages["status@broadcast"] = [
            make_row("S1", T1, "story", sender="628555@s.whatsapp.net", chat_jid="status@broadcast")
        ]
        s = make_session(client=g)
        with Capture() as cap:
            s._emit_status_feed()
            s._emit_status_feed()
            g.messages["status@broadcast"].append(
                make_row("S2", T2, "story2", sender="628555@s.whatsapp.net", chat_jid="status@broadcast")
            )
            s._emit_status_feed()
        self.assertEqual(len(cap.events("status.feed")), 2)


GROUP = "120363000000@g.us"
DAD = "628222222222@s.whatsapp.net"
DAD_LID = "171339147124812@lid"


def reaction_webhook(device_jid, chat_id, target, emoji, frm, ts=T1, **extra):
    payload = {
        "id": "R-" + str(abs(hash((target, emoji, frm, ts)))),
        "chat_id": chat_id,
        "from": frm,
        "from_name": "Dad Push",
        "timestamp": ts,
        "reaction": emoji,
        "reacted_message_id": target,
    }
    payload.update(extra)
    return {"event": "message.reaction", "device_id": device_jid, "payload": payload}


class ReactionTests(unittest.TestCase):
    def setUp(self):
        patcher = mock.patch.object(wa, "load_lid_map", return_value={})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.a = make_session("acc-a", SELF_A)
        self.b = make_session("acc-b", SELF_B)

    def test_live_reaction_is_a_fact_that_carries_the_account(self):
        hook = reaction_webhook(SELF_A, GROUP, "MSG1", "\u2764\ufe0f", DAD)
        with Capture() as cap:
            self.a._handle_ws(hook)
        # Only a reaction event: no chat row change, no message.
        self.assertEqual([m["event"] for m in cap.events()], ["reaction"])
        ev = cap.events("reaction")[0]
        self.assertEqual(ev["account_id"], "acc-a")
        p = ev["payload"]
        self.assertEqual(p["conversation_id"], GROUP)
        self.assertEqual(p["remote_id"], GROUP)
        self.assertFalse(p["history"])
        (fact,) = p["reactions"]
        self.assertEqual(fact["target_message_id"], "MSG1")
        self.assertEqual(fact["reactor_id"], DAD)
        self.assertEqual(fact["reactor_name"], "Dad Push")
        self.assertEqual(fact["emoji"], "\u2764\ufe0f")
        self.assertEqual(fact["timestamp"], T1)
        self.assertFalse(fact["from_me"])

    def test_replace_and_remove_are_reported_with_the_same_reactor(self):
        with Capture() as cap:
            self.a._handle_ws(reaction_webhook(SELF_A, GROUP, "MSG1", "\U0001f44d", DAD, ts=T0))
            self.a._handle_ws(reaction_webhook(SELF_A, GROUP, "MSG1", "\u2764\ufe0f", DAD, ts=T1))
            self.a._handle_ws(reaction_webhook(SELF_A, GROUP, "MSG1", "", DAD, ts=T2))
        facts = [e["payload"]["reactions"][0] for e in cap.events("reaction")]
        self.assertEqual([f["emoji"] for f in facts], ["\U0001f44d", "\u2764\ufe0f", ""])
        self.assertEqual({f["reactor_id"] for f in facts}, {DAD})
        self.assertEqual([f["timestamp"] for f in facts], [T0, T1, T2])

    def test_own_reaction_uses_the_me_reactor(self):
        hook = reaction_webhook(SELF_A, PHONE, "MSG9", "\U0001f64f", SELF_A, is_from_me=True)
        with Capture() as cap:
            self.a._handle_ws(hook)
        (fact,) = cap.events("reaction")[0]["payload"]["reactions"]
        self.assertEqual((fact["reactor_id"], fact["reactor_name"], fact["from_me"]), ("me", "You", True))
        # The same reaction relayed without the flag is still recognised as ours.
        hook2 = reaction_webhook(SELF_A, PHONE, "MSG9", "\U0001f64f", "628100000001:7@s.whatsapp.net")
        with Capture() as cap2:
            self.a._handle_ws(hook2)
        self.assertTrue(cap2.events("reaction")[0]["payload"]["reactions"][0]["from_me"])

    def test_lid_reactor_is_mapped_to_the_phone_and_named_from_contacts(self):
        self.a._contacts = {"628222222222": "Dad"}
        hook = reaction_webhook(
            SELF_A, GROUP, "MSG1", "\U0001f602", DAD_LID, from_name="", from_lid=DAD_LID
        )
        hook["payload"]["from"] = DAD
        with Capture() as cap:
            self.a._handle_ws(hook)
        (fact,) = cap.events("reaction")[0]["payload"]["reactions"]
        self.assertEqual(fact["reactor_id"], DAD)
        self.assertEqual(fact["reactor_name"], "Dad")

    def test_reaction_goes_to_its_own_account_and_bad_ones_are_dropped(self):
        sessions = {"acc-a": self.a, "acc-b": self.b}
        hook = reaction_webhook(SELF_B, GROUP, "MSG1", "\u2764\ufe0f", DAD)
        with Capture() as cap:
            wa.route_webhook(hook, sessions)._handle_ws(hook)
            self.a._handle_ws(reaction_webhook(SELF_A, GROUP, "", "\u2764\ufe0f", DAD))  # no target
            self.a._handle_ws({"event": "message.reaction", "payload": {"chat_id": GROUP, "reacted_message_id": "M"}})
        events = cap.events("reaction")
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["account_id"], "acc-b")

    def test_dump_reaction_extraction(self):
        wrap = {
            "message": {
                "key": {"remoteJID": GROUP, "fromMe": False, "ID": "MSG1"},
                "message": {"conversation": "photo"},
                "messageTimestamp": 1740823200,
                "participant": DAD_LID,
                "reactions": [
                    {"key": {"remoteJID": GROUP, "fromMe": False, "ID": "R1", "participant": DAD_LID},
                     "text": "\u2764\ufe0f", "senderTimestampMS": 1740823300000},
                    {"key": {"remoteJID": GROUP, "fromMe": True, "ID": "R2"},
                     "text": "\U0001f44d", "senderTimestampMS": 1740823400000},
                    {"key": {"remoteJID": GROUP, "fromMe": False, "ID": "R3", "participant": DAD}, "text": ""},
                ],
            }
        }
        target, rows = wa.history_message_reactions(wrap)
        self.assertEqual(target, "MSG1")
        self.assertEqual(
            [(r["jid"], r["from_me"], r["emoji"]) for r in rows],
            [(DAD_LID, False, "\u2764\ufe0f"), (None, True, "\U0001f44d")],
        )
        self.assertEqual(wa.history_message_reactions({"message": {"key": {"ID": "X"}}}), (None, []))

    def test_dump_import_reports_reactions_and_the_real_group_author(self):
        dump_name = "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
        conv = {
            "ID": GROUP,
            "name": "Family",
            "messages": [
                {
                    "message": {
                        "key": {"remoteJID": GROUP, "fromMe": False, "ID": "H1"},
                        "message": {"conversation": "hello all"},
                        "messageTimestamp": 1740823200,
                        "participant": DAD_LID,
                        "reactions": [
                            {"key": {"remoteJID": GROUP, "fromMe": False, "ID": "R1", "participant": DAD_LID},
                             "text": "\u2764\ufe0f", "senderTimestampMS": 1740823300000},
                        ],
                    }
                }
            ],
        }
        with tempfile.TemporaryDirectory() as d:
            store = Path(d)
            (store / dump_name).write_text(json.dumps({"conversations": [conv]}))
            real_hf = wa.history_files
            self.a._lid_map = {DAD_LID: DAD}
            self.a._contacts = {"628222222222": "Dad"}
            with mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    self.a._import_history_dumps()
        msg = cap.events("message.received")[0]["payload"]["message"]
        # The author, not the group JID.
        self.assertEqual(msg["sender_id"], DAD)
        self.assertEqual(msg["sender_name"], "Dad")
        ev = cap.events("reaction")[0]
        self.assertTrue(ev["payload"]["history"])
        (fact,) = ev["payload"]["reactions"]
        self.assertEqual((fact["target_message_id"], fact["reactor_id"], fact["emoji"]), ("H1", DAD, "\u2764\ufe0f"))
        self.assertEqual(ev["account_id"], "acc-a")
        self.assertIn(dump_name, self.a.state.reaction_files)

    def test_dump_imported_by_an_older_build_is_repaired_once(self):
        dump_name = "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
        conv = {
            "ID": GROUP,
            "messages": [
                {"message": {
                    "key": {"remoteJID": GROUP, "fromMe": False, "ID": "H1"},
                    "message": {"conversation": "hello all"},
                    "messageTimestamp": 1740823200,
                    "participant": DAD,
                    "reactions": [{"key": {"remoteJID": GROUP, "fromMe": False, "participant": DAD_LID},
                                   "text": "\U0001f44d", "senderTimestampMS": 1740823300000}],
                }},
                {"message": {  # my own message: nothing to repair
                    "key": {"remoteJID": GROUP, "fromMe": True, "ID": "H2"},
                    "message": {"conversation": "mine"},
                    "messageTimestamp": 1740823210,
                }},
            ],
        }
        with tempfile.TemporaryDirectory() as d:
            store = Path(d)
            (store / dump_name).write_text(json.dumps({"conversations": [conv]}))
            real_hf = wa.history_files
            self.a.state.mark_imported([dump_name])  # imported before reactions existed
            with mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    self.a._import_history_dumps()
                    first = len(cap.out)
                    self.a._import_history_dumps()
                    self.assertEqual(len(cap.out), first, "repair must run once per file")
        msgs = [m["payload"]["message"] for m in cap.events("message.received")]
        self.assertEqual([(m["id"], m["sender_id"]) for m in msgs], [("H1", DAD)])
        self.assertEqual(len(cap.events("reaction")), 1)
        self.assertIn(dump_name, self.a.state.reaction_files)

    def test_live_group_sender_with_source_suffix_is_cleaned(self):
        hook = message_webhook(
            "acc-a", SELF_A, GROUP, "GM1", T1, "hi group", from_name="Dad Push",
            **{"from": f"{DAD} in {GROUP}"},
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        msg = cap.events("message.received")[0]["payload"]["message"]
        self.assertEqual(msg["sender_id"], DAD)
        self.assertEqual(msg["sender_name"], "Dad Push")

    def test_history_sender_is_the_participant_never_the_group(self):
        base = {"key": {"remoteJID": GROUP, "fromMe": False, "ID": "H1"}, "message": {"conversation": "hi"},
                "messageTimestamp": 1740823200}
        self.assertEqual(wa.history_message_fields({"message": {**base, "participant": DAD_LID}})["sender_jid"], DAD_LID)
        self.assertIsNone(wa.history_message_fields({"message": base})["sender_jid"])
        direct = {**base, "key": {"remoteJID": PHONE, "fromMe": False, "ID": "H2"}}
        self.assertEqual(wa.history_message_fields({"message": direct})["sender_jid"], PHONE)


class MentionAndCaptionTests(unittest.TestCase):
    """`@<digits>` mentions are reported as facts; names and mime types are never captions."""

    def setUp(self):
        self.lid_map = {DAD_LID: DAD, "171339147124812": DAD}
        patcher = mock.patch.object(wa, "load_lid_map", return_value=self.lid_map)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.a = make_session("acc-a", SELF_A, _lid_map=dict(self.lid_map))

    def test_tokens_need_a_standalone_number(self):
        self.assertEqual(
            wa.mention_tokens("hi @97433279001 and @171339147124812, mail a@12345678 @123 @@999999"),
            ["97433279001", "171339147124812"],
        )

    def test_history_mentioned_jids_skip_the_quoted_message(self):
        wrap = {"message": {
            "key": {"remoteJID": GROUP, "fromMe": False, "ID": "H1", "participant": DAD_LID},
            "message": {"extendedTextMessage": {"text": "ping @171339147124812", "contextInfo": {
                "mentionedJID": [DAD_LID],
                "quotedMessage": {"extendedTextMessage": {"contextInfo": {"mentionedJID": ["1@lid"]}}},
            }}},
            "messageTimestamp": 1740823200,
        }}
        fields = wa.history_message_fields(wrap)
        self.assertEqual(fields["mentioned_jids"], [DAD_LID])

    def test_lid_mention_is_resolved_to_the_phone_jid(self):
        facts = self.a._mention_facts("hey @171339147124812 and @97433279001", [DAD_LID])
        self.assertEqual(facts, [{"token": "171339147124812", "jid": DAD}])

    def test_lid_without_metadata_is_resolved_from_the_lid_map(self):
        hook = message_webhook(
            "acc-a", SELF_A, GROUP, "GM1", T1, "@171339147124812 look", from_name="Eve",
            **{"from": f"{DAD} in {GROUP}"},
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        msg = cap.events("message.received")[0]["payload"]["message"]
        self.assertEqual(msg["mentions"], [{"token": "171339147124812", "jid": DAD}])

    def test_mention_of_this_account_is_marked_self(self):
        facts = self.a._mention_facts("@628100000001 are you in?", ["628100000001@s.whatsapp.net"])
        self.assertEqual(facts, [{"token": "628100000001", "jid": SELF_A, "self": True}])

    def test_no_mentions_key_without_tokens(self):
        with Capture() as cap:
            self.a._handle_ws(message_webhook("acc-a", SELF_A, GROUP, "GM2", T1, "plain", **{"from": f"{DAD} in {GROUP}"}))
        self.assertNotIn("mentions", cap.events("message.received")[0]["payload"]["message"])

    def test_media_without_a_caption_never_takes_the_senders_name_as_body(self):
        row = {"media_type": "image", "text": "Alam Thakur", "from_name": "Alam Thakur", "filename": "a.jpg"}
        text, extra = wa.message_body_and_media(row)
        self.assertEqual((text, extra["media_type"]), ("[image]", "image"))
        text, _ = wa.message_body_and_media({"media_type": "audio", "content": "audio/ogg; codecs=opus"})
        self.assertEqual(text, "[audio]")
        text, _ = wa.message_body_and_media({"media_type": "document", "content": "a.pdf", "filename": "a.pdf"})
        self.assertEqual(text, "[document]")
        # A file hash is not a caption.
        for blob in ("nDXT4hDDZalrGuLwhkDIwABBzwR5YkjTGaJcVPOhQJs=", "9f86d081884c7d659a2feaa0c55ad015"):
            text, _ = wa.message_body_and_media({"media_type": "image", "content": blob})
            self.assertEqual(text, "[image]", blob)
        # The message's own timestamp is not a caption either.
        for stamp in ("2026-11-24T07:09:13Z", "2026-11-24 07:09:13+00:00"):
            text, _ = wa.message_body_and_media({"media_type": "image", "content": stamp})
            self.assertEqual(text, "[image]", stamp)
        # A real caption stays.
        text, _ = wa.message_body_and_media({"media_type": "image", "content": "Look at this", "from_name": "Bob"})
        self.assertEqual(text, "Look at this")

    def test_push_name_in_a_history_media_message_is_not_a_caption(self):
        wrap = {"message": {
            "key": {"remoteJID": GROUP, "fromMe": False, "ID": "H2", "participant": DAD_LID},
            "pushName": "Dad Push",
            "message": {"imageMessage": {"mimetype": "image/jpeg", "fileSha256": "abc"}},
            "messageTimestamp": 1740823200,
        }}
        self.assertEqual(wa.history_message_fields(wrap)["text"], "[image]")


class PlaceholderBodyTests(unittest.TestCase):
    """A sender's verification facts are never the message text, and live media keeps its kind.

    GOWA's webhook reports the sender's WhatsApp verification level (`verified_level`, whose
    default value is the literal `unknown`) next to the message, and it names the media by
    type (`video`, `image`, ...) instead of a `media_type` field.
    """

    def setUp(self):
        patcher = mock.patch.object(wa, "load_lid_map", return_value={})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.a = make_session("acc-a", SELF_A)

    def test_sender_verification_level_is_not_a_message_body(self):
        self.assertEqual(wa.nested_text({"verified_level": "unknown", "is_business": True}), "")

    def test_live_video_is_media_and_keeps_its_caption(self):
        hook = message_webhook(
            "acc-a", SELF_A, GROUP, "GM1", T1, "3 BHK, monthly",
            **{"from": f"{DAD} in {GROUP}", "video": {"url": "https://mmg.whatsapp.net/x"}},
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        msg = cap.events("message.received")[0]["payload"]["message"]
        self.assertEqual((msg["text"], msg["media_type"]), ("3 BHK, monthly", "video"))

    def test_live_captionless_video_never_takes_the_verification_level_as_its_body(self):
        hook = {
            "event": "message",
            "device_id": SELF_A,
            "session_id": "acc-a",
            "payload": {
                "id": "GM2",
                "timestamp": T1,
                "chat_id": GROUP,
                "from": f"{DAD} in {GROUP}",
                "from_name": "An Agency",
                "is_business": True,
                "verified_name": "An Agency",
                "verified_level": "unknown",
                "video": {"url": "https://mmg.whatsapp.net/x"},
            },
        }
        with Capture() as cap:
            self.a._handle_ws(hook)
        msg = cap.events("message.received")[0]["payload"]["message"]
        self.assertEqual((msg["text"], msg["media_type"]), ("[video]", "video"))

    def test_dump_media_keeps_its_kind_and_its_caption(self):
        with tempfile.TemporaryDirectory() as d:
            store = Path(d)
            dump = store / "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
            dump.write_text(
                json.dumps(
                    {
                        "conversations": [
                            {
                                "ID": PHONE,
                                "messages": [
                                    {
                                        "message": {
                                            "key": {"ID": "H1", "fromMe": False},
                                            "message": {"videoMessage": {"caption": "Tour"}},
                                            "messageTimestamp": 1740823200,
                                        }
                                    },
                                    {
                                        "message": {
                                            "key": {"ID": "H2", "fromMe": False},
                                            "message": {"videoMessage": {}},
                                            "messageTimestamp": 1740823220,
                                        }
                                    },
                                ],
                            }
                        ]
                    }
                )
            )
            real_hf = wa.history_files
            with mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    self.a._import_history_dumps()
        got = {m["payload"]["message"]["id"]: m["payload"]["message"] for m in cap.events("message.received")}
        self.assertEqual((got["H1"]["text"], got["H1"]["media_type"]), ("Tour", "video"))
        self.assertEqual((got["H2"]["text"], got["H2"]["media_type"]), ("[video]", "video"))


class ReplyTests(unittest.TestCase):
    """A reply is reported as a `reply_to` fact on the message; a send can carry the target."""

    def setUp(self):
        patcher = mock.patch.object(wa, "load_lid_map", return_value={})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.a = make_session("acc-a", SELF_A)

    # ---- extraction

    def test_rest_row_context_metadata_is_a_json_string(self):
        row = make_row("R1", T1, "answer", context_metadata=json.dumps({"replied_to_id": "ORIG1"}))
        self.assertEqual(wa.reply_to_from_row(row), {"remote_id": "ORIG1"})
        self.assertIsNone(wa.reply_to_from_row(make_row("R2", T1, "plain", context_metadata="")))
        self.assertIsNone(wa.reply_to_from_row(make_row("R3", T1, "plain", context_metadata="{}")))

    def test_live_payload_keys(self):
        self.assertEqual(
            wa.reply_to_from_row({"replied_to_id": "O1", "quoted_body": "  the   old\ntext "}),
            {"remote_id": "O1", "snippet": "the old text"},
        )
        # Older GOWA nested the ids under `message`.
        self.assertEqual(
            wa.reply_to_from_row({"message": {"replied_id": "O2", "quoted_message": "old"}}),
            {"remote_id": "O2", "snippet": "old"},
        )
        self.assertIsNone(wa.reply_to_from_row({"body": "hi", "quoted_body": "orphan text"}))
        long = wa.reply_to_from_row({"replied_to_id": "O3", "quoted_body": "x" * 500})
        self.assertEqual(len(long["snippet"]), 160)

    def test_dump_context_info(self):
        inner = {
            "extendedTextMessage": {
                "text": "my answer",
                "contextInfo": {
                    "stanzaID": "ORIG9",
                    "participant": DAD_LID,
                    "quotedMessage": {"conversation": "the question"},
                },
            }
        }
        self.assertEqual(
            wa.reply_to_from_dump(inner),
            {"remote_id": "ORIG9", "sender": DAD_LID, "snippet": "the question"},
        )
        media = {
            "imageMessage": {
                "caption": "pic",
                "contextInfo": {"stanzaID": "ORIG8", "quotedMessage": {"imageMessage": {"caption": "sunset"}}},
            }
        }
        self.assertEqual(
            wa.reply_to_from_dump(media),
            {"remote_id": "ORIG8", "snippet": "sunset", "media_type": "image"},
        )
        # No stanza id, or only the quoted message's own contextInfo: not a reply.
        self.assertIsNone(wa.reply_to_from_dump({"extendedTextMessage": {"text": "x", "contextInfo": {}}}))
        nested_only = {
            "extendedTextMessage": {
                "text": "x",
                "contextInfo": {"quotedMessage": {"extendedTextMessage": {"contextInfo": {"stanzaID": "DEEP"}}}},
            }
        }
        self.assertIsNone(wa.reply_to_from_dump(nested_only))

    def test_quoted_text_is_never_the_body(self):
        row = make_row(
            "M1", T1, "", media_type="image", context_metadata=json.dumps({"replied_to_id": "ORIG1"})
        )
        text, extra = wa.message_body_and_media(row)
        self.assertEqual((text, extra["media_type"]), ("[image]", "image"))
        live = {"id": "M2", "media_type": "image", "replied_to_id": "O", "quoted_body": "old words"}
        text, _ = wa.message_body_and_media(live)
        self.assertEqual(text, "[image]")

    # ---- reported on every path

    def test_rest_rows_report_reply_to(self):
        g = FakeGowa()
        g.messages[PHONE] = [
            make_row("A", T0, "question", from_me=True),
            make_row("B", T1, "answer", context_metadata=json.dumps({"replied_to_id": "A"})),
        ]
        s = make_session(client=g)
        with Capture() as cap:
            s._sync_messages(s._chat_ref(PHONE), 50, history=True)
        by_id = {m["payload"]["message"]["id"]: m["payload"]["message"] for m in cap.events()
                 if m["event"].startswith("message.")}
        self.assertEqual(by_id["B"]["reply_to"], {"remote_id": "A"})
        self.assertNotIn("reply_to", by_id["A"])

    def test_live_webhook_reports_reply_to_with_the_account(self):
        hook = message_webhook(
            "acc-a", SELF_A, PHONE, "L1", T1, "yes", replied_to_id="ORIG", quoted_body="coming?"
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        ev = cap.events("message.received")[0]
        self.assertEqual(ev["payload"]["message"]["reply_to"], {"remote_id": "ORIG", "snippet": "coming?"})
        self.assertEqual(ev["account_id"], "acc-a")
        # The preview of the chat is the answer, not the quote.
        self.assertEqual(cap.events("conversation.updated")[0]["payload"]["preview"], "yes")

    def test_dump_message_reports_reply_to_and_marks_own_author(self):
        wrap = {"message": {
            "key": {"remoteJID": PHONE, "fromMe": False, "ID": "H5"},
            "message": {"extendedTextMessage": {"text": "re", "contextInfo": {
                "stanzaID": "MINE1", "participant": SELF_A, "quotedMessage": {"conversation": "my words"}}}},
            "messageTimestamp": 1740823200,
        }}
        fields = wa.history_message_fields(wrap)
        self.assertEqual(fields["reply_to"]["remote_id"], "MINE1")
        with Capture() as cap:
            self.a._emit_history_messages(self.a._chat_ref(PHONE), "Bob", [fields])
        reply = cap.events("message.received")[0]["payload"]["message"]["reply_to"]
        self.assertEqual(
            reply, {"remote_id": "MINE1", "sender": SELF_A, "snippet": "my words", "from_me": True}
        )

    def test_dump_lid_author_is_mapped_to_the_phone(self):
        self.a._lid_map = {DAD_LID: DAD}
        fact = self.a._reply_fact({"remote_id": "X", "sender": DAD_LID})
        self.assertEqual(fact, {"remote_id": "X", "sender": DAD})

    # ---- one-time passes for messages stored before replies were reported

    def test_dump_imported_before_replies_is_repaired_once(self):
        dump_name = "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
        conv = {"ID": PHONE, "name": "Bob", "messages": [
            {"message": {"key": {"remoteJID": PHONE, "fromMe": False, "ID": "H1"},
                         "message": {"conversation": "plain"}, "messageTimestamp": 1740823200}},
            {"message": {"key": {"remoteJID": PHONE, "fromMe": False, "ID": "H2"},
                         "message": {"extendedTextMessage": {"text": "re", "contextInfo": {
                             "stanzaID": "H1", "quotedMessage": {"conversation": "plain"}}}},
                         "messageTimestamp": 1740823260}},
        ]}
        with tempfile.TemporaryDirectory() as d:
            store = Path(d)
            (store / dump_name).write_text(json.dumps({"conversations": [conv]}))
            real_hf = wa.history_files
            self.a.state.mark_imported([dump_name])
            self.a.state.mark_reactions_imported([dump_name])  # only replies are missing
            with mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    self.a._import_history_dumps()
                    first = len(cap.out)
                    self.a._import_history_dumps()
                    self.assertEqual(len(cap.out), first, "repair must run once per file")
        msgs = [m["payload"]["message"] for m in cap.events("message.received")]
        self.assertEqual([(m["id"], m["reply_to"]["remote_id"]) for m in msgs], [("H2", "H1")])
        # Messages only: the chat's own row (title, time, preview) is not reported again.
        self.assertEqual(cap.events("conversation.updated"), [])
        self.assertIn(dump_name, self.a.state.reply_files)

    def test_fresh_dump_import_needs_no_repair_pass(self):
        dump_name = "history-1700-628100000001:1@s.whatsapp.net-1-RECENT.json"
        conv = {"ID": PHONE, "messages": [
            {"message": {"key": {"remoteJID": PHONE, "fromMe": False, "ID": "H2"},
                         "message": {"extendedTextMessage": {"text": "re", "contextInfo": {"stanzaID": "H1"}}},
                         "messageTimestamp": 1740823260}},
        ]}
        with tempfile.TemporaryDirectory() as d:
            store = Path(d)
            (store / dump_name).write_text(json.dumps({"conversations": [conv]}))
            real_hf = wa.history_files
            with mock.patch.object(wa, "history_files", lambda own, st=None: real_hf(own, store)):
                with Capture() as cap:
                    self.a._import_history_dumps()
        self.assertEqual(len(cap.events("message.received")), 1)
        self.assertIn(dump_name, self.a.state.reply_files)

    def test_rest_reply_pass_reports_only_replies_once_per_chat(self):
        g = FakeGowa()
        g.messages[PHONE] = [
            make_row("A", T0, "question"),
            make_row("B", T1, "answer", context_metadata=json.dumps({"replied_to_id": "A"})),
        ]
        s = make_session(client=g)
        ref = s._chat_ref(PHONE)
        s.state.mark_backfilled(ref["all"])
        with Capture() as cap:
            s._start_reply_backfill([ref])
            s._reply_thread.join(5)
        msgs = [m["payload"]["message"] for m in cap.events("message.received")]
        self.assertEqual([(m["id"], m["reply_to"]["remote_id"]) for m in msgs], [("B", "A")])
        self.assertEqual(cap.events("conversation.updated"), [])
        self.assertTrue(s.state.replies_checked(ref["all"]))
        with Capture() as cap2:
            s._start_reply_backfill([ref])
        self.assertEqual(cap2.out, [])

    def test_sync_state_remembers_reply_passes(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "sync-state.json"
            st = wa.SyncState(path)
            st.mark_replies_imported(["dump.json"])
            st.mark_replies_checked([PHONE])
            st.save()
            again = wa.SyncState(path)
            self.assertEqual(again.reply_files, {"dump.json"})
            self.assertTrue(again.replies_checked([PHONE]))

    # ---- sending

    def test_send_text_carries_reply_message_id(self):
        g = FakeGowa()
        s = make_session(client=g)
        with Capture():
            s.send_text(PHONE, "yes", "R1", "ORIG1")
            s.send_text(PHONE, "no reply", "R2")
        self.assertEqual(g.posts[0], ("/send/message", {"phone": PHONE, "message": "yes", "reply_message_id": "ORIG1"}))
        self.assertEqual(g.posts[1], ("/send/message", {"phone": PHONE, "message": "no reply"}))

    def test_dispatch_send_message_with_reply_target(self):
        g = FakeGowa()
        s = make_session(client=g)
        d = wa.Dispatcher()
        d.sessions["acc-a"] = s
        with Capture():
            d.handle(
                {"type": "send_message", "account_id": "acc-a", "conversation_id": GROUP, "text": "hi",
                 "request_id": "R1", "reply_to_id": "ORIG", "reply_to_sender": DAD},
                "acc-a",
            )
        self.assertEqual(g.posts[0][1]["reply_message_id"], "ORIG")
        self.assertNotIn("participant", g.posts[0][1])

    def test_attachment_sends_forward_the_reply_id(self):
        import base64

        g = FakeGowa()
        s = make_session(client=g)
        raw = base64.b64encode(b"x").decode()
        with Capture():
            s.send_attachment(PHONE, {"kind": "image", "data_base64": raw, "filename": "a.jpg", "mime": "image/jpeg",
                                      "caption": "c", "reply_to_id": "ORIG"}, "R1")
            s.send_attachment(PHONE, {"kind": "document", "data_base64": raw, "filename": "a.pdf"}, "R2")
        self.assertEqual(g.posts[0][0], "/send/image")
        self.assertEqual(g.posts[0][1]["reply_message_id"], "ORIG")
        self.assertEqual(g.posts[2][0], "/send/file")
        self.assertNotIn("reply_message_id", g.posts[2][1])

    def test_forwarded_sends_carry_gowas_forwarded_flag(self):
        import base64

        g = FakeGowa()
        s = make_session(client=g)
        d = wa.Dispatcher()
        d.sessions["acc-a"] = s
        raw = base64.b64encode(b"x").decode()
        with Capture():
            d.handle({"type": "send_message", "account_id": "acc-a", "conversation_id": PHONE, "text": "fwd",
                      "request_id": "R1", "forwarded": True}, "acc-a")
            d.handle({"type": "send_message", "account_id": "acc-a", "conversation_id": PHONE, "text": "own",
                      "request_id": "R2"}, "acc-a")
            s.send_attachment(PHONE, {"kind": "video", "data_base64": raw, "filename": "v.mp4", "mime": "video/mp4",
                                      "caption": "clip", "forwarded": True}, "R3")
            s.send_attachment(PHONE, {"kind": "image", "data_base64": raw, "filename": "a.jpg"}, "R4")
        self.assertEqual(g.posts[0], ("/send/message", {"phone": PHONE, "message": "fwd", "is_forwarded": True}))
        self.assertEqual(g.posts[1], ("/send/message", {"phone": PHONE, "message": "own"}))
        self.assertEqual(g.posts[2][0], "/send/video")
        self.assertEqual(g.posts[2][1]["is_forwarded"], "true")
        self.assertEqual(g.posts[2][1]["caption"], "clip")
        self.assertNotIn("is_forwarded", g.posts[4][1])

    def test_a_forward_reads_the_cached_file_by_path(self):
        g = FakeGowa()
        s = make_session(client=g)
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "cached.jpg"
            path.write_bytes(b"jpeg-bytes")
            files = []
            g.post_multipart = lambda p, fields, f=None, timeout=60.0: (files.append((p, f)), (200, {"results": {"message_id": "W"}}))[1]
            with Capture():
                s.send_attachment(PHONE, {"kind": "image", "path": str(path), "filename": "cached.jpg",
                                          "mime": "image/jpeg", "forwarded": True}, "R1")
        self.assertEqual(files[0][0], "/send/image")
        self.assertEqual(files[0][1]["image"][1], b"jpeg-bytes")


if __name__ == "__main__":
    unittest.main()


class FakeGowaServer:
    """A real HTTP server on localhost answering GOWA's /message/:id/<route> calls."""

    def __init__(self) -> None:
        import http.server

        self.requests: list[dict] = []
        self.answers: dict[str, tuple[int, dict]] = {}
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b""
                outer.requests.append(
                    {
                        "path": self.path,
                        "device": self.headers.get("X-Device-Id"),
                        "auth": self.headers.get("Authorization"),
                        "body": json.loads(raw) if raw else None,
                    }
                )
                route = self.path.rsplit("/", 1)[-1]
                code, body = outer.answers.get(
                    route, (200, {"code": "SUCCESS", "results": {"message_id": "X", "status": "ok"}})
                )
                data = json.dumps(body).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, *a):
                pass

        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.httpd.server_address[1]}"
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def close(self) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()


class MessageOpTests(unittest.TestCase):
    def setUp(self):
        self.gowa = FakeGowaServer()
        self.addCleanup(self.gowa.close)
        self.s = make_session(client=wa.GowaClient(self.gowa.url, ("shuttle", "pw"), "acc-a"))
        self.d = wa.Dispatcher()
        self.d.sessions["acc-a"] = self.s

    def op(self, op, mid="WAMID 1", rid="op-1", account="acc-a"):
        req = {
            "type": "message_op",
            "account_id": account,
            "conversation_id": PHONE,
            "message_id": mid,
            "op": op,
            "request_id": rid,
        }
        with Capture() as cap:
            self.d.handle(req, account)
        for m in cap.out:
            self.assertEqual(m.get("account_id"), account, m)
        return cap

    def assert_ok_last(self, cap, rid="op-1"):
        self.assertEqual(cap.out[-1], {"type": "ok", "account_id": "acc-a", "request_id": rid})

    def test_reaction_add_and_remove_is_reported_as_my_reaction(self):
        cap = self.op({"kind": "react", "emoji": "👍"})
        req = self.gowa.requests[-1]
        self.assertEqual(req["path"], "/message/WAMID%201/reaction")
        self.assertEqual(req["body"], {"phone": PHONE, "emoji": "👍"})
        self.assertEqual(req["device"], "acc-a")
        self.assertTrue(req["auth"].startswith("Basic "))
        [ev] = cap.events("reaction")
        fact = ev["payload"]["reactions"][0]
        self.assertEqual(ev["payload"]["remote_id"], PHONE)
        self.assertFalse(ev["payload"]["history"])
        self.assertEqual(
            {k: fact[k] for k in ("target_message_id", "reactor_id", "emoji", "from_me")},
            {"target_message_id": "WAMID 1", "reactor_id": "me", "emoji": "👍", "from_me": True},
        )
        self.assert_ok_last(cap)

        cap = self.op({"kind": "react", "emoji": None}, rid="op-2")
        self.assertEqual(self.gowa.requests[-1]["body"], {"phone": PHONE, "emoji": ""})
        self.assertEqual(cap.events("reaction")[0]["payload"]["reactions"][0]["emoji"], "")
        self.assert_ok_last(cap, "op-2")

    def test_edit_revoke_and_delete_hit_their_routes_and_report_one_change_fact(self):
        cases = [
            ({"kind": "edit", "text": "fixed"}, "update", {"phone": PHONE, "message": "fixed"}, "edited"),
            ({"kind": "delete_for_everyone"}, "revoke", {"phone": PHONE}, "revoked"),
            ({"kind": "delete_for_me"}, "delete", {"phone": PHONE}, "deleted_for_me"),
        ]
        for op, route, body, kind in cases:
            with self.subTest(route=route):
                cap = self.op(op, mid="M1")
                req = self.gowa.requests[-1]
                self.assertEqual(req["path"], f"/message/M1/{route}")
                self.assertEqual(req["body"], body)
                [ev] = cap.events("message.changed")
                p = ev["payload"]
                self.assertEqual((p["remote_id"], p["message_id"], p["change"]["kind"]), (PHONE, "M1", kind))
                if kind == "edited":
                    self.assertEqual(p["change"]["text"], "fixed")
                if kind != "deleted_for_me":
                    self.assertTrue(wa.to_rfc3339_strict(p["change"]["at"]))
                self.assert_ok_last(cap)

    def test_rejected_by_whatsapp_is_an_error_for_that_request_and_no_fact(self):
        self.gowa.answers["update"] = (500, {"code": "INTERNAL_SERVER_ERROR", "message": "message too old to edit"})
        cap = self.op({"kind": "edit", "text": "late"}, rid="op-9")
        self.assertEqual(len(cap.out), 1)
        err = cap.out[0]
        self.assertEqual((err["type"], err["request_id"]), ("error", "op-9"))
        self.assertIn("message too old to edit", err["message"])
        self.assertIn("500", err["message"])

        self.gowa.answers["revoke"] = (400, {"code": "VALIDATION_ERROR", "message": "phone: cannot be blank."})
        cap = self.op({"kind": "delete_for_everyone"}, rid="op-10")
        self.assertEqual([m["type"] for m in cap.out], ["error"])
        self.assertEqual(cap.out[0]["request_id"], "op-10")

    def test_bad_requests_never_reach_gowa(self):
        for op in ({"kind": "edit", "text": "  "}, {"kind": "forward"}, {}):
            with self.subTest(op=op):
                cap = self.op(op, rid="op-x")
                self.assertEqual([(m["type"], m["request_id"]) for m in cap.out], [("error", "op-x")])
        cap = self.op({"kind": "react", "emoji": "👍"}, mid="", rid="op-y")
        self.assertEqual(cap.out[0]["type"], "error")
        self.assertEqual(self.gowa.requests, [])

    def test_not_connected_and_missing_account_reply_with_the_request_id(self):
        with Capture() as cap:
            wa.Dispatcher().handle(
                {"type": "message_op", "account_id": "acc-z", "op": {"kind": "delete_for_me"}, "request_id": "op-3"},
                "acc-z",
            )
            wa.Dispatcher().dispatch({"type": "message_op", "op": {"kind": "delete_for_me"}, "request_id": "op-4"})
        self.assertEqual(cap.out[0], {"type": "error", "message": "not connected", "account_id": "acc-z", "request_id": "op-3"})
        self.assertEqual((cap.out[1]["type"], cap.out[1]["request_id"]), ("error", "op-4"))

    def test_unreachable_gowa_still_answers_the_request(self):
        self.s.client = wa.GowaClient("http://127.0.0.1:9", ("u", "p"), "acc-a")
        with Capture() as cap:
            w = wa.AccountWorker("acc-a", self.d.handle)
            w.submit({"type": "message_op", "account_id": "acc-a", "conversation_id": PHONE,
                      "message_id": "M1", "op": {"kind": "delete_for_me"}, "request_id": "op-5"})
            deadline = time.time() + 5
            while not cap.out and time.time() < deadline:
                time.sleep(0.01)
            w.close()
        self.assertEqual((cap.out[0]["type"], cap.out[0]["request_id"], cap.out[0]["account_id"]), ("error", "op-5", "acc-a"))


class IncomingChangeTests(unittest.TestCase):
    def setUp(self):
        self.a = make_session("acc-a", SELF_A)

    def hook(self, event, **payload):
        return {"event": event, "device_id": SELF_A, "session_id": "acc-a", "timestamp": T2, "payload": payload}

    def test_phone_edits_revokes_and_deletes_become_change_facts(self):
        with Capture() as cap:
            self.a._handle_ws(self.hook("message.edited", id="EV1", chat_id=PHONE, chat_lid=LID,
                                        original_message_id="M1", body="new text"))
            self.a._handle_ws(self.hook("message.revoked", id="EV2", chat_id=PHONE, revoked_message_id="M2",
                                        revoked_from_me=False, revoked_chat=PHONE))
            self.a._handle_ws(self.hook("message.deleted", chat_id=PHONE, deleted_message_id="M3"))
        self.assertTrue(all(m["account_id"] == "acc-a" for m in cap.out))
        self.assertEqual([m["event"] for m in cap.out], ["message.changed"] * 3)
        edited, revoked, deleted = (m["payload"] for m in cap.out)
        self.assertEqual(edited["message_id"], "M1")
        self.assertEqual(edited["alt_remote_ids"], [LID])
        self.assertEqual(edited["change"], {"kind": "edited", "text": "new text", "at": T2})
        self.assertEqual(revoked["message_id"], "M2")
        self.assertEqual(revoked["change"], {"kind": "revoked", "at": T2})
        self.assertEqual((deleted["message_id"], deleted["change"]), ("M3", {"kind": "deleted_for_me"}))

    def test_an_edit_is_never_a_new_message_and_incomplete_changes_are_dropped(self):
        with Capture() as cap:
            self.a._handle_ws(self.hook("message.edited", id="M1", chat_id=PHONE, body="x"))
            self.a._handle_ws(self.hook("message.edited", chat_id=PHONE, original_message_id="M1", body=""))
            self.a._handle_ws(self.hook("message.revoked", revoked_message_id="M2"))
            self.a._handle_ws(self.hook("message.deleted", chat_id="status@broadcast", deleted_message_id="S1"))
        self.assertEqual(cap.out, [])


class GroupSendFactsTests(unittest.TestCase):
    """A group only admins may post to reaches the core as a fact about the chat."""

    GROUP = "120363427536439238@g.us"

    def facts(self, group, self_jid=SELF_A):
        s = make_session(self_jid=self_jid)
        s.client.groups = [group]
        with Capture() as cap:
            s._emit_conversation(s._chat_ref(self.GROUP), {})
            s._refresh_metadata(force=True)
        chat = [x["payload"] for x in cap.events("conversation.updated") if x["payload"]["remote_id"] == self.GROUP]
        return chat[-1]

    @staticmethod
    def group(**kw):
        entry = {"JID": GroupSendFactsTests.GROUP, "Name": "Community"}
        entry.update(kw)
        return entry

    def test_announce_group_where_this_account_is_not_an_admin_is_locked(self):
        admin = {"JID": LID, "LID": LID, "PhoneNumber": PHONE, "IsAdmin": True, "IsSuperAdmin": False}
        payload = self.facts(self.group(IsAnnounce=True, Participants=[admin]))
        self.assertTrue(payload["announce_only"])
        self.assertFalse(payload["self_is_admin"])

    def test_an_admin_match_on_the_lid_or_the_phone_jid_keeps_the_composer_open(self):
        by_lid = {"JID": LID, "LID": SELF_A.replace("@s.whatsapp.net", "@lid"), "PhoneNumber": "",
                  "IsAdmin": False, "IsSuperAdmin": True}
        payload = self.facts(self.group(IsAnnounce=True, Participants=[by_lid]))
        self.assertEqual((payload["announce_only"], payload["self_is_admin"]), (True, True))
        by_phone = {"JID": LID, "LID": "", "PhoneNumber": SELF_A, "IsAdmin": True, "IsSuperAdmin": False}
        payload = self.facts(self.group(IsAnnounce=True, Participants=[by_phone]))
        self.assertTrue(payload["self_is_admin"])

    def test_a_group_open_to_members_reports_an_unlocked_chat(self):
        payload = self.facts(self.group(IsAnnounce=False, IsLocked=True, Participants=[]))
        self.assertEqual((payload["announce_only"], payload["self_is_admin"]), (False, False))

    def test_unknown_group_info_reports_no_fact_at_all(self):
        for group in (self.group(IsLocked=True), self.group(IsAnnounce=True), self.group(IsAnnounce=True, Participants=[])):
            payload = self.facts(group)
            self.assertNotIn("announce_only", payload)
            self.assertNotIn("self_is_admin", payload)

    def test_the_fact_never_rides_on_a_direct_chat(self):
        s = make_session(self_jid=SELF_A)
        s._group_send[PHONE] = (True, False)
        with Capture() as cap:
            s._emit_conversation(s._chat_ref(PHONE), {})
        self.assertNotIn("announce_only", cap.events("conversation.updated")[0]["payload"])


def poll_payload(kind, poll_id, question="", options=(), selectable=1, selected_names=None, selected_hashes=None):
    """A GOWA `poll` object as the webhook carries it (infrastructure/whatsapp/poll_event.go)."""
    poll = {
        "type": kind,
        "poll_id": poll_id,
        "question": question,
        "options": [{"name": name, "hash": oid} for name, oid in options],
        "selectable_options_count": selectable,
    }
    if selected_names is not None:
        poll["selected_options"] = list(selected_names)
    if selected_hashes is not None:
        poll["selected_option_hashes"] = list(selected_hashes)
    if kind == "vote":
        poll["resolution_status"] = "resolved"
    return poll


OPTIONS = (("Pizza", "hash-pizza"), ("Sushi", "hash-sushi"), ("Tacos", "hash-tacos"))


class PollTests(unittest.TestCase):
    """A poll is one message; a vote is folded into it and never becomes a message."""

    def setUp(self):
        patcher = mock.patch.object(wa, "load_lid_map", return_value={})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.a = make_session("acc-a", SELF_A)

    def test_a_poll_creation_is_one_message_with_its_options(self):
        poll = poll_payload("creation", "POLL1", "Where shall we eat?", OPTIONS, selectable=1)
        hook = message_webhook(
            "acc-a", SELF_A, GROUP, "POLL1", T0, "Poll: Where shall we eat?", poll=poll
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        self.assertEqual(cap.events("poll.changed"), [])
        (ev,) = cap.events("message.received")
        msg = ev["payload"]["message"]
        self.assertEqual(msg["media_type"], "poll")
        self.assertEqual(msg["text"], "Where shall we eat?", "the question is the body")
        self.assertEqual(msg["preview"], "Poll: Where shall we eat?")
        self.assertEqual(
            msg["poll"],
            {
                "question": "Where shall we eat?",
                "multiple": False,
                "options": [
                    {"id": "hash-pizza", "text": "Pizza"},
                    {"id": "hash-sushi", "text": "Sushi"},
                    {"id": "hash-tacos", "text": "Tacos"},
                ],
            },
        )

    def test_a_vote_is_a_fact_and_never_a_message(self):
        poll = poll_payload(
            "vote", "POLL1", "Where shall we eat?", OPTIONS,
            selected_names=["Sushi"], selected_hashes=["hash-sushi"],
        )
        hook = message_webhook(
            "acc-a", SELF_A, GROUP, "VOTE1", T1, "Poll vote: Sushi", poll=poll, **{"from": DAD}
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        self.assertEqual(cap.events("message.received"), [], "a vote is not a chat message")
        self.assertEqual(cap.events("message.sent"), [])
        (ev,) = cap.events("poll.changed")
        p = ev["payload"]
        self.assertEqual(p["remote_id"], GROUP)
        self.assertEqual(p["poll_id"], "POLL1")
        self.assertEqual(p["kind"], "vote")
        self.assertEqual(p["vote"]["voter_id"], DAD)
        self.assertEqual(p["vote"]["selected"], ["hash-sushi"])
        self.assertFalse(p["vote"]["from_me"])
        self.assertEqual(p["vote"]["timestamp"], T1)
        self.assertEqual([o["id"] for o in p["poll"]["options"]], [o[1] for o in OPTIONS])

    def test_my_own_vote_is_the_me_voter(self):
        poll = poll_payload(
            "vote", "POLL1", "Where shall we eat?", OPTIONS,
            selected_names=["Pizza"], selected_hashes=["hash-pizza"],
        )
        hook = message_webhook(
            "acc-a", SELF_A, PHONE, "VOTE2", T1, "Poll vote: Pizza", poll=poll, is_from_me=True
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        (ev,) = cap.events("poll.changed")
        self.assertEqual((ev["payload"]["vote"]["voter_id"], ev["payload"]["vote"]["voter_name"]), ("me", "You"))

    def test_a_vote_without_hashes_falls_back_to_the_option_text(self):
        poll = poll_payload("vote", "POLL1", "Where shall we eat?", OPTIONS, selected_names=["Tacos"])
        poll.pop("selected_option_hashes", None)
        hook = message_webhook(
            "acc-a", SELF_A, GROUP, "VOTE3", T1, "", poll=poll, **{"from": DAD}
        )
        with Capture() as cap:
            self.a._handle_ws(hook)
        (ev,) = cap.events("poll.changed")
        self.assertEqual(ev["payload"]["vote"]["selected"], ["hash-tacos"])

    def test_a_vote_with_no_selection_clears_it(self):
        poll = poll_payload("vote", "POLL1", "Where shall we eat?", OPTIONS, selected_names=[], selected_hashes=[])
        hook = message_webhook("acc-a", SELF_A, GROUP, "VOTE4", T1, "", poll=poll, **{"from": DAD})
        with Capture() as cap:
            self.a._handle_ws(hook)
        (ev,) = cap.events("poll.changed")
        self.assertEqual(ev["payload"]["vote"]["selected"], [])

    def test_a_poll_edit_is_a_fact_about_the_same_poll(self):
        poll = poll_payload("edit", "POLL1", "Where shall we eat tonight?", OPTIONS, selectable=2)
        hook = message_webhook("acc-a", SELF_A, GROUP, "EDIT1", T2, "Poll edited", poll=poll)
        with Capture() as cap:
            self.a._handle_ws(hook)
        self.assertEqual(cap.events("message.received"), [])
        (ev,) = cap.events("poll.changed")
        self.assertEqual(ev["payload"]["kind"], "edit")
        self.assertNotIn("vote", ev["payload"])
        self.assertTrue(ev["payload"]["poll"]["multiple"], "two selectable options is a multiple-select poll")

    def test_a_vote_without_a_poll_id_is_dropped(self):
        poll = poll_payload("vote", "", "Where shall we eat?", OPTIONS, selected_hashes=["hash-pizza"])
        hook = message_webhook("acc-a", SELF_A, GROUP, "VOTE5", T1, "Poll vote: Pizza", poll=poll, **{"from": DAD})
        with Capture() as cap:
            self.a._handle_ws(hook)
        self.assertEqual(cap.events(), [], "nothing is invented from a vote with no poll")

    def test_a_poll_payload_without_hashes_uses_the_option_text_as_its_id(self):
        poll = poll_payload("creation", "POLL2", "Lunch?", (("Pizza", ""), ("Sushi", "")))
        definition = wa.poll_definition(poll)
        self.assertEqual([o["id"] for o in definition["options"]], ["Pizza", "Sushi"])
