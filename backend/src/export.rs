use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{database, job::JobRuntime};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format_version: u32,
    generated_at: i64,
    complete: bool,
    records: usize,
    media_downloaded: u64,
    media_failed: u64,
    source: &'static str,
    notice: &'static str,
}

pub async fn package(
    job: Arc<JobRuntime>,
    owner_uin: String,
    complete: bool,
) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || package_blocking(&job, &owner_uin, complete))
        .await
        .map_err(|_| "导出任务意外停止".to_owned())?
}

fn package_blocking(job: &JobRuntime, owner_uin: &str, complete: bool) -> Result<PathBuf, String> {
    let export_dir = job.export_dir();
    let staging = export_dir.join("staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .map_err(|error| format!("清理旧导出暂存区失败：{error}"))?;
    }
    std::fs::create_dir_all(&staging).map_err(|error| format!("创建导出暂存区失败：{error}"))?;

    let result = (|| {
        database::checkpoint_database(&job.db_path())?;
        let database_copy = staging.join("archive.sqlite3");
        std::fs::copy(job.db_path(), &database_copy)
            .map_err(|error| format!("复制任务数据库失败：{error}"))?;
        database::write_raw_jsonl(&job.db_path(), owner_uin, &staging.join("raw-feeds.jsonl"))?;
        let records = database::export_records(&job.db_path(), owner_uin)?;
        let status = job.status_blocking();
        let manifest = Manifest {
            format_version: 1,
            generated_at: crate::job::now(),
            complete,
            records: records.len(),
            media_downloaded: status.media_downloaded,
            media_failed: status.media_failed,
            source: "QQ Zone mobile interaction feed",
            notice: "Only content returned by QQ at archive time can be included.",
        };
        write_json(&staging.join("manifest.json"), &manifest)?;
        write_data_js(&staging.join("data.js"), &records)?;
        std::fs::write(staging.join("index.html"), OFFLINE_VIEWER)
            .map_err(|error| format!("写入离线查看器失败：{error}"))?;
        std::fs::write(staging.join("README.txt"), EXPORT_README)
            .map_err(|error| format!("写入导出说明失败：{error}"))?;

        let target = job.download_path();
        let temporary = export_dir.join("qzone-archive.zip.part");
        if temporary.exists() {
            std::fs::remove_file(&temporary)
                .map_err(|error| format!("清理旧导出文件失败：{error}"))?;
        }
        write_zip(&temporary, &staging, &job.media_dir())?;
        if target.exists() {
            std::fs::remove_file(&target)
                .map_err(|error| format!("替换旧导出文件失败：{error}"))?;
        }
        std::fs::rename(&temporary, &target)
            .map_err(|error| format!("完成导出文件失败：{error}"))?;
        Ok(target)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let encoded =
        serde_json::to_vec_pretty(value).map_err(|error| format!("生成导出 JSON 失败：{error}"))?;
    std::fs::write(path, encoded).map_err(|error| format!("写入导出 JSON 失败：{error}"))
}

fn write_data_js(path: &Path, records: &[database::ExportRecord]) -> Result<(), String> {
    let mut file = File::create(path).map_err(|error| format!("创建查看器数据失败：{error}"))?;
    file.write_all(b"window.__QZONE_ARCHIVE_DATA__=")
        .map_err(|error| format!("写入查看器数据失败：{error}"))?;
    serde_json::to_writer(&mut file, records)
        .map_err(|error| format!("序列化查看器数据失败：{error}"))?;
    file.write_all(b";\n")
        .map_err(|error| format!("完成查看器数据失败：{error}"))
}

fn write_zip(target: &Path, staging: &Path, media: &Path) -> Result<(), String> {
    let file = File::create(target).map_err(|error| format!("创建 ZIP 失败：{error}"))?;
    let mut archive = ZipWriter::new(file);
    add_tree(&mut archive, staging, "", CompressionMethod::Deflated)?;
    if media.exists() {
        add_tree(&mut archive, media, "media", CompressionMethod::Stored)?;
    }
    archive
        .finish()
        .map_err(|error| format!("完成 ZIP 失败：{error}"))?;
    Ok(())
}

fn add_tree(
    archive: &mut ZipWriter<File>,
    root: &Path,
    prefix: &str,
    compression: CompressionMethod,
) -> Result<(), String> {
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| format!("读取导出目录失败：{error}"))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| "导出文件路径超出任务目录".to_owned())?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        let name = if prefix.is_empty() {
            relative
        } else {
            format!("{prefix}/{relative}")
        };
        let options = SimpleFileOptions::default()
            .compression_method(compression)
            .unix_permissions(0o644);
        archive
            .start_file(name, options)
            .map_err(|error| format!("写入 ZIP 目录失败：{error}"))?;
        let mut input =
            File::open(entry.path()).map_err(|error| format!("打开导出文件失败：{error}"))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = input
                .read(&mut buffer)
                .map_err(|error| format!("读取导出文件失败：{error}"))?;
            if read == 0 {
                break;
            }
            archive
                .write_all(&buffer[..read])
                .map_err(|error| format!("写入 ZIP 内容失败：{error}"))?;
        }
    }
    Ok(())
}

const EXPORT_README: &str = r#"QQ 空间归档导出

1. 双击 index.html 可离线浏览已经整理的动态。
2. raw-feeds.jsonl 每一行是一条 QQ 接口返回的原始互动记录。
3. archive.sqlite3 是本任务独立的 SQLite 数据库，适合二次开发或长期备份。
4. media/ 保存本次成功下载的图片和视频；失败数量见 manifest.json。
5. 本工具只能保存 QQ 在归档时仍然返回的内容，无法保证恢复永久删除的数据。

隐私提示：导出包不包含 QQ 登录 Cookie。请像保护个人相册一样妥善保存本文件。

项目源码：https://github.com/YouRen1320/qzone-archive-web
上游项目：https://github.com/Gaoshu705/QzoneArchive
许可证：GPL-3.0-only
"#;

const OFFLINE_VIEWER: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta http-equiv="Content-Security-Policy" content="default-src 'self' data: blob:; img-src 'self' data: blob:; media-src 'self' data: blob:; style-src 'unsafe-inline'; script-src 'self' 'unsafe-inline'">
  <title>我的 QQ 空间归档</title>
  <style>
    :root{color-scheme:light;--ink:#12202d;--muted:#617386;--line:#dfe7ec;--paper:#fff;--wash:#f4f7f8;--accent:#157f78}*{box-sizing:border-box}body{margin:0;background:var(--wash);color:var(--ink);font:15px/1.65 system-ui,-apple-system,"PingFang SC",sans-serif}.shell{width:min(920px,calc(100% - 28px));margin:auto;padding:42px 0 70px}header{margin-bottom:24px}h1{margin:0;font-size:clamp(28px,6vw,48px);letter-spacing:-.04em}.sub{color:var(--muted);margin-top:8px}.tools{position:sticky;top:10px;z-index:2;display:flex;gap:10px;padding:10px;background:#ffffffd9;backdrop-filter:blur(12px);border:1px solid var(--line);border-radius:16px;margin:20px 0}.tools input,.tools select{width:100%;border:0;background:transparent;padding:8px;font:inherit;color:inherit;outline:0}.tools select{max-width:150px}.card{background:var(--paper);border:1px solid var(--line);border-radius:18px;padding:20px;margin:12px 0;box-shadow:0 8px 24px #18333f0a}.meta{display:flex;gap:10px;flex-wrap:wrap;color:var(--muted);font-size:13px}.content{white-space:pre-wrap;margin:12px 0 0}.media{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:8px;margin-top:14px}.media img,.media video{width:100%;max-height:520px;object-fit:contain;background:#eef2f4;border-radius:12px}.empty{text-align:center;color:var(--muted);padding:70px 10px}.badge{color:var(--accent)}@media(max-width:600px){.shell{padding-top:24px}.tools{flex-direction:column}.tools select{max-width:none}.card{padding:16px}}
  </style>
</head>
<body>
  <main class="shell">
    <header><h1>我的 QQ 空间归档</h1><div class="sub"><span id="count">0</span> 条可浏览动态 · 本文件完全离线运行</div></header>
    <div class="tools"><input id="search" type="search" placeholder="搜索昵称或内容"><select id="category"><option value="">全部分类</option><option value="self">自己的动态</option><option value="other">好友动态</option><option value="guestbook">留言</option></select></div>
    <section id="list"></section>
  </main>
  <script src="data.js"></script>
  <script>
    const all=Array.isArray(window.__QZONE_ARCHIVE_DATA__)?window.__QZONE_ARCHIVE_DATA__:[];
    const list=document.querySelector('#list'),search=document.querySelector('#search'),category=document.querySelector('#category'),count=document.querySelector('#count');
    const label={self:'自己的动态',other:'好友动态',guestbook:'留言'};
    function node(tag,className,text){const el=document.createElement(tag);if(className)el.className=className;if(text!=null)el.textContent=text;return el}
    function render(){const q=search.value.trim().toLowerCase(),kind=category.value;const rows=all.filter(item=>(!kind||item.category===kind)&&(!q||`${item.authorName||''} ${item.content||''}`.toLowerCase().includes(q)));count.textContent=String(rows.length);list.replaceChildren();if(!rows.length){list.append(node('div','empty','没有符合条件的内容'));return}for(const item of rows){const card=node('article','card');const meta=node('div','meta');meta.append(node('span','badge',label[item.category]||item.category||'动态'),node('span','',item.authorName||'未知用户'),node('time','',item.publishedAt?new Date(item.publishedAt*1000).toLocaleString():'时间未知'));card.append(meta,node('div','content',item.content||'（无文字内容）'));if(item.media?.length){const media=node('div','media');for(const path of item.media){const isVideo=/\.(mp4|mov|m4v|webm)$/i.test(path);const el=node(isVideo?'video':'img');el.src=path;el.loading='lazy';if(isVideo)el.controls=true;media.append(el)}card.append(media)}list.append(card)}}
    search.addEventListener('input',render);category.addEventListener('change',render);render();
  </script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tempfile::tempdir;

    use super::package_blocking;
    use crate::{database, job::JobRuntime, model::JobStatus, tokens::token_hash};

    #[test]
    fn package_contains_offline_viewer_raw_data_and_sqlite() {
        let directory = tempdir().unwrap();
        let job_dir = directory.path().join("0123456789abcdef0123456789abcdef");
        std::fs::create_dir_all(job_dir.join("media")).unwrap();
        std::fs::create_dir_all(job_dir.join("export")).unwrap();
        let job = Arc::new(
            JobRuntime::new_for_test(
                "0123456789abcdef0123456789abcdef".into(),
                job_dir,
                token_hash("owner"),
                JobStatus::new("0123456789abcdef0123456789abcdef".into(), 1, 100),
            )
            .unwrap(),
        );
        database::initialize(&job.db_path()).unwrap();
        database::save_page(
            &job.db_path(),
            "12345",
            &[json!({
                "comm":{"feedskey":"f1","time":1},
                "original":{"cell_id":{"cellid":"c1"},"cell_summary":{"summary":"hello"},"cell_userinfo":{"user":{"uin":"12345","nickname":"me"}}}
            })],
            None,
        )
        .unwrap();
        let zip_path = package_blocking(&job, "12345", true).unwrap();
        let file = std::fs::File::open(zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert!(archive.by_name("index.html").is_ok());
        assert!(archive.by_name("data.js").is_ok());
        assert!(archive.by_name("raw-feeds.jsonl").is_ok());
        assert!(archive.by_name("archive.sqlite3").is_ok());
    }
}
