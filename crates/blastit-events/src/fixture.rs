//! The two example events of ARCHITECTURE.md §8.1, shared by the encoder tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use time::{Duration, OffsetDateTime};

use crate::{Component, Event, EventKind, RequestInfo, SqlDenied, SqlKind};

/// `2026-10-25T14:02:11.384Z`.
pub(crate) fn request_ts() -> OffsetDateTime {
    at(1_792_936_931, 384)
}

/// `2026-10-25T14:02:12.007Z`.
pub(crate) fn sql_denied_ts() -> OffsetDateTime {
    at(1_792_936_932, 7)
}

fn at(seconds: i64, millis: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(seconds).unwrap() + Duration::milliseconds(millis)
}

/// Any event of `kind` at the request example's timestamp.
pub(crate) fn event(kind: EventKind) -> Event {
    Event::at(request_ts(), Component::Init, kind)
}

/// §8.1's first example line.
pub(crate) fn request_event() -> Event {
    let info = RequestInfo::from_target("GET", "/products/42/")
        .with_rule(0)
        .with_status(200)
        .with_timings(0.1, 18.4)
        .with_bytes(0, 5120)
        .with_remote_addr("203.0.113.7");
    Event::at(request_ts(), Component::Scheduler, EventKind::Request(info))
        .with_request_id("01JB9")
        .with_route("product-detail")
}

/// §8.1's second example line.
pub(crate) fn sql_denied_event() -> Event {
    let denied = SqlDenied::new(
        57,
        "db",
        SqlKind::Parse,
        r#"SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?"#,
        "42501",
        false,
    );
    Event::at(
        sql_denied_ts(),
        Component::Dbproxy,
        EventKind::SqlDenied(denied),
    )
    .with_route("product-detail")
}
