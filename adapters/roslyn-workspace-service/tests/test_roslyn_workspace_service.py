import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
SERVICE_PROJECT = (
    REPO_ROOT
    / "adapters"
    / "roslyn-workspace-service"
    / "CodeLens.Roslyn.WorkspaceService"
    / "CodeLens.Roslyn.WorkspaceService.csproj"
)


class RoslynWorkspaceServiceTests(unittest.TestCase):
    def test_rename_returns_cross_file_workspace_edit(self) -> None:
        if shutil.which("dotnet") is None:
            self.skipTest("dotnet SDK is not installed")

        with tempfile.TemporaryDirectory(prefix="codelens-roslyn-fixture-") as raw_dir:
            project = Path(raw_dir)
            (project / "Fixture.csproj").write_text(
                '<Project Sdk="Microsoft.NET.Sdk">\n'
                "  <PropertyGroup>\n"
                "    <TargetFramework>net9.0</TargetFramework>\n"
                "    <ImplicitUsings>enable</ImplicitUsings>\n"
                "    <Nullable>enable</Nullable>\n"
                "  </PropertyGroup>\n"
                "</Project>\n",
                encoding="utf-8",
            )
            (project / "Widget.cs").write_text(
                "namespace Demo;\n\n"
                "public class Widget\n"
                "{\n"
                "    public string Name => nameof(Widget);\n"
                "}\n",
                encoding="utf-8",
            )
            (project / "Consumer.cs").write_text(
                "namespace Demo;\n\n"
                "public class Consumer\n"
                "{\n"
                "    public Widget Make() => new Widget();\n"
                "}\n",
                encoding="utf-8",
            )
            request = {
                "schema_version": "codelens-semantic-adapter-request-v1",
                "backend": "roslyn",
                "tool": "rename_symbol",
                "operation": "rename",
                "project_root": str(project),
                "arguments": {
                    "file_path": "Widget.cs",
                    "line": 3,
                    "column": 14,
                    "new_name": "RenamedWidget",
                    "dry_run": True,
                },
                "dry_run": True,
            }

            completed = subprocess.run(
                ["dotnet", "run", "--quiet", "--project", str(SERVICE_PROJECT)],
                input=json.dumps(request),
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=60,
                check=False,
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            payload = json.loads(completed.stdout)
            self.assertTrue(payload["success"], payload)
            self.assertEqual(payload["adapter"]["backend"], "roslyn")
            self.assertEqual(payload["adapter"]["operation"], "rename")

            changes = payload["workspace_edit"]["changes"]
            changed_paths = sorted(Path(uri.removeprefix("file://")).name for uri in changes)
            self.assertEqual(changed_paths, ["Consumer.cs", "Widget.cs"])
            self.assertGreaterEqual(sum(len(edits) for edits in changes.values()), 4)
            rendered = json.dumps(changes)
            self.assertIn("Renamed", rendered)
            self.assertNotIn("namespace Demo", rendered)


if __name__ == "__main__":
    unittest.main()
