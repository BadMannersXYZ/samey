use std::collections::HashSet;

use chrono::NaiveDateTime;
use samey_migration::{Expr, Query};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, FromQueryResult, IntoIdentity,
    IntoSimpleExpr, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Select, SelectColumns,
    SelectModel, Selector,
};

use crate::{
    NEGATIVE_PREFIX, RATING_PREFIX, SameyError,
    auth::User,
    entities::{
        prelude::{SameyPool, SameyPoolPost, SameyPost, SameyTag, SameyTagPost},
        samey_pool, samey_pool_post, samey_post, samey_tag, samey_tag_post,
    },
};

#[derive(Debug, FromQueryResult)]
pub(crate) struct PostOverview {
    pub(crate) id: i32,
    pub(crate) thumbnail: String,
    pub(crate) media: String,
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) uploaded_at: NaiveDateTime,
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
            .column(samey_post::Column::Media)
            .column(samey_post::Column::Title)
            .column(samey_post::Column::Description)
            .column(samey_post::Column::UploadedAt)
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
            .column(samey_post::Column::Media)
            .column(samey_post::Column::Title)
            .column(samey_post::Column::Description)
            .column(samey_post::Column::UploadedAt)
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

#[derive(Debug)]
pub(crate) struct PostPoolData {
    pub(crate) id: i32,
    pub(crate) name: String,
    pub(crate) previous_post_id: Option<i32>,
    pub(crate) next_post_id: Option<i32>,
}

#[derive(Debug, FromQueryResult)]
struct PostInPool {
    id: i32,
    name: String,
    position: f32,
}

pub(crate) async fn get_pool_data_for_post(
    db: &DatabaseConnection,
    post_id: i32,
    user: Option<&User>,
) -> Result<Vec<PostPoolData>, SameyError> {
    let mut query = SameyPool::find()
        .inner_join(SameyPoolPost)
        .select_column(samey_pool_post::Column::Position)
        .filter(samey_pool_post::Column::PostId.eq(post_id));
    query = match user {
        None => query.filter(samey_pool::Column::IsPublic.into_simple_expr()),
        Some(user) if user.is_admin => query,
        Some(user) => query.filter(
            Condition::any()
                .add(samey_pool::Column::IsPublic.into_simple_expr())
                .add(samey_pool::Column::UploaderId.eq(user.id)),
        ),
    };
    let pools = query.into_model::<PostInPool>().all(db).await?;

    let mut post_pool_datas = Vec::with_capacity(pools.len());
    for pool in pools.into_iter() {
        let posts_in_pool = get_posts_in_pool(pool.id, user).all(db).await?;
        if let Ok(index) = posts_in_pool.binary_search_by(|post| {
            post.position
                .partial_cmp(&pool.position)
                .expect("position should never be NaN")
        }) {
            post_pool_datas.push(PostPoolData {
                id: pool.id,
                name: pool.name,
                previous_post_id: index
                    .checked_sub(1)
                    .and_then(|idx| posts_in_pool.get(idx))
                    .map(|post| post.id),
                next_post_id: posts_in_pool.get(index + 1).map(|post| post.id),
            });
        }
    }

    Ok(post_pool_datas)
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

pub(crate) async fn clean_dangling_tags(db: &DatabaseConnection) -> Result<(), SameyError> {
    let dangling_tags = SameyTag::find()
        .select_column_as(samey_tag_post::Column::Id.count(), "count")
        .left_join(SameyTagPost)
        .group_by(samey_tag::Column::Id)
        .having(Expr::column("count".into_identity()).eq(0))
        .all(db)
        .await?;
    SameyTag::delete_many()
        .filter(samey_tag::Column::Id.is_in(dangling_tags.into_iter().map(|tag| tag.id)))
        .exec(db)
        .await?;
    Ok(())
}
