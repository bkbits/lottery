//! SQLite 存储层（SeaORM）。
//!
//! 数据库只做两件事：缓存全部开奖记录、保存界面配置。
//! 对外是同步接口，内部自己驱动 tokio 运行时，方便从 UI 线程直接调用。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend, EntityTrait,
    QueryOrder, Set, Statement, TransactionTrait,
};
use tokio::runtime::{Builder, Runtime};

use crate::core::draw::{Draw, parse_blue, parse_reds, sort_newest_first};

use super::entity::{draw as draw_entity, setting as setting_entity};

const CREATE_DRAW: &str = "
CREATE TABLE IF NOT EXISTS draw (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    draw_date TEXT NOT NULL,
    red TEXT NOT NULL,
    blue INTEGER NOT NULL,
    sales INTEGER,
    pool INTEGER
)";

const CREATE_SETTING: &str = "
CREATE TABLE IF NOT EXISTS setting (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
)";

/// 默认数据库位置：系统数据目录下的 `lottery/lottery.db`。
pub fn default_db_path() -> PathBuf {
    if let Ok(custom) = std::env::var("LOTTERY_DB") {
        return PathBuf::from(custom);
    }
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("lottery").join("lottery.db")
}

fn sqlite_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("sqlite://{text}?mode=rwc")
    } else {
        // Windows 盘符路径不能写成 sqlite://C:/...，否则盘符会被当成 host
        format!("sqlite:{text}?mode=rwc")
    }
}

pub struct Store {
    rt: Runtime,
    db: DatabaseConnection,
    path: PathBuf,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建数据目录失败: {}", parent.display()))?;
        }

        let rt = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .context("创建 tokio 运行时失败")?;

        let url = sqlite_url(path);
        let db = rt
            .block_on(async {
                let db = Database::connect(url.as_str()).await?;
                db.execute_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    CREATE_DRAW.to_string(),
                ))
                .await?;
                db.execute_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    CREATE_SETTING.to_string(),
                ))
                .await?;
                Ok::<_, sea_orm::DbErr>(db)
            })
            .with_context(|| format!("打开数据库失败: {}", path.display()))?;

        Ok(Self {
            rt,
            db,
            path: path.to_path_buf(),
        })
    }

    pub fn open_default() -> Result<Self> {
        let path = default_db_path();
        Self::open(&path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取全部开奖记录，按新到旧排序。
    pub fn load_draws(&self) -> Result<Vec<Draw>> {
        let rows = self
            .rt
            .block_on(async {
                draw_entity::Entity::find()
                    .order_by_desc(draw_entity::Column::Code)
                    .all(&self.db)
                    .await
            })
            .context("读取开奖记录失败")?;

        let mut draws = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(reds) = parse_reds(&row.red) else {
                continue;
            };
            let Some(blue) = parse_blue(&row.blue.to_string()) else {
                continue;
            };
            draws.push(Draw {
                code: row.code,
                date: row.draw_date,
                reds,
                blue,
                sales: row.sales,
                pool: row.pool,
            });
        }
        sort_newest_first(&mut draws);
        Ok(draws)
    }

    /// 只插入库里没有的期号，返回新增条数。
    pub fn insert_new(&self, draws: &[Draw]) -> Result<usize> {
        if draws.is_empty() {
            return Ok(0);
        }
        let writes: Vec<Draw> = draws.to_vec();
        self.rt
            .block_on(async {
                let existing: HashSet<String> = draw_entity::Entity::find()
                    .all(&self.db)
                    .await?
                    .into_iter()
                    .map(|m| m.code)
                    .collect();

                let models: Vec<draw_entity::ActiveModel> = writes
                    .iter()
                    .filter(|d| !existing.contains(&d.code))
                    .map(|d| draw_entity::ActiveModel {
                        code: Set(d.code.clone()),
                        draw_date: Set(d.date.clone()),
                        red: Set(d.red_text(",")),
                        blue: Set(d.blue as i32),
                        sales: Set(d.sales),
                        pool: Set(d.pool),
                        ..Default::default()
                    })
                    .collect();

                let inserted = models.len();
                if inserted > 0 {
                    let txn = self.db.begin().await?;
                    for chunk in models.chunks(400) {
                        draw_entity::Entity::insert_many(chunk.to_vec())
                            .exec(&txn)
                            .await?;
                    }
                    txn.commit().await?;
                }
                Ok::<_, sea_orm::DbErr>(inserted)
            })
            .context("写入开奖记录失败")
    }

    pub fn count(&self) -> Result<u64> {
        use sea_orm::PaginatorTrait;
        self.rt
            .block_on(async { draw_entity::Entity::find().count(&self.db).await })
            .context("统计期数失败")
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_string();
        self.rt
            .block_on(async {
                Ok::<_, sea_orm::DbErr>(
                    setting_entity::Entity::find_by_id(key)
                        .one(&self.db)
                        .await?
                        .map(|m| m.value),
                )
            })
            .context("读取配置失败")
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let key = key.to_string();
        let value = value.to_string();
        self.rt
            .block_on(async {
                let existing = setting_entity::Entity::find_by_id(key.clone())
                    .one(&self.db)
                    .await?;
                match existing {
                    Some(model) => {
                        let mut active: setting_entity::ActiveModel = model.into();
                        active.value = Set(value);
                        active.update(&self.db).await?;
                    }
                    None => {
                        setting_entity::ActiveModel {
                            key: Set(key),
                            value: Set(value),
                        }
                        .insert(&self.db)
                        .await?;
                    }
                }
                Ok::<_, sea_orm::DbErr>(())
            })
            .context("保存配置失败")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("lottery-test-{tag}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn sample(code: &str) -> Draw {
        Draw::new(
            code.to_string(),
            "2026-11-08".to_string(),
            [5, 6, 10, 25, 27, 30],
            11,
        )
    }

    #[test]
    fn roundtrip_draws() {
        let path = temp_path("draws");
        let store = Store::open(&path).expect("open db");
        assert_eq!(store.load_draws().unwrap().len(), 0);

        let inserted = store
            .insert_new(&[sample("2026127"), sample("2026128")])
            .unwrap();
        assert_eq!(inserted, 2);
        // 重复写入不增加
        assert_eq!(store.insert_new(&[sample("2026128")]).unwrap(), 0);

        let draws = store.load_draws().unwrap();
        assert_eq!(draws.len(), 2);
        assert_eq!(draws[0].code, "2026128");
        assert_eq!(draws[0].reds, [5, 6, 10, 25, 27, 30]);
        assert_eq!(draws[0].blue, 11);
        assert_eq!(store.count().unwrap(), 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn settings_roundtrip() {
        let path = temp_path("settings");
        let store = Store::open(&path).unwrap();
        assert!(store.get_setting("config").unwrap().is_none());
        store.set_setting("config", "{\"a\":1}").unwrap();
        assert_eq!(store.get_setting("config").unwrap().unwrap(), "{\"a\":1}");
        store.set_setting("config", "{\"a\":2}").unwrap();
        assert_eq!(store.get_setting("config").unwrap().unwrap(), "{\"a\":2}");
        let _ = std::fs::remove_file(&path);
    }
}
