pub(crate) const NEGATIVE_PREFIX: &str = "-";
pub(crate) const RATING_PREFIX: &str = "rating:";
pub(crate) const MEDIA_TYPE_PREFIX: &str = "type:";

#[derive(strum::EnumIter, strum::Display, Debug)]
pub(crate) enum Rating {
    #[strum(serialize = "u")]
    Unrated,
    #[strum(serialize = "s")]
    Safe,
    #[strum(serialize = "q")]
    Questionable,
    #[strum(serialize = "e")]
    Explicit,
}

#[derive(strum::EnumIter, strum::Display, Debug)]
pub(crate) enum MediaType {
    #[strum(serialize = "image")]
    Image,
    #[strum(serialize = "video")]
    Video,
}
