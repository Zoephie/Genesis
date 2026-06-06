# Backend Writer And Xsync Notes

This pass follows the next layer below `sub_1402624C0`: the active backend table entry at offset `+0x60`, the single-file create backend methods it calls, and the xsync difference path that can make another tool reload changed tags.

## Active Backend Table

`sub_140261F30(backend_name)` selects backend tables:

- `"default"` -> `off_141171418`
- `"single_new"` -> `off_14118B1E0`
- `"monolithic_new"` -> `off_14118B300`

`off_141171418` appears to be a default/config block whose first pointer is `off_14118B1E0`; it is not just a plain function table.

For both `single_new` and `monolithic_new`, table slot `+0x60` is:

```text
0x60 -> sub_1402C8B00
```

So the save dispatch `sub_1402624C0(...)` reaches the same writer wrapper for both backend table modes observed here.

Other useful table slots:

```text
+0x58 -> sub_1402C8530  load/read wrapper
+0x60 -> sub_1402C8B00  save/write wrapper
+0xB8 -> sub_1400660E0  returns 1; alternate stream support enabled
```

## Save Writer Wrapper

`sub_1402C8B00(group, path, tag_struct, metadata, stream_iterator)` is the backend save wrapper.

Important globals:

- `qword_1433A35D0` - current concrete backend aggregate/context.
- `*(qword_1433A35D0 + 24)` - create/write backend. If null, save fails.
- `qword_1433A35E0` - allocator/context pointer stored into the writer context.
- `*(qword_1433A35D0 + 80)` - optional service; vfunc `+0x10` supplies extra context for the save descriptor.

It builds:

- `c_tag_group_files_allocator`
- `c_tag_field_editor_manager_default`
- a small save context containing allocator, editor manager, `sub_1402CA640()` result, and optional service context

Then it calls:

```text
sub_1402C8910(context, create_backend, group, path, 0, tag_struct, metadata, stream_iterator)
```

## Main Tag And Stream Writer

`sub_1402C8910(...)` is the lower writer:

1. Asserts context, create backend, and metadata.
2. Opens a writer with create backend vfunc `+0x08`.
3. Gets the main output stream from writer vfunc `+0x00`.
4. Writes the main tag through `sub_1402C77B0(output, context, tag_struct)`.
5. If a stream iterator is present, iterates every stream:
   - stream iterator vfunc `+0x00` advances/checks next
   - stream iterator vfunc `+0x08` returns stream id/fourcc
   - stream iterator vfunc `+0x10` returns stream data
   - writer vfunc `+0x08` opens the named stream
   - `sub_1402C77B0(...)` writes stream data with the same persist path
   - writer vfunc `+0x10` commits/closes the stream
6. If every write succeeds, create backend vfunc `+0x10` commits the final tag file/artifact.
7. On failure, create backend vfunc `+0x18` aborts and cleans up.

## Persist Walker

`sub_1402C77B0(output, context, tag_or_stream_data)` wraps a stream in `c_tag_group_files_persist_context` and calls:

```text
sub_14045EAE0(tag_or_stream_data, persist_context)
```

`sub_14045EAE0`:

- gets the output stream from persist context vfunc `+0x08`
- gets the allocator/editor manager from persist context vfunc `+0x10`
- resolves the tag/stream group name through `qword_1433A0938 + 4 * group_index`
- calls a generic group-file persist operation with operation id `41`

This is the format-critical layer for a Rust backport. A Rust writer would need to reproduce the exact group-file persist layout, including group metadata, blocks, references, fixups, resources, and stream data.

## Single-File Create Backend

`c_single_tag_file_create_backend::vftable` at `0x140D1A320`:

```text
+0x08 -> sub_1402CB860  open temp/create writer
+0x10 -> sub_1402CCBF0  commit final file
+0x18 -> sub_1402CB170  abort/discard writer
```

`sub_1402CB860`:

- builds a backend path with `sub_140243EA0`
- opens a create/write stream with `sub_14029CFD0(..., mode=2)`
- creates a `c_single_tag_file_writer` with `sub_140470FF0`

`sub_140470FF0`:

- installs `c_single_tag_file_writer::vftable`
- writes the initial tag header
- logs `failed to write out tag header` if the header cannot be written

`c_single_tag_file_writer::vftable` at `0x140DBEBC8`:

```text
+0x00 -> sub_140471B20  open main tag stream
+0x08 -> sub_1404718D0  open named data stream
+0x10 -> sub_1404716E0  commit/close current data chunk
```

`sub_140471B20` opens the main stream with fourcc `1952540449` and marks it as the required main stream. `sub_1404718D0` opens extra streams and asserts that callers do not use the main-stream fourcc there.

`sub_1404716E0` commits a data chunk and logs `failed to commit data chunk` on failure. When the main stream commits, it stores checksum/header state back into the writer.

`sub_1402CCBF0` final commit:

- finalizes the writer with `sub_1404715A0`
- seeks back and writes the final tag file header
- builds a destination path under the `tags` root
- overwrites the final `tags/<path>.<group>` artifact
- logs `tags:tag_save: couldn't overwrite '%s.%s' with '%s'` on failure

`sub_1402CB170` abort/discard:

- tears down the active writer
- closes the temp stream
- deletes/discards the temporary path

## Xsync Difference Detection

`c_single_tag_file_xsync_backend::vftable` at `0x140D1A348` includes thin wrappers around a state object at `a1 + 8`.

Relevant wrappers:

```text
sub_1402CC040 -> sub_1402692C0(a1 + 8)  init/open xsync scan root
sub_1402CBA90 -> sub_140269040(a1 + 8)  refresh/update scan state
sub_1402CC2D0 -> sub_140269330(a1 + 8)  iterate differences
sub_1402CCB60 -> sub_140269360(a1 + 8)  clear pending flag/bitset
sub_14022A040(a1 + 8)                  read pending flag byte
```

`sub_1402692C0` gets a backend root/path through `sub_1402620A0()`, then opens it with filesystem helpers.

`sub_140269330` iterates changed paths and calls callback `sub_140269080`.

`sub_140269080(state, event_type, changed_path)` is the strongest live-update clue:

- only handles event type `3`
- parses the changed path into group/path with `sub_140261D70`
- resolves an already-known/open tag index with `sub_140250D30`
- compares old read-only state vs current read-only state
- compares old checksum `sub_14024CAD0(tag_index)` vs new checksum from `sub_140261C30`
- logs `tags:sync:pc: ... RELOADING!` when the tag should reload
- marks a bit in a per-tag bitset for indices below `0x7FFF`
- sets the pending flag byte at `state[0] = 1`

This means xsync is not just a vague notification name. It is a concrete changed-file/path scanner that marks loaded tags for reload when checksum/read-only state changes.

## Rust Feasibility

Feasible, but not as a small "write bytes and hope" job.

A Rust program can update the shared backend if it reproduces the backend artifacts exactly enough for the game tools to accept them:

- main tag group-file layout
- per-group pre-save fixups
- tag blocks and references
- checksum/header fields
- alternate streams/data chunks
- resources/cache side data when relevant
- atomic commit/overwrite semantics

For single-file tags, the backend path is understandable: write a temporary tag file/chunks, finalize the header, then overwrite the destination under `tags`.

For live update in Sapien, xsync likely matters when Sapien is using the xsync backend. The external writer probably does not need to send a message to Sapien, but it does need to create changes that xsync can detect: path changed, checksum changed, timestamp/state changed, and no stale lock/pending state blocking reload.
