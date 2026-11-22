use serde_json::{json, Map, Value};

pub fn global_context(app_version: &str) -> Map<String, Value> {
    let mut ctx = Map::new();
    ctx.insert("product".into(), json!("shuttle"));
    ctx.insert("app".into(), json!("shuttle-desktop"));
    ctx.insert("platform".into(), json!("desktop"));
    ctx.insert("app_version".into(), json!(app_version));
    ctx.insert("build_channel".into(), json!(crate::env::build_channel()));
    ctx.insert(
        "environment".into(),
        json!(crate::env::telemetry_environment()),
    );
    ctx.insert("release".into(), json!(format!("shuttle@{app_version}")));
    if let Some(commit) = crate::env::git_commit() {
        ctx.insert("git_commit".into(), json!(commit));
    }
    ctx.insert("os".into(), json!(std::env::consts::OS));
    ctx.insert("architecture".into(), json!(std::env::consts::ARCH));
    ctx.insert(
        "cpu_core_count".into(),
        json!(std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)),
    );
    ctx.insert("ram_bucket".into(), json!(ram_bucket()));
    ctx
}

pub fn ram_bucket() -> &'static str {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let gb = system.total_memory() / 1024 / 1024 / 1024;
    match gb {
        0 => "unknown",
        1..=4 => "le_4gb",
        5..=8 => "le_8gb",
        9..=16 => "le_16gb",
        _ => "gt_16gb",
    }
}

/// Finer than `bucket_count` because the 5-10 account range is where load starts to matter.
pub fn bucket_accounts(count: u64) -> &'static str {
    match count {
        0 => "0",
        1 => "1",
        2..=3 => "2_3",
        4..=5 => "4_5",
        6..=10 => "6_10",
        _ => "11_plus",
    }
}

pub fn bucket_large_count(count: u64) -> &'static str {
    match count {
        0 => "0",
        1..=100 => "1_100",
        101..=1_000 => "101_1k",
        1_001..=10_000 => "1k_10k",
        10_001..=100_000 => "10k_100k",
        100_001..=1_000_000 => "100k_1m",
        _ => "gt_1m",
    }
}

pub fn bucket_count(count: u64) -> &'static str {
    match count {
        0 => "0",
        1..=10 => "1_10",
        11..=100 => "11_100",
        101..=1000 => "101_1000",
        _ => "1000_plus",
    }
}

pub fn bucket_bytes(bytes: u64) -> &'static str {
    match bytes {
        0..=1_048_576 => "le_1mb",
        1_048_577..=10_485_760 => "le_10mb",
        10_485_761..=104_857_600 => "le_100mb",
        _ => "gt_100mb",
    }
}

pub fn merge_context(base: &Map<String, Value>, extra: &Map<String, Value>) -> Map<String, Value> {
    let mut out = base.clone();
    for (k, v) in extra {
        out.insert(k.clone(), v.clone());
    }
    out
}
