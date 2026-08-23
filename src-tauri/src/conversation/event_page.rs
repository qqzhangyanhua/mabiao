use std::path::Path;

use rusqlite::Connection;

use crate::domain::{ConversationEvent, ConversationEventAnchor, ConversationEventPage};

use super::{
    event_index, event_index_ready, load_prepared_parsed, prepare_detail,
    PreparedConversationDetail, MAX_PAGE_SIZE,
};

pub(crate) enum PreparedEventsRead {
    Ready(ConversationEventPage),
    NeedsParse(Box<PreparedConversationDetail>),
}

pub fn load_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: ConversationEventAnchor,
    limit: u32,
) -> Result<ConversationEventPage, String> {
    finish_prepared_events(
        home,
        prepare_events_read(conn, home, source, session_id, &anchor, limit)?,
        &anchor,
        limit,
    )
}

pub(crate) fn prepare_events_read(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> Result<PreparedEventsRead, String> {
    let prepared = prepare_detail(conn, source, session_id)?;
    if event_index_ready(conn, home, &prepared)? {
        let page = event_index::indexed_events_page(
            conn,
            prepared.source.as_str(),
            &prepared.session.session_id,
            anchor,
            limit,
        )?;
        return Ok(PreparedEventsRead::Ready(page));
    }
    Ok(PreparedEventsRead::NeedsParse(Box::new(prepared)))
}

pub(crate) fn finish_prepared_events(
    home: &Path,
    read: PreparedEventsRead,
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> Result<ConversationEventPage, String> {
    match read {
        PreparedEventsRead::Ready(page) => Ok(page),
        PreparedEventsRead::NeedsParse(prepared) => {
            let parsed = load_prepared_parsed(home, *prepared)?;
            Ok(paginate_events(&parsed.events, anchor, limit))
        }
    }
}

pub(crate) fn paginate_events(
    events: &[ConversationEvent],
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> ConversationEventPage {
    let limit = limit.clamp(1, MAX_PAGE_SIZE) as usize;
    if events.is_empty() {
        return ConversationEventPage {
            events: Vec::new(),
            has_more_before: false,
            has_more_after: false,
        };
    }

    let (start, end) = match anchor {
        ConversationEventAnchor::First => (0, limit.min(events.len())),
        ConversationEventAnchor::Last => (events.len().saturating_sub(limit), events.len()),
        ConversationEventAnchor::Before { sequence } => {
            let prefix_end = events
                .iter()
                .position(|event| event.sequence >= *sequence)
                .unwrap_or(events.len());
            (prefix_end.saturating_sub(limit), prefix_end)
        }
        ConversationEventAnchor::After { sequence } => {
            let start = events
                .iter()
                .position(|event| event.sequence > *sequence)
                .unwrap_or(events.len());
            (start, (start + limit).min(events.len()))
        }
    };

    if start >= end {
        let (has_more_before, has_more_after) = match anchor {
            ConversationEventAnchor::First | ConversationEventAnchor::Last => (false, false),
            ConversationEventAnchor::Before { sequence } => (
                false,
                events.iter().any(|event| event.sequence >= *sequence),
            ),
            ConversationEventAnchor::After { sequence } => (
                events.iter().any(|event| event.sequence <= *sequence),
                false,
            ),
        };
        return ConversationEventPage {
            events: Vec::new(),
            has_more_before,
            has_more_after,
        };
    }

    let page = events[start..end].to_vec();
    let min_sequence = page[0].sequence;
    let max_sequence = page[page.len() - 1].sequence;
    ConversationEventPage {
        events: page,
        has_more_before: events.iter().any(|event| event.sequence < min_sequence),
        has_more_after: events.iter().any(|event| event.sequence > max_sequence),
    }
}
