using System.Text.Json.Nodes;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.Text;

namespace CodeLens.Roslyn.WorkspaceService;

internal sealed record LspWorkspaceEdit(JsonObject Json, int FileCount, int EditCount)
{
    public static async Task<LspWorkspaceEdit> FromSolutionChangesAsync(Solution before, Solution after)
    {
        var changes = new JsonObject();
        var editCount = 0;
        foreach (var projectChange in after.GetChanges(before).GetProjectChanges())
        {
            foreach (var documentId in projectChange.GetChangedDocuments())
            {
                var oldDocument = before.GetDocument(documentId);
                var newDocument = after.GetDocument(documentId);
                if (oldDocument?.FilePath is null || newDocument is null)
                {
                    continue;
                }

                var oldText = await oldDocument.GetTextAsync();
                var textChanges = (await newDocument.GetTextChangesAsync(oldDocument)).ToArray();
                if (textChanges.Length == 0)
                {
                    continue;
                }

                var edits = new JsonArray();
                foreach (var change in textChanges.OrderByDescending(change => change.Span.Start))
                {
                    edits.Add(
                        new JsonObject
                        {
                            ["range"] = RangeFromSpan(oldText, change.Span),
                            ["newText"] = change.NewText ?? string.Empty,
                        }
                    );
                    editCount++;
                }
                changes[FileUri(oldDocument.FilePath)] = edits;
            }
        }

        if (editCount == 0)
        {
            throw new InvalidOperationException("Roslyn produced no document changes");
        }

        return new LspWorkspaceEdit(new JsonObject { ["changes"] = changes }, changes.Count, editCount);
    }

    private static string FileUri(string path) => new Uri(Path.GetFullPath(path)).AbsoluteUri;

    private static JsonObject RangeFromSpan(SourceText source, TextSpan span)
    {
        var startLine = source.Lines.GetLineFromPosition(span.Start);
        var endLine = source.Lines.GetLineFromPosition(span.End);
        return new JsonObject
        {
            ["start"] = new JsonObject
            {
                ["line"] = startLine.LineNumber,
                ["character"] = span.Start - startLine.Start,
            },
            ["end"] = new JsonObject
            {
                ["line"] = endLine.LineNumber,
                ["character"] = span.End - endLine.Start,
            },
        };
    }
}
