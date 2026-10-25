# Editor comparison checkpoint

No claim that Rust's editor is better than Swift's is justified yet. The Swift source at `2ae2c2c251d4f2bc5b918c3b3ae5ab25c8c92229` is read-only reference. No Swift app/data/release changes were made. `fixtures/editor-comparison.md` defines the next shared fixture; a full same-fixture side-by-side run is not complete.

| Operation | Rust evidence | Swift comparison |
| --- | --- | --- |
| Heading, bold prose, task rendering | Native screenshot `rust-note-edited.png` | Existing implementation/source only; no fresh same-fixture run |
| Native paste | Inserted “Autosave verified.” through permitted CUA; persisted exact Markdown | Not rerun |
| Task toggle | CUA checkbox changed off → on; `- [x]` persisted and reappeared on reopen | Not rerun |
| Undo/redo then save | Host integration test passes. Native `cmd+z` removed the table-cell suffix and `cmd+shift+z` restored it; saved file confirmed | Existing tests/source; native same-fixture comparison not rerun |
| IME persistence | Host integration test excludes provisional “候補” and restores source after cancel | Native IME comparison not tested |
| Narrow wrapping | 340-point note wraps pasted prose in native screenshot | Not rerun at matching size |
| Lists, code, tables | Shared fixture rendered nested/numbered lists and highlighted code. Native table-cell paste changed `Editable` to `Editable checked`; undo/redo and Markdown save confirmed. Screenshots `rust-comparison-top.png`, `rust-table-redo.png` | Swift same-fixture run not complete |
| Images | Relative local PNG rendered in `rust-comparison-bottom.png`; host polls reused `refresh_images` so a file created after initial layout appears. Import not wired | Swift same-fixture run not complete |
| Mixed selection | Not tested in this host yet | Not rerun |
| Save/reopen | Native paste and task state confirmed after process relaunch | Not rerun |

Early `super+z`/`super+q` attempts produced no observable edit. With the native `cmd` modifier, undo and redo work. Final `cmd+q` exited the original process (PID 46301) with code 0. Calling the UI tool's app-state observation afterward relaunched it as a different process (PID 46726). Do not misreport that new process as failed shutdown. No forced process-exit workaround is present.

Native GPUI text required the platform's `font-kit` feature; the first empty render was a host setup error. The final graph enables `font-kit`/`runtime_shaders` exactly as Markraft's external consumer does.

No CPU, memory, latency, startup, platform parity or “Rust is faster” measurements were performed.
