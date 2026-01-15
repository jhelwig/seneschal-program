# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Seneschal is an AI-powered assistant for Foundry VTT game masters. It consists of:
- **seneschal-service/** - Rust backend with RAG document search, agentic LLM loop, and REST API
- **fvtt-seneschal/** - JavaScript Foundry VTT module providing UI and FVTT API integration

Primary use case: Mongoose Traveller 2nd Edition rules assistance, but system-agnostic.

## Build Commands

This project uses [Just](https://github.com/casey/just) as the task runner.

```bash
# Rust (from repo root)
just build              # Debug build (auto-downloads PDFium)
just build-release      # Release build
just test               # Run tests
just run                # Debug run (sets library paths for PDFium)
just run-release        # Release run
just clippy             # Lint checks
just fmt                # Format code
just check              # Check without building

# JavaScript (from fvtt-seneschal/)
npm install
npm run lint            # ESLint
npm run format          # Prettier format
npm run format:check    # Prettier check
npm test                # Node.js native test runner
```

**CI enforces**: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, and JS lint/format/test.

**Runtime dependencies**: Ollama with embedding model (e.g., `qwen3-embedding:8b`), libpoppler-glib-dev.

## Architecture

### Backend Service Flow

```
POST /api/chat → SeneschalService.chat() → run_agentic_loop()
    ├─ Build system prompt + RAG context (vector search)
    ├─ Send to Ollama with tool definitions
    └─ LOOP while under limits:
        ├─ Parse response
        ├─ If tool_call:
        │   ├─ Internal (search, traveller_*) → Execute immediately, continue
        │   └─ External (FVTT API) → SSE to client, wait for result
        └─ If content → SSE to client
```

### Key Modules

| Module | Responsibility |
|--------|----------------|
| `service.rs` | Main coordinator, agentic loop, conversation management |
| `api.rs` | HTTP routes (chat, documents, images, search, models) |
| `mcp.rs` | MCP server with Streamable HTTP transport |
| `db.rs` | SQLite schema, embeddings storage, conversations |
| `ingestion.rs` | Document processing (PDF/EPUB/MD), chunking, image extraction |
| `search.rs` | Vector semantic search with access control filtering |
| `ollama.rs` | Ollama LLM client, streaming, tool call parsing |
| `tools/registry.rs` | Unified tool registry with type-safe enum naming |
| `tools/tool_defs/` | Categorized tool definitions (fvtt_crud, traveller, traveller_map, etc.) |
| `config.rs` | Layered config (defaults → config.toml → env vars) |

### Image Extraction Purpose

PDF images are extracted as discrete, reusable assets for Foundry VTT:
- **Tokens** - character/creature portraits for use on maps
- **Actor images** - profile images for NPCs, monsters, vehicles
- **Item images** - equipment, weapons, gear illustrations
- **Journal images** - maps, diagrams, illustrations for handouts

This means "render the page region" is **never** an acceptable approach for image extraction - we need the actual image data, not a screenshot of where it appears on the page. Images must be extracted as individual assets that can be dragged into FVTT and used independently of their PDF context.

### Tool Classification

Tools are classified as **internal** (executed by backend) or **external** (executed in FVTT client):
- Internal: `search`, `traveller_*`, `traveller_map_*` tools
- External: FVTT CRUD operations (actors, items, journals, scenes, tables), dice rolls, asset browsing

### External Tool Execution

External tools execute in the FVTT client (user's browser) via WebSocket:

```
Backend                              FVTT Module (browser)
   │                                        │
   │  chat_tool_call {tool, args}           │
   │ ─────────────────────────────────────► │
   │                                        ├─ Rebuild user context
   │                                        ├─ ToolExecutor.execute()
   │                                        ├─ FvttApiWrapper method
   │                                        ├─ FVTT API call (permissions enforced here)
   │  tool_result {result}                  │
   │ ◄───────────────────────────────────── │
   │                                        │
```

**Important**: The FVTT module does **not** implement its own permission checks. FVTT's native permission system enforces access when API calls are made. This means:
- Players CAN modify actors they own (their characters)
- Players CAN manage embedded items on their owned actors
- Document ownership and permission levels are respected automatically
- GMs have full access to all documents

### Document Access Levels (RAG)

```rust
pub enum AccessLevel {
    Player = 1,
    Trusted = 2,
    Assistant = 3,
    GmOnly = 4,
}
```

Controls which ingested documents the LLM can retrieve via semantic search. Maps to FVTT user roles for filtering search results. This is **separate** from FVTT's native permission system for tool execution (see External Tool Execution above).

### MCP Server

The backend includes an MCP (Model Context Protocol) server for integration with Claude Desktop and other MCP clients:

- **Endpoint**: `/mcp` (configurable)
- **Transport**: Streamable HTTP (MCP 2024-11-05 specification)
- **Tool sharing**: Uses the same unified tool registry as the Ollama agentic loop
- **External tools**: Bridge to FVTT via GM WebSocket connection (requires active GM session)

Configuration:
```bash
SENESCHAL_MCP__ENABLED=true
SENESCHAL_MCP__PATH=/mcp
```

### Unified Tool Registry

All tools are defined in a centralized registry (`tools/registry.rs`) with type-safe enum naming:

```rust
pub enum ToolName {
    DocumentSearch,
    CreateActor,
    AddActorItem,
    // ... all tools as enum variants
}
```

Benefits:
- Single source of truth for both Ollama and MCP
- Impossible to have string name mismatches (compile-time checked)
- Tools can be enabled/disabled independently per protocol (`ollama_enabled`, `mcp_enabled`)
- Definitions organized by category in `tools/tool_defs/`

## Configuration

Environment variables use `SENESCHAL_` prefix with `__` separators:
```bash
SENESCHAL_SERVER__PORT=8080
SENESCHAL_OLLAMA__BASE_URL=http://localhost:11434
SENESCHAL_STORAGE__DATA_DIR=/var/lib/seneschal
SENESCHAL_EMBEDDINGS__MODEL=qwen3-embedding:8b
```

## Key Patterns

- **Error handling**: `thiserror` crates with HTTP response conversion
- **Concurrency**: `tokio` async, `DashMap` for in-memory state
- **Streaming**: SSE (Server-Sent Events) for chat responses
- **Database**: SQLite with vector embeddings as BLOB
- **PDF processing**: `pdfium-render` for text + `poppler-rs` for image layer compositing
