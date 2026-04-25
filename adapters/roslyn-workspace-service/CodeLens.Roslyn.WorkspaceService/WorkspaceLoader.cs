using System.Text;
using Microsoft.Build.Locator;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.MSBuild;
using Microsoft.CodeAnalysis.Text;

namespace CodeLens.Roslyn.WorkspaceService;

internal sealed record LoadedWorkspace(Workspace Workspace, Solution Solution) : IDisposable
{
    public void Dispose() => Workspace.Dispose();
}

internal static class WorkspaceLoader
{
    public static async Task<LoadedWorkspace> LoadAsync(string projectRoot)
    {
        var root = Path.GetFullPath(projectRoot);
        if (!Directory.Exists(root))
        {
            throw new DirectoryNotFoundException($"project_root does not exist: {root}");
        }

        var msbuildWorkspace = await TryLoadMsBuildWorkspaceAsync(root);
        if (msbuildWorkspace is not null)
        {
            return msbuildWorkspace;
        }

        return LoadAdhocWorkspace(root);
    }

    private static async Task<LoadedWorkspace?> TryLoadMsBuildWorkspaceAsync(string root)
    {
        RegisterMsBuild();

        var solutionFile = Directory
            .EnumerateFiles(root, "*.sln", SearchOption.TopDirectoryOnly)
            .Order(StringComparer.Ordinal)
            .FirstOrDefault();
        var projectFile = Directory
            .EnumerateFiles(root, "*.csproj", SearchOption.TopDirectoryOnly)
            .Order(StringComparer.Ordinal)
            .FirstOrDefault();
        if (solutionFile is null && projectFile is null)
        {
            return null;
        }

        var workspace = MSBuildWorkspace.Create();
        workspace.WorkspaceFailed += (_, args) =>
        {
            if (args.Diagnostic.Kind == WorkspaceDiagnosticKind.Failure)
            {
                Console.Error.WriteLine(args.Diagnostic.Message);
            }
        };

        try
        {
            var solution = solutionFile is not null
                ? await workspace.OpenSolutionAsync(solutionFile)
                : (await workspace.OpenProjectAsync(projectFile!)).Solution;
            return new LoadedWorkspace(workspace, solution);
        }
        catch (Exception error)
        {
            Console.Error.WriteLine($"MSBuildWorkspace load failed; falling back to AdhocWorkspace: {error.Message}");
            workspace.Dispose();
            return null;
        }
    }

    private static void RegisterMsBuild()
    {
        if (MSBuildLocator.IsRegistered)
        {
            return;
        }

        var instances = MSBuildLocator.QueryVisualStudioInstances().ToArray();
        if (instances.Length > 0)
        {
            MSBuildLocator.RegisterInstance(instances.OrderByDescending(item => item.Version).First());
        }
        else
        {
            MSBuildLocator.RegisterDefaults();
        }
    }

    private static LoadedWorkspace LoadAdhocWorkspace(string root)
    {
        var workspace = new AdhocWorkspace();
        var projectId = ProjectId.CreateNewId("CodeLensRoslynProject");
        var projectInfo = ProjectInfo
            .Create(projectId, VersionStamp.Create(), "CodeLensRoslynProject", "CodeLensRoslynProject", LanguageNames.CSharp)
            .WithCompilationOptions(new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary))
            .WithParseOptions(CSharpParseOptions.Default.WithLanguageVersion(LanguageVersion.Preview))
            .WithMetadataReferences(TrustedPlatformReferences());

        var solution = workspace.CurrentSolution.AddProject(projectInfo);
        foreach (var file in EnumerateCSharpFiles(root))
        {
            var source = File.ReadAllText(file, Encoding.UTF8);
            var documentId = DocumentId.CreateNewId(projectId, Path.GetFileName(file));
            solution = solution.AddDocument(
                documentId,
                Path.GetFileName(file),
                SourceText.From(source, Encoding.UTF8),
                filePath: file
            );
        }
        if (!workspace.TryApplyChanges(solution))
        {
            throw new InvalidOperationException("failed to create Roslyn AdhocWorkspace");
        }
        return new LoadedWorkspace(workspace, workspace.CurrentSolution);
    }

    private static IEnumerable<string> EnumerateCSharpFiles(string root)
    {
        return Directory
            .EnumerateFiles(root, "*.cs", SearchOption.AllDirectories)
            .Where(path =>
            {
                var relative = Path.GetRelativePath(root, path);
                var parts = relative.Split(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
                return !parts.Any(part => part is "bin" or "obj" or ".git" or ".codelens");
            })
            .Order(StringComparer.Ordinal);
    }

    private static IEnumerable<MetadataReference> TrustedPlatformReferences()
    {
        var raw = AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES") as string;
        return (raw ?? string.Empty)
            .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries)
            .Select(path => MetadataReference.CreateFromFile(path));
    }
}
