use std::collections::HashSet;

use migration::{Expr, Query};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
    Select, SelectModel, Selector,
};

use crate::entities::{
    prelude::{SameyPost, SameyTag, SameyTagPost},
    samey_post, samey_tag, samey_tag_post,
};

#[derive(Debug, FromQueryResult)]
pub(crate) struct SearchPost {
    pub(crate) id: i32,
    pub(crate) thumbnail: String,
    pub(crate) tags: String,
}

pub(crate) fn search_posts(tags: Option<&Vec<&str>>) -> Selector<SelectModel<SearchPost>> {
    let tags: HashSet<String> = match tags {
        Some(tags) if !tags.is_empty() => tags.iter().map(|&tag| tag.to_lowercase()).collect(),
        _ => HashSet::new(),
    };

    if tags.is_empty() {
        let query = SameyPost::find()
            .select_only()
            .column(samey_post::Column::Id)
            .column(samey_post::Column::Thumbnail)
            .column_as(
                Expr::cust("GROUP_CONCAT(\"samey_tag\".\"name\", ' ')"),
                "tags",
            )
            .inner_join(SameyTagPost)
            .join(
                sea_orm::JoinType::InnerJoin,
                samey_tag_post::Relation::SameyTag.def(),
            )
            .group_by(samey_post::Column::Id)
            .order_by_desc(samey_post::Column::Id);
        // println!("{}", &query.build(sea_orm::DatabaseBackend::Sqlite).sql);
        return query.into_model::<SearchPost>();
    };

    let tags_count = tags.len() as u32;
    let subquery = Query::select()
        .column((SameyPost, samey_post::Column::Id))
        .from(SameyPost)
        .inner_join(
            SameyTagPost,
            Expr::col((SameyPost, samey_post::Column::Id))
                .equals((SameyTagPost, samey_tag_post::Column::PostId)),
        )
        .inner_join(
            SameyTag,
            Expr::col((SameyTagPost, samey_tag_post::Column::TagId))
                .equals((SameyTag, samey_tag::Column::Id)),
        )
        .and_where(samey_tag::Column::NormalizedName.is_in(tags))
        .group_by_col((SameyPost, samey_post::Column::Id))
        .and_having(samey_tag::Column::Id.count().eq(tags_count))
        .to_owned();
    let query = SameyPost::find()
        .select_only()
        .column(samey_post::Column::Id)
        .column(samey_post::Column::Thumbnail)
        .column_as(
            Expr::cust("GROUP_CONCAT(\"samey_tag\".\"name\", ' ')"),
            "tags",
        )
        .inner_join(SameyTagPost)
        .join(
            sea_orm::JoinType::InnerJoin,
            samey_tag_post::Relation::SameyTag.def(),
        )
        .filter(samey_post::Column::Id.in_subquery(subquery))
        .group_by(samey_post::Column::Id)
        .order_by_desc(samey_post::Column::Id);
    // println!("{}", &query.build(sea_orm::DatabaseBackend::Sqlite).sql);
    query.into_model::<SearchPost>()
}

pub(crate) fn get_tags_for_post(post_id: i32) -> Select<SameyTag> {
    SameyTag::find()
        .inner_join(SameyTagPost)
        .filter(samey_tag_post::Column::PostId.eq(post_id))
        .order_by_asc(samey_tag::Column::Name)
}
