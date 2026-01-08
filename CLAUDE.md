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
| `db.rs` | SQLite schema, embeddings storage, conversations |
| `ingestion.rs` | Document processing (PDF/EPUB/MD), chunking, image extraction |
| `search.rs` | Vector semantic search with access control filtering |
| `ollama.rs` | Ollama LLM client, streaming, tool call parsing |
| `tools.rs` | Tool definitions, internal vs external classification |
| `config.rs` | Layered config (defaults → config.toml → env vars) |

### Tool Classification

Tools are classified as **internal** (executed by backend) or **external** (requested from FVTT client):
- Internal: `search`, `system_schema`, `traveller_*` tools
- External: FVTT read/write operations, dice rolls

### Access Levels

```rust
pub enum AccessLevel {
    Player = 1,
    Trusted = 2,
    Assistant = 3,
    GmOnly = 4,
}
```

Used for document filtering and tool permissions. Maps to FVTT user roles.

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
