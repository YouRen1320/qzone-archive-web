// The normalized feed schema and parsers are adapted from QzoneArchive
// (https://github.com/Gaoshu705/QzoneArchive), licensed under GPL-3.0.

use std::{
    collections::HashSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use time::{Date, Month, PrimitiveDateTime, Time};

#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub cursor: String,
    pub pages: u64,
    pub fetched: u64,
    pub saved: u64,
    pub updated_at: i64,
}

#[derive(Debug)]
pub struct PageSaveResult {
    pub processed: u64,
    pub unique_feeds: u64,
    pub media_total: u64,
}

#[derive(Clone, Debug)]
pub struct MediaEntry {
    pub id: i64,
    pub kind: String,
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRecord {
    pub id: i64,
    pub cell_id: String,
    pub published_at: i64,
    pub content: Option<String>,
    pub author_name: Option<String>,
    pub category: String,
    pub media: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ViewerRecordsQuery {
    pub offset: u64,
    pub limit: u64,
    pub search: Option<String>,
    pub category: Option<String>,
    pub year: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerRecordsPage {
    pub items: Vec<ExportRecord>,
    pub total: u64,
    pub offset: u64,
    pub next_offset: Option<u64>,
    pub years: Vec<i32>,
}

struct ParsedFeed {
    feed_key: String,
    cell_id: Option<String>,
    event_type: i64,
    event_time: i64,
    title: Option<String>,
    content: Option<String>,
    event_summary: Option<String>,
    actor_uin: Option<String>,
    actor_name: Option<String>,
    original_author_uin: Option<String>,
    original_author_name: Option<String>,
    picture_count: i64,
    pictures_json: Option<String>,
    video_json: Option<String>,
    comments_json: Option<String>,
    raw_json: String,
}

pub fn initialize(path: &Path) -> Result<(), String> {
    let connection = open(path)?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS archive_feeds (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               owner_uin TEXT NOT NULL,
               feed_key TEXT NOT NULL,
               cell_id TEXT,
               event_type INTEGER NOT NULL DEFAULT 0,
               event_time INTEGER NOT NULL DEFAULT 0,
               title TEXT,
               content TEXT,
               event_summary TEXT,
               actor_uin TEXT,
               actor_name TEXT,
               original_author_uin TEXT,
               original_author_name TEXT,
               picture_count INTEGER NOT NULL DEFAULT 0,
               pictures_json TEXT,
               video_json TEXT,
               comments_json TEXT,
               raw_json TEXT NOT NULL,
               archived_at INTEGER NOT NULL,
               UNIQUE(owner_uin, feed_key)
             );
             CREATE INDEX IF NOT EXISTS idx_archive_feeds_owner_time
               ON archive_feeds(owner_uin, event_time DESC);
             CREATE TABLE IF NOT EXISTS archive_dynamics (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               owner_uin TEXT NOT NULL,
               cell_id TEXT NOT NULL,
               published_at INTEGER NOT NULL DEFAULT 0,
               content TEXT,
               author_uin TEXT,
               author_name TEXT,
               category TEXT NOT NULL DEFAULT '',
               pictures_json TEXT,
               video_json TEXT,
               raw_original_json TEXT NOT NULL,
               archived_at INTEGER NOT NULL,
               UNIQUE(owner_uin, cell_id)
             );
             CREATE INDEX IF NOT EXISTS idx_archive_dynamics_owner_time
               ON archive_dynamics(owner_uin, published_at DESC);
             CREATE TABLE IF NOT EXISTS archive_checkpoints (
               owner_uin TEXT PRIMARY KEY,
               attach_info TEXT NOT NULL,
               pages INTEGER NOT NULL DEFAULT 0,
               fetched INTEGER NOT NULL DEFAULT 0,
               saved INTEGER NOT NULL DEFAULT 0,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS archive_media (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               owner_uin TEXT NOT NULL,
               cell_id TEXT NOT NULL,
               kind TEXT NOT NULL,
               ordinal INTEGER NOT NULL,
               candidates_json TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'pending',
               local_path TEXT,
               bytes INTEGER NOT NULL DEFAULT 0,
               error TEXT,
               updated_at INTEGER NOT NULL,
               UNIQUE(owner_uin, cell_id, kind, ordinal)
             );
             CREATE INDEX IF NOT EXISTS idx_archive_media_status
               ON archive_media(owner_uin, status, id);
             CREATE TABLE IF NOT EXISTS archive_viewer_records (
               id INTEGER PRIMARY KEY,
               cell_id TEXT NOT NULL,
               published_at INTEGER NOT NULL,
               content TEXT,
               author_name TEXT,
               category TEXT NOT NULL,
               media_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_archive_viewer_time
               ON archive_viewer_records(published_at DESC, id DESC);
             CREATE INDEX IF NOT EXISTS idx_archive_viewer_category_time
               ON archive_viewer_records(category, published_at DESC, id DESC);",
        )
        .map_err(|error| format!("初始化任务数据库失败：{error}"))?;
    Ok(())
}

pub fn load_checkpoint(path: &Path, owner_uin: &str) -> Result<Option<Checkpoint>, String> {
    let connection = open(path)?;
    connection
        .query_row(
            "SELECT attach_info,pages,fetched,saved,updated_at FROM archive_checkpoints WHERE owner_uin=?1",
            params![owner_uin],
            |row| {
                Ok(Checkpoint {
                    cursor: row.get(0)?,
                    pages: row.get(1)?,
                    fetched: row.get(2)?,
                    saved: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("读取归档断点失败：{error}"))
}

pub fn clear_checkpoint(path: &Path, owner_uin: &str) -> Result<(), String> {
    let connection = open(path)?;
    connection
        .execute(
            "DELETE FROM archive_checkpoints WHERE owner_uin=?1",
            params![owner_uin],
        )
        .map_err(|error| format!("清理过期归档断点失败：{error}"))?;
    Ok(())
}

pub fn save_page(
    path: &Path,
    owner_uin: &str,
    feeds: &[Value],
    next_cursor: Option<&str>,
) -> Result<PageSaveResult, String> {
    let mut connection = open(path)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("无法开始任务数据库事务：{error}"))?;
    for feed in feeds {
        save_original_dynamic(&transaction, owner_uin, feed)?;
        let feed = parse_feed(feed)?;
        transaction
            .execute(
                "INSERT INTO archive_feeds
                 (owner_uin,feed_key,cell_id,event_type,event_time,title,content,event_summary,
                  actor_uin,actor_name,original_author_uin,original_author_name,picture_count,
                  pictures_json,video_json,comments_json,raw_json,archived_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
                 ON CONFLICT(owner_uin,feed_key) DO UPDATE SET
                  cell_id=excluded.cell_id,event_type=excluded.event_type,event_time=excluded.event_time,
                  title=excluded.title,content=excluded.content,event_summary=excluded.event_summary,
                  actor_uin=excluded.actor_uin,actor_name=excluded.actor_name,
                  original_author_uin=excluded.original_author_uin,
                  original_author_name=excluded.original_author_name,
                  picture_count=excluded.picture_count,pictures_json=excluded.pictures_json,
                  video_json=excluded.video_json,comments_json=excluded.comments_json,
                  raw_json=excluded.raw_json,archived_at=excluded.archived_at",
                params![
                    owner_uin,
                    feed.feed_key,
                    feed.cell_id,
                    feed.event_type,
                    feed.event_time,
                    feed.title,
                    feed.content,
                    feed.event_summary,
                    feed.actor_uin,
                    feed.actor_name,
                    feed.original_author_uin,
                    feed.original_author_name,
                    feed.picture_count,
                    feed.pictures_json,
                    feed.video_json,
                    feed.comments_json,
                    feed.raw_json,
                    now()
                ],
            )
            .map_err(|error| format!("保存互动记录失败：{error}"))?;
    }

    if let Some(cursor) = next_cursor {
        transaction
            .execute(
                "INSERT INTO archive_checkpoints(owner_uin,attach_info,pages,fetched,saved,updated_at)
                 VALUES (?1,?2,1,?3,?3,?4)
                 ON CONFLICT(owner_uin) DO UPDATE SET
                  attach_info=excluded.attach_info,pages=archive_checkpoints.pages+1,
                  fetched=archive_checkpoints.fetched+excluded.fetched,
                  saved=archive_checkpoints.saved+excluded.saved,updated_at=excluded.updated_at",
                params![owner_uin, cursor, feeds.len() as u64, now()],
            )
            .map_err(|error| format!("保存归档断点失败：{error}"))?;
    } else {
        transaction
            .execute(
                "DELETE FROM archive_checkpoints WHERE owner_uin=?1",
                params![owner_uin],
            )
            .map_err(|error| format!("完成归档断点失败：{error}"))?;
    }

    let unique_feeds: u64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM archive_feeds WHERE owner_uin=?1",
            params![owner_uin],
            |row| row.get(0),
        )
        .map_err(|error| format!("统计归档记录失败：{error}"))?;
    let media_total: u64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM archive_media WHERE owner_uin=?1",
            params![owner_uin],
            |row| row.get(0),
        )
        .map_err(|error| format!("统计媒体记录失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交归档事务失败：{error}"))?;
    Ok(PageSaveResult {
        processed: feeds.len() as u64,
        unique_feeds,
        media_total,
    })
}

pub fn pending_media(path: &Path, owner_uin: &str) -> Result<Vec<MediaEntry>, String> {
    let connection = open(path)?;
    let mut statement = connection
        .prepare(
            "SELECT id,kind,candidates_json FROM archive_media
             WHERE owner_uin=?1 AND status!='downloaded' ORDER BY id",
        )
        .map_err(|error| format!("准备媒体查询失败：{error}"))?;
    let rows = statement
        .query_map(params![owner_uin], |row| {
            let raw: String = row.get(2)?;
            Ok(MediaEntry {
                id: row.get(0)?,
                kind: row.get(1)?,
                candidates: serde_json::from_str(&raw).unwrap_or_default(),
            })
        })
        .map_err(|error| format!("读取媒体清单失败：{error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取媒体清单失败：{error}"))
}

pub fn mark_media_downloaded(
    path: &Path,
    id: i64,
    local_path: &str,
    bytes: u64,
) -> Result<(), String> {
    let connection = open(path)?;
    connection
        .execute(
            "UPDATE archive_media SET status='downloaded',local_path=?2,bytes=?3,error=NULL,updated_at=?4 WHERE id=?1",
            params![id, local_path, bytes, now()],
        )
        .map_err(|error| format!("更新媒体下载状态失败：{error}"))?;
    Ok(())
}

pub fn mark_media_failed(path: &Path, id: i64, message: &str) -> Result<(), String> {
    let connection = open(path)?;
    connection
        .execute(
            "UPDATE archive_media SET status='failed',error=?2,updated_at=?3 WHERE id=?1",
            params![id, concise_error(message), now()],
        )
        .map_err(|error| format!("更新媒体失败状态失败：{error}"))?;
    Ok(())
}

pub fn export_records(path: &Path, owner_uin: &str) -> Result<Vec<ExportRecord>, String> {
    let connection = open(path)?;
    let mut statement = connection
        .prepare(
            "SELECT id,cell_id,published_at,content,author_name,category
             FROM archive_dynamics WHERE owner_uin=?1 ORDER BY published_at DESC,id DESC",
        )
        .map_err(|error| format!("准备导出查询失败：{error}"))?;
    let dynamics = statement
        .query_map(params![owner_uin], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|error| format!("读取导出记录失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取导出记录失败：{error}"))?;

    dynamics
        .into_iter()
        .map(
            |(id, cell_id, published_at, content, author_name, category)| {
                let mut media_statement = connection
                    .prepare(
                        "SELECT local_path FROM archive_media
                     WHERE owner_uin=?1 AND cell_id=?2 AND status='downloaded'
                     ORDER BY kind,ordinal",
                    )
                    .map_err(|error| format!("准备导出媒体查询失败：{error}"))?;
                let media = media_statement
                    .query_map(params![owner_uin, cell_id], |row| row.get::<_, String>(0))
                    .map_err(|error| format!("读取导出媒体失败：{error}"))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("读取导出媒体失败：{error}"))?;
                Ok(ExportRecord {
                    id,
                    cell_id,
                    published_at,
                    content,
                    author_name,
                    category,
                    media,
                })
            },
        )
        .collect()
}

// The viewer table is a frozen, owner-filtered projection of the final export. It prevents
// a later rescan with another QQ account from ever mixing records in the ready-state reader.
pub fn replace_viewer_records(path: &Path, records: &[ExportRecord]) -> Result<(), String> {
    let mut connection = open(path)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("无法开始阅读数据事务：{error}"))?;
    transaction
        .execute("DELETE FROM archive_viewer_records", [])
        .map_err(|error| format!("清理旧阅读数据失败：{error}"))?;
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO archive_viewer_records
                 (id,cell_id,published_at,content,author_name,category,media_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
            )
            .map_err(|error| format!("准备阅读数据失败：{error}"))?;
        for record in records {
            let media_json = serde_json::to_string(&record.media)
                .map_err(|error| format!("序列化阅读媒体失败：{error}"))?;
            statement
                .execute(params![
                    record.id,
                    record.cell_id,
                    record.published_at,
                    record.content,
                    record.author_name,
                    record.category,
                    media_json
                ])
                .map_err(|error| format!("写入阅读数据失败：{error}"))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| format!("提交阅读数据失败：{error}"))
}

pub fn viewer_records(path: &Path, query: ViewerRecordsQuery) -> Result<ViewerRecordsPage, String> {
    let connection = open(path)?;
    let (year_start, year_end) = query.year.map(year_bounds).transpose()?.unzip();
    let search = query.search.as_deref().map(search_pattern);
    let filters = params![query.category, year_start, year_end, search];
    let predicate = "(?1 IS NULL OR category=?1)
      AND (?2 IS NULL OR published_at>=?2)
      AND (?3 IS NULL OR published_at<?3)
      AND (?4 IS NULL OR content LIKE ?4 ESCAPE '\\' OR author_name LIKE ?4 ESCAPE '\\')";

    let total: u64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM archive_viewer_records WHERE {predicate}"),
            filters,
            |row| row.get(0),
        )
        .map_err(|error| format!("统计阅读记录失败：{error}"))?;

    let mut statement = connection
        .prepare(&format!(
            "SELECT id,cell_id,published_at,content,author_name,category,media_json
             FROM archive_viewer_records WHERE {predicate}
             ORDER BY published_at DESC,id DESC LIMIT ?5 OFFSET ?6"
        ))
        .map_err(|error| format!("准备阅读查询失败：{error}"))?;
    let rows = statement
        .query_map(
            params![
                query.category,
                year_start,
                year_end,
                search,
                query.limit,
                query.offset
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .map_err(|error| format!("读取归档记录失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取归档记录失败：{error}"))?;
    let items = rows
        .into_iter()
        .map(
            |(id, cell_id, published_at, content, author_name, category, media_json)| {
                let media = serde_json::from_str(&media_json)
                    .map_err(|error| format!("解析阅读媒体失败：{error}"))?;
                Ok(ExportRecord {
                    id,
                    cell_id,
                    published_at,
                    content,
                    author_name,
                    category,
                    media,
                })
            },
        )
        .collect::<Result<Vec<_>, String>>()?;
    let years = connection
        .prepare(
            "SELECT DISTINCT CAST(strftime('%Y', published_at, 'unixepoch') AS INTEGER) AS year
             FROM archive_viewer_records WHERE published_at>0 ORDER BY year DESC",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, i32>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| format!("读取归档年份失败：{error}"))?;
    let consumed = query.offset.saturating_add(items.len() as u64);
    Ok(ViewerRecordsPage {
        items,
        total,
        offset: query.offset,
        next_offset: (consumed < total).then_some(consumed),
        years,
    })
}

fn year_bounds(year: i32) -> Result<(i64, i64), String> {
    let start =
        Date::from_calendar_date(year, Month::January, 1).map_err(|_| "归档年份无效".to_owned())?;
    let end = Date::from_calendar_date(year + 1, Month::January, 1)
        .map_err(|_| "归档年份无效".to_owned())?;
    Ok((
        PrimitiveDateTime::new(start, Time::MIDNIGHT)
            .assume_utc()
            .unix_timestamp(),
        PrimitiveDateTime::new(end, Time::MIDNIGHT)
            .assume_utc()
            .unix_timestamp(),
    ))
}

fn search_pattern(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

pub fn write_raw_jsonl(path: &Path, owner_uin: &str, output: &Path) -> Result<(), String> {
    use std::io::Write;

    let connection = open(path)?;
    let mut statement = connection
        .prepare("SELECT raw_json FROM archive_feeds WHERE owner_uin=?1 ORDER BY event_time,id")
        .map_err(|error| format!("准备原始数据导出失败：{error}"))?;
    let rows = statement
        .query_map(params![owner_uin], |row| row.get::<_, String>(0))
        .map_err(|error| format!("读取原始数据失败：{error}"))?;
    let mut file =
        std::fs::File::create(output).map_err(|error| format!("创建原始数据导出失败：{error}"))?;
    for row in rows {
        let value = row.map_err(|error| format!("读取原始数据失败：{error}"))?;
        file.write_all(value.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|error| format!("写入原始数据导出失败：{error}"))?;
    }
    Ok(())
}

pub fn checkpoint_database(path: &Path) -> Result<(), String> {
    let connection = open(path)?;
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|error| format!("整理任务数据库失败：{error}"))
}

fn open(path: &Path) -> Result<Connection, String> {
    let connection =
        Connection::open(path).map_err(|error| format!("无法打开任务数据库：{error}"))?;
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")
        .map_err(|error| format!("配置任务数据库失败：{error}"))?;
    Ok(connection)
}

fn save_original_dynamic(
    transaction: &rusqlite::Transaction<'_>,
    owner_uin: &str,
    feed: &Value,
) -> Result<(), String> {
    let Some(original) = feed.get("original") else {
        return Ok(());
    };
    let Some(cell_id) = text_at(original, "/cell_id/cellid") else {
        return Ok(());
    };
    let original_appid = original
        .pointer("/cell_comm/appid")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let original_key = text_at(original, "/cell_comm/feedskey").unwrap_or_default();
    let is_guestbook = original_appid == 334 || original_key.starts_with("334_");
    let author_uin = if is_guestbook {
        text_at(feed, "/userinfo/user/uin")
    } else {
        text_at(original, "/cell_userinfo/user/uin")
    };
    let category = if is_guestbook {
        "guestbook"
    } else if author_uin.as_deref() == Some(owner_uin) {
        "self"
    } else {
        "other"
    };
    let pictures = original.get("cell_pic").filter(|value| !value.is_null());
    let video = original.get("cell_video").filter(|value| !value.is_null());
    transaction
        .execute(
            "INSERT INTO archive_dynamics
             (owner_uin,cell_id,published_at,content,author_uin,author_name,category,
              pictures_json,video_json,raw_original_json,archived_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(owner_uin,cell_id) DO UPDATE SET
              published_at=excluded.published_at,content=excluded.content,
              author_uin=excluded.author_uin,author_name=excluded.author_name,
              category=excluded.category,
              pictures_json=COALESCE(excluded.pictures_json,archive_dynamics.pictures_json),
              video_json=COALESCE(excluded.video_json,archive_dynamics.video_json),
              raw_original_json=excluded.raw_original_json,archived_at=excluded.archived_at",
            params![
                owner_uin,
                cell_id,
                original
                    .pointer("/cell_comm/time")
                    .and_then(Value::as_i64)
                    .or_else(|| feed.pointer("/comm/time").and_then(Value::as_i64))
                    .unwrap_or(0),
                if is_guestbook {
                    text_at(feed, "/summary/summary")
                } else {
                    text_at(original, "/cell_summary/summary")
                },
                author_uin,
                if is_guestbook {
                    text_at(feed, "/userinfo/user/nickname")
                } else {
                    text_at(original, "/cell_userinfo/user/nickname")
                },
                category,
                pictures.map(Value::to_string),
                video.map(Value::to_string),
                original.to_string(),
                now()
            ],
        )
        .map_err(|error| format!("保存原动态失败：{error}"))?;

    for (ordinal, candidates) in picture_url_candidates(pictures).into_iter().enumerate() {
        save_media(
            transaction,
            owner_uin,
            &cell_id,
            "image",
            ordinal,
            &candidates,
        )?;
    }
    for (ordinal, candidates) in video_url_candidates(video).into_iter().enumerate() {
        save_media(
            transaction,
            owner_uin,
            &cell_id,
            "video",
            ordinal,
            &candidates,
        )?;
    }
    Ok(())
}

fn save_media(
    transaction: &rusqlite::Transaction<'_>,
    owner_uin: &str,
    cell_id: &str,
    kind: &str,
    ordinal: usize,
    candidates: &[String],
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO archive_media
             (owner_uin,cell_id,kind,ordinal,candidates_json,status,updated_at)
             VALUES (?1,?2,?3,?4,?5,'pending',?6)
             ON CONFLICT(owner_uin,cell_id,kind,ordinal) DO UPDATE SET
              candidates_json=excluded.candidates_json,updated_at=excluded.updated_at",
            params![
                owner_uin,
                cell_id,
                kind,
                ordinal as i64,
                serde_json::to_string(candidates)
                    .map_err(|error| format!("序列化媒体地址失败：{error}"))?,
                now()
            ],
        )
        .map_err(|error| format!("保存媒体清单失败：{error}"))?;
    Ok(())
}

fn parse_feed(feed: &Value) -> Result<ParsedFeed, String> {
    let cell_id = text_at(feed, "/original/cell_id/cellid");
    let event_time = feed
        .pointer("/comm/time")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let event_type = feed
        .pointer("/comm/subid")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let actor_uin = text_at(feed, "/userinfo/user/uin");
    let feed_key = text_at(feed, "/comm/feedskey")
        .or_else(|| text_at(feed, "/original/cell_comm/feedskey"))
        .or_else(|| {
            cell_id.as_ref().map(|id| {
                format!(
                    "{event_type}:{id}:{event_time}:{}",
                    actor_uin.as_deref().unwrap_or("unknown")
                )
            })
        })
        .unwrap_or_else(|| {
            format!(
                "fallback:{event_type}:{event_time}:{}:{:016x}",
                actor_uin.as_deref().unwrap_or("unknown"),
                stable_feed_hash(feed)
            )
        });
    let pictures = feed.pointer("/original/cell_pic");
    Ok(ParsedFeed {
        feed_key,
        cell_id,
        event_type,
        event_time,
        title: text_at(feed, "/title/title"),
        content: text_at(feed, "/original/cell_summary/summary"),
        event_summary: text_at(feed, "/summary/summary"),
        actor_uin,
        actor_name: text_at(feed, "/userinfo/user/nickname"),
        original_author_uin: text_at(feed, "/original/cell_userinfo/user/uin"),
        original_author_name: text_at(feed, "/original/cell_userinfo/user/nickname"),
        picture_count: pictures
            .and_then(|value| value.pointer("/picdata/pic"))
            .and_then(Value::as_array)
            .map(|items| items.len() as i64)
            .unwrap_or(0),
        pictures_json: pictures.map(Value::to_string),
        video_json: feed
            .pointer("/original/cell_video")
            .filter(|value| !value.is_null())
            .map(Value::to_string),
        comments_json: feed
            .pointer("/original/cell_comment")
            .filter(|value| !value.is_null())
            .map(Value::to_string),
        raw_json: feed.to_string(),
    })
}

fn picture_url_candidates(value: Option<&Value>) -> Vec<Vec<String>> {
    value
        .and_then(|value| value.pointer("/picdata/pic"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|picture| {
            let values = match picture.get("photourl") {
                Some(Value::Array(items)) => items.iter().collect::<Vec<_>>(),
                Some(Value::Object(items)) => items.values().collect::<Vec<_>>(),
                _ => vec![],
            };
            let mut urls = values
                .into_iter()
                .filter_map(|item| item.get("url").and_then(Value::as_str))
                .map(normalize_url)
                .filter(|url| !url.is_empty())
                .collect::<Vec<_>>();
            if let Some(url) = picture
                .pointer("/busi_param/-1")
                .and_then(Value::as_str)
                .map(normalize_url)
                .filter(|url| !url.is_empty())
            {
                urls.push(url);
            }
            let mut seen = HashSet::new();
            urls.retain(|url| seen.insert(url.clone()));
            (!urls.is_empty()).then_some(urls)
        })
        .collect()
}

fn video_url_candidates(value: Option<&Value>) -> Vec<Vec<String>> {
    let Some(value) = value else {
        return vec![];
    };
    let mut urls = Vec::new();
    if let Some(url) = value.get("videourl").and_then(Value::as_str) {
        urls.push(normalize_url(url));
    }
    if let Some(items) = value.get("videourls").and_then(Value::as_object) {
        for url in items
            .values()
            .filter_map(|item| item.get("url").and_then(Value::as_str))
        {
            urls.push(normalize_url(url));
        }
    }
    let mut seen = HashSet::new();
    urls.retain(|url| !url.is_empty() && seen.insert(url.clone()));
    if urls.is_empty() {
        vec![]
    } else {
        vec![urls]
    }
}

fn normalize_url(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("//") {
        format!("https:{value}")
    } else {
        value.to_owned()
    }
}

fn text_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer).and_then(|value| match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn stable_feed_hash(value: &Value) -> u64 {
    value
        .to_string()
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

fn concise_error(value: &str) -> String {
    value.chars().take(240).collect()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        export_records, initialize, load_checkpoint, pending_media, replace_viewer_records,
        save_page, viewer_records, ViewerRecordsQuery,
    };

    #[test]
    fn saves_deduplicated_feeds_checkpoint_and_media() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        initialize(&path).unwrap();
        let feed = json!({
            "comm": {"feedskey": "feed-1", "time": 1700000000, "subid": 1},
            "userinfo": {"user": {"uin": "10001", "nickname": "访客"}},
            "original": {
                "cell_id": {"cellid": "cell-1"},
                "cell_comm": {"feedskey": "original-1", "time": 1699999999},
                "cell_summary": {"summary": "测试动态"},
                "cell_userinfo": {"user": {"uin": "12345", "nickname": "主人"}},
                "cell_pic": {"picdata": {"pic": [{"photourl": [{"url": "//example.invalid/a.jpg"}]}]}}
            }
        });
        let first = save_page(&path, "12345", &[feed.clone()], Some("cursor-2")).unwrap();
        let second = save_page(&path, "12345", &[feed], None).unwrap();
        assert_eq!(first.unique_feeds, 1);
        assert_eq!(second.unique_feeds, 1);
        assert!(load_checkpoint(&path, "12345").unwrap().is_none());
        assert_eq!(pending_media(&path, "12345").unwrap().len(), 1);
        let records = export_records(&path, "12345").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].content.as_deref(), Some("测试动态"));
    }

    #[test]
    fn viewer_projection_pages_and_filters_without_mixing_owners() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        initialize(&path).unwrap();
        let records = vec![
            super::ExportRecord {
                id: 1,
                cell_id: "older".into(),
                published_at: 1_704_067_200,
                content: Some("冬天的桥".into()),
                author_name: Some("自己".into()),
                category: "self".into(),
                media: vec!["media/a.jpg".into()],
            },
            super::ExportRecord {
                id: 2,
                cell_id: "newer".into(),
                published_at: 1_735_689_600,
                content: Some("雨落在窗前".into()),
                author_name: Some("故人".into()),
                category: "other".into(),
                media: vec![],
            },
        ];
        replace_viewer_records(&path, &records).unwrap();

        let first = viewer_records(
            &path,
            ViewerRecordsQuery {
                offset: 0,
                limit: 1,
                search: None,
                category: None,
                year: None,
            },
        )
        .unwrap();
        assert_eq!(first.total, 2);
        assert_eq!(first.items[0].cell_id, "newer");
        assert_eq!(first.next_offset, Some(1));
        assert_eq!(first.years, vec![2025, 2024]);

        let filtered = viewer_records(
            &path,
            ViewerRecordsQuery {
                offset: 0,
                limit: 30,
                search: Some("窗前".into()),
                category: Some("other".into()),
                year: Some(2025),
            },
        )
        .unwrap();
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items[0].author_name.as_deref(), Some("故人"));

        replace_viewer_records(&path, &records[..1]).unwrap();
        let replaced = viewer_records(
            &path,
            ViewerRecordsQuery {
                offset: 0,
                limit: 30,
                search: None,
                category: None,
                year: None,
            },
        )
        .unwrap();
        assert_eq!(replaced.total, 1);
        assert_eq!(replaced.items[0].cell_id, "older");
    }
}
