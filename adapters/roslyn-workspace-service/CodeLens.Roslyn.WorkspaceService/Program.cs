using System.Text.Json;

namespace CodeLens.Roslyn.WorkspaceService;

internal static class Program
{
    public static async Task<int> Main()
    {
        try
        {
            var stdin = await Console.In.ReadToEndAsync();
            var request = AdapterRequest.Parse(stdin);
            var response = request.Operation switch
            {
                "rename" => await RenameOperation.RunAsync(request),
                _ => AdapterResponse.Failure(
                    $"unsupported operation `{request.Operation}`; only `rename` is implemented"
                ),
            };
            Console.WriteLine(JsonSerializer.Serialize(response.Payload, JsonOptions.Default));
            return response.Success ? 0 : 2;
        }
        catch (Exception error)
        {
            Console.WriteLine(
                JsonSerializer.Serialize(
                    AdapterResponse.Failure(error.Message).Payload,
                    JsonOptions.Default
                )
            );
            return 1;
        }
    }
}
