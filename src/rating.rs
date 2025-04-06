use std::fmt::Display;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rating {
    Unrated,
    Safe,
    Questionable,
    Explicit,
}

impl From<String> for Rating {
    fn from(value: String) -> Self {
        match value.as_ref() {
            "s" => Self::Safe,
            "q" => Self::Questionable,
            "e" => Self::Explicit,
            _ => Self::Unrated,
        }
    }
}

impl Display for Rating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rating::Unrated => f.write_str("Unrated"),
            Rating::Safe => f.write_str("Safe"),
            Rating::Questionable => f.write_str("Questionable"),
            Rating::Explicit => f.write_str("Explicit"),
        }
    }
}
