use chrono::{DateTime, FixedOffset, NaiveDateTime};
use uuid::Uuid;

use crate::CellReference;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommentTimestamp {
    Local(NaiveDateTime),
    Offset(DateTime<FixedOffset>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommentPerson {
    id: Uuid,
    display_name: String,
    user_id: Option<String>,
    provider_id: Option<String>,
}

impl CommentPerson {
    pub(crate) fn new(
        id: Uuid,
        display_name: String,
        user_id: Option<String>,
        provider_id: Option<String>,
    ) -> Self {
        Self { id, display_name, user_id, provider_id }
    }

    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    #[must_use]
    pub fn provider_id(&self) -> Option<&str> {
        self.provider_id.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadedCommentReply {
    id: Uuid,
    parent_id: Uuid,
    person_id: Uuid,
    person: Option<CommentPerson>,
    created_at: Option<CommentTimestamp>,
    text: String,
}

impl ThreadedCommentReply {
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    #[must_use]
    pub const fn parent_id(&self) -> Uuid {
        self.parent_id
    }
    #[must_use]
    pub const fn person_id(&self) -> Uuid {
        self.person_id
    }
    #[must_use]
    pub fn person(&self) -> Option<&CommentPerson> {
        self.person.as_ref()
    }
    #[must_use]
    pub const fn created_at(&self) -> Option<&CommentTimestamp> {
        self.created_at.as_ref()
    }
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadedComment {
    id: Uuid,
    cell: CellReference,
    person_id: Uuid,
    person: Option<CommentPerson>,
    created_at: Option<CommentTimestamp>,
    resolved: bool,
    text: String,
    replies: Vec<ThreadedCommentReply>,
}

impl ThreadedComment {
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    #[must_use]
    pub const fn cell(&self) -> CellReference {
        self.cell
    }
    #[must_use]
    pub const fn person_id(&self) -> Uuid {
        self.person_id
    }
    #[must_use]
    pub fn person(&self) -> Option<&CommentPerson> {
        self.person.as_ref()
    }
    #[must_use]
    pub const fn created_at(&self) -> Option<&CommentTimestamp> {
        self.created_at.as_ref()
    }
    #[must_use]
    pub const fn resolved(&self) -> bool {
        self.resolved
    }
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
    #[must_use]
    pub fn replies(&self) -> &[ThreadedCommentReply] {
        &self.replies
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteComment {
    id: Option<Uuid>,
    cell: CellReference,
    author: Option<String>,
    text: String,
}

impl NoteComment {
    #[must_use]
    pub const fn id(&self) -> Option<Uuid> {
        self.id
    }
    #[must_use]
    pub const fn cell(&self) -> CellReference {
        self.cell
    }
    #[must_use]
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetComments {
    sheet_name: String,
    threaded_comments: Vec<ThreadedComment>,
    notes: Vec<NoteComment>,
}

impl SheetComments {
    pub(crate) fn new(
        sheet_name: String,
        threaded_comments: Vec<ThreadedComment>,
        notes: Vec<NoteComment>,
    ) -> Self {
        Self { sheet_name, threaded_comments, notes }
    }

    #[must_use]
    pub fn sheet_name(&self) -> &str {
        &self.sheet_name
    }
    #[must_use]
    pub fn threaded_comments(&self) -> &[ThreadedComment] {
        &self.threaded_comments
    }
    #[must_use]
    pub fn notes(&self) -> &[NoteComment] {
        &self.notes
    }
}

pub(crate) struct ThreadedCommentParts {
    pub id: Uuid,
    pub cell: CellReference,
    pub person_id: Uuid,
    pub person: Option<CommentPerson>,
    pub created_at: Option<CommentTimestamp>,
    pub resolved: bool,
    pub text: String,
    pub replies: Vec<ThreadedCommentReply>,
}

impl From<ThreadedCommentParts> for ThreadedComment {
    fn from(parts: ThreadedCommentParts) -> Self {
        Self {
            id: parts.id,
            cell: parts.cell,
            person_id: parts.person_id,
            person: parts.person,
            created_at: parts.created_at,
            resolved: parts.resolved,
            text: parts.text,
            replies: parts.replies,
        }
    }
}

pub(crate) struct ThreadedReplyParts {
    pub id: Uuid,
    pub parent_id: Uuid,
    pub person_id: Uuid,
    pub person: Option<CommentPerson>,
    pub created_at: Option<CommentTimestamp>,
    pub text: String,
}

impl From<ThreadedReplyParts> for ThreadedCommentReply {
    fn from(parts: ThreadedReplyParts) -> Self {
        Self {
            id: parts.id,
            parent_id: parts.parent_id,
            person_id: parts.person_id,
            person: parts.person,
            created_at: parts.created_at,
            text: parts.text,
        }
    }
}

pub(crate) struct NoteParts {
    pub id: Option<Uuid>,
    pub cell: CellReference,
    pub author: Option<String>,
    pub text: String,
}

impl From<NoteParts> for NoteComment {
    fn from(parts: NoteParts) -> Self {
        Self { id: parts.id, cell: parts.cell, author: parts.author, text: parts.text }
    }
}
