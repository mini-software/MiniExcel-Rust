namespace MiniExcelLibs;

public sealed class MiniExcelRustCommentResult
{
    internal MiniExcelRustCommentResult(
        string sheetName,
        List<MiniExcelRustThreadedComment> comments,
        List<MiniExcelRustNoteComment> notes)
    {
        SheetName = sheetName;
        Comments = comments;
        Notes = notes;
    }

    public string SheetName { get; }

    public IReadOnlyList<MiniExcelRustThreadedComment> Comments { get; }

    public IReadOnlyList<MiniExcelRustNoteComment> Notes { get; }
}

public sealed class MiniExcelRustThreadedComment
{
    internal MiniExcelRustThreadedComment(
        Guid id,
        string referenceCell,
        MiniExcelRustCommentAuthor? author,
        DateTime? createdAt,
        bool resolved,
        string text,
        List<MiniExcelRustThreadedCommentReply> replies)
    {
        Id = id;
        ReferenceCell = referenceCell;
        Author = author;
        CreatedAt = createdAt;
        Resolved = resolved;
        Text = text;
        Replies = replies;
    }

    public Guid Id { get; }
    public string ReferenceCell { get; }
    public MiniExcelRustCommentAuthor? Author { get; }
    public DateTime? CreatedAt { get; }
    public bool Resolved { get; }
    public string Text { get; }
    public IReadOnlyList<MiniExcelRustThreadedCommentReply> Replies { get; }
}

public sealed class MiniExcelRustThreadedCommentReply
{
    internal MiniExcelRustThreadedCommentReply(
        Guid id,
        Guid parentId,
        MiniExcelRustCommentAuthor? author,
        DateTime? createdAt,
        string text)
    {
        Id = id;
        ParentId = parentId;
        Author = author;
        CreatedAt = createdAt;
        Text = text;
    }

    public Guid Id { get; }
    public Guid ParentId { get; }
    public MiniExcelRustCommentAuthor? Author { get; }
    public DateTime? CreatedAt { get; }
    public string Text { get; }
}

public sealed class MiniExcelRustNoteComment
{
    internal MiniExcelRustNoteComment(Guid? id, string referenceCell, string? author, string text)
    {
        Id = id;
        ReferenceCell = referenceCell;
        Author = author;
        Text = text;
    }

    public Guid? Id { get; }
    public string ReferenceCell { get; }
    public string? Author { get; }
    public string Text { get; }
}

public sealed class MiniExcelRustCommentAuthor
{
    internal MiniExcelRustCommentAuthor(Guid id, string displayName, string? providerId)
    {
        Id = id;
        DisplayName = displayName;
        ProviderId = providerId;
    }

    public Guid Id { get; }
    public string DisplayName { get; }
    public string? ProviderId { get; }
}