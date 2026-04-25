using System.Text.Json.Nodes;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.FindSymbols;
using Microsoft.CodeAnalysis.Rename;

namespace CodeLens.Roslyn.WorkspaceService;

internal static class RenameOperation
{
    public static async Task<AdapterResponse> RunAsync(AdapterRequest request)
    {
        var started = DateTimeOffset.UtcNow;
        var root = Path.GetFullPath(request.ProjectRoot);
        var filePath = Path.GetFullPath(Path.Combine(root, request.RequiredArgument("file_path")));
        var newName = request.RequiredArgument("new_name");
        var line = request.RequiredIntArgument("line");
        var column = request.RequiredIntArgument("column");

        using var loaded = await WorkspaceLoader.LoadAsync(root);
        var document = FindDocument(loaded.Solution, filePath);
        var position = await ToRoslynPositionAsync(document, line, column);
        var symbol = await SymbolFinder.FindSymbolAtPositionAsync(document, position);
        if (symbol is null)
        {
            throw new InvalidOperationException($"no symbol found at {line}:{column} in {request.RequiredArgument("file_path")}");
        }

        var renamed = await Renamer.RenameSymbolAsync(
            loaded.Solution,
            symbol,
            new SymbolRenameOptions(
                RenameOverloads: false,
                RenameInStrings: false,
                RenameInComments: false,
                RenameFile: false
            ),
            newName
        );
        var edit = await LspWorkspaceEdit.FromSolutionChangesAsync(loaded.Solution, renamed);
        var elapsedMs = (int)(DateTimeOffset.UtcNow - started).TotalMilliseconds;
        return new AdapterResponse(
            true,
            new()
            {
                ["success"] = true,
                ["message"] = $"Roslyn rename produced {edit.EditCount} edit(s) in {edit.FileCount} file(s)",
                ["workspace_edit"] = edit.Json,
                ["adapter"] = new JsonObject
                {
                    ["backend"] = "roslyn",
                    ["protocol"] = "codelens-semantic-adapter-v1",
                    ["operation"] = "rename",
                    ["authority"] = "roslyn_workspace",
                    ["dry_run"] = request.DryRun,
                    ["elapsed_ms"] = elapsedMs,
                },
            }
        );
    }

    private static Document FindDocument(Solution solution, string filePath)
    {
        var document = solution
            .Projects
            .SelectMany(project => project.Documents)
            .FirstOrDefault(doc =>
                doc.FilePath is not null
                && Path.GetFullPath(doc.FilePath).Equals(filePath, StringComparison.OrdinalIgnoreCase)
            );
        return document ?? throw new FileNotFoundException($"document not found in Roslyn workspace: {filePath}");
    }

    private static async Task<int> ToRoslynPositionAsync(Document document, int oneBasedLine, int oneBasedColumn)
    {
        if (oneBasedLine < 1 || oneBasedColumn < 1)
        {
            throw new ArgumentOutOfRangeException(nameof(oneBasedLine), "line and column are 1-based");
        }

        var text = await document.GetTextAsync();
        var line = text.Lines[oneBasedLine - 1];
        var offset = Math.Min(oneBasedColumn - 1, line.Span.Length);
        return line.Start + offset;
    }
}
