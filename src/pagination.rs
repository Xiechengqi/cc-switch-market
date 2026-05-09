use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl<T> Page<T> {
    pub fn from_items(
        mut items: Vec<T>,
        requested_limit: i64,
        cursor_fn: impl Fn(&T) -> String,
    ) -> Self {
        let limit = requested_limit.clamp(1, 200) as usize;
        let has_more = items.len() > limit;
        if has_more {
            items.truncate(limit);
        }
        let next_cursor = if has_more {
            items.last().map(cursor_fn)
        } else {
            None
        };
        Self {
            items,
            next_cursor,
            has_more,
        }
    }
}

pub fn query_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 200)
}

pub fn fetch_limit(limit: Option<i64>) -> i64 {
    query_limit(limit) + 1
}
