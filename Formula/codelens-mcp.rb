class CodelensMcp < Formula
  desc "Pure Rust MCP server for code intelligence — 49 tools, 16 languages"
  homepage "https://github.com/mupozg823/codelens-mcp-plugin"
  version "1.0.0"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/mupozg823/codelens-mcp-plugin/releases/download/v#{version}/codelens-mcp-darwin-arm64.tar.gz"
      # sha256 "PLACEHOLDER" # Updated on release
    else
      url "https://github.com/mupozg823/codelens-mcp-plugin/releases/download/v#{version}/codelens-mcp-darwin-x86_64.tar.gz"
      # sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/mupozg823/codelens-mcp-plugin/releases/download/v#{version}/codelens-mcp-linux-arm64.tar.gz"
      # sha256 "PLACEHOLDER"
    else
      url "https://github.com/mupozg823/codelens-mcp-plugin/releases/download/v#{version}/codelens-mcp-linux-x86_64.tar.gz"
      # sha256 "PLACEHOLDER"
    end
  end

  def install
    bin.install "codelens-mcp"
  end

  def caveats
    <<~EOS
      Add to your Claude Code MCP config (~/.claude.json):

        "codelens": {
          "type": "stdio",
          "command": "#{opt_bin}/codelens-mcp",
          "args": ["."]
        }
    EOS
  end

  test do
    assert_match "codelens-mcp", shell_output("#{bin}/codelens-mcp --help 2>&1", 0)
  end
end
