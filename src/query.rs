use std::collections::HashSet;

use migration::{Expr, Query};
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, FromQueryResult, IntoSimpleExpr, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait, Select, SelectModel, Selector,
};

use crate::{
    NEGATIVE_PREFIX, RATING_PREFIX,
    auth::User,
    entities::{
        prelude::{SameyPoolPost, SameyPost, SameyTag, SameyTagPost},
        samey_pool_post, samey_post, samey_tag, samey_tag_post,
    },
};

#[derive(Debug, FromQueryResult)]
pub(crate) struct PostOverview {
    pub(crate) id: i32,
    pub(crate) thumbnail: String,
    pub(crate) tags: Option<String>,
    pub(crate) media_type: String,
    pub(crate) rating: String,
}

pub(crate) fn search_posts(
    tags: Option<&Vec<&str>>,
    user: Option<&User>,
) -> Selector<SelectModel<PostOverview>> {
    let mut include_tags = HashSet::<String>::new();
    let mut exclude_tags = HashSet::<String>::new();
    let mut include_ratings = HashSet::<String>::new();
    let mut exclude_ratings = HashSet::<String>::new();
    if let Some(tags) = tags {
        for mut tag in tags.iter().map(|tag| tag.to_lowercase()) {
            if tag.starts_with(NEGATIVE_PREFIX) {
                if tag.as_str()[NEGATIVE_PREFIX.len()..].starts_with(RATING_PREFIX) {
                    exclude_ratings
                        .insert(tag.split_off(NEGATIVE_PREFIX.len() + RATING_PREFIX.len()));
                } else {
                    exclude_tags.insert(tag.split_off(NEGATIVE_PREFIX.len()));
                }
            } else if tag.starts_with(RATING_PREFIX) {
                include_ratings.insert(tag.split_off(RATING_PREFIX.len()));
            } else {
                include_tags.insert(tag);
            }
        }
    }

    let query = if include_tags.is_empty() && exclude_tags.is_empty() {
        let mut query = SameyPost::find()
            .select_only()
            .column(samey_post::Column::Id)
            .column(samey_post::Column::Thumbnail)
            .column(samey_post::Column::Rating)
            .column(samey_post::Column::MediaType)
            .column_as(
                Expr::cust("GROUP_CONCAT(\"samey_tag\".\"name\", ' ')"),
                "tags",
            )
            .left_join(SameyTagPost)
            .join(
                sea_orm::JoinType::LeftJoin,
                samey_tag_post::Relation::SameyTag.def(),
            );
        if !include_ratings.is_empty() {
            query = query.filter(samey_post::Column::Rating.is_in(include_ratings))
        }
        if !exclude_ratings.is_empty() {
            query = query.filter(samey_post::Column::Rating.is_not_in(exclude_ratings))
        }
        query
    } else {
        let mut query = SameyPost::find()
            .select_only()
            .column(samey_post::Column::Id)
            .column(samey_post::Column::Thumbnail)
            .column(samey_post::Column::Rating)
            .column(samey_post::Column::MediaType)
            .column_as(
                Expr::cust("GROUP_CONCAT(\"samey_tag\".\"name\", ' ')"),
                "tags",
            )
            .left_join(SameyTagPost)
            .join(
                sea_orm::JoinType::LeftJoin,
                samey_tag_post::Relation::SameyTag.def(),
            );
        if !include_tags.is_empty() {
            let include_tags_count = include_tags.len() as u32;
            let include_tags_subquery = Query::select()
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
                .and_where(samey_tag::Column::NormalizedName.is_in(include_tags))
                .group_by_col((SameyPost, samey_post::Column::Id))
                .and_having(samey_tag::Column::Id.count().eq(include_tags_count))
                .to_owned();
            query = query.filter(samey_post::Column::Id.in_subquery(include_tags_subquery));
        }
        if !exclude_tags.is_empty() {
            let exclude_tags_subquery = Query::select()
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
                .and_where(samey_tag::Column::NormalizedName.is_in(exclude_tags))
                .to_owned();
            query = query.filter(samey_post::Column::Id.not_in_subquery(exclude_tags_subquery));
        }
        if !include_ratings.is_empty() {
            query = query.filter(samey_post::Column::Rating.is_in(include_ratings))
        }
        if !exclude_ratings.is_empty() {
            query = query.filter(samey_post::Column::Rating.is_not_in(exclude_ratings))
        }
        query
    };

    filter_posts_by_user(query, user)
        .group_by(samey_post::Column::Id)
        .order_by_desc(samey_post::Column::Id)
        .into_model::<PostOverview>()
}

pub(crate) fn get_tags_for_post(post_id: i32) -> Select<SameyTag> {
    SameyTag::find()
        .inner_join(SameyTagPost)
        .filter(samey_tag_post::Column::PostId.eq(post_id))
        .order_by_asc(samey_tag::Column::Name)
}

#[derive(Debug, FromQueryResult)]
pub(crate) struct PoolPost {
    pub(crate) id: i32,
    pub(crate) thumbnail: String,
    pub(crate) rating: String,
    pub(crate) media_type: String,
    pub(crate) pool_post_id: i32,
    pub(crate) position: f32,
    pub(crate) tags: String,
}

pub(crate) fn get_posts_in_pool(
    pool_id: i32,
    user: Option<&User>,
) -> Selector<SelectModel<PoolPost>> {
    filter_posts_by_user(
        SameyPost::find()
            .column(samey_post::Column::Id)
            .column(samey_post::Column::Thumbnail)
            .column(samey_post::Column::Rating)
            .column(samey_post::Column::MediaType)
            .column_as(samey_pool_post::Column::Id, "pool_post_id")
            .column(samey_pool_post::Column::Position)
            .column_as(
                Expr::cust("GROUP_CONCAT(\"samey_tag\".\"name\", ' ')"),
                "tags",
            )
            .inner_join(SameyPoolPost)
            .inner_join(SameyTagPost)
            .join(
                sea_orm::JoinType::InnerJoin,
                samey_tag_post::Relation::SameyTag.def(),
            )
            .filter(samey_pool_post::Column::PoolId.eq(pool_id)),
        user,
    )
    .group_by(samey_post::Column::Id)
    .order_by_asc(samey_pool_post::Column::Position)
    .into_model::<PoolPost>()
}

pub(crate) fn filter_posts_by_user(
    query: Select<SameyPost>,
    user: Option<&User>,
) -> Select<SameyPost> {
    match user {
        None => query.filter(samey_post::Column::IsPublic.into_simple_expr()),
        Some(user) if user.is_admin => query,
        Some(user) => query.filter(
            Condition::any()
                .add(samey_post::Column::IsPublic.into_simple_expr())
                .add(samey_post::Column::UploaderId.eq(user.id)),
        ),
    }
}
