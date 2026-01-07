# Seneschal Program

AI-powered assistant for Foundry VTT game masters, supporting rule lookups, entity creation, and campaign management. Built with a focus on Mongoose Traveller 2nd Edition but extensible to other game systems.

## Overview

Seneschal Program consists of two components:

1. **Backend Service** (`seneschal-service/`) - A Rust service that provides:
   - RAG (Retrieval-Augmented Generation) document search
   - Ollama LLM integration with streaming responses
   - MCP (Model Context Protocol) server interface
   - Document ingestion (PDF, EPUB, Markdown, plain text)
   - SQLite database for documents, embeddings, and conversations

2. **FVTT Module** (`fvtt-seneschal/`) - A Foundry VTT module that provides:
   - Chat panel for interactive AI assistance
   - One-shot `/sen-ai` chat command
   - FVTT API integration with permission checking
   - Streaming response display

## Requirements

### Backend Service
- Rust 1.92.0 or later
- Ollama running locally or accessible via network
- An embedding model in Ollama (e.g., `nomic-embed-text`)
- A chat model in Ollama (e.g., `llama3.2`, `mistral`, `qwen2.5`)

### FVTT Module
- Foundry VTT v12 or v13

## Installation

### Backend Service

1. **Install Rust 1.85.0+**
   ```bash
   rustup install 1.85.0
   rustup default 1.85.0
   ```

2. **Install Just (recommended)**

   [Just](https://github.com/casey/just) is a command runner that simplifies building:
   ```bash
   # Using cargo
   cargo install just

   # Or via package manager (examples)
   # macOS: brew install just
   # Arch: pacman -S just
   # Ubuntu: snap install --edge --classic just
   ```

3. **Build the service**

   Using Just (recommended - automatically downloads PDFium):
   ```bash
   just build           # Debug build
   just build-release   # Release build
   ```

   Or manually:
   ```bash
   # Download PDFium (required for PDF processing)
   just download-pdfium

   # Build
   cargo build --release -p seneschal-service
   ```

   **Note:** PDFium is dynamically linked and loaded at runtime from `vendor/pdfium/lib/`.

4. **Configure the service**

   Create a `config.toml` file (optional - defaults are sensible):
   ```toml
   [server]
   host = "127.0.0.1"
   port = 8080

   [ollama]
   base_url = "http://localhost:11434"
   model = "llama3.2"
   timeout_secs = 120

   [embeddings]
   model = "nomic-embed-text"
   dimensions = 768
   chunk_size = 512
   chunk_overlap = 50

   [storage]
   data_dir = "./data"

   [mcp]
   enabled = true
   path = "/mcp"
   ```

5. **Run the service**

   Using Just (sets up library paths automatically):
   ```bash
   just run
   ```

   Or manually (ensure PDFium library is in library path):
   ```bash
   LD_LIBRARY_PATH="$(pwd)/vendor/pdfium/lib" ./target/release/seneschal-service
   ```

   Or with environment variable overrides:
   ```bash
   SENESCHAL_OLLAMA__BASE_URL=http://192.168.1.100:11434 just run
   ```

### FVTT Module

#### For Local Development

1. **Symlink or copy the module**
   ```bash
   # Option 1: Symlink (recommended for development)
   ln -s /path/to/seneschal-program/fvtt-seneschal ~/.local/share/FoundryVTT/Data/modules/fvtt-seneschal

   # Option 2: Copy
   cp -r fvtt-seneschal ~/.local/share/FoundryVTT/Data/modules/
   ```

2. **Enable in Foundry VTT**
   - Launch Foundry VTT
   - Go to Game Settings → Manage Modules
   - Enable "Seneschal Program"

3. **Configure the module**
   - Go to Game Settings → Configure Settings → Module Settings
   - Set "Backend Service URL" to your backend address (e.g., `http://localhost:8080`)
   - Optionally set an API key if you've configured authentication

#### For Distribution

1. **Create a release package**
   ```bash
   cd fvtt-seneschal
   zip -r ../fvtt-seneschal-v0.1.0.zip . -x "*.git*"
   ```

2. **Upload to Foundry VTT package repository** (if publishing publicly)
   - Follow [Foundry VTT Package Submission](https://foundryvtt.com/article/package-submission/) guidelines

## Usage

### Chat Panel

1. Click the wizard hat icon in the scene controls (token layer)
2. Type your question in the input field
3. Press Enter or click Send

### One-Shot Commands

In the FVTT chat, use the `/sen-ai` prefix:
```
/sen-ai What are the requirements for a Jump-2 drive on a 200-ton ship?
```

### Document Ingestion

#### Via FVTT Module UI

1. Click the wizard hat icon in the scene controls (token layer)
2. Click the folder icon in the panel header to open Document Management
3. Click "Upload Document" and fill in the details:
   - Select a file (PDF, EPUB, Markdown, or plain text)
   - Enter a title for the document
   - Choose an access level
   - Add optional tags (comma-separated)
4. Click "Upload" to ingest the document

#### Via API

```bash
curl -X POST http://localhost:8080/api/documents \
  -F "file=@rulebook.pdf" \
  -F "title=Core Rulebook" \
  -F "access_level=gm_only" \
  -F "tags=rules,core"
```

### MCP Integration

For Claude Desktop integration, add to your Claude config:
```json
{
  "mcpServers": {
    "seneschal": {
      "url": "http://localhost:8080/mcp"
    }
  }
}
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/metrics` | GET | Prometheus metrics |
| `/api/chat` | POST | Chat with streaming SSE response |
| `/api/chat/continue` | POST | Continue after pause |
| `/api/documents` | GET | List documents |
| `/api/documents` | POST | Upload document (multipart) |
| `/api/documents/:id` | GET | Get document details |
| `/api/documents/:id` | DELETE | Delete document |
| `/api/search` | POST | Search documents |
| `/api/models` | GET | List available Ollama models |
| `/api/conversations` | GET | List conversations |
| `/api/conversations/:id` | GET | Get conversation |
| `/api/conversations/:id` | DELETE | Delete conversation |
| `/mcp/sse` | GET | MCP SSE endpoint |
| `/mcp/messages` | POST | MCP message handler |

## Development

### Backend Service

```bash
# Build and run (downloads PDFium automatically)
just build
just run

# Run tests
just test

# Check for issues
just clippy

# Format code
just fmt

# Clean build artifacts
just clean

# Clean everything including downloaded PDFium
just clean-all
```

Or without Just:
```bash
# Download PDFium first
just download-pdfium

# Then use cargo directly
cargo build -p seneschal-service
cargo test -p seneschal-service
cargo clippy --all-targets
cargo fmt --all
```

### FVTT Module

```bash
cd fvtt-seneschal

# Install dependencies (for linting/testing)
npm install

# Lint JavaScript
npm run lint

# Format JavaScript
npm run format

# Run tests
npm test
```

## Configuration Reference

### Environment Variables

All configuration can be overridden via environment variables using the pattern `SENESCHAL_SECTION__KEY`:

| Variable | Description | Default |
|----------|-------------|---------|
| `SENESCHAL_SERVER__HOST` | Server bind address | `127.0.0.1` |
| `SENESCHAL_SERVER__PORT` | Server port | `8080` |
| `SENESCHAL_OLLAMA__BASE_URL` | Ollama API URL | `http://localhost:11434` |
| `SENESCHAL_OLLAMA__MODEL` | Default chat model | `llama3.2` |
| `SENESCHAL_EMBEDDINGS__MODEL` | Embedding model | `nomic-embed-text` |
| `SENESCHAL_STORAGE__DATA_DIR` | Data directory | `./data` |
| `SENESCHAL_MCP__ENABLED` | Enable MCP server | `true` |

### Access Levels

Documents and tools use access levels aligned with FVTT roles:

| Level | Value | Description |
|-------|-------|-------------|
| `player` | 1 | Any player can access |
| `trusted` | 2 | Trusted players and above |
| `assistant` | 3 | Assistant GMs |
| `gm_only` | 4 | Game Master only |

## Traveller (MGT2E) Features

When used with the Mongoose Traveller 2e system, Seneschal Program provides enhanced support:

- **UWP Parsing**: Understands Universal World Profile format (e.g., `A867949-C`)
- **Jump Calculations**: Compute fuel requirements and jump distances
- **Skill Lookups**: Information about skills, specialities, and characteristics
- **Trade Codes**: Interpret world trade classifications

## License

[License information here]

## Contributing

[Contribution guidelines here]
