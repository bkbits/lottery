//! 开奖记录表。

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "draw")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 期号，统一 7 位（YYYYNNN）
    #[sea_orm(unique)]
    pub code: String,
    /// 开奖日期，YYYY-MM-DD
    pub draw_date: String,
    /// 红球，逗号分隔（零填充）
    pub red: String,
    pub blue: i32,
    pub sales: Option<i64>,
    pub pool: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
