Month 1: The Core Execution Harness (Reliability)

Week 1: Token-Level Tyranny: Why Prompting Local Models for JSON is a Fool’s Errand

    The Angle: Tear into the vanity of prompt engineering. Show how an 8B–12B model predictably chokes on brackets or JSON string escaping once context crosses 16k tokens.

    The Solution: Walk through dynamic GBNF grammar compilation. Show how to narrow down a massive schema roster into a per-turn subset grammar that constrains the model’s sampling distribution at the token layer.

Week 2: Bounded Self-Healing: Treating LLM Failures as Recoverable Protocol Events

    The Angle: Most agent frameworks panic or enter infinite loops when a tool call fails or returns unexpected shapes.

    The Solution: Introduce the Recover state. Show how to build a state machine where execution faults are wrapped as a SessionEvent, injected cleanly back into the context window, and given a strict retry budget (max_recovery_attempts).

Week 3: The Gatekeeper Pattern: Decoupling LLM Tool Execution from Runtime Permissions

    The Angle: Small models hallucinate arguments, mix up keys, and don't understand the difference between reading a file and executing a terminal command.

    The Solution: Detail a monolithic Rust Gatekeeper that validates incoming GBNF-enforced JSON against a strict schema and filters it through a state allowlist (e.g., separating what tools are visible during Reflect vs Chat).

Week 4: Zero unsafe, Zero Panics: Designing a Bulletproof Local Inference Loop

    The Angle: Production-grade system utilities shouldn't segfault or drop execution mid-turn because an unhandled option popped up.

    The Solution: Focus on pure idiomatic Rust error handling. Explain how to map standard LLM/IO drift into custom flat errors (FcpError) and use the ? operator to maintain a completely panic-free agent pipeline.

Month 2: Concurrency & State Management (The Architecture)

Week 5: Actor-Model Agent Architecture: Running TUI, Web, and Inference Without Mutex Deadlocks

    The Angle: If your UI blocks while llama.cpp or your vector DB is processing a heavy round-trip tool call, the user experience dies.

    The Solution: Show how to structure an orchestrator using Rust mpsc channels. Pass light, decoupled UserAction and SessionEvent messages so the UI layer and the execution engine run on completely independent threads.

Week 6: One Brain, Three Faces: Building Polymorphic UI Surfaces Over a Unified Session Layer

    The Angle: How to avoid writing separate orchestration logic for your terminal UI, your browser interface, and your API integrations.

    The Solution: Deep dive into designing a thin, abstract surface layer. Explain how the core engine processes states and streams out server-sent events (SSE) that feed a Ratatui terminal and an Axum server simultaneously.

Week 7: Tiered Recall: Balancing Ephemeral Caching with Flat-File Markdown State

    The Angle: Context windows are expensive to re-tokenize. Reading directly from raw disk on every single agent thought cycle introduces massive I/O overhead.

    The Solution: Walk through a three-tiered memory architecture: an ephemeral high-speed in-memory cache (like Moka) for working variables, standard flat-file Markdown for persistent storage, and background synchronization loops.

Week 8: The Reindex-on-Write Problem: Keeping Local Vector DBs in Sync with an Unpredictable User

    The Angle: If the user edits their local Obsidian notes or files outside of the AI chat window, the agent’s semantic understanding instantly degrades.

    The Solution: Detail a file-system watcher or prefetch loop that handles reindex-on-write actions asynchronously, pushing delta updates to a local vector store (like Qdrant) without blocking active generation turns.

Month 3: Deep Technical Optimization & Hardware Realities

Week 9: Eliminating the "God Object" in Complex State Machines: Refactoring the Agent Orchestrator

    The Angle: A deeply honest, architectural-debt review. Explain how easy it is for an agent loop to grow a single, monolithic orchestration file that handles too many responsibilities.

    The Solution: Provide code transformations that break apart massive state loops into clean, isolated state handlers, passing context explicitly instead of holding massive mutable states.

Week 10: The Hardware Honesty Matrix: Running Multimodal 12B Frameworks on Consumer VRAM

    The Angle: Cut through the marketing hype of hosted benchmarks. What does an agent loop actually cost in the real world when running chat, a 32k context window, and vision concurrently?

    The Solution: Break down exact hardware telemetry profiles. Show performance graphs mapping VRAM consumption on 16GB cards (like an RTX 4080 or base Apple Silicon) running a quantized Gemma class model alongside an active context.

Week 11: Constrained Sight: Bringing Local Vision Tools into the Gatekeeper Pipeline

    The Angle: Multimodal models (vision GGUFs) open up massive file processing capabilities but are twice as erratic when selecting tools based on visual inputs.

    The Solution: Explain how to pass local vault image paths safely through a gated vision:see tool, maintaining the exact same schema and state controls used for text processing.

Week 12: Architecture Over Intelligence: Why System Design Beats Model Size for Local Automations

    The Angle: The ultimate summary piece. Counter the industry trend of chasing frontier models by proving that a highly structured, restricted system harness makes a local 12B model more useful for local files than an unconstrained 70B model.

    The Solution: Contrast the performance, latency, and absolute predictability of a harness-first system (like ERIS) against a generic, unconstrained agent prompt loop.
