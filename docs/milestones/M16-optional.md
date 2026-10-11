# M16: Optional Extensions (pick à la carte)

Each item is independent. Write a short ADR before starting any of them.

| Item | Summary | Notes |
|---|---|---|
| **Linux build** | AppImage/deb built on Linux, or in a Linux VM on the Mac (UTM). CPU or Vulkan llama.cpp build via `scripts/setup-runtime.sh --linux` | Tauri can't cross-compile Linux bundles from macOS reliably. Use a VM |
| **Custom OpenAI-compatible endpoint** | Settings → Runtime → "Use external server" (URL + key), e.g. LM Studio or Ollama on another machine | `LlmClient` is already HTTP. Skip the supervisor when external |
| **Embeddings RAG** | A second `llama-server --embeddings` process with a small embedding GGUF. Vector index (`usearch` or brute-force cosine for < 50k chunks) for DocLens and folder Q&A | Hybrid BM25 + vectors (reciprocal rank fusion) |
| **Vision** | MiniCPM-V GGUF + `--mmproj` for screenshots and images in chat | The catalog flag `vision`. The UI accepts image attachments |
| **Voice input** | whisper.cpp `whisper-server` sidecar, push-to-talk in the composer | Same owned-process pattern |
| **Notarization (macOS)** | Apple Developer ID + `notarytool` so the app opens on other Macs without warnings | Only if you share the app |
| **Windows code signing from the Mac** | `osslsigncode` with a code-signing certificate, via Tauri `bundle.windows.signCommand` | Removes the SmartScreen warning over time |
| **Cross-compile llama.cpp for Windows** (spike S9) | Build `llama-server.exe` from the pinned source on the Mac with clang-cl + xwin SDK (or llvm-mingw), instead of official binaries | Only if you need custom build flags |
| **Speculative decoding** | Use a tiny draft model (MiniCPM5 DSpark) with `--model-draft` for faster generation of bigger models | Measure first |
| **Plugin-style user tools** | User-defined tools as sandboxed WASM (wasmtime) with explicit permissions | A big item. Needs a security review |
