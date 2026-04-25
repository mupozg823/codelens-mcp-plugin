using System.Text.Json;
using System.Text.Json.Nodes;

namespace CodeLens.Roslyn.WorkspaceService;

internal sealed record AdapterRequest(
    string Backend,
    string Tool,
    string Operation,
    string ProjectRoot,
    JsonObject Arguments,
    bool DryRun
)
{
    public static AdapterRequest Parse(string rawJson)
    {
        if (string.IsNullOrWhiteSpace(rawJson))
        {
            throw new InvalidOperationException("empty adapter request");
        }

        var root =
            JsonNode.Parse(rawJson)?.AsObject()
            ?? throw new InvalidOperationException("adapter request must be a JSON object");
        var schema = RequiredString(root, "schema_version");
        if (schema != "codelens-semantic-adapter-request-v1")
        {
            throw new InvalidOperationException($"unsupported schema_version `{schema}`");
        }

        var backend = RequiredString(root, "backend");
        if (backend != "roslyn")
        {
            throw new InvalidOperationException($"roslyn adapter received backend `{backend}`");
        }

        return new AdapterRequest(
            backend,
            RequiredString(root, "tool"),
            RequiredString(root, "operation"),
            RequiredString(root, "project_root"),
            RequiredObject(root, "arguments"),
            root["dry_run"]?.GetValue<bool>() ?? true
        );
    }

    public string RequiredArgument(string name) => RequiredString(Arguments, name);

    public int RequiredIntArgument(string name)
    {
        var value = Arguments[name];
        if (value is null)
        {
            throw new InvalidOperationException($"missing argument `{name}`");
        }
        return value.GetValue<int>();
    }

    public string? OptionalArgument(string name) => Arguments[name]?.GetValue<string>();

    private static string RequiredString(JsonObject root, string name)
    {
        var value = root[name]?.GetValue<string>();
        if (string.IsNullOrWhiteSpace(value))
        {
            throw new InvalidOperationException($"missing `{name}`");
        }
        return value;
    }

    private static JsonObject RequiredObject(JsonObject root, string name)
    {
        return root[name]?.AsObject()
            ?? throw new InvalidOperationException($"missing object `{name}`");
    }
}

internal sealed record AdapterResponse(bool Success, JsonObject Payload)
{
    public static AdapterResponse Failure(string error)
    {
        return new AdapterResponse(
            false,
            new JsonObject
            {
                ["success"] = false,
                ["error"] = error,
                ["adapter"] = new JsonObject
                {
                    ["backend"] = "roslyn",
                    ["protocol"] = "codelens-semantic-adapter-v1",
                },
            }
        );
    }
}

internal static class JsonOptions
{
    public static readonly JsonSerializerOptions Default =
        new(JsonSerializerDefaults.Web) { WriteIndented = false };
}
